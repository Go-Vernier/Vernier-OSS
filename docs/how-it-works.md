# How Vernier works

The details behind the [README](../README.md): how services are found, how
edges are mapped, how runtime data is joined, how the blast radius is
walked, and how to work on the code.

## The four stages

| Stage | What it does |
| --- | --- |
| 1. Discover | Find service boundaries: docker-compose, Kubernetes, monorepo layout, workspace config |
| 2. Map | Static edges: HTTP calls, gRPC stubs, message topics and typed events, shared databases, cross-package imports; datastore and broker hosts found on the way |
| 3. Join | Optional runtime edges from OpenTelemetry (servicegraph scrape or OTLP JSON) or Datadog, with an explicit name-matching report |
| 4. Report | Blast radius of a change: files, a git diff range, or a pull request in the local history; the last N pull requests; terminal, HTML and TUI |

## How discovery works

Strategies are tried in order. The first one that finds more than one
service with code in the repository wins.

1. **docker-compose** - each key under `services:` is a service, with `${VAR}`
   references resolved from the `.env` beside the file. `build.context` names
   its directory, or the directory of `build.dockerfile` when every service
   builds from the repository root. An `image:`-only service is matched to a directory by
   its name or its image name (`springcommunity/spring-petclinic-vets-service`
   matches `spring-petclinic-vets-service/`); otherwise it is infrastructure
   the repository runs but does not build.
2. **Kubernetes manifests** - any `Deployment`, `StatefulSet`, `DaemonSet`,
   `Job`, `CronJob` or `Service`. The directory is found the same way. Helm
   templates are skipped, not misread.
3. **Monorepo layout** - children of `services/`, `apps/`, `packages/`,
   `microservices/` or `src/` that carry their own manifest (`package.json`,
   `go.mod`, `pom.xml`, `requirements.txt`, `Cargo.toml`, `*.csproj`, a
   `Dockerfile`, ...).
4. **Workspace config** - `package.json` workspaces, `pnpm-workspace.yaml`,
   Cargo `[workspace] members`, Nx `project.json`.
5. **Fallback** - when none of the above finds more than one service, the
   report says so: *"This looks like a single service. Blast radius analysis
   needs a multi-service repository."* Boundaries are never invented. A
   deploy-only repository (images, no code) is named for what it is.

## How mapping works

