//! Stage 3: the runtime join. What production actually calls, from an
//! OpenTelemetry servicegraph scrape, an OTLP JSON span export or Datadog's
//! service dependency map, matched to the discovered services and merged
//! into the graph. Every source produces the same `RuntimeGraph`.
pub mod datadog;
pub mod fetch;
pub mod matching;
pub mod merge;
pub mod otlp;
pub mod prometheus;

use crate::analyze::{Analysis, Runtime, RuntimeEdges, RuntimeMapping, RuntimeServices};
use crate::config::RuntimeConfig;
use matching::MatchHow;
use std::collections::BTreeSet;

use thiserror::Error;

pub use crate::model::RuntimeSource;

/// What kind of call the source says it was. Unknown when the source does
/// not say (Datadog, servicegraph without a `connection_type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeKind {
    Http,
    Grpc,
    Event,
    Database,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeCall {
    pub client: String,
    pub server: String,
    /// Count over the source's window; None when the source has no counts.
    pub calls: Option<u64>,
    pub kind: RuntimeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeGraph {
    pub source: RuntimeSource,
    /// The path or URL as the user gave it.
    pub input: String,
    /// Names the shape that was read, for evidence details.
    pub method: &'static str,
    pub services: BTreeSet<String>,
    /// One entry per (client, server), in first-seen order.
    pub calls: Vec<RuntimeCall>,
}

impl RuntimeGraph {
    pub fn new(source: RuntimeSource, input: &str, method: &'static str) -> Self {
        Self {
            source,
            input: input.to_string(),
            method,
            services: BTreeSet::new(),
            calls: Vec::new(),
        }
    }

    /// Adds an observation. A pair already seen sums its counts; a specific
    /// kind replaces Unknown, and the first specific kind otherwise stays.
    pub fn record(&mut self, client: &str, server: &str, calls: Option<u64>, kind: RuntimeKind) {
        self.services.insert(client.to_string());
        self.services.insert(server.to_string());
        if let Some(existing) = self
            .calls
            .iter_mut()
            .find(|c| c.client == client && c.server == server)
        {
            existing.calls = match (existing.calls, calls) {
                (Some(a), Some(b)) => Some(a + b),
                (a, None) => a,
                (None, b) => b,
            };
            if existing.kind == RuntimeKind::Unknown {
                existing.kind = kind;
            }
            return;
        }
        self.calls.push(RuntimeCall {
            client: client.to_string(),
            server: server.to_string(),
            calls,
            kind,
        });
    }
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{0}")]
    Http(String),
    #[error("{0}")]
    Parse(String),
    #[error("{0}")]
    Config(String),
    #[error("{0}")]
    Unsupported(String),
}

/// Matches the runtime graph's names to the discovered services, merges the
/// calls into the analysis graph and fills the `runtime` block.
pub fn join(
    analysis: &mut Analysis,
    runtime: RuntimeGraph,
    config: &RuntimeConfig,
) -> Result<(), RuntimeError> {
    let services = analysis.graph.services();
    let mapping = matching::match_names(&runtime.services, &services, config)?;
    let counts = merge::apply(&mut analysis.graph, &runtime, &mapping);
    let matched = mapping.iter().filter(|m| m.service.is_some()).count();
    let warnings = mapping
        .iter()
        .filter_map(|m| match (&m.how, &m.service) {
            (MatchHow::Fuzzy(score), Some(service)) => Some(format!(
                "fuzzy match: {} -> {service} ({score:.2})",
                m.runtime
            )),
            _ => None,
        })
        .collect();
    analysis.runtime = Runtime {
        connected: true,
        source: Some(runtime.source),
        input: Some(runtime.input),
        services: Some(RuntimeServices {
            runtime: runtime.services.len(),
            matched,
        }),
        mapping: mapping
            .iter()
            .map(|m| RuntimeMapping {
                runtime: m.runtime.clone(),
                service: m.service.clone(),
                how: m.how.as_str(),
            })
            .collect(),
        unmatched: mapping
            .iter()
            .filter(|m| m.how == MatchHow::Unmatched)
            .map(|m| m.runtime.clone())
            .collect(),
        edges: Some(RuntimeEdges {
            observed: counts.observed,
            runtime_only: counts.runtime_only,
            skipped: counts.skipped,
        }),
        warnings,
    };
    Ok(())
}

/// What the CLI asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeInput {
    /// A path or URL holding a servicegraph scrape or an OTLP JSON export.
    Otel(String),
    /// A path or URL holding a saved `service_dependencies` response.
    DatadogFile(String),
    /// Call the Datadog API for this site and environment.
    DatadogLive { site: String, env: String },
}

