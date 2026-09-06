//! Every change runs against the whole corpus. A regression on one repo is
//! a regression on the product. Skips with a message when corpus/ is absent.
use std::fs;
use std::path::Path;
use std::time::Instant;

use blastradius::ServiceRole;
use serde::Deserialize;

#[derive(Deserialize)]
struct ExpectedEdge {
    source: String,
    target: String,
    #[serde(rename = "type")]
    edge_type: String,
}

#[derive(Deserialize)]
struct Expected {
    strategy: Option<String>,
    code: Vec<String>,
    infrastructure: Vec<String>,
    /// Edges the documented architecture says exist. Every one must be
    /// found with the same type; extras are fine and printed with
    /// `CORPUS_VERBOSE=1`.
    #[serde(default)]
    edges: Vec<ExpectedEdge>,
}

#[test]
fn corpus_service_discovery_matches_expected() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let corpus = repo_root.join("corpus");
    if !corpus.is_dir() {
        eprintln!("corpus/ missing: run scripts/corpus.sh; skipping");
        return;
    }
    let mut failures = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(repo_root.join("test/expected/corpus"))
        .unwrap()
        .flatten()
        .collect();
    entries.sort_by_key(std::fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        let name = path.file_stem().unwrap().to_str().unwrap().to_string();
        let repo = corpus.join(&name);
        if !repo.is_dir() {
            eprintln!("corpus/{name} missing; skipping");
            continue;
        }
        let expected: Expected = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        failures.extend(check_repo(&name, &repo, &expected));
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// One repository: discovery counts, edge recall, timing. Returns the
/// failures found instead of asserting, so every repository is reported.
fn check_repo(name: &str, repo: &Path, expected: &Expected) -> Vec<String> {
    let mut failures = Vec::new();
    let started = Instant::now();
    let analysis = blastradius::analyze(repo).unwrap();
    let elapsed = started.elapsed();
    let json = analysis.to_json();
    let mut code: Vec<String> = json
        .services
        .iter()
        .filter(|s| s.role == ServiceRole::Code)
        .map(|s| s.name.clone())
        .collect();
    code.sort();
    let mut infra: Vec<String> = json
        .services
        .iter()
        .filter(|s| s.role == ServiceRole::Infrastructure)
        .map(|s| s.name.clone())
        .collect();
    infra.sort();
    let strategy = json.discovery.strategy.map(|s| s.as_str().to_string());
    let by = |c: blastradius::Confidence| json.edges.iter().filter(|e| e.confidence == c).count();
    eprintln!(
        "{name:<32} {:>4} code {:>4} infra {:>4} edges ({}s/{}i/{}u) {:>4} unresolved {:>7.1} ms  {}",
        code.len(),
        infra.len(),
        json.edges.len(),
        by(blastradius::Confidence::Static),
        by(blastradius::Confidence::Inferred),
        by(blastradius::Confidence::Uncertain),
        json.mapping.unresolved,
        elapsed.as_secs_f64() * 1000.0,
        strategy.as_deref().unwrap_or("-")
    );
    if std::env::var_os("CORPUS_VERBOSE").is_some() {
        for e in &json.edges {
            let first = e.evidence.first();
            eprintln!(
                "  + {} -> {} {} {}  {}:{}  {}",
                e.source,
                e.target,
                e.edge_type.as_str(),
                e.confidence.as_str(),
                first.map_or("", |v| v.file.as_str()),
                first.and_then(|v| v.line).unwrap_or(0),
                first.and_then(|v| v.detail.as_deref()).unwrap_or("")
            );
        }
        eprintln!(
            "  unresolved: {}",
            json.mapping.unresolved_targets.join(", ")
        );
    }
    for want in &expected.edges {
        let found = json.edges.iter().any(|e| {
            e.source == want.source
                && e.target == want.target
                && e.edge_type.as_str() == want.edge_type
        });
        if !found {
            let from_source: Vec<String> = json
                .edges
                .iter()
                .filter(|e| e.source == want.source)
                .map(|e| format!("{} {}", e.target, e.edge_type.as_str()))
                .collect();
            failures.push(format!(
                "{name}: missing edge {} -> {} {} (edges from {}: {})",
                want.source,
                want.target,
                want.edge_type,
                want.source,
                if from_source.is_empty() {
                    "none".to_string()
                } else {
                    from_source.join(", ")
                }
            ));
        }
    }
    if strategy != expected.strategy || code != expected.code || infra != expected.infrastructure {
        failures.push(format!(
            "{name}: strategy {strategy:?} vs {:?}\n  code   {code:?}\n  expect {:?}\n  infra  {infra:?}\n  expect {:?}",
            expected.strategy, expected.code, expected.infrastructure
        ));
    }
    assert!(elapsed.as_millis() < 500, "{name} took {elapsed:?}");
    failures
}
