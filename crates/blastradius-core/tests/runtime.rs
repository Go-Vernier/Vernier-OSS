mod common;

use blastradius::runtime::{self, RuntimeGraph};
use blastradius::*;
use common::fixture;
use pretty_assertions::assert_eq;

fn joined(file: &str) -> Analysis {
    let root = fixture("runtime-app");
    let mut analysis = analyze(&root).unwrap();
    let path = root.join("runtime").join(file);
    let text = std::fs::read_to_string(&path).unwrap();
    let graph: RuntimeGraph = match file {
        "traces.prom" => runtime::prometheus::parse(&text, file).unwrap(),
        "spans.json" => runtime::otlp::parse(&text, file).unwrap(),
        _ => runtime::datadog::parse(&text, file).unwrap(),
    };
    let config = config::load(&root).unwrap().runtime;
    runtime::join(&mut analysis, graph, &config).unwrap();
    analysis
}

fn edge<'a>(a: &'a Analysis, s: &str, t: &str, ty: EdgeType) -> &'a Edge {
    a.graph
        .edges()
        .iter()
        .find(|e| e.source == s && e.target == t && e.edge_type == ty)
        .unwrap_or_else(|| {
            panic!(
                "no edge {s} -> {t} {ty:?}: {:?}",
                a.graph
                    .edges()
                    .iter()
                    .map(|e| (&e.source, &e.target, e.edge_type))
                    .collect::<Vec<_>>()
            )
        })
}

