//! Every change runs against the whole corpus. A regression on one repo is
//! a regression on the product. Skips with a message when corpus/ is absent.
use std::fs;
use std::path::Path;
use std::time::Instant;

use blastradius::ServiceRole;
use serde::Deserialize;

#[derive(Deserialize)]
struct Expected {
    strategy: Option<String>,
    code: Vec<String>,
    infrastructure: Vec<String>,
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
        let started = Instant::now();
        let analysis = blastradius::analyze(&repo).unwrap();
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
        eprintln!(
            "{name:<32} {:>4} code {:>4} infra  {:>7.1} ms  {}",
            code.len(),
            infra.len(),
            elapsed.as_secs_f64() * 1000.0,
            strategy.as_deref().unwrap_or("-")
        );
        if strategy != expected.strategy
            || code != expected.code
            || infra != expected.infrastructure
        {
            failures.push(format!(
                "{name}: strategy {strategy:?} vs {:?}\n  code   {code:?}\n  expect {:?}\n  infra  {infra:?}\n  expect {:?}",
                expected.strategy, expected.code, expected.infrastructure
            ));
        }
        assert!(elapsed.as_millis() < 500, "{name} took {elapsed:?}");
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
