# Blast Radius — Open Source Build Spec

Phase 0. A CLI that analyses a repository and reports what a change can
reach. No accounts, no hosted service, no GitHub App.

**Stack:** Node 20+ / TypeScript. Distributed via `npx`. MIT licence.

---

## What it must do

```bash
npx blast-radius analyze .              # full repo report
npx blast-radius analyze . --pr 481     # blast radius of one PR
npx blast-radius analyze . --history 50 # blast radius of last 50 PRs
npx blast-radius analyze . --otel <url> # join with runtime traces
npx blast-radius analyze . --html out.html
```

---

## Architecture — four stages

```
  DISCOVER          MAP              JOIN            REPORT
  services   ->  dependencies  ->  runtime edges  ->  output
```

Each stage writes to a single in-memory graph. Keep them independently
testable — the join is the risky part and you need to be able to swap it.

---

## Stage 1 — Service discovery

Find the service boundaries in the repo. Try these in order and stop at
the first that yields more than one service:

**1. docker-compose.yml** — each key under `services:` is a service. Map
`build.context` to its directory.

**2. Kubernetes manifests** — any `kind: Deployment` or `kind: Service`.
Read the container image name and any path annotations.

**3. Monorepo conventions** — directories under `/services/*`, `/apps/*`,
`/packages/*` that contain their own `package.json`, `go.mod`,
`pom.xml`, or `requirements.txt`.

**4. Workspace config** — `package.json` workspaces, `nx.json`,
`turbo.json`, `pnpm-workspace.yaml`, Cargo workspace members.

**5. Fallback** — if none of the above finds more than one service, report
honestly: *"This looks like a single service. Blast radius analysis needs
a multi-service repository."* Do not fabricate boundaries.

Output per service: name, root directory, language, entry points.

---

## Stage 2 — Static dependency mapping

For each service, find outbound calls to other services. This is
heuristic and will be imperfect — that is expected and it is why the
confidence model exists.

**HTTP calls.** Scan for client calls — `fetch`, `axios`, `got`,
`http.Get`, `requests.get`, `RestTemplate`, `HttpClient`. Extract the URL
argument. Resolve it three ways:
- Literal URL containing a known service name → confident edge
- Environment variable → look it up in docker-compose `environment`, k8s
  ConfigMaps, or `.env.example` → confident if resolvable
- Template or computed string → record as **Uncertain**, include anyway

**Message queues and events.** Look for publish and subscribe calls —
Kafka (`producer.send`, `@KafkaListener`), RabbitMQ, SQS/SNS, NATS,
Redis pub/sub, EventBridge. Extract the topic or queue name as a string
literal. Build edges: service → topic → consuming services. Classify as
**Inferred**.

**gRPC.** Proto imports and generated client stubs. The service name is
usually in the proto package.

**Databases.** Connection strings and ORM model definitions. Extract table
or collection names where cheaply possible. A shared database between two
services is an important edge — flag it.

**Package imports.** Cross-service imports within a monorepo, from
workspace configuration. These are the most reliable edges you have.

Store every edge with: source, target, type, evidence (file + line), and
confidence.

---

## Stage 3 — Runtime join (optional but the differentiator)

Two supported sources. Both optional — the tool must work without them.

**OpenTelemetry.** Accept either a Prometheus endpoint exposing
`traces_service_graph_request_total` (from the OTel Collector's
servicegraph connector), or a raw span export in OTLP JSON. Extract
client/server service pairs and call counts.

**Datadog.** `GET /api/v1/service_dependencies`. Requires an API key
passed by flag or environment variable. Returns each service and its
`calls_out` list.

**The join.** Match runtime service names to discovered services. Names
will not match cleanly — `checkout-api` in traces versus `/services/checkout`
in the repo. Implement in this order:
1. Exact match
2. Normalised match (lowercase, strip `-api`, `-svc`, `-service`)
3. Fuzzy match above a similarity threshold, reported as a warning
4. Manual mapping via optional `blast-radius.config.json`

