# Stage 3: Runtime Join Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `blast-radius analyze . --otel <file|url>` and `--datadog [<file|url>]` read what production actually calls, match runtime service names to discovered services with the result always printed, and merge: static edges production confirmed become `Observed` with their call counts, calls static analysis missed become new `Observed` edges, and no static edge is ever dropped.

**Architecture:** A new `runtime` module in `blastradius-core` with one parser per source (`prometheus.rs`, `otlp.rs`, `datadog.rs`) all producing the same `RuntimeGraph`; a `fetch.rs` that reads a file or, only when asked, a URL; `matching.rs` resolving each runtime name through config, exact, normalised and fuzzy tiers; `merge.rs` applying the build spec's merge rules to the existing `BlastGraph`. `analyze()` is unchanged; the CLI calls `runtime::load` and `runtime::join` after it. The JSON contract grows additively (`edge.observed`, a fuller `runtime` block).

**Tech Stack:** Rust 1.98 stable via Homebrew rustup; existing serde, serde_json, indexmap, regex, thiserror, clap; new `ureq = "3"` (HTTP, rustls) and `strsim = "0.11"` (Jaro–Winkler).

**Spec:** `docs/superpowers/specs/2026-09-09-runtime-join-design.md` (all sections). `docs/build-spec.md` § "Stage 3 — Runtime join" is the parent requirement.

## Global Constraints

- Toolchain: `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"` before any `cargo` command. Every task ends green on `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings` (pedantic lints are warnings, so they fail clippy) and `cargo test`.
- No test touches the network. Every fetch test uses files; the live Datadog path is tested only up to the point where missing keys or a missing `--dd-env` produce an error.
- The static path never makes a network call. Without `--otel`/`--datadog` nothing in this plan runs, and `runtime` serialises exactly as today: `{ "connected": false }`. The discovery parity test (`cargo test --test parity`) and the corpus test must stay green and unchanged.
- The JSON contract is additive: `observed` on an edge is omitted when absent, `calls` inside it is omitted when the source has none; every existing field keeps its name, shape and order.
- Confidence: a runtime-confirmed or runtime-only edge is `Observed`. `import` edges are never promoted. Self calls are dropped. A call with an unmatched or ignored end is skipped and counted in `runtime.edges.skipped`.
- Matching order: config `map`/`ignore`, exact, normalised (`crate::discover::directories::normalise`), fuzzy at Jaro–Winkler ≥ 0.9 on normalised names both at least 4 characters. A config `map` entry naming an undiscovered service is an error. Fuzzy matches are warnings in `runtime.warnings` and marked in the report.
- Honest failure: an unreadable or unparseable input, a non-2xx HTTP status, a timeout, missing `DD_API_KEY`/`DD_APP_KEY`, or a bare `--datadog` without `--dd-env` exits 1 with a message on stderr and prints no report. A partial join is not an error and is stated in the header and the RUNTIME section.
- Secrets only from the environment (`DD_API_KEY`, `DD_APP_KEY`), never from flags.
- Every runtime-touched edge gains one `Evidence { file: <input path or URL>, line: None, detail }` with `detail` `"<n> calls (<method>)"` or `"observed (<method>)"`, where method is `otel servicegraph`, `otlp spans` or `datadog service_dependencies`.
- Commit after every task with a conventional-commit subject and a body that says what and why, as in `git log`.

## File Structure

```
Cargo.toml                                          + ureq, strsim in [workspace.dependencies]
crates/blastradius-core/Cargo.toml                  + ureq.workspace, strsim.workspace
crates/blastradius-core/src/model.rs                + RuntimeSource, Observed; Edge.observed
crates/blastradius-core/src/analyze.rs              Runtime block: connected, source, input, services, mapping, unmatched, edges, warnings
crates/blastradius-core/src/graph.rs                + edges_mut(), sort_edges()
crates/blastradius-core/src/config.rs               NEW  blast-radius.config.json: Config { runtime: RuntimeConfig { map, ignore } }
crates/blastradius-core/src/runtime/mod.rs          NEW  RuntimeKind, RuntimeCall, RuntimeGraph, RuntimeInput, RuntimeError; detect(), load(), join()
crates/blastradius-core/src/runtime/prometheus.rs   NEW  servicegraph text format
crates/blastradius-core/src/runtime/otlp.rs         NEW  OTLP JSON spans
crates/blastradius-core/src/runtime/datadog.rs      NEW  service_dependencies JSON; url()
crates/blastradius-core/src/runtime/fetch.rs        NEW  read(), http_get(), datadog_live()
crates/blastradius-core/src/runtime/matching.rs     NEW  MatchHow, Mapping, match_names()
crates/blastradius-core/src/runtime/merge.rs        NEW  MergeCounts, apply()
crates/blastradius-core/src/lib.rs                  + pub mod config; pub mod runtime; re-exports
crates/blastradius-core/src/report.rs               header, RUNTIME section, FINDINGS line
crates/blastradius-core/tests/runtime.rs            NEW  integration tests on the fixture
crates/blastradius-core/tests/edges.rs              contract test extended
crates/blastradius-cli/src/main.rs                  --otel, --datadog, --dd-env, --dd-site
crates/blastradius-cli/tests/cli.rs                 + runtime flag tests and error paths
test/fixtures/runtime-app/                          compose + 5 services + blast-radius.config.json + runtime/{traces.prom,spans.json,datadog.json}
README.md, docs/superpowers/specs/2026-09-09-runtime-join-design.md
```

---

### Task 1: Contract: `Observed` on edges and the fuller `runtime` block

**Files:**
- Modify: `crates/blastradius-core/src/model.rs`
- Modify: `crates/blastradius-core/src/analyze.rs`
- Modify: `crates/blastradius-core/src/map/mod.rs` (the `Edge { .. }` literal in `collect_outcomes` gains `observed: None`)
- Modify: `crates/blastradius-core/tests/analyze.rs` (the `edge()` helper gains `observed: None`), `crates/blastradius-core/tests/edges.rs`
- Modify: `crates/blastradius-core/src/lib.rs` (re-exports)

**Interfaces:**
- Produces:
  ```rust
  // model.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
  #[serde(rename_all = "lowercase")]
  pub enum RuntimeSource { Otel, Datadog }
  impl RuntimeSource { pub fn as_str(self) -> &'static str; /* "otel" | "datadog" */ pub fn label(self) -> &'static str; /* "OTel" | "Datadog" */ }
  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  pub struct Observed { #[serde(default, skip_serializing_if = "Option::is_none")] pub calls: Option<u64>, pub source: RuntimeSource }
  pub struct Edge { ..., #[serde(default, skip_serializing_if = "Option::is_none")] pub observed: Option<Observed> }

  // analyze.rs (the struct keeps its name `Runtime`, already re-exported)
  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "camelCase")]
  pub struct RuntimeMapping { pub runtime: String, pub service: Option<String>, pub how: String }
  #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "camelCase")]
  pub struct RuntimeServices { pub runtime: usize, pub matched: usize }
  #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "camelCase")]
  pub struct RuntimeEdges { pub observed: usize, pub runtime_only: usize, pub skipped: usize }
  #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "camelCase")]
  pub struct Runtime {
      pub connected: bool,
      #[serde(default, skip_serializing_if = "Option::is_none")] pub source: Option<RuntimeSource>,
      #[serde(default, skip_serializing_if = "Option::is_none")] pub input: Option<String>,
      #[serde(default, skip_serializing_if = "Option::is_none")] pub services: Option<RuntimeServices>,
      #[serde(default, skip_serializing_if = "Vec::is_empty")] pub mapping: Vec<RuntimeMapping>,
      #[serde(default, skip_serializing_if = "Vec::is_empty")] pub unmatched: Vec<String>,
      #[serde(default, skip_serializing_if = "Option::is_none")] pub edges: Option<RuntimeEdges>,
      #[serde(default, skip_serializing_if = "Vec::is_empty")] pub warnings: Vec<String>,
  }
  ```
- `Runtime` loses `Copy` (it holds Vecs); `Analysis::to_json` clones it.

- [ ] **Step 1: Write the failing tests**

In `model.rs` tests add:

```rust
    #[test]
    fn edge_observed_is_optional_and_omits_missing_calls() {
        let mut e = Edge {
            source: "checkout".into(),
            target: "payment".into(),
            edge_type: EdgeType::Http,
            confidence: Confidence::Observed,
            evidence: vec![],
            observed: None,
        };
        let json = serde_json::to_value(&e).unwrap();
        assert!(json.get("observed").is_none(), "{json}");
        e.observed = Some(Observed {
            calls: Some(132),
            source: RuntimeSource::Otel,
        });
        assert_eq!(
            serde_json::to_value(&e).unwrap()["observed"],
            serde_json::json!({ "calls": 132, "source": "otel" })
        );
        e.observed = Some(Observed {
            calls: None,
            source: RuntimeSource::Datadog,
        });
        assert_eq!(
            serde_json::to_value(&e).unwrap()["observed"],
            serde_json::json!({ "source": "datadog" })
        );
        let back: Edge = serde_json::from_value(serde_json::json!({
            "source": "a", "target": "b", "type": "http", "confidence": "static", "evidence": []
        }))
        .unwrap();
        assert_eq!(back.observed, None);
        assert_eq!(RuntimeSource::Otel.label(), "OTel");
    }
```

In `tests/edges.rs`, extend `json_contract_gains_mapping_between_edges_and_runtime` with, before its end:

```rust
    assert_eq!(json["runtime"], serde_json::json!({ "connected": false }), "{}", json["runtime"]);
    assert!(json["edges"][0].get("observed").is_none());
```

In `tests/analyze.rs` add:

```rust
#[test]
fn runtime_block_serialises_only_what_is_known() {
    let mut r = Runtime::default();
    assert_eq!(serde_json::to_value(&r).unwrap(), serde_json::json!({ "connected": false }));
    r.connected = true;
    r.source = Some(RuntimeSource::Otel);
    r.input = Some("traces.prom".into());
    r.services = Some(RuntimeServices { runtime: 10, matched: 8 });
    r.mapping = vec![RuntimeMapping { runtime: "checkout-api".into(), service: Some("checkout".into()), how: "normalised".into() }];
    r.unmatched = vec!["auth-proxy".into()];
    r.edges = Some(RuntimeEdges { observed: 4, runtime_only: 1, skipped: 2 });
    r.warnings = vec!["fuzzy match: chckout -> checkout (0.97)".into()];
    let json = serde_json::to_value(&r).unwrap();
    let keys: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["connected", "source", "input", "services", "mapping", "unmatched", "edges", "warnings"]);
    assert_eq!(json["services"], serde_json::json!({ "runtime": 10, "matched": 8 }));
    assert_eq!(json["edges"], serde_json::json!({ "observed": 4, "runtimeOnly": 1, "skipped": 2 }));
    assert_eq!(json["mapping"][0], serde_json::json!({ "runtime": "checkout-api", "service": "checkout", "how": "normalised" }));
}
```

