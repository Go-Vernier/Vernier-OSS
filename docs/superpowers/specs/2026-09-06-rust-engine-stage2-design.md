# Rust engine and Stage 2 static edges — design

Date: 2026-09-06. Status: approved in conversation, implementation follows.

## Decision

The engine moves from TypeScript to Rust before Stage 2, because Stage 2
scans every source file and tree-sitter is first-class in Rust. Everything
around the engine that faces a browser or another tool (the HTML report's
script, the GitHub Action, the MCP server, the npm wrapper) stays
TypeScript. The JSON the engine prints is the contract between the two.

Two side effects were approved explicitly: the Rust toolchain is installed
on the development machine through Homebrew's rustup, and the TypeScript
engine is deleted once the Rust engine reproduces its discovery output on
every fixture and every corpus repository.

## Repository shape

```
Cargo.toml                      workspace
crates/
  blastradius-core/             library: discover, facts, map, graph, model, report
  blastradius-cli/              binary `blast-radius`: clap, terminal output, --json
test/fixtures/                  unchanged, shared by both crates' tests
test/expected/
  discovery/<fixture>.json      TypeScript baseline, checked by a parity test
  corpus/<repo>.json            hand-written expected services and edges
scripts/corpus.sh               unchanged
docs/build-spec.md              unchanged
package.json                    name reserved; becomes the npm wrapper in the release session
.github/workflows/ci.yml        cargo fmt --check, clippy -D warnings, test; ubuntu + macos
```

Deleted after parity: `src/`, `test/*.test.ts`, `tsup.config.ts`,
`vitest.config.ts`, `tsconfig.json`, `pnpm-lock.yaml`, `dist/`.

## JSON contract

Field names and shapes stay exactly as the TypeScript engine printed them.

```json
{
  "repository": "owner/repo",
  "root": "/abs/path",
  "discovery": { "strategy": "docker-compose", "attempted": [{ "strategy": "docker-compose", "services": 11 }] },
  "services": [{ "name", "root", "language", "entryPoints", "role", "discoveredBy", "evidence": { "file", "line", "detail" }, "image", "packageName" }],
  "edges": [{ "source", "target", "type", "confidence", "evidence": [{ "file", "line", "detail" }] }],
  "mapping": { "filesScanned", "filesSkipped": { "<language or ext>": n }, "filesOutsideServices", "unresolved", "parsers": { "<language>": "tree-sitter" | "regex" } },
  "runtime": { "connected": false }
}
```

`mapping` is new. It exists so the report can say what was not read. Optional
fields (`line`, `detail`, `image`, `packageName`) are omitted when absent,
never printed as null. `root` is null for image-only services, as before.

Ordering: services sort code-first then by name, case-insensitive. Edges sort
by source, target, type. Evidence within an edge sorts by file then line.

## Stage 1 port

A straight port of the four strategies plus the fallback, module for module.
Behaviour that must survive, because each came from a corpus failure:

- compose `${VAR}` interpolation from `.env.example` then `.env`, unresolved
  references left visible in the evidence
- Dockerfile-directory detection when every service builds from the root
- image-only services matched to a directory by name or image basename, three
  tiers (exact, normalised, suffix after a separator), shallower wins
- Helm templates skipped when the text contains `{{`
- a workload beats a Service object of the same name; code beats infrastructure
- the deploy-only repository keeps its images so the report can name it

Two deliberate improvements over the TypeScript version: the YAML loader
understands anchors, aliases and `<<` merge keys, and Cargo workspace
members are read with a real TOML parser.

File walking uses the `ignore` crate: hidden entries and the fixed list
(`node_modules`, `vendor`, `dist`, `build`, `target`, `.venv`, `bin`, `obj`,
...) are skipped, and `.gitignore` is honoured. If honouring `.gitignore`
changes any corpus count, the parity test says so and the decision is
revisited then.

