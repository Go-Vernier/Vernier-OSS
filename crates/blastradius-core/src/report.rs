//! The terminal reports. The repository report says what was found and how;
//! the change report says what a change can reach. Neither fills a gap with
//! a guess.
use owo_colors::OwoColorize;

use std::collections::{BTreeMap, BTreeSet};

use crate::analyze::Analysis;
use crate::blast::{self, Blast, ChangeKind, Hop, Relation};
use crate::history::{History, Unit};
use crate::map::facts::Parser;
use crate::model::{Confidence, Edge, EdgeType, Service, ServiceRole};

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

/// The change report when a change was analysed, else the repository report.
pub fn format_report(analysis: &Analysis, color: bool) -> String {
    if analysis.blast.is_some() {
        format_change_report(analysis, color)
    } else {
        format_repo_report(analysis, color)
    }
}

pub fn format_repo_report(analysis: &Analysis, color: bool) -> String {
    let c = Paint { color };
    let services = analysis.graph.services();
    let infra: Vec<&Service> = services
        .iter()
        .filter(|s| s.role == ServiceRole::Infrastructure)
        .collect();
    let mut out: Vec<String> = Vec::new();
    header(analysis, &services, &c, &mut out);

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
    if let Some(history) = &analysis.history {
        history_section(history, &c, &mut out);
    }
    out.join("\n")
}

/// Headline first. Services, structure, edges and findings belong to the
/// repository report and are not repeated; the runtime mapping is, because
/// a partial join changes the radius.
pub fn format_change_report(analysis: &Analysis, color: bool) -> String {
    let c = Paint { color };
    let services = analysis.graph.services();
    let mut out: Vec<String> = Vec::new();
    header(analysis, &services, &c, &mut out);
    if let Some(b) = &analysis.blast {
        blast_section(b, &c, &mut out);
    }
    if analysis.runtime.connected {
        runtime_section(analysis, &c, &mut out);
    }
    if let Some(history) = &analysis.history {
        history_section(history, &c, &mut out);
    }
    out.join("\n")
}

