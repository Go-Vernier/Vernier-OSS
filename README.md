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

Phase 0, week 1. This is the open-source CLI described in
[docs/build-spec.md](docs/build-spec.md). Service discovery is built. The
rest is not, and the report says so instead of guessing.

| Stage | What it does | State |
| --- | --- | --- |
| 1. Discover | Find service boundaries: docker-compose, Kubernetes, monorepo layout, workspace config | Built |
| 2. Map | Static edges: HTTP calls, message topics, gRPC, shared databases, cross-package imports | Next |
| 3. Join | Optional runtime edges from OpenTelemetry or Datadog, with an explicit name-matching report | Planned |
| 4. Report | Blast radius of a change, one PR, or the last N PRs; terminal and self-contained HTML | Planned |

Not yet published to npm. Run it from source:

```bash
git clone https://github.com/Go-Vernier/Vernier-OSS.git
cd Vernier-OSS
pnpm install && pnpm build
node dist/cli.js analyze /path/to/a/repository
node dist/cli.js analyze /path/to/a/repository --json
```

Once published, it will run as `npx blastradius analyze .` and install as
`blast-radius`.

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
  payment    python      payment                                 dispatch/docker-compose.yaml:10
  shipping   java        shipping                                docker-compose.yaml:82
  user       javascript  user                                    docker-compose.yaml:41
  rabbitmq   -           (image rabbitmq:3.7-management-alpine)  dispatch/docker-compose.yaml:3
  redis      -           (image redis:6.2-alpine)                docker-compose.yaml:14
  ...

  2 declared but not built here (images): rabbitmq, redis
```

Every row points at the file and line that declared it.

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
pnpm install
pnpm test          # vitest, fixtures under test/fixtures
pnpm typecheck
pnpm build         # tsup -> dist/cli.js, dist/index.js
pnpm corpus        # shallow-clone the eight reference repositories into corpus/
```

Every change should run against the whole corpus. A regression on one repo
is a regression on the product.

The package also exposes a library:

```ts
import { analyze, formatRepoReport } from "blastradius";

const analysis = await analyze("./my-repo");
console.log(formatRepoReport(analysis));
```

## Relationship to Vernier

Blast Radius is built by the team behind [Vernier](https://github.com/Go-Vernier),
a behavioural verification platform. Vernier reasons about behaviours inside
one repository. Blast Radius reasons about reach across services. They share
the same idea: every claim carries its evidence and its confidence.

## Licence

MIT. See [LICENSE](LICENSE).