YAML with line numbers: a small marked tree built on the event parser of
`libyaml-safer`, a Rust port of libyaml. The yaml-rust family was tried
first and rejected the OpenTelemetry demo's compose file (a flow sequence
closed at the key's indentation), which Docker Compose accepts. libyaml also
reads sock-shop's `grafana-import-dashboards` Job, which the TypeScript
engine's parser failed on, so that repository gains one infrastructure
service over the baseline. Every mapping key carries its 1-based line.
Parsing stops at the first error and keeps the documents completed before
it; a parser panic is caught and treated as an unparseable file.

## Stage 2 architecture

Three layers. Each has one job and a plain data type between them.

### Facts

`facts::extract(path, language, text) -> Vec<Fact>` where

```rust
enum Fact {
    StringLit { value: String, line: u32 },
    Call { callee: String, args: Vec<Arg>, line: u32 },   // callee is the dotted or scoped name as written
    Import { path: String, line: u32 },
    Annotation { name: String, args: Vec<Arg>, line: u32 }, // Java/C#/Python decorators
    Template { parts: Vec<String>, line: u32 },            // interpolated or concatenated string, literal parts only
}
enum Arg { Str(String), Template(Vec<String>), Other }
```

Tree-sitter grammars in the first cut: JavaScript, TypeScript and TSX,
Python, Go, Java, C#, PHP. Seven languages, eight grammars. Every other
extension falls back to `facts::regex_extract`, which yields string
literals, obvious call patterns, and import lines from the raw text. The
`mapping.parsers` field reports which files went through which. A grammar is
added when a corpus failure shows the regex path missing an edge.

Facts are language-neutral. Nothing below this layer knows a language.

### Matchers

One module per edge type. Input: the facts of one service plus the
repository-wide configuration index. Output: `Vec<Candidate>` where

```rust
struct Candidate {
    kind: EdgeType,            // http | grpc | event | database | import
    target: Target,            // what was found, before resolution
    confidence_cap: Confidence,// the best this edge can be
    evidence: Evidence,
}
enum Target {
    Host(String),              // hostname from a URL or bare host literal
    EnvVar(String),            // CART_SERVICE_ADDR
    ProtoService(String),      // CartService
    Topic(String),             // orders.created, or an integration event class
    Database(String),          // host + db name, e.g. postgres/orderingdb
    Package(String),           // @acme/shared, github.com/x/y/z, ProjectReference path
    Template(Vec<String>),     // literal fragments of a computed string
}
```

- **http**: URLs in string literals and templates; client calls (`fetch`,
  `axios`, `got`, `http.Get`, `requests.*`, `RestTemplate`, `WebClient`,
  `HttpClient`, `Guzzle`) whose first argument is a string or template;
  Feign `@FeignClient(name = "...")`; environment lookups (`process.env.X`,
  `os.Getenv`, `os.environ`, `System.getenv`,
  `Environment.GetEnvironmentVariable`, `getenv`) whose name ends in a
  hostish suffix (`_HOST`, `_URL`, `_ADDR`, `_ADDRESS`, `_ENDPOINT`,
  `_BASE_URL`, `_SERVICE`, `_API`).
- **grpc**: `.proto` files give `service X` definitions with their file. Server
  side: `RegisterXServer`, `: X.XBase`, `add_XServicer_to_server`,
  `XGrpc.XImplBase`. Client side: `NewXClient`, `XClient`, `X.XStub`,
  `XGrpc.newBlockingStub`, `AddGrpcClient<X.XClient>`. The service that
  registers the server owns the proto service; every other service that
  creates a client gets an edge to it. The address the client dials is also
  fed to the http resolver, so a gRPC edge is found even when only the
  `_ADDR` variable is visible.
