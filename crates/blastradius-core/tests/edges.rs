mod common;

use std::collections::BTreeSet;

use blastradius::map::facts::Parser;
use blastradius::*;
use common::fixture;
use pretty_assertions::assert_eq;

fn edges_of(name: &str) -> (Vec<Edge>, AnalysisJson) {
    let json = analyze(&fixture(name)).unwrap().to_json();
    (json.edges.clone(), json)
}

fn triple(e: &Edge) -> (String, String, EdgeType) {
    (e.source.clone(), e.target.clone(), e.edge_type)
}

fn find<'a>(edges: &'a [Edge], s: &str, t: &str, ty: EdgeType) -> &'a Edge {
    edges
        .iter()
        .find(|e| e.source == s && e.target == t && e.edge_type == ty)
        .unwrap_or_else(|| {
            panic!(
                "no edge {s} -> {t} {ty:?} in {:?}",
                edges.iter().map(triple).collect::<Vec<_>>()
            )
        })
}

#[test]
fn http_edges_from_literals_env_templates_and_config_files() {
    let (edges, json) = edges_of("edges-http-app");
    let set: BTreeSet<(String, String, EdgeType)> = edges.iter().map(triple).collect();
    let expect = |s: &str, t: &str, ty: EdgeType| {
        assert!(
            set.contains(&(s.into(), t.into(), ty)),
            "missing {s} -> {t} {ty:?}; have {set:?}"
        );
    };
    expect("web", "catalogue", EdgeType::Http); // nginx template ${CATALOGUE_HOST} through compose environment
    expect("web", "cart", EdgeType::Http); // ${CART_HOST} unresolved, matched by name
    expect("web", "ratings", EdgeType::Http);
    expect("cart", "catalogue", EdgeType::Http); // process.env default + template
    expect("cart", "redis", EdgeType::Database); // REDIS_HOST via compose -> infrastructure redis
    expect("catalogue", "mongodb", EdgeType::Database);
    expect("payment", "cart", EdgeType::Http);
    expect("payment", "rabbitmq", EdgeType::Event); // AMQP_HOST default "rabbitmq"
    expect("ratings", "catalogue", EdgeType::Http);

    let e = find(&edges, "web", "catalogue", EdgeType::Http);
    assert_eq!(e.confidence, Confidence::Static);
    assert_eq!(e.evidence[0].file, "web/default.conf.template");
    assert_eq!(e.evidence[0].line, Some(1));
    assert!(
        e.evidence[0]
            .detail
            .as_deref()
            .unwrap()
            .contains("CATALOGUE_HOST=catalogue via docker-compose.yml:5"),
        "{:?}",
        e.evidence
    );
    assert_eq!(
        find(&edges, "web", "cart", EdgeType::Http).confidence,
        Confidence::Uncertain
    );
    assert_eq!(
        find(&edges, "cart", "catalogue", EdgeType::Http).confidence,
        Confidence::Static
    );
    assert_eq!(
        find(&edges, "catalogue", "mongodb", EdgeType::Database).evidence[0]
            .detail
            .as_deref(),
        Some("mongodb://mongodb:27017/catalogue")
    );

    assert!(
        !edges
            .iter()
            .any(|e| e.target == "paypal.com" || e.source == e.target)
    );
    assert!(
        json.mapping
            .unresolved_targets
            .contains(&"USER_HOST".to_string()),
        "{:?}",
        json.mapping.unresolved_targets
    );
    assert!(
        json.mapping.files_scanned >= 5,
        "{}",
        json.mapping.files_scanned
    );
    assert_eq!(
        json.mapping.parsers.get("template").copied(),
        Some(Parser::Regex)
    );
    assert_eq!(
        json.mapping.parsers.get("go").copied(),
        Some(Parser::TreeSitter)
    );
    assert!(
        json.mapping.files_outside_services >= 1,
        "docker-compose.yml and .env are outside every root"
    );
}

#[test]
fn grpc_edges_from_stubs_and_proto_ownership() {
    let (edges, json) = edges_of("edges-grpc-app");
    let e = find(&edges, "frontend", "cartservice", EdgeType::Grpc);
    assert_eq!(e.confidence, Confidence::Static);
    assert!(
        e.evidence
            .iter()
            .any(|v| v.detail.as_deref() == Some("proto service CartService")),
        "{:?}",
        e.evidence
    );
    assert!(
        e.evidence.iter().any(|v| v
            .detail
            .as_deref()
            .unwrap()
            .starts_with("CART_SERVICE_ADDR=cartservice:7070")),
        "the _ADDR variable merges into the grpc edge: {:?}",
        e.evidence
    );
    find(&edges, "frontend", "shippingservice", EdgeType::Grpc);
    find(&edges, "checkoutservice", "emailservice", EdgeType::Grpc);
    assert!(
        !edges.iter().any(|e| e.edge_type == EdgeType::Http
            && e.source == "frontend"
            && e.target == "cartservice"),
        "no duplicate http edge for the same pair: {:?}",
        edges.iter().map(triple).collect::<Vec<_>>()
    );
    assert_eq!(
        json.mapping.unresolved, 0,
        "{:?}",
        json.mapping.unresolved_targets
    );
}

#[test]
fn json_contract_gains_mapping_between_edges_and_runtime() {
    let json =
        serde_json::to_value(analyze(&fixture("edges-http-app")).unwrap().to_json()).unwrap();
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
            "mapping",
            "runtime"
        ]
    );
    let m = &json["mapping"];
    for k in [
        "filesScanned",
        "filesSkipped",
        "filesOutsideServices",
        "unresolved",
        "unresolvedTargets",
        "parsers",
    ] {
        assert!(m.get(k).is_some(), "{k}");
    }
    let first = &json["edges"][0];
    assert_eq!(
        first
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["source", "target", "type", "confidence", "evidence"]
    );
    assert!(json["edges"][0]["evidence"][0]["detail"].is_string());
}

#[test]
fn discovery_only_fixtures_still_have_no_edges_and_report_skips() {
    let json = analyze(&fixture("compose-app")).unwrap().to_json();
    assert!(json.edges.is_empty(), "{:?}", json.edges);
    assert!(json.mapping.files_scanned >= 1);
}