`tests/analyze.rs` already has `use blastradius::*;`; add `RuntimeEdges, RuntimeMapping, RuntimeServices` to the `pub use analyze::{...}` list in `lib.rs` and `Observed, RuntimeSource` come through `pub use model::*`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p blastradius-core 2>&1 | grep -E 'error\[|FAILED|test result' | head`
Expected: compile errors (`observed` field, `Observed`, `RuntimeSource`, `RuntimeServices` unknown).

- [ ] **Step 3: Implement**

In `model.rs`, after `EdgeType`:

```rust
/// Where runtime data came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeSource {
    Otel,
    Datadog,
}

impl RuntimeSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Otel => "otel",
            Self::Datadog => "datadog",
        }
    }

    /// How the report names it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Otel => "OTel",
            Self::Datadog => "Datadog",
        }
    }
}

/// Production saw this edge. `calls` is the count over the source's window
/// when the source has one; Datadog's dependency map does not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observed {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calls: Option<u64>,
    pub source: RuntimeSource,
}
```

and on `Edge`, after `evidence`:

```rust
    /// Set when a runtime source confirmed or discovered this edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed: Option<Observed>,
```

In `analyze.rs`, replace the `Runtime` struct with the block in Interfaces (doc comment: "Stage 3 fills this in. Without a runtime source it serialises as `{ \"connected\": false }`."), import `RuntimeSource` from `crate::model`, and change `runtime: self.runtime,` in `to_json` to `runtime: self.runtime.clone(),`. In `map/mod.rs::collect_outcomes` add `observed: None,` to the `Edge { .. }` literal. In `tests/analyze.rs::edge` add `observed: None,`. Fix any other `Edge { .. }` literal the compiler reports the same way.

- [ ] **Step 4: Run everything**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E '^test result|FAILED|panicked'`
Expected: all green, including `parity` (the `runtime` block still prints `{"connected": false}`) and `corpus`.

- [ ] **Step 5: Commit**

```bash
git add crates
git commit -m "feat(model): observed runtime data on edges and a fuller runtime block in the contract" -m "An edge may carry observed { calls, source } once a runtime source confirmed it; the runtime block gains source, input, matched-service counts, the name mapping, unmatched names, edge counts and warnings. Every addition is omitted when absent, so the contract without a runtime source is byte-identical to before."
```

---

### Task 2: The runtime module and the Prometheus servicegraph parser

**Files:**
- Create: `crates/blastradius-core/src/runtime/mod.rs`, `crates/blastradius-core/src/runtime/prometheus.rs`
- Modify: `crates/blastradius-core/src/lib.rs` (`pub mod runtime;`)

**Interfaces:**
- Produces:
  ```rust
  // runtime/mod.rs
  pub use crate::model::RuntimeSource;
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum RuntimeKind { Http, Grpc, Event, Database, Unknown }
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct RuntimeCall { pub client: String, pub server: String, pub calls: Option<u64>, pub kind: RuntimeKind }
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct RuntimeGraph {
      pub source: RuntimeSource,
      pub input: String,              // the path or URL as given
      pub method: &'static str,       // "otel servicegraph" | "otlp spans" | "datadog service_dependencies"
      pub services: BTreeSet<String>,
      pub calls: Vec<RuntimeCall>,    // one per (client, server), in first-seen order
  }
  impl RuntimeGraph {
      pub fn new(source: RuntimeSource, input: &str, method: &'static str) -> Self;
      /// Adds an observation. A pair already seen sums its counts (None + n = n); a
      /// specific kind replaces Unknown, the first specific kind otherwise stays.
      pub fn record(&mut self, client: &str, server: &str, calls: Option<u64>, kind: RuntimeKind);
  }
  #[derive(Debug, Error)]
  pub enum RuntimeError {
      #[error("cannot read {path}: {source}")] Io { path: String, #[source] source: std::io::Error },
      #[error("{0}")] Http(String),
      #[error("{0}")] Parse(String),
      #[error("{0}")] Config(String),
      #[error("{0}")] Unsupported(String),
  }
  // runtime/prometheus.rs
  pub fn is_prometheus(text: &str) -> bool;
  pub fn parse(text: &str, input: &str) -> Result<RuntimeGraph, RuntimeError>;
  ```

- [ ] **Step 1: Write the failing tests**

`runtime/mod.rs` tests:

```rust
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
                RuntimeCall { client: "checkout-api".into(), server: "payment".into(), calls: Some(125), kind: RuntimeKind::Grpc },
                RuntimeCall { client: "orders".into(), server: "notifications".into(), calls: Some(43), kind: RuntimeKind::Event },
            ]
        );
        assert_eq!(
            g.services.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["checkout-api", "notifications", "orders", "payment"]
        );
        let mut d = RuntimeGraph::new(RuntimeSource::Datadog, "deps.json", "datadog service_dependencies");
        d.record("a", "b", None, RuntimeKind::Unknown);
        d.record("a", "b", None, RuntimeKind::Unknown);
        assert_eq!(d.calls[0].calls, None);
    }
}
```

`runtime/prometheus.rs` tests:

```rust
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
        assert_eq!((g.source, g.input.as_str(), g.method), (RuntimeSource::Otel, "traces.prom", "otel servicegraph"));
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
        let err = parse("traces_service_graph_request_total{client=\"a\"} 1\n", "x.prom").unwrap_err();
        assert!(matches!(err, RuntimeError::Parse(_)), "{err}");
        assert!(err.to_string().contains("client and server"), "{err}");
        let g = parse("traces_service_graph_request_total{server=\"b\",client=\"a b\"} 2\n", "x").unwrap();
        assert_eq!(g.calls[0].client, "a b");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p blastradius-core --lib runtime 2>&1 | grep -E 'error|test result' | head`
Expected: compile error, module missing.

- [ ] **Step 3: Implement `runtime/mod.rs`**

```rust
//! Stage 3: the runtime join. What production actually calls, from an
//! OpenTelemetry servicegraph scrape, an OTLP span export or Datadog's
//! service dependency map, matched to the discovered services and merged
//! into the graph. Every source produces the same `RuntimeGraph`.
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
```

- [ ] **Step 4: Implement `runtime/prometheus.rs`**

```rust
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
    Regex::new(r"^traces_service_graph_request_total(?:_total)?\{([^}]*)\}\s+([0-9eE+.\-]+)(?:\s+-?\d+)?\s*$")
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
```

Add `pub mod runtime;` to `lib.rs` and `pub use runtime::{RuntimeCall, RuntimeGraph, RuntimeKind};` (the CLI needs `RuntimeInput` later; add it in Task 6).

- [ ] **Step 5: Run, format, lint, commit**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test -p blastradius-core --lib runtime 2>&1 | grep -E 'test result|FAILED'`
Expected: 3 tests pass. Then the full `cargo test`.

```bash
git add crates/blastradius-core/src/runtime crates/blastradius-core/src/lib.rs
git commit -m "feat(runtime): runtime graph types and the servicegraph scrape parser" -m "A RuntimeGraph is what every runtime source produces: the service names seen and one call per (client, server) with summed counts and the kind the source knows. The first parser reads the OpenTelemetry Collector's traces_service_graph_request_total samples from a Prometheus scrape, typing calls by connection_type."
```

---

### Task 3: OTLP JSON span parser and Datadog dependency parser

**Files:**
- Create: `crates/blastradius-core/src/runtime/otlp.rs`, `crates/blastradius-core/src/runtime/datadog.rs`
- Modify: `crates/blastradius-core/src/runtime/mod.rs` (`pub mod otlp; pub mod datadog;`)

**Interfaces:**
- Produces:
  ```rust
  // otlp.rs
  pub fn parse(text: &str, input: &str) -> Result<RuntimeGraph, RuntimeError>   // method "otlp spans"
  // datadog.rs
  pub fn parse(text: &str, input: &str) -> Result<RuntimeGraph, RuntimeError>   // method "datadog service_dependencies"
  pub fn url(site: &str, env: &str) -> String   // https://api.<site>/api/v1/service_dependencies?env=<env>
  ```
- Consumes: `RuntimeGraph::record`, `RuntimeKind`, `RuntimeError` (Task 2).

- [ ] **Step 1: Write the failing tests**

`otlp.rs` tests:

```rust
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
```

Note on the expected count for `frontend -> cart`: span `f1` (CLIENT, peer.service cart) has a child `c1` in `cart`, which yields the pair through the parent rule and must not be counted again through `peer.service`; span `f2` (CLIENT, peer.service cart, no child in the export) yields it through `peer.service`. Two calls in total. Span `c2` has a parent that is not in the export: no pair.

`datadog.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn reads_calls_and_calls_out_lists() {
        let text = r#"{ "checkout-api": { "calls": ["payment", "catalogue-service"] }, "orders": { "calls_out": ["notifications"] }, "lonely": { "calls": [] } }"#;
        let g = parse(text, "deps.json").unwrap();
        assert_eq!((g.source, g.method), (RuntimeSource::Datadog, "datadog service_dependencies"));
        let calls: Vec<(&str, &str, Option<u64>, RuntimeKind)> = g
            .calls
            .iter()
            .map(|c| (c.client.as_str(), c.server.as_str(), c.calls, c.kind))
            .collect();
        assert_eq!(
            calls,
            vec![
                ("checkout-api", "payment", None, RuntimeKind::Unknown),
                ("checkout-api", "catalogue-service", None, RuntimeKind::Unknown),
                ("orders", "notifications", None, RuntimeKind::Unknown),
            ]
        );
        assert!(g.services.contains("lonely"), "a service with no calls is still a runtime service");
        assert_eq!(g.services.len(), 6);
    }

    #[test]
    fn needs_an_object_of_services() {
        assert!(matches!(parse("[]", "d.json").unwrap_err(), RuntimeError::Parse(_)));
        assert!(matches!(parse("{}", "d.json").unwrap_err(), RuntimeError::Parse(_)));
        assert_eq!(
            url("datadoghq.eu", "prod"),
            "https://api.datadoghq.eu/api/v1/service_dependencies?env=prod"
        );
        assert_eq!(url("datadoghq.com", "staging us"), "https://api.datadoghq.com/api/v1/service_dependencies?env=staging%20us");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p blastradius-core --lib runtime 2>&1 | grep -E 'error|test result' | head`