- **event**: producers and consumers by library: Kafka (`send`, `produce`,
  `@KafkaListener(topics=)`, `subscribe`), RabbitMQ (`basic_publish`,
  `queue_declare`, `basicConsume`, `@RabbitListener`, `sendToQueue`,
  `assertQueue`, `consume`), SQS/SNS (`QueueUrl`, `TopicArn`), NATS
  (`publish`, `subscribe`), Redis pub/sub, and typed event buses
  (`PublishAsync(new XIntegrationEvent`, `AddSubscription<X,`). The topic
  or event type is the join key. Edge: producer → every consumer of the same
  key, plus producer → broker and consumer → broker when the broker is a
  discovered infrastructure service.
- **database**: connection strings (`postgres://`, `mysql://`, `mongodb://`,
  `redis://`, `amqp://`, JDBC, ADO.NET `Server=;Database=`), ORM
  configuration, and infrastructure hostnames in environment values. Key is
  host plus database name when present. Every service using a key gets an
  edge to the infrastructure service; two code services on the same key get
  a `database` edge between them in both directions, confidence Inferred,
  detail naming the shared key.
- **import**: workspace package names (`packageName` from discovery) in JS/TS
  imports; Go module paths from each service's `go.mod` in Go imports; Cargo
  path dependencies; `.csproj` `ProjectReference` paths; Python and Java only
  when the monorepo layout makes the owning directory explicit.

### Resolver

`resolve(candidate, index) -> Option<Edge>`. The index holds: service names,
normalised names (`checkout-api`, `CheckoutService`, `Checkout.API` all
become `checkout`), k8s Service names, compose and k8s environment maps per
service, dotenv files, proto ownership, topic producers and consumers, and
database keys.

| Target | Resolution | Confidence |
| --- | --- | --- |
| Host equals a service name | direct | Static |
| Host normalises to a service | normalised | Static |
| EnvVar found in compose/k8s/dotenv for the calling service, value resolves as a Host | via config, detail names the variable and value | Static |
| EnvVar not found, its normalised name contains a service | by name only | Uncertain |
| Template with a fragment matching a service | fragment | Uncertain |
| ProtoService owned by a service | ownership | Static |
| Topic with consumers | fan-out | Inferred |
| Database shared | join | Inferred |
| Package owned by a workspace member | manifest | Static |

Self-edges are dropped. Edges to a service that was not discovered are
dropped and counted in `mapping.unresolved`, so the number of candidates
that found no home is visible. Duplicate edges (same source, target, type)
merge their evidence and keep the highest confidence.

File ownership: a file belongs to the service with the longest `root` that
prefixes its path. Files outside every root are counted, not scanned.

### Decisions made while building

- Configuration files are scanned by the regex extractor: nginx templates,
  Spring properties, ini, toml, json, yaml, xml, shell scripts and
  Dockerfiles. robot-shop's web tier exists only as an nginx template, and
  its six edges live there.
- An edge's type follows its target when the candidate was a host or a
  variable: datastore names and images give `database`, broker names give
  `event`, a scheme-less `host:port` whose target owns a proto service gives
  `grpc`, everything else `http`. A specific URL scheme (`amqp`, `redis`,
  `jdbc:mysql`) decides first.
- A plain string literal equal to a service's name counts as an Uncertain
  edge only when that service is a datastore or broker, whatever its role;
  a plain word naming a code service is dropped without being counted.
- PHP PDO data source names (`mysql:host=mysql;dbname=...`) are read as
  database URLs.
- An HTTP URL on a pair that also has a gRPC stub folds into the gRPC edge:
  it is the stub's address.
- `mapping` gained `unresolvedTargets`, the distinct unresolved names (at
  most 50), because a count alone does not tell the reader what was missed.
  Format placeholders, punctuation and bare numbers are not listed.
- The parity baseline predates `mapping`; the parity test removes that key
  before comparing.
- Settings are facts. `key: value`, `key=value`, `<key>value</key>` and
  Dockerfile `ENV` lines in configuration files, and `name = "literal"`
  assignments in every language, become `Setting` facts. A hostish key
  (`host`, `bootstrap-servers`, `DB_CONNECTION_STRING`) makes the value a
  Static host candidate; the rest feed a per-service symbol table.
