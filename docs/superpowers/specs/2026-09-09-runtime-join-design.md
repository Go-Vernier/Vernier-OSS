# Stage 3: runtime join — design

Date: 2026-09-09. Status: approved in conversation, implementation follows.
Builds on `docs/build-spec.md` § "Stage 3 — Runtime join" and on the Stage 2
engine (`docs/superpowers/specs/2026-09-06-rust-engine-stage2-design.md`).

## Decision

Static edges say what the code *could* call. The runtime join adds what
production *actually* calls, from one of two sources the user already has:
OpenTelemetry's servicegraph metric (or a raw span export) and Datadog's
service dependency map. Both are optional. `analyze` without them behaves
exactly as today, and the static path still makes no network call.

Runtime data is read from a file first and from a URL only when the user
gives one, so every test runs on fixture files and a saved export is as good
as a live endpoint. Two crates are added: `ureq` (HTTP, rustls) for the
optional fetch, `strsim` for fuzzy name matching.

## CLI

```bash
blast-radius analyze . --otel traces.prom                # Prometheus text scrape, servicegraph metric
blast-radius analyze . --otel spans.json                 # OTLP JSON span export
blast-radius analyze . --otel http://collector:8889/metrics
blast-radius analyze . --datadog deps.json               # saved GET /api/v1/service_dependencies
blast-radius analyze . --datadog --dd-env prod           # live, keys from DD_API_KEY and DD_APP_KEY
blast-radius analyze . --datadog --dd-env prod --dd-site datadoghq.eu
```

`--otel` and `--datadog` are mutually exclusive in this stage. `--otel`
takes a path or an `http(s)://` URL. `--datadog` takes an optional path or
URL; without one it calls the API and requires `--dd-env` plus the two
environment variables. API keys are never accepted as flags, so they do not
land in shell history. `--json` prints the contract below; the terminal
report gains a RUNTIME section.

## Sources

Every source produces the same thing:

```rust
pub enum RuntimeSource { Otel, Datadog }
pub enum RuntimeKind { Http, Grpc, Event, Database, Unknown }
pub struct RuntimeCall { pub client: String, pub server: String, pub calls: Option<u64>, pub kind: RuntimeKind }
pub struct RuntimeGraph {
    pub source: RuntimeSource,
    pub input: String,                 // path as given, or the URL
    pub services: BTreeSet<String>,    // every runtime service name seen
    pub calls: Vec<RuntimeCall>,       // one per (client, server), counts summed
}
```

**Prometheus text.** Lines of the form
`traces_service_graph_request_total{client="a",server="b",connection_type=""} 123`
(an optional timestamp after the value is ignored; the `_total_total` spelling
some exporters produce is accepted). Values are summed over label variants
for the same pair. The `connection_type` label the servicegraph connector
emits types the call: `messaging_system` → Event, `database` → Database,
anything else → Unknown. `virtual_node` pairs are kept (the virtual node is
a runtime service name like any other). `traces_service_graph_request_failed_total`
is not read: it counts a subset of the same requests.

**OTLP JSON.** The export shape `resourceSpans[].resource.attributes[]` gives
each resource's `service.name`; `scopeSpans[].spans[]` gives spans with
`spanId`, `parentSpanId`, `kind` (the integer 1–5 or the `SPAN_KIND_*` name)
and `attributes[]`. A call is derived two ways and counted once per span:
a span whose parent lives in a different service gives `parent service →
span service`; a CLIENT or PRODUCER span with a `peer.service` attribute
gives `span service → peer.service`. The kind comes from the span: a
`messaging.system` attribute or a PRODUCER/CONSUMER kind → Event;
`db.system` → Database; `rpc.system` equal to `grpc` → Grpc; otherwise
Http. Spans per pair are the call count.

**Datadog.** `GET https://api.<site>/api/v1/service_dependencies?env=<env>`
with headers `DD-API-KEY` and `DD-APPLICATION-KEY`, or a file holding that
response: `{ "<service>": { "calls": ["<service>", ...] } }`. The key
`calls_out` is accepted as a synonym. Datadog gives no counts and no
protocol: `calls` is None and the kind is Unknown.

**Detection.** For `--otel`, content starting with `{` is OTLP JSON; content
containing `traces_service_graph_request_total` is a Prometheus scrape;
anything else is an error naming both accepted shapes.

**Fetching.** A path is read from disk; an `http://` or `https://` input is
fetched with a 10 second timeout and no redirects across hosts. Any failure
(unreadable file, non-2xx status, timeout, missing keys) is an error that
exits 1 with the reason; the static analysis is not printed in that case,
because a half-run that looks complete is worse than none.

