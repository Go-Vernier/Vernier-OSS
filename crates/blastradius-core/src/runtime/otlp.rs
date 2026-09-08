//! A raw OTLP JSON span export (`otlpjson` file exporter, or a collector's
//! debug output). Spans are paired across services: a span whose parent lives
//! in another service is a call from that service, and a CLIENT or PRODUCER
//! span naming `peer.service` is a call to it. Spans per pair is the count.
use std::collections::HashMap;

use serde::Deserialize;

use super::{RuntimeError, RuntimeGraph, RuntimeKind, RuntimeSource};

#[derive(Deserialize, Default)]
struct Export {
    #[serde(default, rename = "resourceSpans")]
    resource_spans: Vec<ResourceSpans>,
}

#[derive(Deserialize, Default)]
struct ResourceSpans {
    #[serde(default)]
    resource: Resource,
    #[serde(default, rename = "scopeSpans", alias = "instrumentationLibrarySpans")]
    scope_spans: Vec<ScopeSpans>,
}

#[derive(Deserialize, Default)]
struct Resource {
    #[serde(default)]
    attributes: Vec<KeyValue>,
}

#[derive(Deserialize, Default)]
struct ScopeSpans {
    #[serde(default)]
    spans: Vec<Span>,
}

#[derive(Deserialize, Default)]
#[allow(clippy::struct_field_names)]
struct Span {
    #[serde(default, rename = "spanId")]
    span_id: String,
    #[serde(default, rename = "parentSpanId")]
    parent_span_id: String,
    #[serde(default)]
    kind: serde_json::Value,
    #[serde(default)]
    attributes: Vec<KeyValue>,
}

#[derive(Deserialize, Default)]
struct KeyValue {
    #[serde(default)]
    key: String,
    #[serde(default)]
    value: AnyValue,
}

#[derive(Deserialize, Default)]
struct AnyValue {
    #[serde(default, rename = "stringValue")]
    string_value: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpanKind {
    Other,
    Server,
    Client,
    Producer,
    Consumer,
}

fn span_kind(value: &serde_json::Value) -> SpanKind {
    match value {
        serde_json::Value::Number(n) => match n.as_u64() {
            Some(2) => SpanKind::Server,
            Some(3) => SpanKind::Client,
            Some(4) => SpanKind::Producer,
            Some(5) => SpanKind::Consumer,
            _ => SpanKind::Other,
        },
        serde_json::Value::String(s) => match s.as_str() {
            "SPAN_KIND_SERVER" => SpanKind::Server,
            "SPAN_KIND_CLIENT" => SpanKind::Client,
            "SPAN_KIND_PRODUCER" => SpanKind::Producer,
            "SPAN_KIND_CONSUMER" => SpanKind::Consumer,
            _ => SpanKind::Other,
        },
        _ => SpanKind::Other,
    }
}

fn attr<'a>(attributes: &'a [KeyValue], key: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|kv| kv.key == key)
        .and_then(|kv| kv.value.string_value.as_deref())
        .filter(|v| !v.is_empty())
}

/// What kind of call a span describes, from its semantic-convention
/// attributes and its kind.
fn kind_of(attributes: &[KeyValue], kind: SpanKind) -> RuntimeKind {
    if attr(attributes, "messaging.system").is_some()
        || matches!(kind, SpanKind::Producer | SpanKind::Consumer)
    {
        return RuntimeKind::Event;
    }
    if attr(attributes, "db.system").is_some() {
        return RuntimeKind::Database;
    }
    if attr(attributes, "rpc.system").is_some_and(|s| s.eq_ignore_ascii_case("grpc")) {
        return RuntimeKind::Grpc;
    }
    RuntimeKind::Http
}

struct Flat<'a> {
    service: &'a str,
    span: &'a Span,
    kind: SpanKind,
}