- Topics are mostly constants. `self.EXCHANGE`, `Queues.queueName` and
  `TopicName` resolve through the symbol table of the service, never across
  services. A topic with a producer or consumer but no counterpart is
  listed as `topic:<key>`; a bare declaration (`queue_declare`) with no
  counterpart is dropped.
- Constructing a typed event is producing it. eShop publishes variables, so
  `new OrderStartedIntegrationEvent(...)` anywhere in a service makes it a
  producer of that event; `*DomainEvent` types stay in-process and are
  ignored, as is MediatR's `INotificationHandler`.
- A client library import is an Inferred edge to the broker of its family
  when the repository declares one (`import pika` → the `rabbitmq` image).
- A shared-database key needs a database name: `host/dbname` from a URL or
  connection string, a host setting paired with a database setting in the
  same file, or an Aspire resource name. A bare host is not a key: two
  services on one MySQL server with different schemas do not share data.
- Import edges never count as unresolved. Almost every import is an
  external library; listing them would bury the hostnames the reader needs.
- Compose `environment: - KEY` without a value takes the value from the
  `.env` beside the compose file, and dotenv values interpolate against
  earlier lines of the same file. Both came from the OpenTelemetry demo's
  `VALKEY_ADDR=valkey-cart:${VALKEY_PORT}`.
- `"event"` is a stop word for topic keys, and the `new <Name>Event(...)`
  producer rule is gated by that same topic-key filter, because eShop's
  browser code constructs `new Event('change')`.
- A database key is never taken from a value that still contains `$` or
  `{`: a `${VAR}` template that reaches the matcher unrendered is not a key.
- An assignment is a setting: `String order_service_url =
  getServiceUrl("ts-order-service")` binds a hostish name to a service name
  and gives a Static http edge, which is how forty-odd train-ticket calls
  surfaced.
- eShop lists IdentityServer's in-process audit events
  (`topic:ConsentDeniedEvent` and five more) as unresolved topics. They are
  real event types nothing in the repository consumes; listing them is
  honest, and the bare-name rule's `X -> EventBus` Uncertain event edges on
  the EventBus library project are a known pre-existing noise source left
  for a follow-up.
- The OpenTelemetry demo's Kafka topic is not found: the literal `"orders"`
  sits in a `return` statement one step removed from the variable the
  producer reads, and the facts layer reads assignments, not returns.

### Terminal report additions

A STRUCTURE section: total edges, by confidence, by type. An EDGES table:
source, target, type, confidence, first evidence as `file:line`. A FINDINGS
section limited to what edges alone support: never called by another
service, most connected, shared databases. Widest change surface waits for
Stage 4's inbound walk.

## Validation

- Fixture tests, test-first: one fixture per matcher (`edges-http-app`,
  `edges-grpc-app`, `edges-events-app`, `edges-db-app`, `edges-import-app`)
  plus a `edges-mixed-app` that exercises the resolver's confidence table.
- Discovery parity: `test/expected/discovery/<fixture>.json` is the
  TypeScript output. The Rust output must match, ignoring `root` and
  `repository`.
- Corpus: `test/expected/corpus/<repo>.json` lists code and infrastructure
  service names for all eight repositories, and hand-written expected edges
  for microservices-demo (from its architecture diagram and manifests),
  robot-shop (from its compose wiring and source), and eShop (from the
  Aspire AppHost `WithReference` graph). The corpus test skips with a
  message when `corpus/` is absent. A missing expected edge fails. Extra
  edges are printed with their evidence and do not fail, because recall
  over precision is the rule, but the count is tracked in the expected
  file so growth is noticed.
- Performance: `analyze` on train-ticket and opentelemetry-demo under
  500 ms wall clock on the development machine, measured by the corpus test.

## Out of scope for this spec

Blast radius computation (`--pr`, `--history`), the runtime join, the HTML
report, npm and cargo distribution of the binary. The JSON contract above
is designed so none of them need to change it.
