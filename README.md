<div align="center">

# Vernier

**Which services can this change reach?**

See the blast radius of a pull request before you merge it.

[![CI](https://github.com/Go-Vernier/Vernier-OSS/actions/workflows/ci.yml/badge.svg)](https://github.com/Go-Vernier/Vernier-OSS/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-3ddc97.svg)](LICENSE)
[![Rust 1.85+](https://img.shields.io/badge/rust-1.85%2B-ff5ca8.svg?logo=rust)](https://www.rust-lang.org)
[![Release](https://img.shields.io/github/v/release/Go-Vernier/Vernier-OSS?color=ffb454)](https://github.com/Go-Vernier/Vernier-OSS/releases/latest)
[![npm](https://img.shields.io/npm/v/@go-vernier/cli?color=cb3837&logo=npm)](https://www.npmjs.com/package/@go-vernier/cli)
[![No telemetry](https://img.shields.io/badge/telemetry-none-5aa9ff.svg)](#principles)
[![Works offline](https://img.shields.io/badge/works-offline-8b93a7.svg)](#principles)

</div>

---

Vernier reads a repository, finds its services, maps which service calls
which, and tells you what a change can affect. It combines what the code
*could* call (static analysis) with what production *actually* calls
(OpenTelemetry or Datadog traces). Every edge it reports points to a file and
line, and says how sure the tool is. For the links it isn't sure about,
`--judge` has your own Claude Code or Codex read the code and say which are
real.

![vernier: the blast radius of one file, then judged by Claude Code](docs/demo.gif)

## Install

```bash
npx @go-vernier/cli                      # run it once, nothing to install
npm install -g @go-vernier/cli           # npm
curl -fsSL https://github.com/Go-Vernier/Vernier-OSS/releases/latest/download/install.sh | sh
```

On Windows, in Command Prompt or PowerShell:

```powershell
powershell -ExecutionPolicy ByPass -c "[Net.ServicePointManager]::SecurityProtocol = 'Tls12'; irm https://github.com/Go-Vernier/Vernier-OSS/releases/latest/download/install.ps1 | iex"
```

Prebuilt for macOS (Apple silicon, Intel), Linux (x64, arm64; any
distribution) and Windows (x64). Every download is checked against its
SHA-256. From source, with a stable Rust toolchain:

```bash
cargo install --git https://github.com/Go-Vernier/Vernier-OSS vernier-cli
```

## Quick start

Run it inside a repository:

```bash
vernier                            # what your changes on this branch can reach
vernier --judge                    # and have your Claude Code or Codex check it
vernier tui                        # explore it interactively
```

With no arguments, `vernier` walks your changes: the commits on this branch
since the default branch, uncommitted edits and new files. With nothing
changed it prints a short summary and what to try next. Test, fixture and
example directories are skipped (`--include-tests` reads them).

## What you get

```
$ vernier robot-shop --files cart/server.js

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
| `vernier` | Blast radius of your changes on this branch |
| `vernier --pr 481` | Blast radius of one pull request from the local git history |
| `vernier --diff main...HEAD` | Blast radius of a git diff range |
| `vernier --files a.js b.py` | Blast radius of specific files |
| `vernier --explain` | Add a plain-English summary written by your own LLM |
| `vernier --judge=codex` | Have your Claude Code or Codex check the uncertain links against the code |
| `vernier agent` | Open Claude Code or Codex here with Vernier's tools attached |
| `vernier --full` | Every service, edge and finding in the repository |
| `vernier --history 50` | Blast radius of each of the last 50 pull requests |
| `vernier --otel traces.prom` | Add production traces from OpenTelemetry |
| `vernier --datadog deps.json` | Add production traces from Datadog |
| `vernier --html report.html` | A self-contained HTML report with the graph |
| `vernier --json` | The full graph as JSON |
| `vernier tui` | Interactive view: overview, services, changes, blast radius |

Every command takes a repository path (`vernier ../shop --pr 12`); the
default is the current directory. `vernier analyze` is the same as `vernier`.
`--depth N` (default 3) sets how many hops the walk follows.

In the TUI: `1`–`4` switch tabs, `j`/`k` move, `Enter` walks the blast radius
of the selected service or pull request, `+`/`-` change the depth, `?` shows
every key, and `q` quits.

## Judge it with your coding agent

Some links are inferred or uncertain. `--judge` has the Claude Code or Codex
you already use read the repository, read-only, and check each of those links
against the code. It checks at most 20. No API key is needed: it runs on your
signed-in agent.

```bash
vernier --judge                    # uses the agent it finds
vernier --judge=codex              # pick one: claude or codex
vernier --judge --judge-model opus # pass a model name to the agent
vernier agent                      # open the agent here and ask follow-ups
vernier agent --with codex --prompt "What breaks if cart goes down?"
```

Write `--judge=codex` with an `=`. `--judge codex` with a space reads `codex`
as the repository path. Without a value, `VERNIER_AGENT` picks the agent, and
then the first of `claude` and `codex` that is installed and signed in.
`--judge` and `--explain` can't be used together.

The report gains a JUDGMENT section: an overall risk, a short summary, a
verdict for each link (confirmed, doubtful or unclear, with `file:line`), and
CHECK items. It is `judgment` in `--json` and a panel in `--html`. A run takes
from about 30 seconds with Claude Code to a minute or more with Codex, and
counts against your plan's limits. Vernier stops waiting after 10 minutes. If
nothing is uncertain, no agent runs. From
`vernier --files cart/server.js --judge=codex`, trimmed:

```
JUDGMENT  risk medium · by Codex

  The web-to-cart route is configuration-dependent, not statically bound to
  the cart service. The supplied Compose configuration does not set CART_HOST
  for web, so runtime routing must be verified for deployments using this
  change.

  unclear    web -> cart  web/default.conf.template:2
    Web proxies /api/cart/ to the runtime value of CART_HOST; the repository
    does not bind that variable to cart in the web service configuration.

CHECK
  - Confirm CART_HOST resolves to the cart service in every deployed web
    environment.
  - Exercise /api/cart/ against the changed cart server and verify response
    compatibility.

  Judged by an agent that read the code.
  It can be wrong; verify before relying on it.
```

`vernier agent [PATH]` opens Claude Code or Codex in the repository with
Vernier's tools (`blast_radius`, `services`, `edges`) attached for that
session only, so you can ask follow-up questions. `--with` picks the agent,
`--model` the model and `--prompt` the first message. Nothing is written to
your Claude Code or Codex configuration.

**What it sends.** Unlike `--explain`, the judge and `vernier agent` let your
own agent read the repository's source code, under your plan and your terms
with that provider. Consumer plans are not meant for regulated or health
data. For that, use `--explain` with an API key on an endpoint your
agreements cover, or don't use the AI features.

## Explain it with your own LLM

`--explain` sends the blast radius to an LLM you choose and prints a short
summary for the pull request under the report. Bring your own key; Vernier
has no account and no server.

```bash
export ANTHROPIC_API_KEY=...       # or OPENAI_API_KEY, or GEMINI_API_KEY
vernier --explain
vernier --explain --llm openai --model gpt-6-luna
vernier --explain --llm ollama --model qwen3     # local, nothing leaves the machine
```

| Provider | `--llm` | Key | Default model |
| --- | --- | --- | --- |
| Anthropic | `anthropic` | `ANTHROPIC_API_KEY` | `claude-opus-5-5` |
| OpenAI | `openai` | `OPENAI_API_KEY` | `gpt-6.1-sol` |
| Google Gemini | `gemini` | `GEMINI_API_KEY` | `gemini-3.8-flash` |
| Ollama | `ollama` | none | pass `--model` |
| Any OpenAI-compatible API | `openai-compatible` | `VERNIER_LLM_API_KEY` | pass `--model` |

Without `--llm`, the first key found picks the provider. `VERNIER_LLM`,
`VERNIER_LLM_MODEL` and `VERNIER_LLM_URL` (the API base, such as
`https://openrouter.ai/api/v1` or a local server) set the same things from the
environment. Keys are only ever read from the environment.

The model is sent the blast radius only: service names, the changed file
paths, the hops between services and their confidence. No source code, and no
evidence snippets, which can hold connection strings. The explanation is a
summary; the blast radius above it is the evidence.

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
- **No telemetry.** It sends nothing anywhere unless you ask it to.
- **Works offline.** Nothing touches the network unless you pass a URL, `--explain` or `--judge`.

## Learn more

- [How it works](docs/how-it-works.md): discovery, mapping, the runtime join, the blast radius walk, the TUI, and developing
- [Build spec](docs/build-spec.md): what Phase 0 set out to build

Vernier is the open-source part of the [Vernier](https://github.com/Go-Vernier)
behavioural verification platform.

## License

MIT. See [LICENSE](LICENSE).
