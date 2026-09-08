//! Datadog's service dependency map: `GET /api/v1/service_dependencies?env=`
//! returns each service and the services it calls. No counts, no protocol.
use indexmap::IndexMap;

use super::{RuntimeError, RuntimeGraph, RuntimeKind, RuntimeSource};

pub fn url(site: &str, env: &str) -> String {
    let env: String = env
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    format!("https://api.{site}/api/v1/service_dependencies?env={env}")
}

pub fn parse(text: &str, input: &str) -> Result<RuntimeGraph, RuntimeError> {
    let services: IndexMap<String, serde_json::Value> =
        serde_json::from_str(text).map_err(|e| {
            RuntimeError::Parse(format!("{input}: not a service_dependencies response: {e}"))
        })?;
    if services.is_empty() {
        return Err(RuntimeError::Parse(format!(
            "{input}: no services in the response"
        )));
    }
    let mut graph = RuntimeGraph::new(
        RuntimeSource::Datadog,
        input,
        "datadog service_dependencies",
    );
    for (service, deps) in &services {
        graph.services.insert(service.clone());
        let callees = deps
            .get("calls")
            .or_else(|| deps.get("calls_out"))
            .and_then(|v| v.as_array());
        for callee in callees.into_iter().flatten() {
            if let Some(name) = callee.as_str() {
                if !name.is_empty() {
                    graph.record(service, name, None, RuntimeKind::Unknown);
                }
            }
        }
    }
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn reads_calls_and_calls_out_lists() {
        let text = r#"{ "checkout-api": { "calls": ["payment", "catalogue-service"] }, "orders": { "calls_out": ["notifications"] }, "lonely": { "calls": [] } }"#;
        let g = parse(text, "deps.json").unwrap();
        assert_eq!(
            (g.source, g.method),
            (RuntimeSource::Datadog, "datadog service_dependencies")
        );
        let calls: Vec<(&str, &str, Option<u64>, RuntimeKind)> = g
            .calls
            .iter()
            .map(|c| (c.client.as_str(), c.server.as_str(), c.calls, c.kind))
            .collect();
        assert_eq!(
            calls,
            vec![
                ("checkout-api", "payment", None, RuntimeKind::Unknown),
                (
                    "checkout-api",
                    "catalogue-service",
                    None,
                    RuntimeKind::Unknown
                ),
                ("orders", "notifications", None, RuntimeKind::Unknown),
            ]
        );
        assert!(
            g.services.contains("lonely"),
            "a service with no calls is still a runtime service"
        );
        assert_eq!(g.services.len(), 6);
    }

    #[test]
    fn needs_an_object_of_services() {
        assert!(matches!(
            parse("[]", "d.json").unwrap_err(),
            RuntimeError::Parse(_)
        ));
        assert!(matches!(
            parse("{}", "d.json").unwrap_err(),
            RuntimeError::Parse(_)
        ));
        assert_eq!(
            url("datadoghq.eu", "prod"),
            "https://api.datadoghq.eu/api/v1/service_dependencies?env=prod"
        );
        assert_eq!(
            url("datadoghq.com", "staging us"),
            "https://api.datadoghq.com/api/v1/service_dependencies?env=staging%20us"
        );
    }
}
