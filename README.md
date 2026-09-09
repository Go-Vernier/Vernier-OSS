# Vernier

**Which services can this change reach?**

Vernier reads a repository, finds its service boundaries, maps which
service calls which, and reports what a pull request can affect. It joins two
things nobody connects today: what the code *could* call (static analysis)
and what production *actually* calls (traces). Every edge carries evidence
and a confidence label. It optimises for recall over precision: it will never
tell you a service cannot be affected, only that no static or observed path
was found.

Run on eight open-source microservice repositories, it finds their service
boundaries in about 100 ms each, without configuration:

| Repository | Services | Found through |
| --- | ---: | --- |
| FudanSELab/train-ticket | 45 | docker-compose |
| dotnet/eShop | 19 | monorepo layout |
| open-telemetry/opentelemetry-demo | 16 | docker-compose |
| GoogleCloudPlatform/microservices-demo | 11 | Kubernetes manifests |
| instana/robot-shop | 11 | docker-compose |
| spring-petclinic/spring-petclinic-microservices | 10 | docker-compose |
| ewolff/microservice | 6 | docker-compose |
| microservices-demo/microservices-demo (Sock Shop) | 0, 14 images | deploy-only repository, reported as such |

## Status

Phase 0. This is the open-source CLI described in
[docs/build-spec.md](docs/build-spec.md). All four stages are built: service
discovery, static dependency mapping, the runtime join, and the blast radius
of a change with its reports. Where the tool cannot answer, the report says
so instead of guessing.

| Stage | What it does | State |
| --- | --- | --- |
| 1. Discover | Find service boundaries: docker-compose, Kubernetes, monorepo layout, workspace config | Built |
| 2. Map | Static edges: HTTP calls, gRPC stubs, message topics and typed events, shared databases, cross-package imports; datastore and broker hosts found on the way | Built |
| 3. Join | Optional runtime edges from OpenTelemetry (servicegraph scrape or OTLP JSON) or Datadog, with an explicit name-matching report | Built |
| 4. Report | Blast radius of a change: files, a git diff range, or a pull request in the local history; the last N pull requests; terminal and self-contained HTML | Built |

Not yet published. Run it from source with a stable Rust toolchain:

```bash
git clone https://github.com/Go-Vernier/Vernier-OSS.git
cd Vernier-OSS
cargo build --release
./target/release/vernier analyze /path/to/a/repository
./target/release/vernier analyze /path/to/a/repository --json
./target/release/vernier analyze /path/to/a/repository --otel traces.prom     # servicegraph scrape or OTLP JSON
./target/release/vernier analyze /path/to/a/repository --datadog deps.json    # saved service_dependencies response
./target/release/vernier analyze /path/to/a/repository --pr 481               # one pull request, from the local git history
./target/release/vernier analyze /path/to/a/repository --diff main...HEAD     # any git diff range
./target/release/vernier analyze /path/to/a/repository --files a/b.js c/d.py  # explicit files
./target/release/vernier analyze /path/to/a/repository --history 50           # the last 50 pull requests
./target/release/vernier analyze /path/to/a/repository --html report.html     # the self-contained HTML report
```

`--pr`, `--diff` and `--files` take one change at a time; `--history` adds a
section to either report; `--depth` (default 3) bounds the walk; `--otel` and
`--datadog` combine with all of them, so observed edges take part in the walk.

The engine is Rust. An npm package will wrap the binary when it is
published, so it will install as `vernier` and run as `npx vernier analyze .`.

## What it looks like

```
VERNIER

  Repository    instana/robot-shop
  Services      11 detected  (docker-compose)
  Runtime       not connected - static only

SERVICES

  NAME       LANGUAGE    ROOT                                    EVIDENCE
  cart       javascript  cart                                    docker-compose.yaml:57
  catalogue  javascript  catalogue                               docker-compose.yaml:26
  dispatch   -           dispatch                                dispatch/docker-compose.yaml:20
  load       python      load-gen                                docker-compose-load.yaml:3
  mongodb    -           mongo                                   docker-compose.yaml:3
  mysql      -           mysql                                   docker-compose.yaml:72
  payment    python      payment                                 dispatch/docker-compose.yaml:10
  ratings    -           ratings                                 docker-compose.yaml:97
  shipping   java        shipping                                docker-compose.yaml:82
  user       javascript  user                                    docker-compose.yaml:41
  web        -           web                                     docker-compose.yaml:142
  rabbitmq   -           (image rabbitmq:3.7-management-alpine)  dispatch/docker-compose.yaml:3
  redis      -           (image redis:6.2-alpine)                docker-compose.yaml:14

  2 declared but not built here (images): rabbitmq, redis

STRUCTURE

  Total edges              20
  Static                   12
  Inferred                 2
  Uncertain                6
  By type                  http 11 · database 6 · event 3

  Scanned 75 files in 11 services (tree-sitter: go, java, javascript, php, python · regex: conf, css, dockerfile, html, ini, json, properties, sh, sql, template, xml, yaml)
  69 files outside every service were not read
  31 targets could not be matched to a service: AMQP_HOST, DB_HOST, INSTANA_EUM_REPORTING_URL, PAYMENT_GATEWAY, PDO_URL, ...

EDGES

  SOURCE         TARGET     TYPE      CONFIDENCE  EVIDENCE
  cart       ->  catalogue  http      static      cart/server.js:30  CATALOGUE_HOST default catalogue
  cart       ->  redis      database  static      cart/server.js:29  REDIS_HOST default redis
  catalogue  ->  mongodb    database  static      catalogue/server.js:157  MONGO_URL default mongodb://mongodb:27017/catalogue
  dispatch   ->  rabbitmq   event     inferred    dispatch/main.go:16  imports github.com/streadway/amqp (rabbitmq client)
  payment    ->  cart       http      static      payment/payment.py:24  CART_HOST default cart
  payment    ->  dispatch   event     inferred    dispatch/main.go:72  mentions "robot-shop", published by payment
  payment    ->  rabbitmq   event     static      payment/rabbitmq.py:6  AMQP_HOST default rabbitmq
  payment    ->  user       http      static      payment/payment.py:25  USER_HOST default user
  ratings    ->  catalogue  http      static      ratings/html/src/Kernel.php:76  http://catalogue:8080
  ratings    ->  mysql      database  static      ratings/html/src/Kernel.php:77  mysql://mysql
  ...

FINDINGS

  Never called by another service        2 services
    load, web                            (dead, or just quiet?)

  Most connected                         cart
    touched by 3 services

  Widest change surface                  mongodb
    a change here reaches 8 services

  Shared databases                       0
```

