//! The repository report. It says what was found, how it was found, and
//! what this version does not do yet. It never fills a gap with a guess.
use owo_colors::OwoColorize;

use std::collections::{BTreeMap, BTreeSet};

use crate::analyze::Analysis;
use crate::map::facts::Parser;
use crate::model::{Confidence, Edge, Service, ServiceRole};

struct Paint {
    color: bool,
}

impl Paint {
    fn bold(&self, s: &str) -> String {
        if self.color {
            s.bold().to_string()
        } else {
            s.to_string()
        }
    }
    fn dim(&self, s: &str) -> String {
        if self.color {
            s.dimmed().to_string()
        } else {
            s.to_string()
        }
    }
    fn yellow(&self, s: &str) -> String {
        if self.color {
            s.yellow().to_string()
        } else {
            s.to_string()
        }
    }
}

pub fn format_repo_report(analysis: &Analysis, color: bool) -> String {
    let c = Paint { color };
    let services = analysis.graph.services();
    let code: Vec<&Service> = services
        .iter()
        .filter(|s| s.role == ServiceRole::Code)
        .collect();
    let infra: Vec<&Service> = services
        .iter()
        .filter(|s| s.role == ServiceRole::Infrastructure)
        .collect();
    let mut out: Vec<String> = Vec::new();

    out.push(c.bold("BLAST RADIUS"));
    out.push(String::new());
    out.push(row("Repository", &analysis.repository));
    let detected = match analysis.discovery.strategy {
        Some(strategy) => format!(
            "{} detected  {}",
            code.len(),
            c.dim(&format!("({})", strategy.as_str()))
        ),
        None => format!("{} detected", code.len()),
    };
    out.push(row("Services", &detected));
    out.push(row("Runtime", &c.dim("not connected - static only")));
    out.push(String::new());

    if analysis.discovery.strategy.is_none() {
        let lines: Vec<String> = if code.is_empty() && !infra.is_empty() {
            vec![
                format!(
                    "This repository declares {} services but builds none of them here,",
                    infra.len()
                ),
                "so there is no code to trace. Run blast-radius on the repository that".to_string(),
                "holds the services.".to_string(),
            ]
        } else {
            vec![
                "This looks like a single service. Blast radius analysis needs a".to_string(),
                "multi-service repository.".to_string(),
            ]
        };
        out.extend(lines.iter().map(|l| format!("  {}", c.yellow(l))));
        out.push(String::new());
        let tried = analysis
            .discovery
            .attempted
            .iter()
            .map(|a| format!("{} {}", a.strategy.as_str(), a.services))
            .collect::<Vec<_>>()
            .join(" · ");
        out.push(row("Tried", &c.dim(&tried)));
        out.push(String::new());
    }

    if !services.is_empty() {
        out.push(c.bold("SERVICES"));
        out.push(String::new());
        out.extend(table(&services, &c));
        out.push(String::new());
        if !infra.is_empty() {
            let names = infra
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            out.push(format!(
                "  {}",
                c.dim(&format!(
                    "{} declared but not built here (images): {names}",
                    infra.len()
                ))
            ));
            out.push(String::new());
        }
    }

    structure(analysis, &services, &c, &mut out);
    out.join("\n")
}

fn row(label: &str, value: &str) -> String {
    format!("  {label:<13} {value}")
}

struct Row {
    name: String,
    language: String,
    root: String,
    evidence: String,
    infra: bool,
}