## Matching runtime names to discovered services

Each runtime service name is resolved once, in this order, and the first
tier that answers wins:

| Tier | Rule | Reported as |
| --- | --- | --- |
| config | `blast-radius.config.json` at the repository root: `{"runtime": {"map": {"checkout-api": "checkout"}, "ignore": ["load-generator"]}}` | `config`, or `ignored` |
| exact | the runtime name equals a discovered service's name | `exact` |
| normalised | `normalise()` from discovery (lowercase, drop `-_.`, strip a trailing `service`, `svc`, `api`, `server`, `deployment`, `deploy`) equal on both sides | `normalised` |
| fuzzy | Jaro–Winkler similarity of the normalised names ≥ 0.9, both at least four characters; the best-scoring service wins | `fuzzy 0.93`, and a warning |
| none | | `unmatched` |

The config is consulted first, not fourth as the build spec lists it: an
explicit mapping must beat a wrong fuzzy guess. A `map` entry naming a
service that was not discovered is an error. Several runtime names may map
to one service (`checkout` and `checkout-api` both to `checkout`).

The result is printed whether or not it is complete. When fewer runtime
services matched than existed, the report header says
`connected (OTel, 6 of 42 runtime services matched)` in the warning colour,
and the RUNTIME section lists every unmatched name. A partial join is never
silent and never an error.

## Merge rules

For each `RuntimeCall` whose client and server both matched, with `a → b`
the matched services:

- `a == b`: dropped (a self call).
- Static edges exist between `a` and `b` whose type is compatible with the
  call's kind: each becomes `Observed`, gains `observed { calls, source }`
  and a runtime evidence entry. Compatible means: Unknown or Http kind →
  `http` and `grpc` edges; Grpc → `grpc`, then `http`; Event → `event`;
  Database → `database`. `import` edges are never promoted: an import is not
  a call.
- No compatible static edge: a new edge `a → b` is added with confidence
  `Observed`, type from the kind (Unknown → `http`), the runtime evidence as
  its only evidence.
- A static edge no runtime call confirmed keeps its confidence and type.
  Nothing is ever dropped.

A call with an unmatched or ignored end is skipped and counted. The
runtime evidence entry is `Evidence { file: <input path or URL>, line: None,
detail: "123 calls (otel servicegraph)" | "observed (otlp spans, 17 spans)"
| "observed (datadog)" }`.

## JSON contract

Additive. Nothing existing changes shape or order.

```json
"edges": [{ "source", "target", "type", "confidence", "evidence": [...],
            "observed": { "calls": 123, "source": "otel" } }],
"runtime": {
  "connected": true,
  "source": "otel",
  "input": "traces.prom",
  "services": { "runtime": 42, "matched": 6 },
  "mapping": [{ "runtime": "checkout-api", "service": "checkout", "how": "normalised" },
              { "runtime": "chckout", "service": "checkout", "how": "fuzzy 0.93" },
              { "runtime": "load-generator", "service": null, "how": "ignored" }],
  "unmatched": ["auth-proxy", "..."],
  "edges": { "observed": 11, "runtimeOnly": 7, "skipped": 9 },
  "warnings": ["fuzzy match: chckout -> checkout (0.93)"]
}
```

`observed` is omitted from an edge that was never observed; `calls` is
omitted when the source has no counts. Without `--otel`/`--datadog`,
`runtime` stays exactly `{ "connected": false }`. `mapping` lists every
runtime name in sorted order, matched or not, so a consumer can rebuild
the join; `unmatched` repeats the unmatched ones for the reader.

## Report additions

Header: `Runtime  connected (OTel, 42 services matched)` or, when partial,
`connected (OTel, 6 of 42 runtime services matched)` in yellow; a live
Datadog call says `connected (Datadog env prod, ...)`.

STRUCTURE already prints `Observed in production` when the count is
non-zero. A RUNTIME section follows it:

```
RUNTIME

  Source        OTel servicegraph  traces.prom
  Services      6 of 42 runtime services matched
  Edges         11 observed (4 static confirmed, 7 runtime only) · 9 calls skipped, one end unmatched

  RUNTIME NAME    SERVICE    HOW
  checkout-api    checkout   normalised
  chckout         checkout   fuzzy 0.93  (check this)
  load-generator  -          ignored (blast-radius.config.json)
  pay             payment    config

  36 runtime services matched nothing: auth-proxy, billing-legacy, ...
```

The EDGES table's confidence column shows `observed` for promoted and new
edges. FINDINGS gains `Static edges never observed  N`, listing the pairs,
so a code path production never took is visible.