pub fn parse(text: &str, input: &str) -> Result<RuntimeGraph, RuntimeError> {
    let export: Export = serde_json::from_str(text)
        .map_err(|e| RuntimeError::Parse(format!("{input}: not an OTLP JSON export: {e}")))?;
    let mut spans: Vec<Flat<'_>> = Vec::new();
    for rs in &export.resource_spans {
        let service = attr(&rs.resource.attributes, "service.name").unwrap_or("");
        if service.is_empty() {
            continue;
        }
        for ss in &rs.scope_spans {
            for span in &ss.spans {
                spans.push(Flat {
                    service,
                    span,
                    kind: span_kind(&span.kind),
                });
            }
        }
    }
    let by_id: HashMap<&str, &Flat<'_>> = spans
        .iter()
        .filter(|f| !f.span.span_id.is_empty())
        .map(|f| (f.span.span_id.as_str(), f))
        .collect();
    let mut graph = RuntimeGraph::new(RuntimeSource::Otel, input, "otlp spans");
    // Parents whose call was counted through a child, so `peer.service` on
    // the same parent does not count it twice.
    let mut counted_parents: Vec<&str> = Vec::new();
    for f in &spans {
        if let Some(parent) = by_id.get(f.span.parent_span_id.as_str()) {
            if parent.service != f.service {
                let kind = if parent.kind == SpanKind::Producer {
                    RuntimeKind::Event
                } else {
                    kind_of(&f.span.attributes, f.kind)
                };
                graph.record(parent.service, f.service, Some(1), kind);
                counted_parents.push(parent.span.span_id.as_str());
            }
        }
    }
    for f in &spans {
        if !matches!(f.kind, SpanKind::Client | SpanKind::Producer) {
            continue;
        }
        if counted_parents.contains(&f.span.span_id.as_str()) {
            continue;
        }
        if let Some(peer) = attr(&f.span.attributes, "peer.service") {
            if peer != f.service {
                graph.record(
                    f.service,
                    peer,
                    Some(1),
                    kind_of(&f.span.attributes, f.kind),
                );
            }
        }
    }
    if graph.calls.is_empty() {
        return Err(RuntimeError::Parse(format!(
            "{input}: no spans that cross a service boundary (a parent in another service, or peer.service on a client span)"
        )));
    }
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const EXPORT: &str = r#"{
  "resourceSpans": [
    { "resource": { "attributes": [ { "key": "service.name", "value": { "stringValue": "frontend" } } ] },
      "scopeSpans": [ { "spans": [
        { "traceId": "t1", "spanId": "f1", "parentSpanId": "", "kind": 3,
          "attributes": [ { "key": "rpc.system", "value": { "stringValue": "grpc" } }, { "key": "peer.service", "value": { "stringValue": "cart" } } ] },
        { "traceId": "t1", "spanId": "f2", "parentSpanId": "", "kind": "SPAN_KIND_CLIENT",
          "attributes": [ { "key": "peer.service", "value": { "stringValue": "cart" } } ] },
        { "traceId": "t2", "spanId": "f3", "parentSpanId": "", "kind": 4,
          "attributes": [ { "key": "messaging.system", "value": { "stringValue": "kafka" } }, { "key": "peer.service", "value": { "stringValue": "accounting" } } ] },
        { "traceId": "t3", "spanId": "f4", "parentSpanId": "", "kind": 3,
          "attributes": [ { "key": "db.system", "value": { "stringValue": "redis" } }, { "key": "peer.service", "value": { "stringValue": "redis-cart" } } ] },
        { "traceId": "t4", "spanId": "f5", "parentSpanId": "", "kind": 1, "attributes": [] }
      ] } ] },
    { "resource": { "attributes": [ { "key": "service.name", "value": { "stringValue": "cart" } } ] },
      "scopeSpans": [ { "spans": [
        { "traceId": "t1", "spanId": "c1", "parentSpanId": "f1", "kind": 2, "attributes": [ { "key": "rpc.system", "value": { "stringValue": "grpc" } } ] },
        { "traceId": "t5", "spanId": "c2", "parentSpanId": "c9", "kind": 2, "attributes": [] }
      ] } ] }
  ]
}"#;

    #[test]
    fn pairs_spans_across_services_and_types_them() {
        let g = parse(EXPORT, "spans.json").unwrap();
        assert_eq!((g.source, g.method), (RuntimeSource::Otel, "otlp spans"));
        let calls: Vec<(&str, &str, Option<u64>, RuntimeKind)> = g
            .calls
            .iter()
            .map(|c| (c.client.as_str(), c.server.as_str(), c.calls, c.kind))
            .collect();
        assert_eq!(
            calls,
            vec![
                ("frontend", "cart", Some(2), RuntimeKind::Grpc),
                ("frontend", "accounting", Some(1), RuntimeKind::Event),
                ("frontend", "redis-cart", Some(1), RuntimeKind::Database),
            ]
        );
        assert_eq!(
            g.services.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["accounting", "cart", "frontend", "redis-cart"]
        );
    }

    #[test]
    fn rejects_exports_without_cross_service_calls() {
        let err = parse(r#"{"resourceSpans": []}"#, "e.json").unwrap_err();
        assert!(matches!(err, RuntimeError::Parse(_)), "{err}");
        let err = parse("not json", "e.json").unwrap_err();
        assert!(matches!(err, RuntimeError::Parse(_)), "{err}");
    }
}
