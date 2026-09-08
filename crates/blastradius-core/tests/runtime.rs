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