Every file under a discovered service is read once. Seven languages go
through tree-sitter (JavaScript, TypeScript, Python, Go, Java, C#, PHP);
everything else, including configuration files such as nginx templates,
Spring properties and Dockerfiles, goes through a regex extractor. Both
produce the same language-neutral facts: string literals, interpolated
templates, calls with their string arguments, imports, annotations, base
types and environment lookups with their literal defaults.

Five matchers read those facts. The HTTP matcher takes URLs, `host:port`
literals, templates around them, Feign clients, environment variables whose
name says they hold a host, and configuration settings whose key does
(`spring.data.mongodb.host: ts-order-mongo`). The gRPC matcher takes
generated client stubs and works out which service owns each proto service
from its server registration, or from its name when nothing registers it.
The event matcher takes publish, send, produce, subscribe, consume and
declare calls by library, listener annotations, typed event buses
(`new OrderStartedIntegrationEvent(...)`, `AddSubscription<X, H>`) and Go
struct literals; a topic written as a constant (`self.EXCHANGE`,
`Queues.queueName`) is read back from the literal the service assigns to it.
The database matcher keys every connection string, URL, rendered template
and host-plus-database setting by host and database name. The import
matcher reads imports in code and dependencies, Maven sibling artifacts,
Cargo path dependencies and `.csproj` project references in manifests.

A resolver places each candidate on a discovered service and says how:

| Found | Resolved through | Confidence |
| --- | --- | --- |
| `http://catalogue:8080/...` | hostname equals a service name | Static |
| `payment:50051` | `host:port`; gRPC when the target owns a proto service | Static |
| `CATALOGUE_HOST` with `CATALOGUE_HOST: catalogue` in compose or Kubernetes | the configured value | Static |
| `os.getenv("USER_HOST", "user")` | the literal default | Static |
| `PRODUCT_CATALOG_SERVICE_ADDR` with no configuration | the variable's name, suffixes stripped | Uncertain |
| `http://${CART_HOST}:8080/` in a template | the variable inside it | Uncertain |
| `"rabbitmq"` as a plain string | the name of a datastore or broker | Uncertain |
| `pb.NewCartServiceClient(conn)` | the service that registers `CartService` | Static |
| `spring.data.mongodb.host: ts-order-mongo` in a service's config | the setting's value | Static |
| `basic_publish(routing_key='orders')` and `Consume("orders")` in two services | the topic, producer → consumer | Inferred |
| `import pika` with a `rabbitmq` image declared | the client library's broker family | Inferred |
| `jdbc:mysql://mysql/shop` in two services | the shared `host/database` key, both directions | Inferred |
| `import '@acme/shared/utils'` with `packages/shared` declaring `@acme/shared` | the package name, or a prefix ending at a separator | Static |
| `<ProjectReference Include="..\EventBus\EventBus.csproj" />` | the service whose root holds the path | Static |

An edge's type follows its target: a variable pointing at Redis is a
`database` edge, one pointing at RabbitMQ is an `event` edge, and an HTTP
URL on a pair that also has a gRPC stub folds into the gRPC edge. Anything
that matches no discovered service is counted and listed under `mapping` in
the JSON and in the report, never guessed; a topic with a producer but no
consumer in the repository is listed as `topic:<name>`. An import that
matches no discovered package is an external library and is not counted.

## How the runtime join works

Static edges say what the code could call. `--otel` and `--datadog` add what
production actually called. `--otel` takes a Prometheus scrape of the OpenTelemetry
Collector's servicegraph connector (`traces_service_graph_request_total`) or a raw
OTLP JSON span export, as a file or a URL; `--datadog` takes a saved
`service_dependencies` response, or calls the API when given `--dd-env` and the
`DD_API_KEY` and `DD_APP_KEY` environment variables. Nothing is fetched unless you
pass a URL or ask for the live call.

Runtime names rarely equal repository names. Each one is matched in order: an
entry in `vernier.config.json` (`{"runtime": {"map": {"checkout-api":
"checkout"}, "ignore": ["load-generator"]}}`), the exact name, the normalised name
(`checkout-api`, `CheckoutService` and `checkout` are the same), then a fuzzy match
that is flagged for you to check. The whole table is printed, and the header says
how many runtime services matched, so a partial join can never look complete.

Then the merge: a static edge production confirmed becomes Observed and carries
the call count; a call static analysis missed becomes a new Observed edge; a static
edge production never took keeps its label and is listed under FINDINGS. Nothing is
dropped.

With a servicegraph scrape from the fixture repository:

```
  Runtime       connected (OTel, 8 of 10 runtime services matched)

RUNTIME

  Source        OTel  test/fixtures/runtime-app/runtime/traces.prom
  Services      8 of 10 runtime services matched (1 ignored by vernier.config.json)
  Edges         4 observed (3 static confirmed, 1 runtime only) · 2 calls skipped, one end unmatched or ignored

  RUNTIME NAME       SERVICE        HOW
  catalogue-service  catalogue      normalised
  chckout            checkout       fuzzy 0.97  (check this)
  checkout-api       checkout       normalised
  load-generator     -              ignored (vernier.config.json)
  notifications      notifications  exact
  orders             orders         exact
  pay                payment        config
  payment            payment        exact
  redis              redis          exact

  1 runtime service matched nothing: auth-proxy

FINDINGS

  Never called by another service        2 services
    checkout, orders                     (dead, or just quiet?)

  Most connected                         catalogue
    touched by 1 service

  Shared databases                       0

  Static edges never observed            1
    orders -> rabbitmq
```

## How the blast radius works

A change is a set of files. `--files` lists them; `--diff RANGE` asks git
(`git diff --name-only RANGE`, verbatim); `--pr N` finds the pull request in
the local history: a first-parent commit on `HEAD` whose subject carries the
number (`Merge pull request #N ...`, a squash-merge `(#N)`, Bitbucket's
`(pull request #N)`) or a ref such as `refs/pull/N/head`. The tool never
calls a forge API; when the pull request is not there, the error says how to
fetch it. When the analysed directory is a subdirectory of the repository,
paths are made relative to it and files outside it are counted.

Each file belongs to the service whose root holds it; the longest root wins,
and a file under no root is listed as unowned and seeds nothing. The changed
services seed a walk that follows these edges, to `--depth` hops (default 3):

| From changed or reached service N | Reaches | Because |
| --- | --- | --- |
| inbound `http` or `grpc` edge `S -> N` | S | S calls N |
| inbound `import` edge `S -> N` | S | S imports N |
| inbound `database` edge `S -> N` (a shared database) | S | S shares a database with N |
| outbound `event` edge `N -> T`, T a service | T | T consumes events from N |
| outbound `event` edge `N -> B`, B a broker, N a *changed* service | every other client of B | they share the broker; the topic is unknown, so Uncertain |

Not followed: what N itself depends on (its own outbound `http`, `grpc`,
`import` and `database` edges are not affected by a change to N), a
consumer's producer (inbound `event`), and the broker step from a service
that was reached rather than changed, because that step carries no topic
evidence and chaining it would join the whole repository through one broker.
Brokers on the path are listed, not counted.

A reached service's confidence is the weakest hop on its path; when several
paths reach it, the strongest wins and, among equally strong ones, the
shortest. The walk runs on the graph as it is at `HEAD`, also for older pull
requests in `--history`.

`--history N` takes the last N pull requests from the first-parent history
and runs the walk for each. When the history carries no pull request markers
at all, the last N commits are used and the section says so.

`--html report.html` writes one file with no external resource: the graph
drawn with nodes sized by inbound edges and coloured by confidence,
infrastructure hollow, uncertain edges dashed; when a change was analysed the
changed services are ringed and everything outside the radius is dimmed.
Clicking a node lists its edges and their evidence. `--json` includes the
`blast` and `history` blocks, present only when asked for.

## The TUI

`vernier tui` keeps the graph in memory and lets you ask it one question
after another. It takes the same `--otel`, `--datadog`, `--pr`, `--diff`,
`--files` and `--depth` flags as `analyze`. `--history N` (default 50) sets how
many recent pull requests it lists.

| Tab | Shows |
| --- | --- |
| 1 Overview | Bar charts of the edges by confidence and by type, the most depended-on services and the widest change surfaces; the repository report below, exactly as `analyze` prints it |
| 2 Services | Every service; the selected one drawn with what depends on it and what it depends on, its edges, and each edge's evidence |
| 3 Changes | The recent pull requests, each with its reach as a bar, and the numbers across them |
| 4 Blast | One change drawn as a tree of the walk, coloured by confidence; its reach by depth and by confidence; the path to the selected service; what it does not reach |

```
◆ catalogue  changed · 1 file            BY DEPTH
├── ● cart  calls catalogue (http)       depth 1 ████████████████ 3
│   ├── ● payment  calls cart (http)     depth 2 ██████████▋      2
│   │   └── ● dispatch  consumes events  depth 3 █████▎           1
│   └── ● shipping  calls cart (http)
├── ● ratings  calls catalogue (http)    BY CONFIDENCE
└── ● web  calls catalogue (http)        static    ██████████████ 4
```

`Enter` on a service walks a change to that service's directory; on a pull
request, it walks that pull request. `d` and `f` on the Changes tab walk a
diff range or a list of files, and `+`/`-` re-run the walk one hop deeper or
shallower. `/` filters a list, `?` lists every key, `q` quits. Confidence
uses the HTML report's colours; `NO_COLOR` turns them off. The TUI computes
nothing the engine does not, and uses the terminal report's wording, including
the fixed wording for what is not reached.

## Developing

```bash
cargo test                                # fixtures under test/fixtures, parity with the baseline
cargo fmt --check && cargo clippy --all-targets -- -D warnings
sh scripts/corpus.sh                      # shallow-clone the eight reference repositories into corpus/
cargo test --test corpus -- --nocapture   # discovery counts and timing on the corpus
cargo run -q -- analyze corpus/robot-shop --files cart/server.js --html /tmp/robot-shop.html
cargo run -q -- tui corpus/train-ticket
UPDATE_SNAPSHOTS=1 cargo test -p vernier-tui   # rewrite the TUI's text snapshots after a deliberate change
```

Every change should run against the whole corpus. A regression on one repo
is a regression on the product. `test/expected/corpus/` holds the expected
services per repository; `test/expected/discovery/` holds the output the
original TypeScript engine produced on every fixture, which the Rust engine
must reproduce.

The `vernier-core` crate is also a library:

```rust
use vernier::{analyze, blast, format_report, Change};

let mut analysis = analyze(std::path::Path::new("./my-repo"))?;
let change = Change::from_files(&["services/checkout/src/pay.ts".to_string()]);
analysis.blast = Some(blast::of_change(&analysis.graph, change, blast::DEFAULT_DEPTH));
println!("{}", format_report(&analysis, false));
```