/// An `--otel` input by its shape: JSON is an OTLP export, text with the
/// servicegraph metric is a Prometheus scrape.
pub fn detect(text: &str, input: &str) -> Result<RuntimeGraph, RuntimeError> {
    let trimmed = text.trim_start();
    if trimmed.starts_with('{') {
        return match otlp::parse(text, input) {
            // A Datadog dependency map is JSON too; say so instead of
            // complaining about missing spans. Only when it really is one:
            // no span export key, and at least one service with callees.
            Err(_)
                if !text.contains("\"resourceSpans\"")
                    && datadog::parse(text, input).is_ok_and(|g| !g.calls.is_empty()) =>
            {
                Err(RuntimeError::Unsupported(format!(
                    "{input} looks like a Datadog service_dependencies response; pass it with --datadog instead of --otel"
                )))
            }
            other => other,
        };
    }
    if prometheus::is_prometheus(text) {
        return prometheus::parse(text, input);
    }
    Err(RuntimeError::Unsupported(format!(
        "{input} is neither a Prometheus scrape with {} nor an OTLP JSON export",
        prometheus::METRIC
    )))
}

pub fn load(input: &RuntimeInput) -> Result<RuntimeGraph, RuntimeError> {
    match input {
        RuntimeInput::Otel(source) => detect(&fetch::read(source)?, source),
        RuntimeInput::DatadogFile(source) => datadog::parse(&fetch::read(source)?, source),
        RuntimeInput::DatadogLive { site, env } => {
            let text = fetch::datadog_live(site, env)?;
            datadog::parse(&text, &format!("datadog env {env}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn record_sums_counts_and_keeps_the_specific_kind() {
        let mut g = RuntimeGraph::new(RuntimeSource::Otel, "traces.prom", "otel servicegraph");
        g.record("checkout-api", "payment", Some(120), RuntimeKind::Unknown);
        g.record("checkout-api", "payment", Some(5), RuntimeKind::Unknown);
        g.record("checkout-api", "payment", None, RuntimeKind::Grpc);
        g.record("orders", "notifications", Some(42), RuntimeKind::Event);
        g.record("orders", "notifications", Some(1), RuntimeKind::Unknown);
        assert_eq!(
            g.calls,
            vec![
                RuntimeCall {
                    client: "checkout-api".into(),
                    server: "payment".into(),
                    calls: Some(125),
                    kind: RuntimeKind::Grpc
                },
                RuntimeCall {
                    client: "orders".into(),
                    server: "notifications".into(),
                    calls: Some(43),
                    kind: RuntimeKind::Event
                },
            ]
        );
        assert_eq!(
            g.services.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["checkout-api", "notifications", "orders", "payment"]
        );
        let mut d = RuntimeGraph::new(
            RuntimeSource::Datadog,
            "deps.json",
            "datadog service_dependencies",
        );
        d.record("a", "b", None, RuntimeKind::Unknown);
        d.record("a", "b", None, RuntimeKind::Unknown);
        assert_eq!(d.calls[0].calls, None);
    }

    #[test]
    fn detects_the_shape_of_an_otel_input() {
        let prom = "traces_service_graph_request_total{client=\"a\",server=\"b\"} 1\n";
        assert_eq!(detect(prom, "x.prom").unwrap().method, "otel servicegraph");
        let otlp = r#"{"resourceSpans":[{"resource":{"attributes":[{"key":"service.name","value":{"stringValue":"a"}}]},"scopeSpans":[{"spans":[{"spanId":"1","parentSpanId":"","kind":3,"attributes":[{"key":"peer.service","value":{"stringValue":"b"}}]}]}]}]}"#;
        assert_eq!(detect(otlp, "x.json").unwrap().method, "otlp spans");
        let err = detect("hello\n", "x.txt").unwrap_err();
        assert!(matches!(err, RuntimeError::Unsupported(_)), "{err}");
        assert!(
            err.to_string()
                .contains("traces_service_graph_request_total")
                && err.to_string().contains("OTLP"),
            "{err}"
        );
    }

    #[test]
    fn a_datadog_response_given_to_otel_is_pointed_at_the_right_flag() {
        let err = detect(r#"{"checkout-api": {"calls": ["payment"]}}"#, "deps.json").unwrap_err();
        assert!(matches!(err, RuntimeError::Unsupported(_)), "{err}");
        assert!(err.to_string().contains("--datadog"), "{err}");
        let err = detect(r#"{"resourceSpans": []}"#, "empty.json").unwrap_err();
        assert!(
            matches!(err, RuntimeError::Parse(_)),
            "an empty export is still an OTLP error: {err}"
        );
    }

    #[test]
    fn load_reads_files_and_reports_missing_ones() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/runtime-app/runtime");
        let g = load(&RuntimeInput::Otel(
            root.join("traces.prom").display().to_string(),
        ))
        .unwrap();
        assert_eq!(g.method, "otel servicegraph");
        let g = load(&RuntimeInput::DatadogFile(
            root.join("datadog.json").display().to_string(),
        ))
        .unwrap();
        assert_eq!(g.source, RuntimeSource::Datadog);
        let err = load(&RuntimeInput::Otel("/nonexistent/traces.prom".into())).unwrap_err();
        assert!(matches!(err, RuntimeError::Io { .. }), "{err}");
        assert!(
            err.to_string().contains("/nonexistent/traces.prom"),
            "{err}"
        );
    }
}