fn table(services: &[Service], c: &Paint) -> Vec<String> {
    let rows: Vec<Row> = services
        .iter()
        .map(|s| Row {
            name: s.name.clone(),
            language: s.language.clone().unwrap_or_else(|| "-".to_string()),
            root: s.root.clone().unwrap_or_else(|| match &s.image {
                Some(image) => format!("(image {image})"),
                None => "(no directory)".to_string(),
            }),
            evidence: match s.evidence.line {
                Some(line) => format!("{}:{line}", s.evidence.file),
                None => s.evidence.file.clone(),
            },
            infra: s.role == ServiceRole::Infrastructure,
        })
        .collect();
    let width =
        |min: usize, pick: fn(&Row) -> usize| rows.iter().map(pick).max().unwrap_or(0).max(min);
    let w_name = width(4, |r| r.name.chars().count());
    let w_lang = width(8, |r| r.language.chars().count());
    let w_root = width(4, |r| r.root.chars().count());

    let header = [
        format!("{:<w_name$}", "NAME"),
        format!("{:<w_lang$}", "LANGUAGE"),
        format!("{:<w_root$}", "ROOT"),
        "EVIDENCE".to_string(),
    ]
    .join("  ");
    let mut lines = vec![format!("  {}", c.dim(&header))];
    for r in &rows {
        let cells = [
            format!("{:<w_name$}", r.name),
            format!("{:<w_lang$}", r.language),
            format!("{:<w_root$}", r.root),
            c.dim(&r.evidence),
        ]
        .join("  ");
        lines.push(format!("  {}", if r.infra { c.dim(&cells) } else { cells }));
    }
    lines
}

fn wide_row(label: &str, value: &str) -> String {
    format!("  {label:<24} {value}")
}

/// STRUCTURE, then EDGES and FINDINGS when there are edges. The mapping
/// summary says what was read and what could not be placed.
fn structure(analysis: &Analysis, services: &[Service], c: &Paint, out: &mut Vec<String>) {
    let edges = analysis.graph.edges();
    out.push(c.bold("STRUCTURE"));
    out.push(String::new());
    structure_counts(edges, out);
    out.push(String::new());
    mapping_summary(analysis, services, c, out);
    if edges.is_empty() {
        out.push(format!(
            "  {}",
            c.dim("No static edges found: no HTTP or gRPC call to another discovered service was recognised.")
        ));
        return;
    }
    out.push(String::new());
    out.push(c.bold("EDGES"));
    out.push(String::new());
    out.extend(edges_table(edges, c));
    out.push(String::new());
    findings(edges, services, c, out);
}

fn structure_counts(edges: &[Edge], out: &mut Vec<String>) {
    out.push(wide_row("Total edges", &edges.len().to_string()));
    let count = |conf: Confidence| edges.iter().filter(|e| e.confidence == conf).count();
    if count(Confidence::Observed) > 0 {
        out.push(wide_row(
            "Observed in production",
            &count(Confidence::Observed).to_string(),
        ));
    }
    out.push(wide_row("Static", &count(Confidence::Static).to_string()));
    out.push(wide_row(
        "Inferred",
        &count(Confidence::Inferred).to_string(),
    ));
    out.push(wide_row(
        "Uncertain",
        &count(Confidence::Uncertain).to_string(),
    ));
    if !edges.is_empty() {
        let mut by_type: BTreeMap<&str, usize> = BTreeMap::new();
        for e in edges {
            *by_type.entry(e.edge_type.as_str()).or_insert(0) += 1;
        }
        let mut pairs: Vec<(&str, usize)> = by_type.into_iter().collect();
        pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        let text = pairs
            .iter()
            .map(|(t, n)| format!("{t} {n}"))
            .collect::<Vec<_>>()
            .join(" · ");
        out.push(wide_row("By type", &text));
    }
}

fn mapping_summary(analysis: &Analysis, services: &[Service], c: &Paint, out: &mut Vec<String>) {
    let m = &analysis.mapping;
    let owning = services
        .iter()
        .filter(|s| s.role == ServiceRole::Code && s.root.is_some())
        .count();
    let mut tree_sitter: Vec<&str> = Vec::new();
    let mut regex: Vec<&str> = Vec::new();
    for (label, parser) in &m.parsers {
        match parser {
            Parser::TreeSitter => tree_sitter.push(label),
            Parser::Regex => regex.push(label),
        }
    }
    let mut parsers: Vec<String> = Vec::new();
    if !tree_sitter.is_empty() {
        parsers.push(format!("tree-sitter: {}", tree_sitter.join(", ")));
    }
    if !regex.is_empty() {
        parsers.push(format!("regex: {}", regex.join(", ")));
    }
    let scanned = if parsers.is_empty() {
        format!("Scanned {} files in {owning} services", m.files_scanned)
    } else {
        format!(
            "Scanned {} files in {owning} services ({})",
            m.files_scanned,
            parsers.join(" · ")
        )
    };
    out.push(format!("  {}", c.dim(&scanned)));
    if m.files_outside_services > 0 {
        out.push(format!(
            "  {}",
            c.dim(&format!(
                "{} files outside every service were not read",
                m.files_outside_services
            ))
        ));
    }
    if m.unresolved > 0 {
        let shown: Vec<&str> = m
            .unresolved_targets
            .iter()
            .take(5)
            .map(String::as_str)
            .collect();
        let more = if m.unresolved_targets.len() > 5 {
            ", ..."
        } else {
            ""
        };
        let noun = if m.unresolved == 1 {
            "target"
        } else {
            "targets"
        };
        out.push(format!(
            "  {}",
            c.dim(&format!(
                "{} {noun} could not be matched to a service: {}{more}",
                m.unresolved,
                shown.join(", ")
            ))
        ));
    }
}