Expected: compile errors, modules missing.

- [ ] **Step 3: Implement `otlp.rs`**

```rust
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
                graph.record(f.service, peer, Some(1), kind_of(&f.span.attributes, f.kind));
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
```

If `counted_parents.contains` shows up in profiling of a large export, make it a `HashSet<&str>`; the plan keeps the Vec for clarity because exports are small.

- [ ] **Step 4: Implement `datadog.rs`**

```rust
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
    let services: IndexMap<String, serde_json::Value> = serde_json::from_str(text)
        .map_err(|e| RuntimeError::Parse(format!("{input}: not a service_dependencies response: {e}")))?;
    if services.is_empty() {
        return Err(RuntimeError::Parse(format!("{input}: no services in the response")));
    }
    let mut graph = RuntimeGraph::new(RuntimeSource::Datadog, input, "datadog service_dependencies");
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
```

Note `parse("{}")` errors on the empty object; `{"lonely": {"calls": []}}` is valid (one service, no calls) and joins nothing, which the RUNTIME section will show as one runtime service and zero edges.

- [ ] **Step 5: Run, format, lint, commit**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test -p blastradius-core --lib runtime 2>&1 | grep -E 'test result|FAILED'`
Expected: 7 tests pass.

```bash
git add crates/blastradius-core/src/runtime
git commit -m "feat(runtime): OTLP JSON span pairing and Datadog service dependencies" -m "Spans are paired across services through their parent, or through peer.service on client and producer spans, and typed by messaging.system, db.system and rpc.system. Datadog's dependency map gives callee lists with no counts and no protocol, so those calls are Unknown and count-less."
```

---

### Task 4: `blast-radius.config.json` and name matching

**Files:**
- Create: `crates/blastradius-core/src/config.rs`, `crates/blastradius-core/src/runtime/matching.rs`
- Modify: `Cargo.toml` (workspace: `strsim = "0.11"`), `crates/blastradius-core/Cargo.toml` (`strsim.workspace = true`), `crates/blastradius-core/src/lib.rs` (`pub mod config;`), `crates/blastradius-core/src/runtime/mod.rs` (`pub mod matching;`)

**Interfaces:**
- Produces:
  ```rust
  // config.rs
  pub const FILE_NAME: &str = "blast-radius.config.json";
  #[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
  pub struct Config { #[serde(default)] pub runtime: RuntimeConfig }
  #[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
  pub struct RuntimeConfig { #[serde(default)] pub map: IndexMap<String, String>, #[serde(default)] pub ignore: Vec<String> }
  #[derive(Debug, Error)] pub enum ConfigError { #[error("{path}: {message}")] Invalid { path: String, message: String } }
  /// Ok(Config::default()) when the file is absent; Err when it is unreadable or not valid JSON.
  pub fn load(root: &Path) -> Result<Config, ConfigError>

  // runtime/matching.rs
  pub const FUZZY_THRESHOLD: f64 = 0.9;
  #[derive(Debug, Clone, PartialEq)]
  pub enum MatchHow { Config, Exact, Normalised, Fuzzy(f64), Ignored, Unmatched }
  impl MatchHow { pub fn as_str(&self) -> String; /* "config" | "exact" | "normalised" | "fuzzy 0.97" | "ignored" | "unmatched" */ }
  #[derive(Debug, Clone, PartialEq)]
  pub struct Mapping { pub runtime: String, pub service: Option<String>, pub how: MatchHow }
  pub fn match_names(runtime: &BTreeSet<String>, services: &[Service], config: &RuntimeConfig) -> Result<Vec<Mapping>, RuntimeError>
  ```
- Consumes: `crate::discover::directories::normalise`, `crate::model::Service`, `strsim::jaro_winkler`.

- [ ] **Step 1: Write the failing tests**

`config.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn dir_with(contents: Option<&str>) -> tempdir_like::Dir { unreachable!() }
```

Do not use a temp-dir crate (none is in the workspace). Write the tests against fixture directories instead:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures")
            .join(name)
    }

    #[test]
    fn absent_file_is_the_default_and_present_file_is_read() {
        assert_eq!(load(&fixture("compose-app")).unwrap(), Config::default());
        let cfg = load(&fixture("runtime-app")).unwrap();
        assert_eq!(cfg.runtime.map.get("pay").map(String::as_str), Some("payment"));
        assert_eq!(cfg.runtime.ignore, vec!["load-generator"]);
    }

    #[test]
    fn invalid_json_is_an_error_naming_the_file() {
        let root = std::env::temp_dir().join(format!("blast-radius-config-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(FILE_NAME), "{ not json").unwrap();
        let err = load(&root).unwrap_err();
        assert!(err.to_string().contains(FILE_NAME), "{err}");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
```

`runtime-app`'s `blast-radius.config.json` is created in this task (only that file; the rest of the fixture comes in Task 5):

```
test/fixtures/runtime-app/blast-radius.config.json
{
  "runtime": {
    "map": { "pay": "payment" },
    "ignore": ["load-generator"]
  },
  "future": { "ignored": true }
}
```

`matching.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DiscoveryStrategy, Evidence, ServiceRole, ServiceSource};
    use pretty_assertions::assert_eq;

    fn svc(name: &str, infra: bool) -> Service {
        Service {
            name: name.into(),
            root: (!infra).then(|| name.to_string()),
            language: None,
            entry_points: vec![],
            role: if infra { ServiceRole::Infrastructure } else { ServiceRole::Code },
            discovered_by: ServiceSource::Strategy(DiscoveryStrategy::DockerCompose),
            evidence: Evidence { file: "docker-compose.yml".into(), line: None, detail: None },
            image: infra.then(|| format!("{name}:latest")),
            package_name: None,
        }
    }

    fn names(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn tiers_in_order_config_exact_normalised_fuzzy() {
        let services = vec![svc("checkout", false), svc("payment", false), svc("catalogue", false), svc("orders", false), svc("notifications", false), svc("redis", true)];
        let mut config = RuntimeConfig::default();
        config.map.insert("pay".into(), "payment".into());
        config.ignore.push("load-generator".into());
        let runtime = names(&["checkout", "checkout-api", "chckout", "pay", "load-generator", "auth-proxy", "catalogue-service", "redis", "Orders"]);
        let got: Vec<(String, Option<String>, String)> = match_names(&runtime, &services, &config)
            .unwrap()
            .into_iter()
            .map(|m| (m.runtime, m.service, m.how.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("Orders".into(), Some("orders".into()), "normalised".into()),
                ("auth-proxy".into(), None, "unmatched".into()),
                ("catalogue-service".into(), Some("catalogue".into()), "normalised".into()),
                ("chckout".into(), Some("checkout".into()), "fuzzy 0.97".into()),
                ("checkout".into(), Some("checkout".into()), "exact".into()),
                ("checkout-api".into(), Some("checkout".into()), "normalised".into()),
                ("load-generator".into(), None, "ignored".into()),
                ("pay".into(), Some("payment".into()), "config".into()),
                ("redis".into(), Some("redis".into()), "exact".into()),
            ]
        );
    }

    #[test]
    fn fuzzy_needs_four_characters_and_the_threshold() {
        let services = vec![svc("cart", false), svc("payment", false)];
        let config = RuntimeConfig::default();
        let got = match_names(&names(&["car", "carts", "paymnt", "billing"]), &services, &config).unwrap();
        let how: Vec<String> = got.iter().map(|m| m.how.as_str()).collect();
        assert_eq!(how[0], "unmatched", "car: too short for fuzzy");
        assert!(how[1].starts_with("fuzzy"), "carts -> cart: {}", how[1]);
        assert!(how[2].starts_with("fuzzy"), "paymnt -> payment: {}", how[2]);
        assert_eq!(how[3], "unmatched", "billing is nothing like payment");
    }

    #[test]
    fn config_naming_an_undiscovered_service_is_an_error() {
        let services = vec![svc("payment", false)];
        let mut config = RuntimeConfig::default();
        config.map.insert("pay".into(), "payments-v2".into());
        let err = match_names(&names(&["pay"]), &services, &config).unwrap_err();
        assert!(matches!(err, RuntimeError::Config(_)), "{err}");
        assert!(err.to_string().contains("payments-v2"), "{err}");
    }
}
```

The `chckout` score: Jaro–Winkler of `chckout` and `checkout` is 0.967, printed with two decimals as `fuzzy 0.97`. If `strsim` prints 0.96 on your machine (rounding differs by version), fix the expected string to what `strsim::jaro_winkler("chckout", "checkout")` actually returns, formatted `{:.2}`, and note it in the report.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p blastradius-core --lib 2>&1 | grep -E 'error|test result' | head`
Expected: compile errors (modules missing, `strsim` unknown).

- [ ] **Step 3: Dependencies and `config.rs`**

In the workspace `Cargo.toml` `[workspace.dependencies]` add `strsim = "0.11"`; in `crates/blastradius-core/Cargo.toml` add `strsim.workspace = true`.

```rust
//! `blast-radius.config.json` at the repository root. Read only when a stage
//! needs it; absent means defaults; invalid means an error, because a file
//! the user wrote must not be silently skipped. Unknown keys are ignored so
//! later stages can add their own.
use std::path::Path;

use indexmap::IndexMap;
use serde::Deserialize;
use thiserror::Error;

use crate::fs::read_text;

pub const FILE_NAME: &str = "blast-radius.config.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub runtime: RuntimeConfig,
}

/// How runtime service names map onto discovered services when the
/// automatic tiers get it wrong, and which runtime names to leave out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default)]
    pub map: IndexMap<String, String>,
    #[serde(default)]
    pub ignore: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("{path}: {message}")]
    Invalid { path: String, message: String },
}