#[test]
#[allow(clippy::too_many_lines)]
fn servicegraph_join_confirms_adds_and_skips_by_the_rules() {
    let a = joined("traces.prom");
    let r = &a.runtime;
    assert!(r.connected);
    assert_eq!(
        (r.source, r.input.as_deref()),
        (Some(RuntimeSource::Otel), Some("traces.prom"))
    );
    assert_eq!(
        r.services,
        Some(RuntimeServices {
            runtime: 10,
            matched: 8
        })
    );
    let hows: Vec<(&str, Option<&str>, &str)> = r
        .mapping
        .iter()
        .map(|m| (m.runtime.as_str(), m.service.as_deref(), m.how.as_str()))
        .collect();
    assert_eq!(
        hows,
        vec![
            ("auth-proxy", None, "unmatched"),
            ("catalogue-service", Some("catalogue"), "normalised"),
            ("chckout", Some("checkout"), "fuzzy 0.97"),
            ("checkout-api", Some("checkout"), "normalised"),
            ("load-generator", None, "ignored"),
            ("notifications", Some("notifications"), "exact"),
            ("orders", Some("orders"), "exact"),
            ("pay", Some("payment"), "config"),
            ("payment", Some("payment"), "exact"),
            ("redis", Some("redis"), "exact"),
        ]
    );
    assert_eq!(r.unmatched, vec!["auth-proxy"]);
    assert_eq!(
        r.edges,
        Some(RuntimeEdges {
            observed: 4,
            runtime_only: 1,
            skipped: 2
        })
    );
    assert_eq!(r.warnings, vec!["fuzzy match: chckout -> checkout (0.97)"]);

    // static + runtime -> Observed with the summed count (120 + 5 + 7 through chckout)
    let e = edge(&a, "checkout", "payment", EdgeType::Http);
    assert_eq!(e.confidence, Confidence::Observed);
    assert_eq!(
        e.observed,
        Some(Observed {
            calls: Some(132),
            source: RuntimeSource::Otel
        })
    );
    assert!(
        e.evidence.iter().any(|v| v.file == "traces.prom"
            && v.line.is_none()
            && v.detail.as_deref() == Some("132 calls (otel servicegraph)")),
        "{:?}",
        e.evidence
    );
    assert!(
        e.evidence.iter().any(|v| v.file == "checkout/server.js"),
        "static evidence kept: {:?}",
        e.evidence
    );
    assert_eq!(
        e.evidence[0].detail.as_deref(),
        Some("132 calls (otel servicegraph)"),
        "the runtime entry leads, so the EDGES table shows the call count: {:?}",
        e.evidence
    );
    assert_eq!(
        edge(&a, "checkout", "catalogue", EdgeType::Http)
            .observed
            .as_ref()
            .and_then(|o| o.calls),
        Some(300)
    );
    // config-mapped client confirms the database edge
    assert_eq!(
        edge(&a, "payment", "redis", EdgeType::Database)
            .observed
            .as_ref()
            .and_then(|o| o.calls),
        Some(900)
    );
    // runtime only -> new Observed event edge
    let e = edge(&a, "orders", "notifications", EdgeType::Event);
    assert_eq!(
        (e.confidence, e.observed.as_ref().and_then(|o| o.calls)),
        (Confidence::Observed, Some(42))
    );
    assert_eq!(e.evidence.len(), 1);
    // static only stays as it was
    let e = edge(&a, "orders", "rabbitmq", EdgeType::Event);
    assert_eq!(
        (e.confidence, e.observed.is_none()),
        (Confidence::Static, true)
    );
    // no self edge, nothing to load-generator or auth-proxy
    assert!(
        !a.graph.edges().iter().any(|e| e.source == e.target
            || e.source == "load-generator"
            || e.source == "auth-proxy")
    );
    // edges stay sorted
    let keys: Vec<(String, String, EdgeType)> = a
        .graph
        .edges()
        .iter()
        .map(|e| (e.source.clone(), e.target.clone(), e.edge_type))
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

#[test]
fn span_and_datadog_joins_apply_the_same_rules() {
    let a = joined("spans.json");
    assert_eq!(
        edge(&a, "checkout", "payment", EdgeType::Http).observed,
        Some(Observed {
            calls: Some(2),
            source: RuntimeSource::Otel
        })
    );
    assert_eq!(
        edge(&a, "orders", "notifications", EdgeType::Event).confidence,
        Confidence::Observed
    );
    assert_eq!(
        a.runtime.edges,
        Some(RuntimeEdges {
            observed: 3,
            runtime_only: 1,
            skipped: 0
        })
    );

    let a = joined("datadog.json");
    let e = edge(&a, "checkout", "payment", EdgeType::Http);
    assert_eq!(
        e.observed,
        Some(Observed {
            calls: None,
            source: RuntimeSource::Datadog
        })
    );
    assert!(
        e.evidence
            .iter()
            .any(|v| v.detail.as_deref() == Some("observed (datadog service_dependencies)")),
        "{:?}",
        e.evidence
    );
    // Datadog gives no protocol: a runtime-only call becomes an http edge
    assert_eq!(
        edge(&a, "orders", "notifications", EdgeType::Http).confidence,
        Confidence::Observed
    );
    assert_eq!(
        a.runtime.edges,
        Some(RuntimeEdges {
            observed: 3,
            runtime_only: 1,
            skipped: 1
        })
    );
    assert_eq!(
        a.runtime.services,
        Some(RuntimeServices {
            runtime: 6,
            matched: 5
        })
    );
}

#[test]
fn json_contract_with_a_runtime_source() {
    let json = serde_json::to_value(joined("traces.prom").to_json()).unwrap();
    let keys: Vec<&str> = json["runtime"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        vec![
            "connected",
            "source",
            "input",
            "services",
            "mapping",
            "unmatched",
            "edges",
            "warnings"
        ]
    );
    let observed: Vec<&serde_json::Value> = json["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e.get("observed").is_some())
        .collect();
    assert_eq!(observed.len(), 4);
    assert_eq!(observed[0]["observed"]["source"], "otel");
    assert!(
        json["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e.get("observed").is_none()),
        "the static-only edge has no observed key"
    );
}

#[test]
fn report_with_a_runtime_source_shows_the_join() {
    let r = format_repo_report(&joined("traces.prom"), false);
    assert!(
        r.contains("connected (OTel, 8 of 10 runtime services matched)"),
        "{r}"
    );
    assert!(r.contains("\nRUNTIME\n"), "{r}");
    assert!(
        regex::Regex::new(r"Source\s+OTel\s+traces\.prom")
            .unwrap()
            .is_match(&r),
        "{r}"
    );
    assert!(
        regex::Regex::new(
            r"Services\s+8 of 10 runtime services matched \(1 ignored by vernier\.config\.json\)"
        )
        .unwrap()
        .is_match(&r),
        "{r}"
    );
    assert!(
        regex::Regex::new(r"checkout\s+->\s+payment\s+http\s+observed\s+traces\.prom\s+132 calls")
            .unwrap()
            .is_match(&r),
        "{r}"
    );
    assert!(
        regex::Regex::new(
            r"Edges\s+4 observed \(3 static confirmed, 1 runtime only\) · 2 calls skipped"
        )
        .unwrap()
        .is_match(&r),
        "{r}"
    );
    assert!(
        regex::Regex::new(r"chckout\s+checkout\s+fuzzy 0\.97\s+\(check this\)")
            .unwrap()
            .is_match(&r),
        "{r}"
    );
    assert!(
        regex::Regex::new(r"load-generator\s+-\s+ignored \(vernier\.config\.json\)")
            .unwrap()
            .is_match(&r),
        "{r}"
    );
    assert!(
        regex::Regex::new(r"pay\s+payment\s+config")
            .unwrap()
            .is_match(&r),
        "{r}"
    );
    assert!(
        r.contains("1 runtime service matched nothing: auth-proxy"),
        "{r}"
    );
    assert!(
        regex::Regex::new(r"checkout\s+->\s+payment\s+http\s+observed")
            .unwrap()
            .is_match(&r),
        "{r}"
    );
    assert!(
        regex::Regex::new(r"Static edges never observed\s+1")
            .unwrap()
            .is_match(&r),
        "{r}"
    );
    assert!(r.contains("orders -> rabbitmq"), "{r}");
}

#[test]
fn report_without_a_runtime_source_is_unchanged() {
    let r = format_repo_report(&analyze(&fixture("runtime-app")).unwrap(), false);
    assert!(r.contains("not connected - static only"), "{r}");
    assert!(
        !r.contains("RUNTIME\n") && !r.contains("never observed"),
        "{r}"
    );
}

fn code_service(name: &str) -> Service {
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

#[test]
#[allow(clippy::too_many_lines)]
fn never_observed_leaves_out_shared_databases_and_imports_and_ignores_do_not_warn() {
    let mut graph = BlastGraph::new();
    for n in ["orders", "reports", "web"] {
        graph.add_service(code_service(n));
    }
    let mut edge = |s: &str, t: &str, ty: EdgeType, c: Confidence, detail: &str| {
        graph
            .add_edge(Edge {
                source: s.into(),
                target: t.into(),
                edge_type: ty,
                confidence: c,
                evidence: vec![Evidence {
                    file: format!("{s}/x"),
                    line: Some(1),
                    detail: Some(detail.into()),
                }],
                observed: None,
            })
            .unwrap();
    };
    edge(
        "orders",
        "reports",
        EdgeType::Database,
        Confidence::Inferred,
        "shared database mysql/shop with reports",
    );
    edge(
        "reports",
        "orders",
        EdgeType::Database,
        Confidence::Inferred,
        "shared database mysql/shop with orders",
    );
    edge(
        "web",
        "orders",
        EdgeType::Import,
        Confidence::Static,
        "import orders",
    );
    edge(
        "web",
        "reports",
        EdgeType::Http,
        Confidence::Static,
        "http://reports:8080",
    );
    let analysis = Analysis {
        repository: "acme/shop".into(),
        root: std::path::PathBuf::from("/tmp/acme"),
        discovery: Discovery {
            strategy: Some(DiscoveryStrategy::Monorepo),
            attempted: vec![],
        },
        graph,
        mapping: MappingStats::default(),
        runtime: Runtime {
            connected: true,
            source: Some(RuntimeSource::Otel),
            input: Some("traces.prom".into()),
            services: Some(RuntimeServices {
                runtime: 3,
                matched: 2,
            }),
            mapping: vec![
                RuntimeMapping {
                    runtime: "orders".into(),
                    service: Some("orders".into()),
                    how: "exact".into(),
                },
                RuntimeMapping {
                    runtime: "reports".into(),
                    service: Some("reports".into()),
                    how: "exact".into(),
                },
                RuntimeMapping {
                    runtime: "load-generator".into(),
                    service: None,
                    how: "ignored".into(),
                },
            ],
            unmatched: vec![],
            edges: Some(RuntimeEdges {
                observed: 0,
                runtime_only: 0,
                skipped: 1,
            }),
            warnings: vec![],
        },
    };
    let r = format_repo_report(&analysis, false);
    assert!(
        regex::Regex::new(r"Static edges never observed\s+1\n\s+web -> reports")
            .unwrap()
            .is_match(&r),
        "only the http call is a path production could have taken: {r}"
    );
    assert!(
        !r.contains("orders -> reports") && !r.contains("web -> orders"),
        "{r}"
    );
    // Two matched plus one ignored is every runtime name accounted for: no partial-join wording.
    assert!(r.contains("connected (OTel, 2 services matched)"), "{r}");
    assert!(
        r.contains("2 of 3 runtime services matched (1 ignored by vernier.config.json)"),
        "{r}"
    );
}
