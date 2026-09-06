//! The repository report. It says what was found, how it was found, and
//! what this version does not do yet. It never fills a gap with a guess.
use owo_colors::OwoColorize;

use crate::analyze::Analysis;
use crate::model::{Service, ServiceRole};

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

    out.push(c.bold("STRUCTURE"));
    out.push(String::new());
    out.push(format!(
        "  {}",
        c.dim("Dependency mapping is not built yet: this release reports service")
    ));
    out.push(format!(
        "  {}",
        c.dim("boundaries only. Static edges, the runtime join and the blast radius")
    ));
    out.push(format!(
        "  {}",
        c.dim("itself come next. See docs/build-spec.md for the order.")
    ));
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