fn findings(edges: &[Edge], services: &[Service], c: &Paint, out: &mut Vec<String>) {
    out.push(c.bold("FINDINGS"));
    out.push(String::new());
    let mut inbound: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for e in edges {
        inbound
            .entry(e.target.as_str())
            .or_default()
            .insert(e.source.as_str());
    }
    let never: Vec<&str> = services
        .iter()
        .filter(|s| s.role == ServiceRole::Code && !inbound.contains_key(s.name.as_str()))
        .map(|s| s.name.as_str())
        .collect();
    let noun = if never.len() == 1 {
        "service"
    } else {
        "services"
    };
    out.push(format!(
        "  {:<38} {} {noun}",
        "Never called by another service",
        never.len()
    ));
    if !never.is_empty() {
        out.push(format!(
            "    {:<36} {}",
            wrap(&never.join(", "), 36),
            c.dim("(dead, or just quiet?)")
        ));
    }
    out.push(String::new());
    if let Some((name, sources)) = inbound
        .iter()
        .max_by(|a, b| a.1.len().cmp(&b.1.len()).then_with(|| b.0.cmp(a.0)))
    {
        out.push(format!("  {:<38} {name}", "Most connected"));
        let noun = if sources.len() == 1 {
            "service"
        } else {
            "services"
        };
        out.push(format!(
            "    {}",
            c.dim(&format!("touched by {} {noun}", sources.len()))
        ));
    }
}

/// Joins names on one line up to `width`, continuing on indented lines.
fn wrap(text: &str, width: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split(' ') {
        if !current.is_empty() && current.len() + 1 + word.len() > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines.join("\n    ")
}

fn edges_table(edges: &[Edge], c: &Paint) -> Vec<String> {
    let w_source = edges
        .iter()
        .map(|e| e.source.chars().count())
        .max()
        .unwrap_or(6)
        .max(6);
    let w_target = edges
        .iter()
        .map(|e| e.target.chars().count())
        .max()
        .unwrap_or(6)
        .max(6);
    let header = [
        format!("{:<w_source$}", "SOURCE"),
        "  ".to_string(),
        format!("{:<w_target$}", "TARGET"),
        format!("{:<8}", "TYPE"),
        format!("{:<10}", "CONFIDENCE"),
        "EVIDENCE".to_string(),
    ]
    .join("  ");
    let mut lines = vec![format!("  {}", c.dim(&header))];
    for e in edges {
        let first = e.evidence.first();
        let at = first.map_or(String::new(), |v| match v.line {
            Some(line) => format!("{}:{line}", v.file),
            None => v.file.clone(),
        });
        let detail: String = first
            .and_then(|v| v.detail.as_deref())
            .map(|d| d.chars().take(60).collect())
            .unwrap_or_default();
        let evidence = if detail.is_empty() {
            at
        } else {
            format!("{at}  {}", c.dim(&detail))
        };
        let row = [
            format!("{:<w_source$}", e.source),
            "->".to_string(),
            format!("{:<w_target$}", e.target),
            format!("{:<8}", e.edge_type.as_str()),
            format!("{:<10}", e.confidence.as_str()),
            evidence,
        ]
        .join("  ");
        lines.push(if e.confidence == Confidence::Uncertain {
            format!("  {}", c.dim(&row))
        } else {
            format!("  {row}")
        });
    }
    lines
}