## Configuration file

`blast-radius.config.json` at the repository root, read only when a runtime
source is given:

```json
{ "runtime": { "map": { "checkout-api": "checkout" }, "ignore": ["load-generator"] } }
```

Unknown keys are ignored (future stages add theirs). Invalid JSON is an
error: a file the user wrote must not be silently skipped.

## Architecture

```
crates/blastradius-core/src/runtime/mod.rs        RuntimeSource, RuntimeKind, RuntimeCall, RuntimeGraph, RuntimeJoin; load(input) and join(analysis)
crates/blastradius-core/src/runtime/prometheus.rs parse(text) -> RuntimeGraph
crates/blastradius-core/src/runtime/otlp.rs       parse(text) -> RuntimeGraph
crates/blastradius-core/src/runtime/datadog.rs    parse(text) -> RuntimeGraph; request URL and headers
crates/blastradius-core/src/runtime/fetch.rs      read(input) -> String: file or URL (ureq), Datadog live call
crates/blastradius-core/src/runtime/matching.rs   match_names(runtime services, discovered services, config) -> Vec<Mapping>
crates/blastradius-core/src/runtime/merge.rs      apply(graph, calls, mapping) -> counts; edge promotion and new edges
crates/blastradius-core/src/config.rs             blast-radius.config.json
crates/blastradius-core/src/model.rs              Edge.observed
crates/blastradius-core/src/analyze.rs            Runtime replaced by RuntimeJoin (serialises to the block above)
crates/blastradius-core/src/report.rs             header, RUNTIME section, FINDINGS line
crates/blastradius-cli/src/main.rs                --otel, --datadog, --dd-env, --dd-site
```

`analyze(root)` keeps its signature. The CLI calls `analyze`, then
`runtime::load(&input)` and `runtime::join(&mut analysis, graph, &config)`.
The library exposes the same two calls.

## Validation

- Fixture `test/fixtures/runtime-app/`: a compose file with `checkout`,
  `payment`, `catalogue`, `orders`, `notifications`, a `rabbitmq` image and a
  `redis` image; sources giving static edges `checkout → payment` (http),
  `checkout → catalogue` (http), `orders → rabbitmq` (event),
  `payment → redis` (database); `blast-radius.config.json` mapping `pay` to
  `payment` and ignoring `load-generator`; `runtime/traces.prom`,
  `runtime/spans.json`, `runtime/datadog.json`.
- `traces.prom` exercises every tier and rule: `checkout-api → payment`
  (normalised; confirms a static edge), `checkout-api → catalogue-service`
  (normalised both sides), `orders → notifications` with
  `connection_type="messaging_system"` (runtime only, event), `chckout →
  payment` (fuzzy, warning, merges into the same edge), `pay → redis`
  (config; confirms the database edge), `load-generator → checkout-api`
  (ignored), `auth-proxy → payment` (unmatched, skipped).
- Unit tests per parser on inline text; integration tests on the fixture for
  the mapping table, every merge rule, the JSON block and the report; a CLI
  test per flag including the error paths (missing file, both flags, live
  Datadog without keys). No test touches the network.
- Corpus: unchanged; no corpus repository ships traces. The corpus test must
  stay green and the parity test must not change.

## Out of scope

The blast-radius walk (Stage 4), the HTML report, a `fetch` subcommand that
saves a snapshot, historical windows and "last observed" dates (neither
source above carries them), Jaeger and Tempo APIs, sampling correction.

## Decisions made while building

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
- `join` takes `RuntimeGraph` by value so the input path can move into the
  report block rather than being cloned.
- Matching tests look up rows by runtime name, not input order: `match_names`
  walks a `BTreeSet`, so the table is sorted.
- OTLP's `Span.span_id` keeps the protocol field name; clippy's
  `struct_field_names` is allowed on that struct because renaming it would
  lie about the export.
- Edition 2024 makes `std::env::remove_var` unsafe; the live-Datadog unit
  test wraps the two key removals in an `unsafe` block with a SAFETY comment.
- Review fixes after the first implementation: the runtime evidence entry
  goes first on a confirmed edge (evidence is ordered strongest first, and
  the EDGES table shows the first entry, so the call count is what a reader
  sees); a request that carries Datadog keys follows no redirect; the
  partial-join warning colour ignores names the config ignores on purpose,
  and the RUNTIME section says how many were ignored; "Static edges never
  observed" leaves out shared-database edges between two code services,
  which are not calls and can never be observed; an `--otel` file that is
  really a Datadog response says so and points at `--datadog`.
