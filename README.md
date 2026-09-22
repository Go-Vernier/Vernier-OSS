<div align="center">

# Vernier

**Which services can this change reach?**

See the blast radius of a pull request before you merge it.

[![CI](https://github.com/Go-Vernier/Vernier-OSS/actions/workflows/ci.yml/badge.svg)](https://github.com/Go-Vernier/Vernier-OSS/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-3ddc97.svg)](LICENSE)
[![Rust 1.85+](https://img.shields.io/badge/rust-1.85%2B-ff5ca8.svg?logo=rust)](https://www.rust-lang.org)
[![Status: Phase 0](https://img.shields.io/badge/status-phase%200-ffb454.svg)](docs/build-spec.md)
[![No telemetry](https://img.shields.io/badge/telemetry-none-5aa9ff.svg)](#principles)
[![Works offline](https://img.shields.io/badge/works-offline-8b93a7.svg)](#principles)

</div>

---

Vernier reads a repository, finds its services, maps which service calls
which, and tells you what a change can affect. It combines what the code
*could* call (static analysis) with what production *actually* calls
(OpenTelemetry or Datadog traces). Every edge it reports points to a file and
line, and says how sure the tool is.

## Quick start

```bash
git clone https://github.com/Go-Vernier/Vernier-OSS.git
cd Vernier-OSS
cargo build --release

./target/release/vernier analyze /path/to/repo      # the report
./target/release/vernier tui /path/to/repo          # explore it interactively
```

Not yet published to npm or crates.io. You need a stable Rust toolchain.

## What you get

```
$ vernier analyze robot-shop --files cart/server.js

BLAST RADIUS

  1 service changed -> 4 services in the blast radius
  2 static · 1 inferred · 1 uncertain · depth 3

  REACHED
  SERVICE   DEPTH  CONFIDENCE  PATH
  payment   1      static      payment calls cart (http)
  shipping  1      static      shipping calls cart (http)
  web       1      uncertain   web calls cart (http)
  dispatch  2      inferred    dispatch consumes events from payment; payment calls cart (http)

  Not in the computed blast radius - no static or observed runtime path found
    6 services  catalogue, load, mongodb, mysql, ratings, user
```

Or explore it in the terminal with `vernier tui`:

```
◆ catalogue  changed · 1 file            BY DEPTH
├── ● cart  calls catalogue (http)       depth 1 ████████████████ 3
│   ├── ● payment  calls cart (http)     depth 2 ██████████▋      2
│   │   └── ● dispatch  consumes events  depth 3 █████▎           1
│   └── ● shipping  calls cart (http)
├── ● ratings  calls catalogue (http)    BY CONFIDENCE
└── ● web  calls catalogue (http)        static    ██████████████ 4
```

## Commands

| Command | What it does |
| --- | --- |
| `vernier analyze .` | Services, edges and findings for the whole repository |
| `vernier analyze . --pr 481` | Blast radius of one pull request from the local git history |
| `vernier analyze . --diff main...HEAD` | Blast radius of a git diff range |
| `vernier analyze . --files a.js b.py` | Blast radius of specific files |
| `vernier analyze . --history 50` | Blast radius of each of the last 50 pull requests |
| `vernier analyze . --otel traces.prom` | Add production traces from OpenTelemetry |
| `vernier analyze . --datadog deps.json` | Add production traces from Datadog |
| `vernier analyze . --html report.html` | A self-contained HTML report with the graph |
| `vernier analyze . --json` | The full graph as JSON |
| `vernier tui .` | Interactive view: overview, services, changes, blast radius |

`--depth N` (default 3) sets how many hops the walk follows.

In the TUI: `1`–`4` switch tabs, `j`/`k` move, `Enter` walks the blast radius
of the selected service or pull request, `+`/`-` change the depth, `?` shows
every key, and `q` quits.

## Tested on real repositories

It finds the services in each of these in about 100 ms, with no configuration:

| Repository | Services |
| --- | ---: |
| FudanSELab/train-ticket | 45 |
| dotnet/eShop | 19 |
| open-telemetry/opentelemetry-demo | 16 |
| GoogleCloudPlatform/microservices-demo | 11 |
| instana/robot-shop | 11 |
| spring-petclinic/spring-petclinic-microservices | 10 |
| ewolff/microservice | 6 |

## Confidence labels

| Label | Meaning |
| --- | --- |
| 🟢 **Observed** | Seen in production traces |
| 🔵 **Static** | Found in the code, never observed |
| 🟠 **Inferred** | Joined through a topic or a shared database |
| ⚪ **Uncertain** | A computed URL or a fuzzy name match, included on purpose |

A reached service gets the weakest label on its path.

## Principles

- **Recall over precision.** When unsure, it includes the service and labels it Uncertain.
- **Evidence for everything.** Every service and edge points to a file and line, or a trace count.
- **Honest about gaps.** It never says a service *cannot* be affected, only that no path was found.
- **No telemetry.** It sends nothing anywhere.
- **Works offline.** Nothing touches the network unless you pass a URL.

## Learn more

- [How it works](docs/how-it-works.md): discovery, mapping, the runtime join, the blast radius walk, the TUI, and developing
- [Build spec](docs/build-spec.md): what Phase 0 set out to build

Vernier is the open-source part of the [Vernier](https://github.com/Go-Vernier)
behavioural verification platform.

## License

MIT. See [LICENSE](LICENSE).