pub fn load(root: &Path) -> Result<Config, ConfigError> {
    let path = root.join(FILE_NAME);
    let Some(text) = read_text(&path) else {
        return Ok(Config::default());
    };
    serde_json::from_str(&text).map_err(|e| ConfigError::Invalid {
        path: path.display().to_string(),
        message: e.to_string(),
    })
}
```

Add `pub mod config;` to `lib.rs` and `pub use config::{Config, ConfigError, RuntimeConfig};`.

- [ ] **Step 4: `runtime/matching.rs`**

```rust
//! Runtime service names rarely equal repository service names:
//! `checkout-api` in traces, `services/checkout` in the tree. Each runtime
//! name is resolved once, through the first tier that answers, and the whole
//! table is reported so a partial join is never silent.
use std::collections::BTreeSet;

use crate::config::RuntimeConfig;
use crate::discover::directories::normalise;
use crate::model::Service;

use super::RuntimeError;

pub const FUZZY_THRESHOLD: f64 = 0.9;
const FUZZY_MIN_LEN: usize = 4;

#[derive(Debug, Clone, PartialEq)]
pub enum MatchHow {
    Config,
    Exact,
    Normalised,
    Fuzzy(f64),
    Ignored,
    Unmatched,
}

impl MatchHow {
    pub fn as_str(&self) -> String {
        match self {
            Self::Config => "config".into(),
            Self::Exact => "exact".into(),
            Self::Normalised => "normalised".into(),
            Self::Fuzzy(score) => format!("fuzzy {score:.2}"),
            Self::Ignored => "ignored".into(),
            Self::Unmatched => "unmatched".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mapping {
    pub runtime: String,
    pub service: Option<String>,
    pub how: MatchHow,
}

/// One mapping per runtime name, in sorted order. Errors when the config
/// maps a name to a service that was not discovered.
pub fn match_names(
    runtime: &BTreeSet<String>,
    services: &[Service],
    config: &RuntimeConfig,
) -> Result<Vec<Mapping>, RuntimeError> {
    let normalised: Vec<(String, &Service)> = services.iter().map(|s| (normalise(&s.name), s)).collect();
    let mut out = Vec::with_capacity(runtime.len());
    for name in runtime {
        let mapping = |service: Option<&Service>, how: MatchHow| Mapping {
            runtime: name.clone(),
            service: service.map(|s| s.name.clone()),
            how,
        };
        if config.ignore.iter().any(|i| i == name) {
            out.push(mapping(None, MatchHow::Ignored));
            continue;
        }
        if let Some(target) = config.map.get(name) {
            let Some(service) = services.iter().find(|s| &s.name == target) else {
                return Err(RuntimeError::Config(format!(
                    "blast-radius.config.json maps {name} to {target}, which was not discovered"
                )));
            };
            out.push(mapping(Some(service), MatchHow::Config));
            continue;
        }
        if let Some(service) = services.iter().find(|s| &s.name == name) {
            out.push(mapping(Some(service), MatchHow::Exact));
            continue;
        }
        let n = normalise(name);
        if !n.is_empty() {
            if let Some((_, service)) = normalised.iter().find(|(ns, _)| *ns == n) {
                out.push(mapping(Some(service), MatchHow::Normalised));
                continue;
            }
        }
        let best = normalised
            .iter()
            .filter(|(ns, _)| n.len() >= FUZZY_MIN_LEN && ns.len() >= FUZZY_MIN_LEN)
            .map(|(ns, s)| (strsim::jaro_winkler(&n, ns), *s))
            .filter(|(score, _)| *score >= FUZZY_THRESHOLD)
            .max_by(|a, b| a.0.total_cmp(&b.0));
        match best {
            Some((score, service)) => out.push(mapping(Some(service), MatchHow::Fuzzy(score))),
            None => out.push(mapping(None, MatchHow::Unmatched)),
        }
    }
    Ok(out)
}
```

Add `pub mod matching;` to `runtime/mod.rs`.

- [ ] **Step 5: Run, format, lint, commit**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test -p blastradius-core --lib 2>&1 | grep -E 'test result|FAILED'`
Expected: all pass, including the five new tests.

```bash
git add Cargo.toml Cargo.lock crates/blastradius-core test/fixtures/runtime-app/blast-radius.config.json
git commit -m "feat(runtime): name matching through config, exact, normalised and fuzzy tiers" -m "blast-radius.config.json maps or ignores runtime names explicitly and is consulted first, because an explicit mapping must beat a wrong guess. Exact and normalised matches reuse discovery's normaliser; fuzzy matches need Jaro-Winkler 0.9 on names of four characters or more and are reported as such. A config entry naming an undiscovered service is an error."
```

---

### Task 5: Merge rules, `join`, the `runtime-app` fixture and integration tests

**Files:**
- Create: `crates/blastradius-core/src/runtime/merge.rs`, `crates/blastradius-core/tests/runtime.rs`
- Modify: `crates/blastradius-core/src/runtime/mod.rs` (`pub mod merge;`, `join`), `crates/blastradius-core/src/graph.rs` (`edges_mut`, `sort_edges`), `crates/blastradius-core/src/lib.rs`
- Create: `test/fixtures/runtime-app/**` (all files except the config, which Task 4 made)

**Interfaces:**
- Produces:
  ```rust
  // graph.rs
  impl BlastGraph { pub fn edges_mut(&mut self) -> &mut Vec<Edge>; pub fn sort_edges(&mut self); /* by (source, target, type) */ }
  // runtime/merge.rs
  #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
  pub struct MergeCounts { pub observed: usize, pub confirmed: usize, pub runtime_only: usize, pub skipped: usize }
  pub fn apply(graph: &mut BlastGraph, runtime: &RuntimeGraph, mapping: &[Mapping]) -> MergeCounts
  // runtime/mod.rs
  pub fn join(analysis: &mut Analysis, runtime: RuntimeGraph, config: &RuntimeConfig) -> Result<(), RuntimeError>
  ```
- `MergeCounts.observed` is the number of edges carrying `observed` after the merge (confirmed static edges plus runtime-only ones); `confirmed` counts calls that promoted at least one static edge; `runtime_only` counts new edges; `skipped` counts calls with an unmatched or ignored end.

- [ ] **Step 1: Create the fixture**

```
test/fixtures/runtime-app/docker-compose.yml
services:
  checkout:
    build: ./checkout
  payment:
    build: ./payment
  catalogue:
    build: ./catalogue
  orders:
    build: ./orders
  notifications:
    build: ./notifications
  rabbitmq:
    image: rabbitmq:3-management
  redis:
    image: redis:7

test/fixtures/runtime-app/checkout/package.json
{ "name": "checkout", "main": "server.js" }

test/fixtures/runtime-app/checkout/server.js
const paymentHost = process.env.PAYMENT_HOST || 'payment';
const catalogueHost = process.env.CATALOGUE_HOST || 'catalogue';

test/fixtures/runtime-app/payment/requirements.txt
redis

test/fixtures/runtime-app/payment/payment.py
import os
REDIS = os.getenv('REDIS_HOST', 'redis')

test/fixtures/runtime-app/catalogue/go.mod
module catalogue

go 1.22

test/fixtures/runtime-app/catalogue/main.go
package main

func main() {}

test/fixtures/runtime-app/orders/go.mod
module orders

go 1.22

test/fixtures/runtime-app/orders/main.go
package main

func main() {
	uri := "amqp://guest:guest@rabbitmq:5672/"
	_ = uri
}

test/fixtures/runtime-app/notifications/package.json
{ "name": "notifications", "main": "index.js" }

test/fixtures/runtime-app/notifications/index.js
console.log('notifications');

test/fixtures/runtime-app/runtime/traces.prom
# HELP traces_service_graph_request_total Total count of requests between two nodes
# TYPE traces_service_graph_request_total counter
traces_service_graph_request_total{client="checkout-api",server="payment",connection_type=""} 120
traces_service_graph_request_total{client="checkout-api",server="payment",connection_type="virtual_node"} 5
traces_service_graph_request_total{client="checkout-api",server="catalogue-service",connection_type=""} 300
traces_service_graph_request_total{client="orders",server="notifications",connection_type="messaging_system"} 42
traces_service_graph_request_total{client="chckout",server="payment",connection_type=""} 7
traces_service_graph_request_total{client="pay",server="redis",connection_type="database"} 900
traces_service_graph_request_total{client="load-generator",server="checkout-api",connection_type=""} 1000
traces_service_graph_request_total{client="auth-proxy",server="payment",connection_type=""} 3
traces_service_graph_request_total{client="payment",server="payment",connection_type=""} 1

test/fixtures/runtime-app/runtime/spans.json
{
  "resourceSpans": [
    { "resource": { "attributes": [ { "key": "service.name", "value": { "stringValue": "checkout-api" } } ] },
      "scopeSpans": [ { "spans": [
        { "traceId": "t1", "spanId": "a1", "parentSpanId": "", "kind": 3, "attributes": [ { "key": "peer.service", "value": { "stringValue": "payment" } } ] },
        { "traceId": "t2", "spanId": "a2", "parentSpanId": "", "kind": 3, "attributes": [ { "key": "peer.service", "value": { "stringValue": "payment" } } ] },
        { "traceId": "t3", "spanId": "a3", "parentSpanId": "", "kind": 3, "attributes": [ { "key": "peer.service", "value": { "stringValue": "catalogue" } } ] }
      ] } ] },
    { "resource": { "attributes": [ { "key": "service.name", "value": { "stringValue": "orders" } } ] },
      "scopeSpans": [ { "spans": [
        { "traceId": "t4", "spanId": "b1", "parentSpanId": "", "kind": 4, "attributes": [ { "key": "messaging.system", "value": { "stringValue": "rabbitmq" } }, { "key": "peer.service", "value": { "stringValue": "notifications" } } ] }
      ] } ] }
  ]
}

test/fixtures/runtime-app/runtime/datadog.json
{
  "checkout-api": { "calls": ["payment", "catalogue-service"] },
  "orders": { "calls_out": ["notifications"] },
  "auth-proxy": { "calls": ["payment"] }
}
```

The Go file uses a tab before `uri`. Static edges this fixture yields before any join: `checkout -> payment` http, `checkout -> catalogue` http (both from `process.env` defaults), `payment -> redis` database, `orders -> rabbitmq` event.

- [ ] **Step 2: Write the failing tests**

`crates/blastradius-core/tests/runtime.rs`:

```rust
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
        .unwrap_or_else(|| panic!("no edge {s} -> {t} {ty:?}: {:?}", a.graph.edges().iter().map(|e| (&e.source, &e.target, e.edge_type)).collect::<Vec<_>>()))
}

#[test]
fn servicegraph_join_confirms_adds_and_skips_by_the_rules() {
    let a = joined("traces.prom");
    let r = &a.runtime;
    assert!(r.connected);
    assert_eq!((r.source, r.input.as_deref()), (Some(RuntimeSource::Otel), Some("traces.prom")));
    assert_eq!(r.services, Some(RuntimeServices { runtime: 10, matched: 8 }));
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
    assert_eq!(r.edges, Some(RuntimeEdges { observed: 4, runtime_only: 1, skipped: 2 }));
    assert_eq!(r.warnings, vec!["fuzzy match: chckout -> checkout (0.97)"]);

    // static + runtime -> Observed with the summed count (120 + 5 + 7 through chckout)
    let e = edge(&a, "checkout", "payment", EdgeType::Http);
    assert_eq!(e.confidence, Confidence::Observed);
    assert_eq!(e.observed, Some(Observed { calls: Some(132), source: RuntimeSource::Otel }));
    assert!(e.evidence.iter().any(|v| v.file == "traces.prom" && v.line.is_none() && v.detail.as_deref() == Some("132 calls (otel servicegraph)")), "{:?}", e.evidence);
    assert!(e.evidence.iter().any(|v| v.file == "checkout/server.js"), "static evidence kept: {:?}", e.evidence);
    assert_eq!(edge(&a, "checkout", "catalogue", EdgeType::Http).observed.as_ref().and_then(|o| o.calls), Some(300));
    // config-mapped client confirms the database edge
    assert_eq!(edge(&a, "payment", "redis", EdgeType::Database).observed.as_ref().and_then(|o| o.calls), Some(900));
    // runtime only -> new Observed event edge
    let e = edge(&a, "orders", "notifications", EdgeType::Event);
    assert_eq!((e.confidence, e.observed.as_ref().and_then(|o| o.calls)), (Confidence::Observed, Some(42)));
    assert_eq!(e.evidence.len(), 1);
    // static only stays as it was
    let e = edge(&a, "orders", "rabbitmq", EdgeType::Event);
    assert_eq!((e.confidence, e.observed.is_none()), (Confidence::Static, true));
    // no self edge, nothing to load-generator or auth-proxy
    assert!(!a.graph.edges().iter().any(|e| e.source == e.target || e.source == "load-generator" || e.source == "auth-proxy"));
    // edges stay sorted
    let keys: Vec<(String, String, EdgeType)> = a.graph.edges().iter().map(|e| (e.source.clone(), e.target.clone(), e.edge_type)).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

#[test]
fn span_and_datadog_joins_apply_the_same_rules() {
    let a = joined("spans.json");
    assert_eq!(edge(&a, "checkout", "payment", EdgeType::Http).observed, Some(Observed { calls: Some(2), source: RuntimeSource::Otel }));
    assert_eq!(edge(&a, "orders", "notifications", EdgeType::Event).confidence, Confidence::Observed);
    assert_eq!(a.runtime.edges, Some(RuntimeEdges { observed: 3, runtime_only: 1, skipped: 0 }));

    let a = joined("datadog.json");
    let e = edge(&a, "checkout", "payment", EdgeType::Http);
    assert_eq!(e.observed, Some(Observed { calls: None, source: RuntimeSource::Datadog }));
    assert!(e.evidence.iter().any(|v| v.detail.as_deref() == Some("observed (datadog service_dependencies)")), "{:?}", e.evidence);
    // Datadog gives no protocol: a runtime-only call becomes an http edge
    assert_eq!(edge(&a, "orders", "notifications", EdgeType::Http).confidence, Confidence::Observed);
    assert_eq!(a.runtime.edges, Some(RuntimeEdges { observed: 3, runtime_only: 1, skipped: 1 }));
    assert_eq!(a.runtime.services, Some(RuntimeServices { runtime: 6, matched: 5 }));
}

#[test]
fn json_contract_with_a_runtime_source() {
    let json = serde_json::to_value(joined("traces.prom").to_json()).unwrap();
    let keys: Vec<&str> = json["runtime"].as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["connected", "source", "input", "services", "mapping", "unmatched", "edges", "warnings"]);
    let observed: Vec<&serde_json::Value> = json["edges"].as_array().unwrap().iter().filter(|e| e.get("observed").is_some()).collect();
    assert_eq!(observed.len(), 4);
    assert_eq!(observed[0]["observed"]["source"], "otel");
    assert!(json["edges"].as_array().unwrap().iter().any(|e| e.get("observed").is_none()), "the static-only edge has no observed key");
}
```

`common::fixture` is `pub fn fixture(name: &str) -> PathBuf` in `tests/common/mod.rs`. `Observed`, `RuntimeSource`, `RuntimeServices`, `RuntimeEdges`, `config` are re-exported/public from Task 1 and Task 4.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p blastradius-core --test runtime 2>&1 | grep -E 'error|FAILED|test result' | head`
Expected: compile errors (`runtime::join`, `merge` missing).

- [ ] **Step 4: `graph.rs` additions**

```rust
    /// The edges, for a stage that rewrites them (the runtime join).
    pub fn edges_mut(&mut self) -> &mut Vec<Edge> {
        &mut self.edges
    }

    /// The contract's order: by source, target, type.
    pub fn sort_edges(&mut self) {
        self.edges.sort_by(|a, b| {
            a.source
                .cmp(&b.source)
                .then_with(|| a.target.cmp(&b.target))
                .then_with(|| a.edge_type.cmp(&b.edge_type))
        });
    }
```

- [ ] **Step 5: `runtime/merge.rs`**

```rust
//! The build spec's merge rules. Static and runtime agree: Observed, with
//! the count. Runtime alone: a new Observed edge, since static analysis
//! missed an async or dynamic call. Static alone: untouched, never dropped.
use std::collections::HashMap;

use crate::graph::BlastGraph;
use crate::model::{Confidence, Edge, EdgeType, Evidence, Observed};

use super::matching::Mapping;
use super::{RuntimeCall, RuntimeGraph, RuntimeKind};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MergeCounts {
    /// Edges carrying `observed` after the merge.
    pub observed: usize,
    /// Calls that confirmed at least one static edge.
    pub confirmed: usize,
    /// Edges added because no static edge matched.
    pub runtime_only: usize,
    /// Calls with an unmatched or ignored end.
    pub skipped: usize,
}

/// A runtime call of this kind confirms a static edge of this type.
fn compatible(kind: RuntimeKind, ty: EdgeType) -> bool {
    match kind {
        RuntimeKind::Unknown | RuntimeKind::Http | RuntimeKind::Grpc => {
            matches!(ty, EdgeType::Http | EdgeType::Grpc)
        }
        RuntimeKind::Event => ty == EdgeType::Event,
        RuntimeKind::Database => ty == EdgeType::Database,
    }
}

fn edge_type_for(kind: RuntimeKind) -> EdgeType {
    match kind {
        RuntimeKind::Grpc => EdgeType::Grpc,
        RuntimeKind::Event => EdgeType::Event,
        RuntimeKind::Database => EdgeType::Database,
        RuntimeKind::Http | RuntimeKind::Unknown => EdgeType::Http,
    }
}

fn detail(calls: Option<u64>, method: &str) -> String {
    match calls {
        Some(n) => format!("{n} calls ({method})"),
        None => format!("observed ({method})"),
    }
}

fn add_calls(existing: Option<u64>, more: Option<u64>) -> Option<u64> {
    match (existing, more) {
        (Some(a), Some(b)) => Some(a + b),
        (a, None) => a,
        (None, b) => b,
    }
}

pub fn apply(graph: &mut BlastGraph, runtime: &RuntimeGraph, mapping: &[Mapping]) -> MergeCounts {
    let resolved: HashMap<&str, Option<&str>> = mapping
        .iter()
        .map(|m| (m.runtime.as_str(), m.service.as_deref()))
        .collect();
    let mut counts = MergeCounts::default();
    let mut added: Vec<Edge> = Vec::new();
    for call in &runtime.calls {
        let (Some(Some(a)), Some(Some(b))) = (resolved.get(call.client.as_str()), resolved.get(call.server.as_str())) else {
            counts.skipped += 1;
            continue;
        };
        if a == b {
            continue;
        }
        let evidence = Evidence {
            file: runtime.input.clone(),
            line: None,
            detail: Some(detail(call.calls, runtime.method)),
        };
        let mut matched_static = false;
        for edge in graph
            .edges_mut()
            .iter_mut()
            .filter(|e| e.source == *a && e.target == *b && compatible(call.kind, e.edge_type))
        {
            matched_static = true;
            confirm(edge, call, runtime, &evidence);
        }
        if let Some(edge) = added
            .iter_mut()
            .find(|e| e.source == *a && e.target == *b && compatible(call.kind, e.edge_type))
        {
            // A second runtime call on a pair this join already added.
            confirm(edge, call, runtime, &evidence);
            continue;
        }
        if matched_static {
            counts.confirmed += 1;
            continue;
        }
        added.push(Edge {
            source: (*a).to_string(),
            target: (*b).to_string(),
            edge_type: edge_type_for(call.kind),
            confidence: Confidence::Observed,
            evidence: vec![evidence],
            observed: Some(Observed {
                calls: call.calls,
                source: runtime.source,
            }),
        });
        counts.runtime_only += 1;
    }
    for edge in added {
        graph
            .add_edge(edge)
            .expect("runtime edges join two discovered services");
    }
    graph.sort_edges();
    counts.observed = graph.edges().iter().filter(|e| e.observed.is_some()).count();
    counts
}

/// Promotes one edge: Observed, counts summed, one runtime evidence entry
/// whose detail carries the running total.
fn confirm(edge: &mut Edge, call: &RuntimeCall, runtime: &RuntimeGraph, evidence: &Evidence) {
    edge.confidence = Confidence::Observed;
    let total = add_calls(edge.observed.as_ref().and_then(|o| o.calls), call.calls);
    edge.observed = Some(Observed {
        calls: total,
        source: runtime.source,
    });
    let text = detail(total, runtime.method);
    match edge.evidence.iter_mut().find(|v| v.file == runtime.input && v.line.is_none()) {
        Some(existing) => existing.detail = Some(text),
        None => edge.evidence.push(Evidence {
            detail: Some(text),
            ..evidence.clone()
        }),
    }
}
```

The `checkout -> payment` edge in the fixture is confirmed by two runtime pairs (`checkout-api -> payment` and `chckout -> payment`); `confirm` sums the counts into one `observed` and rewrites the single runtime evidence detail to `132 calls (otel servicegraph)`. `counts.confirmed` is 3 for the fixture (checkout-api→payment, chckout→payment and checkout-api→catalogue-service and pay→redis make 4 confirming calls; if the test's `RuntimeEdges` derives `observed` from `counts.observed`, `confirmed` is informational only and not serialised).

- [ ] **Step 6: `join` in `runtime/mod.rs`**

Add `pub mod merge;` and:

```rust
use crate::analyze::{Analysis, Runtime, RuntimeEdges, RuntimeMapping, RuntimeServices};
use crate::config::RuntimeConfig;
use matching::MatchHow;

/// Matches the runtime graph's names to the discovered services, merges the
/// calls into the analysis graph and fills the `runtime` block.
pub fn join(analysis: &mut Analysis, runtime: RuntimeGraph, config: &RuntimeConfig) -> Result<(), RuntimeError> {
    let services = analysis.graph.services();
    let mapping = matching::match_names(&runtime.services, &services, config)?;
    let counts = merge::apply(&mut analysis.graph, &runtime, &mapping);
    let matched = mapping.iter().filter(|m| m.service.is_some()).count();
    let warnings = mapping
        .iter()
        .filter_map(|m| match (&m.how, &m.service) {
            (MatchHow::Fuzzy(score), Some(service)) => {
                Some(format!("fuzzy match: {} -> {service} ({score:.2})", m.runtime))
            }
            _ => None,
        })
        .collect();
    analysis.runtime = Runtime {
        connected: true,
        source: Some(runtime.source),
        input: Some(runtime.input.clone()),
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
```

Re-export from `lib.rs`: `pub use runtime::{RuntimeCall, RuntimeError, RuntimeGraph, RuntimeKind};` (keep the module public so tests reach `runtime::prometheus::parse`).

- [ ] **Step 7: Run the tests until green**

Run: `cargo test -p blastradius-core 2>&1 | grep -E 'FAILED|panicked|test result'`
Expected: all pass. Likely first failures: the fuzzy score string (see Task 4's note; use the value `strsim` gives); the `RuntimeServices { runtime: 10, matched: 8 }` count (ten distinct runtime names in `traces.prom`; eight have a service; `load-generator` is ignored and `auth-proxy` unmatched); the Datadog `matched: 5` (`checkout-api`, `payment`, `catalogue-service`, `orders`, `notifications` matched; `auth-proxy` not).

- [ ] **Step 8: Format, lint, commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E '^test result|FAILED'
git add crates test/fixtures/runtime-app
git commit -m "feat(runtime): merge runtime calls into the graph under the build spec's rules" -m "A call between two matched services confirms every compatible static edge on the pair, which becomes Observed with the summed count and a runtime evidence entry, or adds a new Observed edge when no static edge exists. Static edges nothing confirmed keep their confidence. Calls with an unmatched or ignored end are counted as skipped, never guessed. The runtime block reports the source, the name mapping, the unmatched names, edge counts and fuzzy-match warnings."
```

---

### Task 6: Fetching, input detection and `load`

**Files:**
- Create: `crates/blastradius-core/src/runtime/fetch.rs`
- Modify: `crates/blastradius-core/src/runtime/mod.rs` (`pub mod fetch;`, `RuntimeInput`, `detect`, `load`), `Cargo.toml` (`ureq = "3"`), `crates/blastradius-core/Cargo.toml`, `crates/blastradius-core/src/lib.rs`

**Interfaces:**
- Produces:
  ```rust
  // runtime/mod.rs
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum RuntimeInput { Otel(String), DatadogFile(String), DatadogLive { site: String, env: String } }
  pub fn detect(text: &str, input: &str) -> Result<RuntimeGraph, RuntimeError>
  pub fn load(input: &RuntimeInput) -> Result<RuntimeGraph, RuntimeError>
  // runtime/fetch.rs
  pub fn is_url(input: &str) -> bool
  pub fn read(input: &str) -> Result<String, RuntimeError>            // file, or URL when is_url
  pub fn http_get(url: &str, headers: &[(&str, &str)]) -> Result<String, RuntimeError>
  pub fn datadog_live(site: &str, env: &str) -> Result<String, RuntimeError>   // keys from DD_API_KEY, DD_APP_KEY
  pub const TIMEOUT_SECONDS: u64 = 10;
  ```

- [ ] **Step 1: Write the failing tests**

`runtime/mod.rs` tests, add:

```rust
    #[test]
    fn detects_the_shape_of_an_otel_input() {
        let prom = "traces_service_graph_request_total{client=\"a\",server=\"b\"} 1\n";
        assert_eq!(detect(prom, "x.prom").unwrap().method, "otel servicegraph");
        let otlp = r#"{"resourceSpans":[{"resource":{"attributes":[{"key":"service.name","value":{"stringValue":"a"}}]},"scopeSpans":[{"spans":[{"spanId":"1","parentSpanId":"","kind":3,"attributes":[{"key":"peer.service","value":{"stringValue":"b"}}]}]}]}]}"#;
        assert_eq!(detect(otlp, "x.json").unwrap().method, "otlp spans");
        let err = detect("hello\n", "x.txt").unwrap_err();
        assert!(matches!(err, RuntimeError::Unsupported(_)), "{err}");
        assert!(err.to_string().contains("traces_service_graph_request_total") && err.to_string().contains("OTLP"), "{err}");
    }

    #[test]
    fn load_reads_files_and_reports_missing_ones() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/runtime-app/runtime");
        let g = load(&RuntimeInput::Otel(root.join("traces.prom").display().to_string())).unwrap();
        assert_eq!(g.method, "otel servicegraph");
        let g = load(&RuntimeInput::DatadogFile(root.join("datadog.json").display().to_string())).unwrap();
        assert_eq!(g.source, RuntimeSource::Datadog);
        let err = load(&RuntimeInput::Otel("/nonexistent/traces.prom".into())).unwrap_err();
        assert!(matches!(err, RuntimeError::Io { .. }), "{err}");
        assert!(err.to_string().contains("/nonexistent/traces.prom"), "{err}");
    }
```

`runtime/fetch.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_recognised_and_live_datadog_needs_keys() {
        assert!(is_url("http://collector:8889/metrics") && is_url("https://x/y"));
        assert!(!is_url("traces.prom") && !is_url("/tmp/x.json") && !is_url("httpd.conf"));
        // The test binary's environment: make sure the keys are absent, then expect a Config error naming them.
        std::env::remove_var("DD_API_KEY");
        std::env::remove_var("DD_APP_KEY");
        let err = datadog_live("datadoghq.com", "prod").unwrap_err();
        assert!(matches!(err, RuntimeError::Config(_)), "{err}");
        assert!(err.to_string().contains("DD_API_KEY"), "{err}");
    }
}
```

(`remove_var` is process-wide; this is the only test touching those variables, and the CLI tests set them per subprocess.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p blastradius-core --lib runtime 2>&1 | grep -E 'error|test result' | head`
Expected: compile errors (`detect`, `load`, `RuntimeInput`, `fetch` missing).

- [ ] **Step 3: Dependency and `fetch.rs`**

Workspace `Cargo.toml`: `ureq = "3"` under `[workspace.dependencies]`; core crate: `ureq.workspace = true`. `ureq` 3's defaults bring rustls and gzip; do not enable `native-tls`.

```rust
//! Reading a runtime input: a file, or a URL only when the user gave one.
//! The static path never comes here. Failures are errors with the reason,
//! never silently empty data.
use std::time::Duration;

use super::RuntimeError;

pub const TIMEOUT_SECONDS: u64 = 10;

pub fn is_url(input: &str) -> bool {
    input.starts_with("http://") || input.starts_with("https://")
}

/// A file's contents, or the body of a GET when `input` is a URL.
pub fn read(input: &str) -> Result<String, RuntimeError> {
    if is_url(input) {
        return http_get(input, &[]);
    }
    std::fs::read_to_string(input).map_err(|source| RuntimeError::Io {
        path: input.to_string(),
        source,
    })
}

pub fn http_get(url: &str, headers: &[(&str, &str)]) -> Result<String, RuntimeError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(TIMEOUT_SECONDS)))
        .build()
        .into();
    let mut request = agent.get(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let mut response = request
        .call()
        .map_err(|e| RuntimeError::Http(format!("GET {url}: {e}")))?;
    response
        .body_mut()
        .read_to_string()
        .map_err(|e| RuntimeError::Http(format!("GET {url}: reading the body: {e}")))
}

/// Datadog's dependency map for one environment, authenticated from the
/// environment: `DD_API_KEY` and `DD_APP_KEY`. Keys are never flags.
pub fn datadog_live(site: &str, env: &str) -> Result<String, RuntimeError> {
    let api_key = std::env::var("DD_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())
        .ok_or_else(|| RuntimeError::Config("DD_API_KEY is not set; a live Datadog call needs DD_API_KEY and DD_APP_KEY in the environment".into()))?;
    let app_key = std::env::var("DD_APP_KEY")
        .ok()
        .filter(|k| !k.is_empty())
        .ok_or_else(|| RuntimeError::Config("DD_APP_KEY is not set; a live Datadog call needs DD_API_KEY and DD_APP_KEY in the environment".into()))?;
    let url = super::datadog::url(site, env);
    http_get(&url, &[("DD-API-KEY", &api_key), ("DD-APPLICATION-KEY", &app_key)])
}
```

`ureq` 3 API as used above: `Agent::config_builder().timeout_global(Some(d)).build()` gives a config, `.into()` an `Agent`; `agent.get(url).header(k, v).call()` returns `http::Response<Body>`; `body_mut().read_to_string()`. Non-2xx statuses are `Err(ureq::Error::StatusCode(_))` by default, which the `map_err` turns into `RuntimeError::Http` with the code in the message. If the installed `ureq` 3.x names any of these differently, read `cargo doc -p ureq --open` (or the crate's `docs.rs` page) and use the equivalent; note the change in the report.

- [ ] **Step 4: `RuntimeInput`, `detect`, `load` in `runtime/mod.rs`**

```rust
pub mod fetch;

/// What the CLI asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeInput {
    /// A path or URL holding a servicegraph scrape or an OTLP JSON export.
    Otel(String),
    /// A path or URL holding a saved service_dependencies response.
    DatadogFile(String),
    /// Call the Datadog API for this site and environment.
    DatadogLive { site: String, env: String },
}

/// An `--otel` input by its shape: JSON is an OTLP export, text with the
/// servicegraph metric is a Prometheus scrape.
pub fn detect(text: &str, input: &str) -> Result<RuntimeGraph, RuntimeError> {
    let trimmed = text.trim_start();
    if trimmed.starts_with('{') {
        return otlp::parse(text, input);
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
```

Re-export `RuntimeInput` from `lib.rs`.

- [ ] **Step 5: Run, format, lint, commit**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E '^test result|FAILED'`
Expected: all green; no test made a network call (the only URL-taking code paths are exercised with files or fail before the request).

```bash
git add Cargo.toml Cargo.lock crates
git commit -m "feat(runtime): load a runtime input from a file or, when asked, a URL" -m "An --otel input is read from disk, or fetched with a ten-second timeout when it is a URL, and its shape decides the parser. A live Datadog call takes its keys from DD_API_KEY and DD_APP_KEY only. Every failure is an error with the path or URL and the reason."
```

---

### Task 7: CLI flags, report header, RUNTIME section, FINDINGS line

**Files:**
- Modify: `crates/blastradius-cli/src/main.rs`, `crates/blastradius-cli/tests/cli.rs`
- Modify: `crates/blastradius-core/src/report.rs`, `crates/blastradius-core/tests/runtime.rs`

**Interfaces:**
- Consumes: `RuntimeInput`, `runtime::load`, `runtime::join`, `config::load` (Tasks 4–6); `Runtime` block fields (Task 1).
- Produces: flags `--otel <PATH|URL>`, `--datadog [<PATH|URL>]`, `--dd-env <ENV>`, `--dd-site <SITE>` (default `datadoghq.com`); the report additions.

- [ ] **Step 1: Write the failing tests**

In `tests/runtime.rs` add:

```rust
#[test]
fn report_with_a_runtime_source_shows_the_join() {
    let r = format_repo_report(&joined("traces.prom"), false);
    assert!(r.contains("connected (OTel, 8 of 10 runtime services matched)"), "{r}");
    assert!(r.contains("\nRUNTIME\n"), "{r}");
    assert!(regex::Regex::new(r"Source\s+OTel\s+traces\.prom").unwrap().is_match(&r), "{r}");
    assert!(regex::Regex::new(r"Edges\s+4 observed \(3 static confirmed, 1 runtime only\) · 2 calls skipped").unwrap().is_match(&r), "{r}");
    assert!(regex::Regex::new(r"chckout\s+checkout\s+fuzzy 0\.97\s+\(check this\)").unwrap().is_match(&r), "{r}");
    assert!(regex::Regex::new(r"load-generator\s+-\s+ignored \(blast-radius\.config\.json\)").unwrap().is_match(&r), "{r}");
    assert!(regex::Regex::new(r"pay\s+payment\s+config").unwrap().is_match(&r), "{r}");
    assert!(r.contains("1 runtime service matched nothing: auth-proxy"), "{r}");
    assert!(regex::Regex::new(r"checkout\s+->\s+payment\s+http\s+observed").unwrap().is_match(&r), "{r}");
    assert!(regex::Regex::new(r"Static edges never observed\s+1").unwrap().is_match(&r), "{r}");
    assert!(r.contains("orders -> rabbitmq"), "{r}");
}

#[test]
fn report_without_a_runtime_source_is_unchanged() {
    let r = format_repo_report(&analyze(&fixture("runtime-app")).unwrap(), false);
    assert!(r.contains("not connected - static only"), "{r}");
    assert!(!r.contains("RUNTIME\n") && !r.contains("never observed"), "{r}");
}
```

Add `regex.workspace = true` under `[dev-dependencies]` of the core crate if the `edges.rs` tests do not already bring it (they do).

In `crates/blastradius-cli/tests/cli.rs` add:

```rust
fn runtime_fixture(file: &str) -> String {
    fixture("runtime-app").join("runtime").join(file).to_str().unwrap().to_string()
}

#[test]
fn otel_flag_joins_a_servicegraph_scrape() {
    let out = bin()
        .args(["analyze", fixture("runtime-app").to_str().unwrap(), "--otel", &runtime_fixture("traces.prom"), "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["runtime"]["connected"], true);
    assert_eq!(json["runtime"]["source"], "otel");
    assert_eq!(json["runtime"]["services"]["matched"], 8);
    let report = bin()
        .args(["analyze", fixture("runtime-app").to_str().unwrap(), "--otel", &runtime_fixture("traces.prom")])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&report.stdout);
    assert!(text.contains("RUNTIME") && text.contains("8 of 10 runtime services matched"), "{text}");
}

#[test]
fn datadog_flag_reads_a_saved_response() {
    let out = bin()
        .args(["analyze", fixture("runtime-app").to_str().unwrap(), "--datadog", &runtime_fixture("datadog.json"), "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["runtime"]["source"], "datadog");
    assert_eq!(json["runtime"]["edges"]["runtimeOnly"], 1);
}

#[test]
fn runtime_errors_exit_1_and_print_no_report() {
    let missing = bin()
        .args(["analyze", fixture("runtime-app").to_str().unwrap(), "--otel", "/nonexistent/traces.prom"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("/nonexistent/traces.prom"));
    assert!(missing.stdout.is_empty());

    let both = bin()
        .args(["analyze", fixture("runtime-app").to_str().unwrap(), "--otel", "a", "--datadog", "b"])
        .output()
        .unwrap();
    assert_eq!(both.status.code(), Some(2), "clap rejects the conflict");

    let no_env = bin()
        .args(["analyze", fixture("runtime-app").to_str().unwrap(), "--datadog"])
        .output()
        .unwrap();
    assert_eq!(no_env.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&no_env.stderr).contains("--dd-env"), "{}", String::from_utf8_lossy(&no_env.stderr));

    let no_keys = bin()
        .args(["analyze", fixture("runtime-app").to_str().unwrap(), "--datadog", "--dd-env", "prod"])
        .env_remove("DD_API_KEY")
        .env_remove("DD_APP_KEY")
        .output()
        .unwrap();
    assert_eq!(no_keys.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&no_keys.stderr).contains("DD_API_KEY"), "{}", String::from_utf8_lossy(&no_keys.stderr));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test 2>&1 | grep -E 'error|FAILED|test result' | head`
Expected: the new report tests fail (no RUNTIME section); the CLI tests fail with clap's "unexpected argument '--otel'".

- [ ] **Step 3: CLI**

In `main.rs`, add to `Cmd::Analyze`:

```rust
        /// Join with OpenTelemetry runtime data: a Prometheus scrape holding the
        /// servicegraph metric, or an OTLP JSON span export (path or URL)
        #[arg(long, value_name = "PATH|URL", conflicts_with = "datadog")]
        otel: Option<String>,
        /// Join with Datadog's service dependency map: a saved response (path or
        /// URL), or bare to call the API with --dd-env and DD_API_KEY/DD_APP_KEY
        #[arg(long, value_name = "PATH|URL", num_args = 0..=1, default_missing_value = "")]
        datadog: Option<String>,
        /// Datadog environment for a live call
        #[arg(long = "dd-env", value_name = "ENV", requires = "datadog")]
        dd_env: Option<String>,
        /// Datadog site for a live call
        #[arg(long = "dd-site", value_name = "SITE", default_value = "datadoghq.com")]
        dd_site: String,
```

and in `run`, after `let analysis = blastradius::analyze(&path)?;` (make it `let mut analysis`):

```rust
            if let Some(input) = runtime_input(otel, datadog, dd_env, dd_site)? {
                let config = blastradius::config::load(&analysis.root)?.runtime;
                let graph = blastradius::runtime::load(&input)?;
                blastradius::runtime::join(&mut analysis, graph, &config)?;
            }
```

with, at module level:

```rust
use blastradius::RuntimeInput;

/// Which runtime source the flags ask for, if any. A bare `--datadog` means a
/// live call, which needs `--dd-env`.
fn runtime_input(
    otel: Option<String>,
    datadog: Option<String>,
    dd_env: Option<String>,
    dd_site: String,
) -> anyhow::Result<Option<RuntimeInput>> {
    if let Some(source) = otel {
        return Ok(Some(RuntimeInput::Otel(source)));
    }
    match datadog {
        None => Ok(None),
        Some(source) if !source.is_empty() => Ok(Some(RuntimeInput::DatadogFile(source))),
        Some(_) => {
            let env = dd_env.ok_or_else(|| {
                anyhow::anyhow!("--datadog without a file calls the Datadog API and needs --dd-env <ENV>")
            })?;
            Ok(Some(RuntimeInput::DatadogLive { site: dd_site, env }))
        }
    }
}
```

Errors propagate through the existing `anyhow` path: `blast-radius: <message>` on stderr, exit 1, nothing on stdout.

- [ ] **Step 4: Report**

In `report.rs`, replace the fixed `Runtime` row with:

```rust
    out.push(row("Runtime", &runtime_header(analysis, &c)));
```

```rust
fn runtime_header(analysis: &Analysis, c: &Paint) -> String {
    let r = &analysis.runtime;
    let (Some(source), Some(services)) = (r.source, r.services) else {
        return c.dim("not connected - static only");
    };
    let what = match r.input.as_deref() {
        Some(input) if input.starts_with("datadog env ") => {
            format!("{} {}", source.label(), input.trim_start_matches("datadog "))
        }
        _ => source.label().to_string(),
    };
    if services.matched < services.runtime {
        c.yellow(&format!(
            "connected ({what}, {} of {} runtime services matched)",
            services.matched, services.runtime
        ))
    } else {
        format!("connected ({what}, {} services matched)", services.matched)
    }
}
```

In `structure`, after `mapping_summary(...)` and before the `if edges.is_empty()` check, add `runtime_section(analysis, c, out);`:

```rust
/// The join, printed whether or not it is complete: a partial mapping that
/// looks complete is worse than none.
fn runtime_section(analysis: &Analysis, c: &Paint, out: &mut Vec<String>) {
    let r = &analysis.runtime;
    let (Some(source), Some(services), Some(edges)) = (r.source, r.services, r.edges) else {
        return;
    };
    out.push(String::new());
    out.push(c.bold("RUNTIME"));
    out.push(String::new());
    out.push(row("Source", &format!("{}  {}", source.label(), r.input.as_deref().unwrap_or(""))));
    out.push(row(
        "Services",
        &format!("{} of {} runtime services matched", services.matched, services.runtime),
    ));
    let noun = if edges.skipped == 1 { "call" } else { "calls" };
    out.push(row(
        "Edges",
        &format!(
            "{} observed ({} static confirmed, {} runtime only) · {} {noun} skipped, one end unmatched or ignored",
            edges.observed,
            edges.observed - edges.runtime_only,
            edges.runtime_only,
            edges.skipped
        ),
    ));
    out.push(String::new());
    let w_name = r.mapping.iter().map(|m| m.runtime.chars().count()).max().unwrap_or(12).max(12);
    let w_service = r.mapping.iter().filter_map(|m| m.service.as_ref()).map(|s| s.chars().count()).max().unwrap_or(7).max(7);
    out.push(format!("  {}", c.dim(&format!("{:<w_name$}  {:<w_service$}  HOW", "RUNTIME NAME", "SERVICE"))));
    for m in r.mapping.iter().filter(|m| m.how != "unmatched") {
        let how = match m.how.as_str() {
            "ignored" => "ignored (blast-radius.config.json)".to_string(),
            h if h.starts_with("fuzzy") => format!("{h}  (check this)"),
            h => h.to_string(),
        };
        let line = format!(
            "  {:<w_name$}  {:<w_service$}  {how}",
            m.runtime,
            m.service.as_deref().unwrap_or("-")
        );
        out.push(if m.how.starts_with("fuzzy") { c.yellow(&line) } else { line });
    }
    if !r.unmatched.is_empty() {
        out.push(String::new());
        let noun = if r.unmatched.len() == 1 { "service" } else { "services" };
        out.push(format!(
            "  {}",
            c.yellow(&format!(
                "{} runtime {noun} matched nothing: {}",
                r.unmatched.len(),
                wrap(&r.unmatched.join(", "), 60)
            ))
        ));
    }
}
```

In `findings`, after the "Shared databases" block, when `analysis.runtime.connected`:

```rust
    if runtime_connected {
        let never: Vec<String> = edges
            .iter()
            .filter(|e| e.observed.is_none() && e.edge_type != EdgeType::Import)
            .map(|e| format!("{} -> {}", e.source, e.target))
            .collect();
        out.push(String::new());
        out.push(format!("  {:<38} {}", "Static edges never observed", never.len()));
        if !never.is_empty() {
            out.push(format!("    {}", c.dim(&wrap(&never.join(", "), 60))));
        }
    }
```

`findings` currently takes `(edges, services, c, out)`; pass `analysis.runtime.connected` as a fifth argument `runtime_connected: bool` from `structure`. Also update the EDGES table: nothing to do, the confidence column already prints `observed` for `Confidence::Observed`, and `structure_counts` already prints `Observed in production` when non-zero.

- [ ] **Step 5: Run, format, lint, commit**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E '^test result|FAILED|panicked'`
Expected: all green. Then run the binary once and read the whole report:

```bash
cargo run -q -- analyze test/fixtures/runtime-app --otel test/fixtures/runtime-app/runtime/traces.prom --no-color
```

Check the RUNTIME table aligns, the header is yellow-worthy text (`8 of 10`), and FINDINGS ends with `Static edges never observed  1` and `orders -> rabbitmq`.

```bash
git add crates
git commit -m "feat(cli,report): --otel and --datadog join runtime data; RUNTIME section and never-observed finding" -m "analyze gains --otel <path|url>, --datadog [<path|url>], --dd-env and --dd-site. The header says how many runtime services matched, in the warning colour when not all did; a RUNTIME section prints the source, the counts and the whole name mapping with fuzzy matches marked and unmatched names listed; FINDINGS names the static edges production never took. Every runtime error exits 1 with the reason and no report."
```

---

### Task 8: Documentation

**Files:**
- Modify: `README.md`, `docs/superpowers/specs/2026-09-09-runtime-join-design.md`

- [ ] **Step 1: README**

Status table: the Stage 3 row becomes `| 3. Join | Optional runtime edges from OpenTelemetry (servicegraph scrape or OTLP JSON) or Datadog, with an explicit name-matching report | Built |`. In the paragraph under "## Status", change "Service discovery and static dependency mapping are built." to "Service discovery, static dependency mapping and the runtime join are built."

In the intro, after the `analyze` examples, add:

```bash
./target/release/blast-radius analyze /path/to/a/repository --otel traces.prom     # servicegraph scrape or OTLP JSON
./target/release/blast-radius analyze /path/to/a/repository --datadog deps.json    # saved service_dependencies response
```

Add a section "## How the runtime join works" after "## How mapping works":

```
Static edges say what the code could call. `--otel` and `--datadog` add what
production actually called. `--otel` takes a Prometheus scrape of the OpenTelemetry
Collector's servicegraph connector (`traces_service_graph_request_total`) or a raw
OTLP JSON span export, as a file or a URL; `--datadog` takes a saved
`service_dependencies` response, or calls the API when given `--dd-env` and the
`DD_API_KEY` and `DD_APP_KEY` environment variables. Nothing is fetched unless you
pass a URL or ask for the live call.

Runtime names rarely equal repository names. Each one is matched in order: an
entry in `blast-radius.config.json` (`{"runtime": {"map": {"checkout-api":
"checkout"}, "ignore": ["load-generator"]}}`), the exact name, the normalised name
(`checkout-api`, `CheckoutService` and `checkout` are the same), then a fuzzy match
that is flagged for you to check. The whole table is printed, and the header says
how many runtime services matched, so a partial join can never look complete.

Then the merge: a static edge production confirmed becomes Observed and carries
the call count; a call static analysis missed becomes a new Observed edge; a static
edge production never took keeps its label and is listed under FINDINGS. Nothing is
dropped.
```

Under "## Rules that hold throughout", change the "Works offline" bullet to: "**Works offline.** The static path never makes a network call. The runtime join reads a file, or fetches only the URL you give it."

Add a sample after the mapping sample, headed with one sentence ("With a servicegraph scrape from the fixture repository:"), pasting the real RUNTIME section and the FINDINGS block from:

```bash
cargo run -q --release -- analyze test/fixtures/runtime-app --otel test/fixtures/runtime-app/runtime/traces.prom --no-color
```

Trim to the header's Runtime line, the RUNTIME section and FINDINGS.

- [ ] **Step 2: Spec decisions**

Append under "## Decisions made while building" in `docs/superpowers/specs/2026-09-09-runtime-join-design.md`, replacing "(Appended during implementation.)":

```
- The `Runtime` struct keeps its name (the spec draft said `RuntimeJoin`); it
  was already exported, and the block's shape is what matters.
- The JSON `runtime` block does not repeat the parser method (`otel
  servicegraph` versus `otlp spans`); `source` plus `input` identify the data,
  and the evidence detail on each edge names the method.
- A bare `--datadog` is parsed as an empty string and means "call the API";
  a value is a file or URL.
- When two runtime pairs confirm one static edge (`checkout-api -> payment`
  and the fuzzy `chckout -> payment`), the counts are summed into one
  `observed` and the single runtime evidence entry is rewritten with the
  running total, so the table stays one row per edge.
- `confirmed` in the merge counts is informational; the block reports
  `observed`, `runtimeOnly` and `skipped`, from which "static confirmed" is
  `observed - runtimeOnly`.
- Fuzzy matching runs on normalised names, so `checkout-api` and
  `CheckoutService` never need the fuzzy tier; it exists for typos and
  abbreviations (`chckout`, `paymnt`) and is always flagged.
- <anything found during implementation, one line each>
```

- [ ] **Step 3: Final check and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E '^test result|FAILED'
git add README.md docs/superpowers/specs/2026-09-09-runtime-join-design.md
git commit -m "docs: runtime join built; how matching and merging work; decisions made while building" -m "Stage 3 is built. The README explains the two sources, the matching order, the merge rules and shows the RUNTIME section on the fixture; the spec records the decisions the implementation forced."
```

---

## Self-review

**Spec coverage.** CLI surface (`--otel`, `--datadog`, `--dd-env`, `--dd-site`, mutual exclusion, bare `--datadog` = live): Task 7. Sources — Prometheus text with `connection_type` typing and `_total_total`: Task 2; OTLP JSON pairing by parent and `peer.service`, typing by semantic conventions: Task 3; Datadog `calls`/`calls_out`, URL: Task 3. Detection and fetching with timeout, keys from the environment only: Task 6. Matching tiers with config first, the undiscovered-service error, fuzzy threshold and minimum length, warnings: Task 4. Merge rules including `import` never promoted, self calls dropped, skipped counted, evidence entry shape: Task 5. JSON contract additions and the `{ "connected": false }` invariant: Task 1, asserted in Tasks 1 and 5. Report header, RUNTIME section, never-observed finding: Task 7. Config file semantics (absent = default, invalid = error, unknown keys ignored): Task 4. Fixture with every tier and rule: Tasks 4 and 5. Corpus and parity untouched: every task's gate. Out-of-scope items are not implemented anywhere.

**Placeholders.** The only open line is `<anything found during implementation, one line each>` in Task 8, filled from the implementation.

**Type consistency.** `RuntimeSource` lives in `model.rs` (Task 1) and is re-exported from `runtime/mod.rs` (Task 2). `RuntimeGraph::method` is `&'static str` and is set by each parser to the three literals the merge's `detail` uses (Tasks 2, 3, 5). `Mapping.how: MatchHow` (Task 4) becomes `RuntimeMapping.how: String` through `MatchHow::as_str` in `join` (Task 5); the report matches on that string (Task 7). `RuntimeEdges { observed, runtime_only, skipped }` (Task 1) is filled from `MergeCounts` (Task 5) and read by the report (Task 7). `RuntimeInput` is defined in Task 6 and consumed by the CLI in Task 7. `graph.edges_mut()` and `sort_edges()` (Task 5) are the only new `BlastGraph` methods. `config::load` returns `Config`; callers take `.runtime` (Tasks 5, 7).
