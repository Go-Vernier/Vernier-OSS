# Blast Radius

**Which services can this change reach?**

Blast Radius reads a repository, finds its service boundaries, maps which
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

Phase 0, week 2. This is the open-source CLI described in
[docs/build-spec.md](docs/build-spec.md). Service discovery and the first
static edges are built. The rest is not, and the report says so instead of
guessing.

| Stage | What it does | State |
| --- | --- | --- |
| 1. Discover | Find service boundaries: docker-compose, Kubernetes, monorepo layout, workspace config | Built |
| 2. Map | Static edges: HTTP calls, gRPC stubs, and the datastore and broker hosts found on the way; message topics, shared databases and cross-package imports next | HTTP and gRPC built |
| 3. Join | Optional runtime edges from OpenTelemetry or Datadog, with an explicit name-matching report | Planned |
| 4. Report | Blast radius of a change, one PR, or the last N PRs; terminal and self-contained HTML | Planned |

Not yet published. Run it from source with a stable Rust toolchain:

```bash
git clone https://github.com/Go-Vernier/Vernier-OSS.git
cd Vernier-OSS
cargo build --release
./target/release/blast-radius analyze /path/to/a/repository
./target/release/blast-radius analyze /path/to/a/repository --json
```

The engine is Rust. The npm package `blastradius` will wrap the binary when
it is published, so it will run as `npx blastradius analyze .` and install
as `blast-radius`.

## What it looks like

```
BLAST RADIUS

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

  Total edges              19
  Static                   11
  Inferred                 0
  Uncertain                8
  By type                  http 11 · database 6 · event 2

  Scanned 75 files in 11 services (tree-sitter: go, java, javascript, php, python · regex: conf, css, dockerfile, html, ini, json, properties, sh, sql, template, xml, yaml)
  69 files outside every service were not read
  31 targets could not be matched to a service: AMQP_HOST, DB_HOST, INSTANA_EUM_REPORTING_URL, PAYMENT_GATEWAY, PDO_URL, ...

EDGES

  SOURCE         TARGET     TYPE      CONFIDENCE  EVIDENCE
  cart       ->  catalogue  http      static      cart/server.js:30  CATALOGUE_HOST default catalogue
  cart       ->  redis      database  static      cart/server.js:29  REDIS_HOST default redis
  catalogue  ->  mongodb    database  static      catalogue/server.js:157  MONGO_URL default mongodb://mongodb:27017/catalogue
  dispatch   ->  rabbitmq   event     uncertain   dispatch/main.go:141  "rabbitmq"
  payment    ->  cart       http      static      payment/payment.py:24  CART_HOST default cart
  payment    ->  rabbitmq   event     static      payment/rabbitmq.py:6  AMQP_HOST default rabbitmq
  payment    ->  user       http      static      payment/payment.py:25  USER_HOST default user
  ratings    ->  catalogue  http      static      ratings/html/src/Kernel.php:76  http://catalogue:8080
  ...

FINDINGS

  Never called by another service        3 services
    dispatch, load, web                  (dead, or just quiet?)

  Most connected                         cart
    touched by 3 services
```

Every service row points at the file and line that declared it. Every edge
row points at the file and line where the call was found, and says how sure
the tool is: the nginx template resolved `${CATALOGUE_HOST}` by name only, so
that edge is Uncertain; the cart service's `REDIS_HOST` default is a
literal, so that one is Static.

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

Two matchers read those facts. The HTTP matcher takes URLs, `host:port`
literals, templates around them, Feign clients, and environment variables
whose name says they hold a host. The gRPC matcher takes generated client
stubs and works out which service owns each proto service from its server
registration, or from its name when nothing registers it.

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

An edge's type follows its target: a variable pointing at Redis is a
`database` edge, one pointing at RabbitMQ is an `event` edge, and an HTTP
URL on a pair that also has a gRPC stub folds into the gRPC edge. Anything
that matches no discovered service is counted and listed under `mapping` in
the JSON and in the report, never guessed.

## The confidence model

Every edge will carry one of four labels. The weakest label on a path decides
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
- **Works offline.** The runtime join is opt-in. The static path never makes
  a network call.

## Developing

```bash
cargo test                                # fixtures under test/fixtures, parity with the baseline
cargo fmt --check && cargo clippy --all-targets -- -D warnings
sh scripts/corpus.sh                      # shallow-clone the eight reference repositories into corpus/
cargo test --test corpus -- --nocapture   # discovery counts and timing on the corpus
```

Every change should run against the whole corpus. A regression on one repo
is a regression on the product. `test/expected/corpus/` holds the expected
services per repository; `test/expected/discovery/` holds the output the
original TypeScript engine produced on every fixture, which the Rust engine
must reproduce.

The `blastradius-core` crate is also a library:

```rust
use blastradius::{analyze, format_repo_report};

let analysis = analyze(std::path::Path::new("./my-repo"))?;
println!("{}", format_repo_report(&analysis, false));
```

## Relationship to Vernier

Blast Radius is built by the team behind [Vernier](https://github.com/Go-Vernier),
a behavioural verification platform. Vernier reasons about behaviours inside
one repository. Blast Radius reasons about reach across services. They share
the same idea: every claim carries its evidence and its confidence.

## Licence

MIT. See [LICENSE](LICENSE).
