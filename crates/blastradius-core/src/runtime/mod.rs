//! Stage 3: the runtime join. What production actually calls, from an
//! OpenTelemetry servicegraph scrape, an OTLP JSON span export or Datadog's
//! service dependency map, matched to the discovered services and merged
//! into the graph. Every source produces the same `RuntimeGraph`.
pub mod datadog;
pub mod matching;
pub mod otlp;
pub mod prometheus;

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
}