Every service row points at the file and line that declared it. Every edge
row points at the file and line where the call was found, and says how sure
the tool is: `dispatch`'s edge to `rabbitmq` comes from an import of the AMQP
client library joined to the broker image declared in compose, so it is
Inferred rather than Static; the cart service's `REDIS_HOST` default is a
literal, so that one is Static.

## What a change looks like

```
$ vernier analyze corpus/robot-shop --files cart/server.js

VERNIER

  Repository    instana/robot-shop
  Services      11 detected  (docker-compose)
  Runtime       not connected - static only
  Change        1 file given
                1 file in 1 service

BLAST RADIUS

  1 service changed -> 4 services in the blast radius
  2 static · 1 inferred · 1 uncertain · depth 3

  CHANGED
  cart       1 file   cart/server.js

  REACHED
  SERVICE   DEPTH  CONFIDENCE  PATH
  payment   1      static      payment calls cart (http)
  shipping  1      static      shipping calls cart (http)
  web       1      uncertain   web calls cart (http)
  dispatch  2      inferred    dispatch consumes events from payment; payment calls cart (http)

  Not in the computed blast radius - no static or observed runtime path found
    6 services  catalogue, load, mongodb, mysql, ratings, user
```

The headline counts the services a change reaches, not the ones it touches.
Each reached service shows the depth it was reached at, the weakest
confidence on the path, and the path itself, read from that service back to
the change. The last block uses the only wording the tool has for the rest:
it never says a service cannot be affected.

With `--pr 481` the `Change` row names the pull request, the commit that
merged it and its date; with `--history 50` a CHANGE HISTORY section gives
the average, median and largest blast radius over the last fifty pull
requests and how many reached more than ten services, with one row per pull
request under it.

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

## The confidence model

Every edge carries one of four labels. The weakest label on a path decides
the label of everything reached through it.

| Label | Meaning |
| --- | --- |
| Observed | seen in production traces |
| Static | a code path exists, never observed |
| Inferred | joined through an indirection: a topic, a shared table |
| Uncertain | a computed URL or a fuzzy name match, included on purpose |

## Rules that hold throughout

- **Recall over precision.** If unsure, include it and label it Uncertain.
- **Never assert without evidence.** Every service and edge carries a file
  and line, or a trace count and timestamp.
- **Fail honestly.** If a runtime join matched 6 of 42 services, the report
  header says so. A partial join that looks complete is worse than no join.
- **No telemetry.** The tool sends nothing anywhere.
- **Works offline.** The static path never makes a network call. The runtime join reads a file, or fetches only the URL you give it.

## Developing

```bash
cargo test                                # fixtures under test/fixtures, parity with the baseline
cargo fmt --check && cargo clippy --all-targets -- -D warnings
sh scripts/corpus.sh                      # shallow-clone the eight reference repositories into corpus/
cargo test --test corpus -- --nocapture   # discovery counts and timing on the corpus
cargo run -q -- analyze corpus/robot-shop --files cart/server.js --html /tmp/robot-shop.html
```

Every change should run against the whole corpus. A regression on one repo
is a regression on the product. `test/expected/corpus/` holds the expected
services per repository; `test/expected/discovery/` holds the output the
original TypeScript engine produced on every fixture, which the Rust engine
must reproduce.

The `blastradius-core` crate is also a library:

```rust
use blastradius::{analyze, blast, format_report, Change};

let mut analysis = analyze(std::path::Path::new("./my-repo"))?;
let change = Change::from_files(&["services/checkout/src/pay.ts".to_string()]);
analysis.blast = Some(blast::of_change(&analysis.graph, change, blast::DEFAULT_DEPTH));
println!("{}", format_report(&analysis, false));
```

## Part of Vernier

This CLI is the open-source part of [Vernier](https://github.com/Go-Vernier),
a behavioural verification platform. The platform reasons about behaviours
inside one repository; this tool reasons about reach across services, and
the blast radius of a change is the number it exists to report. They share
the same idea: every claim carries its evidence and its confidence.

## Licence

MIT. See [LICENSE](LICENSE).
