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

// ----------------------------------------------------------------- report

#[test]
fn report_shows_structure_edges_and_findings() {
    let r = format_repo_report(&analyze(&fixture("edges-http-app")).unwrap(), false);
    assert!(r.contains("STRUCTURE"), "{r}");
    assert!(
        regex::Regex::new(r"Total edges\s+\d+")
            .unwrap()
            .is_match(&r),
        "{r}"
    );
    assert!(r.contains("Static") && r.contains("Uncertain"), "{r}");
    assert!(r.contains("EDGES"), "{r}");
    assert!(
        regex::Regex::new(r"web\s+->\s+catalogue\s+http\s+static\s+web/default\.conf\.template:1")
            .unwrap()
            .is_match(&r),
        "{r}"
    );
    assert!(r.contains("FINDINGS"), "{r}");
    assert!(r.contains("Never called by another service"), "{r}");
    assert!(r.contains("Most connected"), "{r}");
    assert!(
        regex::Regex::new(r"Scanned \d+ files in \d+ services")
            .unwrap()
            .is_match(&r),
        "{r}"
    );
    assert!(
        r.contains("could not be matched to a service:") && r.contains("USER_HOST"),
        "{r}"
    );
    assert!(!r.contains("Dependency mapping is not built yet"));
}

#[test]
fn report_without_edges_says_none_were_found() {
    let r = format_repo_report(&analyze(&fixture("compose-app")).unwrap(), false);
    assert!(
        regex::Regex::new(r"Total edges\s+0").unwrap().is_match(&r),
        "{r}"
    );
    assert!(r.contains("No static edges found"), "{r}");
    assert!(!r.contains("EDGES") && !r.contains("FINDINGS"), "{r}");
}

#[test]
fn report_lists_shared_databases() {
    let r = format_repo_report(&analyze(&fixture("edges-db-app")).unwrap(), false);
    assert!(
        regex::Regex::new(r"Shared databases\s+2")
            .unwrap()
            .is_match(&r),
        "{r}"
    );
    assert!(
        regex::Regex::new(r"ledgerdb\s+audit, ledger")
            .unwrap()
            .is_match(&r),
        "{r}"
    );
    assert!(
        regex::Regex::new(r"mysql/shop\s+orders, reports")
            .unwrap()
            .is_match(&r),
        "{r}"
    );
    assert!(
        !r.contains("localhost/ledgerdb"),
        "one key per pair, the named resource wins: {r}"
    );
}

#[test]
fn report_without_shared_databases_says_so_and_names_every_edge_type() {
    let r = format_repo_report(&analyze(&fixture("edges-http-app")).unwrap(), false);
    assert!(
        regex::Regex::new(r"Shared databases\s+0")
            .unwrap()
            .is_match(&r),
        "{r}"
    );
    let r = format_repo_report(&analyze(&fixture("compose-app")).unwrap(), false);
    assert!(
        r.contains("No static edges found: no HTTP, gRPC, event, database or import edge to another discovered service was recognised."),
        "{r}"
    );
}

#[test]
fn database_edges_from_settings_connection_strings_and_dotenv() {
    let (edges, json) = edges_of("edges-db-app");
    let e = find(&edges, "inventory", "mongodb", EdgeType::Database);
    assert_eq!(e.confidence, Confidence::Static);
    assert_eq!(
        e.evidence[0].detail.as_deref(),
        Some("spring.data.mongodb.host=mongodb")
    );
    let e = find(&edges, "cart", "valkey-cart", EdgeType::Database);
    assert_eq!(e.confidence, Confidence::Static);
    assert!(
        e.evidence[0]
            .detail
            .as_deref()
            .unwrap()
            .starts_with("VALKEY_ADDR=valkey-cart:6379 via docker-compose.yml:"),
        "{:?}",
        e.evidence
    );
    let e = find(&edges, "reports", "postgres", EdgeType::Database);
    assert!(
        e.evidence[0]
            .detail
            .as_deref()
            .unwrap()
            .starts_with("DB_CONNECTION_STRING=postgres://app:secret@postgres/shop"),
        "{:?}",
        e.evidence
    );
    find(&edges, "reports", "mysql", EdgeType::Database);
    find(&edges, "orders", "mysql", EdgeType::Database);
    find(&edges, "catalogue", "mongodb", EdgeType::Database);
    find(&edges, "user", "mongodb", EdgeType::Database);
    assert!(
        !edges.iter().any(|e| e.target == "localhost"),
        "{:?}",
        edges.iter().map(triple).collect::<Vec<_>>()
    );
    assert_eq!(
        json.mapping.unresolved_targets,
        Vec::<String>::new(),
        "{:?}",
        json.mapping.unresolved_targets
    );
}

