//! The OpenTelemetry Collector's servicegraph connector exposes
//! `traces_service_graph_request_total{client, server, connection_type}`
//! through the Prometheus exporter. One scrape of that endpoint, or a file
//! holding one, is the cheapest runtime source there is.
use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use super::{RuntimeError, RuntimeGraph, RuntimeKind, RuntimeSource};

pub const METRIC: &str = "traces_service_graph_request_total";

static SAMPLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^traces_service_graph_request_total(?:_total)?\{([^}]*)\}\s+([0-9eE+.\-]+)(?:\s+-?\d+)?\s*$",
    )
    .unwrap()
});
static LABEL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"([A-Za-z_][A-Za-z0-9_]*)="((?:[^"\\]|\\.)*)""#).unwrap());

pub fn is_prometheus(text: &str) -> bool {
    text.contains(METRIC)
}

fn kind_of(connection_type: &str) -> RuntimeKind {
    match connection_type {
        "messaging_system" => RuntimeKind::Event,
        "database" => RuntimeKind::Database,
        _ => RuntimeKind::Unknown,
    }
}

/// Every `traces_service_graph_request_total` sample with both `client` and
/// `server`, counts summed over the other labels. Comments, other metric
/// families and malformed lines are skipped.
pub fn parse(text: &str, input: &str) -> Result<RuntimeGraph, RuntimeError> {
    let mut graph = RuntimeGraph::new(RuntimeSource::Otel, input, "otel servicegraph");
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(caps) = SAMPLE.captures(line) else {
            continue;
        };
        let labels: HashMap<&str, String> = LABEL
            .captures_iter(&caps[1])
            .map(|c| {
                let name = c.get(1).unwrap().as_str();
                let value = c[2].replace("\\\"", "\"").replace("\\\\", "\\");
                (name, value)
            })
            .collect();
        let (Some(client), Some(server)) = (labels.get("client"), labels.get("server")) else {
            continue;
        };
        if client.is_empty() || server.is_empty() {
            continue;
        }
        let Ok(value) = caps[2].parse::<f64>() else {
            continue;
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let count = value.max(0.0).round() as u64;
        let kind = kind_of(labels.get("connection_type").map_or("", String::as_str));
        graph.record(client, server, Some(count), kind);
    }
    if graph.calls.is_empty() {
        return Err(RuntimeError::Parse(format!(
            "{input}: no {METRIC} samples with client and server labels"
        )));
    }
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const SCRAPE: &str = r#"# HELP traces_service_graph_request_total Total count of requests between two nodes
# TYPE traces_service_graph_request_total counter
traces_service_graph_request_total{client="checkout-api",server="payment",connection_type=""} 120
traces_service_graph_request_total{client="checkout-api",server="payment",connection_type="virtual_node"} 5 1725800000000
traces_service_graph_request_total{client="orders",server="notifications",connection_type="messaging_system"} 42
traces_service_graph_request_total{client="pay",server="redis",connection_type="database"} 9e2
traces_service_graph_request_total_total{client="legacy",server="payment"} 1
traces_service_graph_request_failed_total{client="checkout-api",server="payment",connection_type=""} 3
traces_service_graph_request_server_seconds_bucket{client="checkout-api",server="payment",le="0.1"} 7
up 1
"#;

    #[test]
    fn parses_servicegraph_samples_summing_label_variants() {
        assert!(is_prometheus(SCRAPE) && !is_prometheus("{}"));
        let g = parse(SCRAPE, "traces.prom").unwrap();
        assert_eq!(
            (g.source, g.input.as_str(), g.method),
            (RuntimeSource::Otel, "traces.prom", "otel servicegraph")
        );
        let calls: Vec<(&str, &str, Option<u64>, RuntimeKind)> = g
            .calls
            .iter()
            .map(|c| (c.client.as_str(), c.server.as_str(), c.calls, c.kind))
            .collect();
        assert_eq!(
            calls,
            vec![
                ("checkout-api", "payment", Some(125), RuntimeKind::Unknown),
                ("orders", "notifications", Some(42), RuntimeKind::Event),
                ("pay", "redis", Some(900), RuntimeKind::Database),
                ("legacy", "payment", Some(1), RuntimeKind::Unknown),
            ]
        );
        assert_eq!(g.services.len(), 7);
    }

    #[test]
    fn needs_at_least_one_sample_with_both_labels() {
        let err = parse(
            "traces_service_graph_request_total{client=\"a\"} 1\n",
            "x.prom",
        )
        .unwrap_err();
        assert!(matches!(err, RuntimeError::Parse(_)), "{err}");
        assert!(err.to_string().contains("client and server"), "{err}");
        let g = parse(
            "traces_service_graph_request_total{server=\"b\",client=\"a b\"} 2\n",
            "x",
        )
        .unwrap();
        assert_eq!(g.calls[0].client, "a b");
    }
}