Print the mapping result explicitly. If only six of forty-two services
matched, say so loudly — a silent partial join produces a confidently
wrong report.

**Merge rules:**
- Edge in both static and runtime → **Observed**, with call frequency
- Edge in runtime only → **Observed** (async or dynamic; static missed it)
- Edge in static only → **Static**, with last-observed date if known
- Never drop a static edge because runtime did not see it

---

## Stage 4 — Blast radius computation

Given a set of changed files:

1. Map each file to its owning service by directory
2. Seed the frontier with those services
3. Walk the graph outward following **inbound** edges — who calls this —
   to depth 3 by default
4. Also walk event edges: if the changed service publishes a topic,
   include every consumer of that topic
5. Classify each result by the weakest confidence on its path
6. Everything not reached is "not in the computed blast radius"

**Never** claim a service cannot be affected. The wording is fixed:
*"Not in the computed blast radius — no static or observed runtime path
found."*

---

## The reports

### Repo report — `analyze .`

```
BLAST RADIUS

  Repository    acme/platform
  Services      42 detected
  Runtime       connected (OTel, 30d)   [or: not connected — static only]

STRUCTURE

  Total edges              186
  Observed in production   134
  Static only               41
  Inferred (async/event)    11

FINDINGS

  Never called by another service        17 services
    payments-legacy, report-gen, ...     (dead, or just quiet?)

  Most connected                         orders-db
    touched by 23 services

  Widest change surface                  checkout-api
    a change here reaches 14 services

  No observed traffic in 30 days          6 services

  Shared databases                        3
    orders-db written by checkout-api, refunds-api

CHANGE HISTORY  (last 50 PRs)

  Average blast radius     4.2 services
  Median                   3
  Largest                  PR #388 — 31 services
  PRs reaching >10          7  (14%)
```

### PR report — `analyze . --pr 481`

Matches the format in the product doc. Headline number first:
`1 service changed → 7 services in the blast radius`.

### HTML report

Single self-contained file. Force-directed graph, nodes sized by inbound
edges, coloured by confidence. Click a node for its edges and evidence.
No external CDN — inline everything so it works offline and can be
emailed.

**This file is the marketing.** Make it good enough to screenshot.

---

## Build order

**Week 1** — Service discovery + repo structure report. No dependencies
yet. Ship something that runs on any repo and says something true.

**Week 2** — Static edges: HTTP, imports, events. Terminal report with
the graph.

**Week 3** — Blast radius computation + `--pr` and `--history`. This is
where it becomes interesting.

**Week 4** — Runtime join, OTel first, Datadog second.

**Week 5** — HTML report, docs, README, licence, polish.

---

## Test corpus — build this in week 1

You need real multi-service repos to develop against. Collect 8–10 open
source ones with genuine service boundaries. Good candidates: Google's
microservices-demo, Sock Shop, Instana's robot-shop, Alibaba's
train-ticket, TrainTicket, Spring PetClinic microservices.

Every change runs against all of them. A regression on one repo is a
regression on the product.

---

## Rules that hold throughout

**Recall over precision.** If unsure, include it and label it Uncertain.

**Never assert without evidence.** Every edge carries a file and line, or
a trace count and timestamp.

**Fail honestly.** If the join matched 6 of 42 services, say so in the
report header. A partial join that looks complete is worse than no join.

**No telemetry.** The tool sends nothing anywhere. State this in the
README — it will be the first question asked.

**Works offline.** Runtime join is opt-in. The static path must never
require a network call.

---

## The README is part of the product

It should open with the finding, not the installation instructions:

> We analysed 40 open source microservice repositories. On average, 38%
> of services had no inbound calls from any other service. The average
> pull request could reach 4.2 services. The largest could reach 31.
>
> Run it on yours:
> ```
> npx blast-radius analyze .
> ```

That framing is what gets it posted. The tool is how someone reproduces
the finding on their own code.