/// Banner, the header rows, and the honest message when discovery found no
/// service boundaries.
fn header(analysis: &Analysis, services: &[Service], c: &Paint, out: &mut Vec<String>) {
    let code: Vec<&Service> = services
        .iter()
        .filter(|s| s.role == ServiceRole::Code)
        .collect();
    let infra: Vec<&Service> = services
        .iter()
        .filter(|s| s.role == ServiceRole::Infrastructure)
        .collect();
    out.push(c.bold("VERNIER"));
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
    out.push(row("Runtime", &runtime_header(analysis, c)));
    if let Some(b) = &analysis.blast {
        change_rows(b, c, out);
    }
    out.push(String::new());

    if analysis.discovery.strategy.is_none() {
        let lines: Vec<String> = if code.is_empty() && !infra.is_empty() {
            vec![
                format!(
                    "This repository declares {} services but builds none of them here,",
                    infra.len()
                ),
                "so there is no code to trace. Run vernier on the repository that".to_string(),
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
}

/// `Change  PR #481  merge commit 7d13248  2026-09-09`, the title under it,
/// then how many files landed in how many services.
fn change_rows(b: &Blast, c: &Paint, out: &mut Vec<String>) {
    let ch = &b.change;
    let mut first: Vec<String> = match ch.kind {
        ChangeKind::Pr => vec![format!("PR {}", ch.reference)],
        ChangeKind::Commit => vec![format!("commit {}", ch.reference)],
        ChangeKind::Diff => vec![format!("diff {}", ch.reference)],
        ChangeKind::Files => vec![format!("{} given", ch.reference)],
    };
    if let Some(how) = &ch.how {
        first.push(how.clone());
    }
    if let Some(date) = &ch.date {
        first.push(date.clone());
    }
    out.push(row("Change", &first.join("  ")));
    if let Some(title) = &ch.title {
        out.push(row("", &c.dim(title)));
    }
    let files = ch.files.len();
    let file_noun = if files == 1 { "file" } else { "files" };
    let service_noun = if b.changed.len() == 1 {
        "service"
    } else {
        "services"
    };
    let mut summary = vec![format!(
        "{files} {file_noun} in {} {service_noun}",
        b.changed.len()
    )];
    if !b.unowned.is_empty() {
        summary.push(format!("{} in no service", b.unowned.len()));
    }
    if ch.outside_root > 0 {
        summary.push(format!(
            "{} outside the analysed directory",
            ch.outside_root
        ));
    }
    out.push(row("", &c.dim(&summary.join(", "))));
}

fn row(label: &str, value: &str) -> String {
    format!("  {label:<13} {value}")
}

fn runtime_header(analysis: &Analysis, c: &Paint) -> String {
    let r = &analysis.runtime;
    let (Some(source), Some(services)) = (r.source, r.services) else {
        return c.dim("not connected - static only");
    };
    let what = match r.input.as_deref() {
        Some(input) if input.starts_with("datadog env ") => {
            format!(
                "{} {}",
                source.label(),
                input.trim_start_matches("datadog ")
            )
        }
        _ => source.label().to_string(),
    };
    // Names the user ignored on purpose are not a partial join.
    if services.matched + ignored_count(analysis) < services.runtime {
        c.yellow(&format!(
            "connected ({what}, {} of {} runtime services matched)",
            services.matched, services.runtime
        ))
    } else {
        format!("connected ({what}, {} services matched)", services.matched)
    }
}

fn ignored_count(analysis: &Analysis) -> usize {
    analysis
        .runtime
        .mapping
        .iter()
        .filter(|m| m.how == "ignored")
        .count()
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
    runtime_section(analysis, c, out);
    if edges.is_empty() {
        out.push(format!(
            "  {}",
            c.dim("No static edges found: no HTTP, gRPC, event, database or import edge to another discovered service was recognised.")
        ));
        return;
    }
    out.push(String::new());
    out.push(c.bold("EDGES"));
    out.push(String::new());
    out.extend(edges_table(edges, c));
    out.push(String::new());
    let widest = blast::widest(&analysis.graph, blast::DEFAULT_DEPTH);
    findings(edges, services, c, out, analysis.runtime.connected, widest);
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

/// The join, printed whether or not it is complete: a partial mapping that
/// looks complete is worse than none.
fn runtime_section(analysis: &Analysis, c: &Paint, out: &mut Vec<String>) {
    let r = &analysis.runtime;
    let (Some(source), Some(services), Some(edges)) = (r.source, r.services, r.edges) else {
        return;
    };
    out.push(String::new());
    out.push(c.bold("RUNTIME"));
    out.push(String::new());
    out.push(row(
        "Source",
        &format!("{}  {}", source.label(), r.input.as_deref().unwrap_or("")),
    ));
    let ignored = ignored_count(analysis);
    let ignored_note = match ignored {
        0 => String::new(),
        n => format!(" ({n} ignored by {})", crate::config::FILE_NAME),
    };
    out.push(row(
        "Services",
        &format!(
            "{} of {} runtime services matched{ignored_note}",
            services.matched, services.runtime
        ),
    ));
    let noun = if edges.skipped == 1 { "call" } else { "calls" };
    out.push(row(
        "Edges",
        &format!(
            "{} observed ({} static confirmed, {} runtime only) · {} {noun} skipped, one end unmatched or ignored",
            edges.observed,
            edges.observed - edges.runtime_only,
            edges.runtime_only,
            edges.skipped
        ),
    ));
    out.push(String::new());
    let w_name = r
        .mapping
        .iter()
        .map(|m| m.runtime.chars().count())
        .max()
        .unwrap_or(12)
        .max(12);
    let w_service = r
        .mapping
        .iter()
        .filter_map(|m| m.service.as_ref())
        .map(|s| s.chars().count())
        .max()
        .unwrap_or(7)
        .max(7);
    out.push(format!(
        "  {}",
        c.dim(&format!(
            "{:<w_name$}  {:<w_service$}  HOW",
            "RUNTIME NAME", "SERVICE"
        ))
    ));
    for m in r.mapping.iter().filter(|m| m.how != "unmatched") {
        let how = match m.how.as_str() {
            "ignored" => format!("ignored ({})", crate::config::FILE_NAME),
            h if h.starts_with("fuzzy") => format!("{h}  (check this)"),
            h => h.to_string(),
        };
        let line = format!(
            "  {:<w_name$}  {:<w_service$}  {how}",
            m.runtime,
            m.service.as_deref().unwrap_or("-")
        );
        out.push(if m.how.starts_with("fuzzy") {
            c.yellow(&line)
        } else {
            line
        });
    }
    if !r.unmatched.is_empty() {
        out.push(String::new());
        let noun = if r.unmatched.len() == 1 {
            "service"
        } else {
            "services"
        };
        out.push(format!(
            "  {}",
            c.yellow(&format!(
                "{} runtime {noun} matched nothing: {}",
                r.unmatched.len(),
                wrap(&r.unmatched.join(", "), 60)
            ))
        ));
    }
}

#[allow(clippy::too_many_lines)]
fn findings(
    edges: &[Edge],
    services: &[Service],
    c: &Paint,
    out: &mut Vec<String>,
    runtime_connected: bool,
    widest: Option<(String, usize)>,
) {
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
    out.push(String::new());
    match widest {
        Some((name, count)) if count > 0 => {
            out.push(format!("  {:<38} {name}", "Widest change surface"));
            let noun = if count == 1 { "service" } else { "services" };
            out.push(format!(
                "    {}",
                c.dim(&format!("a change here reaches {count} {noun}"))
            ));
        }
        _ => {
            out.push(format!("  {:<38} -", "Widest change surface"));
            out.push(format!(
                "    {}",
                c.dim("no change to one service reaches another")
            ));
        }
    }
    out.push(String::new());
    // One line per pair of code services that read the same database, under
    // the key their strongest evidence names.
    let code_names: BTreeSet<&str> = services
        .iter()
        .filter(|s| s.role == ServiceRole::Code)
        .map(|s| s.name.as_str())
        .collect();
    let mut shared: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
    let mut seen_pairs: BTreeSet<(&str, &str)> = BTreeSet::new();
    for e in edges.iter().filter(|e| {
        e.edge_type == EdgeType::Database
            && code_names.contains(e.source.as_str())
            && code_names.contains(e.target.as_str())
    }) {
        let pair = if e.source <= e.target {
            (e.source.as_str(), e.target.as_str())
        } else {
            (e.target.as_str(), e.source.as_str())
        };
        if !seen_pairs.insert(pair) {
            continue;
        }
        let keys: Vec<String> = e
            .evidence
            .iter()
            .filter_map(|v| {
                v.detail
                    .as_deref()?
                    .strip_prefix("shared database ")?
                    .split(" with ")
                    .next()
                    .map(str::to_string)
            })
            .collect();
        let key = keys
            .iter()
            .find(|k| !k.contains('/'))
            .or_else(|| keys.first())
            .cloned();
        if let Some(key) = key {
            shared.entry(key).or_default().extend([pair.0, pair.1]);
        }
    }
    out.push(format!("  {:<38} {}", "Shared databases", shared.len()));
    for (key, names) in &shared {
        let list = names.iter().copied().collect::<Vec<_>>().join(", ");
        out.push(format!("    {key:<36} {}", c.dim(&list)));
    }
    if runtime_connected {
        never_observed_finding(edges, &code_names, c, out);
    }
}

/// Call paths production never took. Imports are not calls, and a shared
/// database between two code services is not one either, so neither can be
/// observed and neither is listed.
fn never_observed_finding(
    edges: &[Edge],
    code_names: &BTreeSet<&str>,
    c: &Paint,
    out: &mut Vec<String>,
) {
    let never: Vec<String> = edges
        .iter()
        .filter(|e| e.observed.is_none() && e.edge_type != EdgeType::Import)
        .filter(|e| {
            !(e.edge_type == EdgeType::Database
                && code_names.contains(e.source.as_str())
                && code_names.contains(e.target.as_str()))
        })
        .map(|e| format!("{} -> {}", e.source, e.target))
        .collect();
    out.push(String::new());
    out.push(format!(
        "  {:<38} {}",
        "Static edges never observed",
        never.len()
    ));
    if !never.is_empty() {
        out.push(format!("    {}", c.dim(&wrap(&never.join(", "), 60))));
    }
}

/// BLAST RADIUS: the headline, what changed, what it reaches and how, and
/// what it does not reach, in the fixed wording.
fn blast_section(b: &Blast, c: &Paint, out: &mut Vec<String>) {
    out.push(c.bold("BLAST RADIUS"));
    out.push(String::new());
    let changed_noun = if b.summary.changed == 1 {
        "service"
    } else {
        "services"
    };
    let reached_noun = if b.summary.reached == 1 {
        "service"
    } else {
        "services"
    };
    out.push(format!(
        "  {}",
        c.bold(&format!(
            "{} {changed_noun} changed -> {} {reached_noun} in the blast radius",
            b.summary.changed, b.summary.reached
        ))
    ));
    let by = b.summary.by_confidence;
    let mut parts: Vec<String> = Vec::new();
    for (n, label) in [
        (by.observed, "observed"),
        (by.static_, "static"),
        (by.inferred, "inferred"),
        (by.uncertain, "uncertain"),
    ] {
        if n > 0 {
            parts.push(format!("{n} {label}"));
        }
    }
    parts.push(format!("depth {}", b.depth));
    out.push(format!("  {}", c.dim(&parts.join(" · "))));
    out.push(String::new());

    changed_block(b, c, out);

    if !b.reached.is_empty() {
        out.push(format!("  {}", c.dim("REACHED")));
        out.extend(reached_table(&b.reached, c));
        out.push(String::new());
    }

    if !b.infrastructure.is_empty() {
        let list = b
            .infrastructure
            .iter()
            .map(|t| format!("{} (published to by {})", t.service, t.via))
            .collect::<Vec<_>>()
            .join(", ");
        out.push(format!(
            "  {:<28} {}",
            "Infrastructure on the path",
            c.dim(&list)
        ));
        out.push(format!(
            "  {}",
            c.dim("  every other client of a broker a changed service publishes to is included as uncertain")
        ));
        out.push(String::new());
    }

    if !b.unowned.is_empty() && !b.changed.is_empty() {
        let noun = if b.unowned.len() == 1 {
            "file belongs"
        } else {
            "files belong"
        };
        out.push(format!(
            "  {}",
            c.dim(&format!(
                "{} changed {noun} to no service: {}",
                b.unowned.len(),
                wrap(&b.unowned.join(", "), 60)
            ))
        ));
        out.push(String::new());
    }

    not_reached_block(b, c, out);
}

/// What changed, or the honest line when nothing the change touched belongs
/// to a service.
fn changed_block(b: &Blast, c: &Paint, out: &mut Vec<String>) {
    if b.changed.is_empty() {
        let noun = if b.change.files.len() == 1 {
            "file belongs"
        } else {
            "files belong"
        };
        out.push(format!(
            "  {}",
            c.yellow(&format!(
                "None of the {} changed {noun} to a discovered service: {}",
                b.change.files.len(),
                wrap(&b.unowned.join(", "), 60)
            ))
        ));
        if b.change.files.is_empty() {
            out.push(format!("  {}", c.yellow("The change lists no files.")));
        }
        out.push(String::new());
        return;
    }
    out.push(format!("  {}", c.dim("CHANGED")));
    let w_name = b
        .changed
        .iter()
        .map(|x| x.service.chars().count())
        .max()
        .unwrap_or(7)
        .max(7);
    for ch in &b.changed {
        let noun = if ch.files.len() == 1 { "file" } else { "files" };
        let shown: Vec<&str> = ch.files.iter().take(3).map(String::as_str).collect();
        let more = if ch.files.len() > 3 { ", ..." } else { "" };
        out.push(format!(
            "  {:<w_name$}  {:>3} {noun:<5}  {}",
            ch.service,
            ch.files.len(),
            c.dim(&format!("{}{more}", shown.join(", ")))
        ));
    }
    out.push(String::new());
}

/// The fixed wording, then the services it applies to.
fn not_reached_block(b: &Blast, c: &Paint, out: &mut Vec<String>) {
    out.push(format!("  {}", blast::NOT_REACHED));
    if b.not_reached.is_empty() {
        out.push(format!(
            "    {}",
            c.dim("0 services  (every other service is in the computed blast radius)")
        ));
    } else {
        let noun = if b.not_reached.len() == 1 {
            "service"
        } else {
            "services"
        };
        out.push(format!(
            "    {:<11} {}",
            format!("{} {noun}", b.not_reached.len()),
            c.dim(&wrap(&b.not_reached.join(", "), 60))
        ));
    }
}

fn reached_table(reached: &[blast::Reached], c: &Paint) -> Vec<String> {
    let w_name = reached
        .iter()
        .map(|r| r.service.chars().count())
        .max()
        .unwrap_or(7)
        .max(7);
    let header = format!(
        "{:<w_name$}  {:<5}  {:<10}  PATH",
        "SERVICE", "DEPTH", "CONFIDENCE"
    );
    let mut lines = vec![format!("  {}", c.dim(&header))];
    for r in reached {
        let path = r
            .path
            .iter()
            .rev()
            .map(hop_words)
            .collect::<Vec<_>>()
            .join("; ");
        let line = format!(
            "  {:<w_name$}  {:<5}  {:<10}  {path}",
            r.service,
            r.depth,
            r.confidence.as_str()
        );
        lines.push(if r.confidence == Confidence::Uncertain {
            c.dim(&line)
        } else {
            line
        });
    }
    lines
}

/// One hop, read from the reached service's side.
fn hop_words(h: &Hop) -> String {
    match h.relation {
        Relation::Calls => match h.calls {
            Some(n) => format!(
                "{} calls {} ({}, {n} calls)",
                h.to,
                h.from,
                h.edge_type.as_str()
            ),
            None => format!("{} calls {} ({})", h.to, h.from, h.edge_type.as_str()),
        },
        Relation::Imports => format!("{} imports {}", h.to, h.from),
        Relation::SharesDatabase => format!("{} shares a database with {}", h.to, h.from),
        Relation::Consumes => match h.calls {
            Some(n) => format!("{} consumes events from {} ({n} calls)", h.to, h.from),
            None => format!("{} consumes events from {}", h.to, h.from),
        },
        Relation::SharesBroker => format!(
            "{} shares broker {} with {}",
            h.to,
            h.via.as_deref().unwrap_or("?"),
            h.from
        ),
    }
}

/// CHANGE HISTORY: the numbers over the last N pull requests, or commits
/// when the history carries no pull request markers.
fn history_section(h: &History, c: &Paint, out: &mut Vec<String>) {
    out.push(String::new());
    let scope = if h.found < h.requested {
        format!(
            "(last {} {}, {} asked for)",
            h.found,
            h.unit.noun(h.found),
            h.requested
        )
    } else {
        format!("(last {} {})", h.found, h.unit.noun(h.found))
    };
    out.push(format!("{}  {}", c.bold("CHANGE HISTORY"), c.dim(&scope)));
    out.push(String::new());
    if h.unit == Unit::Commits {
        out.push(format!(
            "  {}",
            c.yellow(
                "No pull request markers in the history; each first-parent commit counts as one change."
            )
        ));
        out.push(String::new());
    }
    if h.found == 0 {
        out.push(format!("  {}", c.yellow("No commits found.")));
        return;
    }
    let noun = if (h.average - 1.0).abs() < f64::EPSILON {
        "service"
    } else {
        "services"
    };
    out.push(wide_row(
        "Average blast radius",
        &format!("{:.1} {noun}", h.average),
    ));
    out.push(wide_row("Median", &format_number(h.median)));
    if let Some(largest) = &h.largest {
        let label = match h.unit {
            Unit::PullRequests => format!("PR {}", largest.reference),
            Unit::Commits => format!("commit {}", largest.reference),
        };
        let noun = if largest.reached == 1 {
            "service"
        } else {
            "services"
        };
        out.push(wide_row(
            "Largest",
            &format!("{label} - {} {noun}", largest.reached),
        ));
    }
    let unit_noun = match h.unit {
        Unit::PullRequests => "PRs",
        Unit::Commits => "Commits",
    };
    out.push(wide_row(
        &format!("{unit_noun} reaching >10"),
        &format!(
            "{}  {}",
            h.over_10.count,
            c.dim(&format!("({}%)", h.over_10.percent))
        ),
    ));
    if h.touching_no_service > 0 {
        out.push(wide_row(
            &format!("{unit_noun} touching no service"),
            &h.touching_no_service.to_string(),
        ));
    }
    out.push(String::new());
    let w_ref = h
        .entries
        .iter()
        .map(|e| e.reference.chars().count())
        .max()
        .unwrap_or(3)
        .max(3);
    out.push(format!(
        "  {}",
        c.dim(&format!(
            "{:<w_ref$}  {:<10}  {:>5}  {:>7}  {:>7}  TITLE",
            "REF", "DATE", "FILES", "CHANGED", "REACHED"
        ))
    ));
    for e in &h.entries {
        let title: String = e.title.chars().take(60).collect();
        out.push(format!(
            "  {:<w_ref$}  {:<10}  {:>5}  {:>7}  {:>7}  {}",
            e.reference,
            e.date,
            e.files,
            e.changed,
            e.reached,
            c.dim(&title)
        ));
    }
}

/// `3` for a whole number, `3.5` otherwise.
fn format_number(x: f64) -> String {
    if x.fract().abs() < f64::EPSILON {
        format!("{x:.0}")
    } else {
        format!("{x:.1}")
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
