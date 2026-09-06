mod common;

use blastradius::*;
use common::fixture;
use pretty_assertions::assert_eq;

fn service(name: &str) -> Service {
    Service {
        name: name.into(),
        root: Some(name.into()),
        language: Some("go".into()),
        entry_points: vec![],
        role: ServiceRole::Code,
        discovered_by: ServiceSource::Strategy(DiscoveryStrategy::Monorepo),
        evidence: Evidence {
            file: format!("{name}/go.mod"),
            line: None,
            detail: None,
        },
        image: None,
        package_name: None,
    }
}

fn edge(s: &str, t: &str, ty: EdgeType, c: Confidence) -> Edge {
    Edge {
        source: s.into(),
        target: t.into(),
        edge_type: ty,
        confidence: c,
        evidence: vec![],
    }
}

// ------------------------------------------------------------------ graph

#[test]
fn graph_stores_services_and_answers_inbound() {
    let mut g = BlastGraph::new();
    for n in ["checkout", "orders", "payments"] {
        g.add_service(service(n));
    }
    g.add_edge(edge(
        "checkout",
        "orders",
        EdgeType::Http,
        Confidence::Static,
    ))
    .unwrap();
    g.add_edge(edge(
        "payments",
        "orders",
        EdgeType::Event,
        Confidence::Inferred,
    ))
    .unwrap();
    assert_eq!(g.size(), (3, 2));
    let mut inbound: Vec<&str> = g
        .inbound("orders")
        .iter()
        .map(|e| e.source.as_str())
        .collect();
    inbound.sort_unstable();
    assert_eq!(inbound, vec!["checkout", "payments"]);
    assert!(g.outbound("orders").is_empty());
    assert_eq!(g.outbound("checkout").len(), 1);
    assert_eq!(
        g.services()
            .iter()
            .map(|s| s.name.clone())
            .collect::<Vec<_>>(),
        vec!["checkout", "orders", "payments"]
    );
    assert!(g.has_service("orders") && !g.has_service("ghost"));
    assert_eq!(g.service("orders").map(|s| s.name.as_str()), Some("orders"));
}

#[test]
fn graph_keeps_parallel_edges_and_refuses_unknown_services() {
    let mut g = BlastGraph::new();
    g.add_service(service("a"));
    g.add_service(service("b"));
    g.add_edge(edge("a", "b", EdgeType::Http, Confidence::Static))
        .unwrap();
    g.add_edge(edge("a", "b", EdgeType::Import, Confidence::Static))
        .unwrap();
    assert_eq!(g.edges().len(), 2);
    let err = g
        .add_edge(edge("a", "ghost", EdgeType::Http, Confidence::Static))
        .unwrap_err();
    assert!(
        err.to_string().contains("Unknown service \"ghost\""),
        "{err}"
    );
    assert_eq!(g.edges().len(), 2);
}

#[test]
fn graph_replaces_a_service_by_name() {
    let mut g = BlastGraph::new();
    g.add_service(service("a"));
    let mut again = service("a");
    again.language = Some("rust".into());
    g.add_service(again);
    assert_eq!(g.size(), (1, 0));
    assert_eq!(g.service("a").unwrap().language.as_deref(), Some("rust"));
}

// ---------------------------------------------------------------- analyze

#[test]
fn analyze_builds_a_graph_of_discovered_services() {
    let a = analyze(&fixture("compose-app")).unwrap();
    assert!(!a.repository.is_empty());
    assert_eq!(a.discovery.strategy, Some(DiscoveryStrategy::DockerCompose));
    assert_eq!(a.graph.size(), (3, 0));
    assert!(!a.runtime.connected);
    assert!(a.root.is_absolute());
}

#[test]
fn analyze_serialises_to_the_contract() {
    let json = serde_json::to_value(analyze(&fixture("monorepo-app")).unwrap().to_json()).unwrap();
    assert_eq!(
        json["services"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["api", "web", "worker"]
    );
    assert_eq!(json["edges"], serde_json::json!([]));
    assert_eq!(json["runtime"], serde_json::json!({ "connected": false }));
    assert_eq!(json["discovery"]["strategy"], "monorepo");
    assert_eq!(
        json["discovery"]["attempted"][0],
        serde_json::json!({ "strategy": "docker-compose", "services": 0 })
    );
    let keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        vec![
            "repository",
            "root",
            "discovery",
            "services",
            "edges",
            "runtime"
        ]
    );
    assert!(json["root"].as_str().unwrap().ends_with("monorepo-app"));
}

#[test]
fn analyze_rejects_a_file_path() {
    let err = analyze(&fixture("compose-app").join("docker-compose.yml")).unwrap_err();
    assert!(err.to_string().contains("not a directory"), "{err}");
}

#[test]
fn repository_name_falls_back_to_the_directory_name() {
    // Fixtures are inside this repository's git tree but the function is
    // asked about the directory itself; either the remote's owner/repo or
    // the basename is acceptable, never empty.
    let name = repository_name(&fixture("compose-app"));
    assert!(!name.is_empty());
}

// ----------------------------------------------------------------- report

#[test]
fn report_shows_findings_and_says_what_is_not_built() {
    let r = format_repo_report(&analyze(&fixture("compose-app")).unwrap(), false);
    assert!(r.contains("BLAST RADIUS"));
    assert!(r.contains("2 detected  (docker-compose)"));
    assert!(r.contains("not connected - static only"));
    let re =
        regex::Regex::new(r"checkout\s+typescript\s+services/checkout\s+docker-compose\.yml:2")
            .unwrap();
    assert!(re.is_match(&r), "{r}");
    assert!(r.contains("1 declared but not built here (images): redis"));
    assert!(r.contains("Dependency mapping is not built yet"));
    assert!(!r.contains('\u{1b}'));
}

#[test]
fn report_is_honest_about_single_and_deploy_only() {
    let r = format_repo_report(&analyze(&fixture("single-app")).unwrap(), false);
    assert!(r.contains("This looks like a single service."), "{r}");
    assert!(
        r.contains("docker-compose 0") && r.contains("workspace 0"),
        "{r}"
    );
    let r = format_repo_report(&analyze(&fixture("deploy-only-app")).unwrap(), false);
    assert!(
        r.contains("declares 3 services but builds none of them here"),
        "{r}"
    );
    assert!(
        r.contains("3 declared but not built here (images): carts, carts-db, front-end"),
        "{r}"
    );
}

#[test]
fn report_colours_only_when_asked() {
    assert!(
        format_repo_report(&analyze(&fixture("compose-app")).unwrap(), true).contains('\u{1b}')
    );
}