#[test]
fn shared_databases_join_services_in_both_directions() {
    let (edges, _) = edges_of("edges-db-app");
    for (s, t, key) in [
        ("orders", "reports", "mysql/shop"),
        ("reports", "orders", "mysql/shop"),
        ("ledger", "audit", "ledgerdb"),
        ("audit", "ledger", "ledgerdb"),
    ] {
        let e = find(&edges, s, t, EdgeType::Database);
        assert_eq!(e.confidence, Confidence::Inferred, "{s} -> {t}");
        assert!(
            e.evidence
                .iter()
                .any(|v| v.detail.as_deref() == Some(&format!("shared database {key} with {t}"))),
            "{s} -> {t}: {:?}",
            e.evidence
        );
    }
    let e = find(&edges, "ledger", "audit", EdgeType::Database);
    assert!(
        e.evidence
            .iter()
            .any(|v| v.detail.as_deref() == Some("shared database localhost/ledgerdb with audit")),
        "the development connection strings share the same key too: {:?}",
        e.evidence
    );
    assert!(
        !edges.iter().any(|e| e.edge_type == EdgeType::Database
            && e.source == "catalogue"
            && e.target == "user"),
        "same host, different databases: not shared"
    );
    assert!(
        !edges.iter().any(|e| e.edge_type == EdgeType::Database
            && e.source == "reports"
            && e.target == "ledger"),
        "postgres host alone is not a key"
    );
}

#[test]
fn event_edges_join_producers_to_consumers_and_brokers_to_libraries() {
    let (edges, json) = edges_of("edges-events-app");
    let e = find(&edges, "payment", "dispatch", EdgeType::Event);
    assert_eq!(e.confidence, Confidence::Inferred);
    let files: Vec<&str> = e.evidence.iter().map(|v| v.file.as_str()).collect();
    assert!(
        files.contains(&"payment/rabbitmq.py") && files.contains(&"dispatch/main.go"),
        "{:?}",
        e.evidence
    );
    assert!(
        e.evidence
            .iter()
            .any(|v| v.detail.as_deref() == Some("publishes \"orders\", consumed by dispatch")),
        "{:?}",
        e.evidence
    );
    assert!(
        e.evidence
            .iter()
            .any(|v| v.detail.as_deref() == Some("consumes \"orders\", published by payment")),
        "{:?}",
        e.evidence
    );
    find(&edges, "payment", "notifications", EdgeType::Event); // 'email' through Queues.queueName
    find(&edges, "checkout", "accounting", EdgeType::Event); // kafkajs object literal -> Confluent Subscribe(TopicName)
    find(&edges, "checkout", "notifications", EdgeType::Event); // @KafkaListener
    find(&edges, "accounting", "webhooks", EdgeType::Event); // new OrderPaidIntegrationEvent -> AddSubscription<...>
    for (s, t) in [
        ("payment", "rabbitmq"),
        ("dispatch", "rabbitmq"),
        ("notifications", "rabbitmq"),
        ("checkout", "kafka"),
        ("accounting", "kafka"),
        ("notifications", "kafka"),
    ] {
        let e = find(&edges, s, t, EdgeType::Event);
        assert_eq!(e.confidence, Confidence::Inferred, "{s} -> {t}");
        assert!(
            e.evidence[0]
                .detail
                .as_deref()
                .unwrap()
                .starts_with("imports "),
            "{:?}",
            e.evidence
        );
    }
    assert!(!edges.iter().any(|e| e.source == e.target));
    assert!(
        !edges
            .iter()
            .any(|e| e.source == "dispatch" && e.target == "payment"),
        "no reverse edge"
    );
    assert!(
        json.mapping
            .unresolved_targets
            .contains(&"topic:audit-log".to_string()),
        "{:?}",
        json.mapping.unresolved_targets
    );
    assert!(
        !json
            .mapping
            .unresolved_targets
            .iter()
            .any(|t| t == "topic:ok"),
        "res.send is not a producer"
    );
}

#[test]
fn import_edges_from_imports_dependencies_and_project_references() {
    let (edges, json) = edges_of("edges-import-app");
    let e = find(&edges, "web", "shared", EdgeType::Import);
    assert_eq!(e.confidence, Confidence::Static);
    let details: Vec<&str> = e
        .evidence
        .iter()
        .filter_map(|v| v.detail.as_deref())
        .collect();
    assert!(
        details.contains(&"import @acme/shared/utils"),
        "{details:?}"
    );
    assert!(details.contains(&"dependency @acme/shared"), "{details:?}");
    assert_eq!(
        find(&edges, "checkout", "cart", EdgeType::Import).evidence[0]
            .detail
            .as_deref(),
        Some("import github.com/acme/demo/services/cart/genproto")
    );
    assert_eq!(
        find(&edges, "basket-api", "eventbus", EdgeType::Import).evidence[0]
            .detail
            .as_deref(),
        Some("ProjectReference ..\\eventbus\\EventBus.csproj")
    );
    assert_eq!(
        find(&edges, "orders", "common", EdgeType::Import).evidence[0]
            .detail
            .as_deref(),
        Some("artifactId common")
    );
    assert_eq!(
        find(&edges, "indexer", "core-rs", EdgeType::Import).evidence[0]
            .detail
            .as_deref(),
        Some("path ../core-rs")
    );
    assert_eq!(
        find(&edges, "worker", "shared_py", EdgeType::Import).evidence[0]
            .detail
            .as_deref(),
        Some("import shared_py.tasks")
    );
    assert!(
        edges.iter().all(|e| e.edge_type == EdgeType::Import),
        "{:?}",
        edges.iter().map(triple).collect::<Vec<_>>()
    );
    assert_eq!(
        edges.len(),
        6,
        "{:?}",
        edges.iter().map(triple).collect::<Vec<_>>()
    );
    assert_eq!(
        json.mapping.unresolved, 0,
        "external libraries are not unresolved targets: {:?}",
        json.mapping.unresolved_targets
    );
}
