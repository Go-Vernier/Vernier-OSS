# Changelog

Every release of Vernier, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **`vernier` with no arguments** walks your changes on this branch: the
  commits since the default branch, uncommitted edits and new files. With
  nothing changed it prints a short summary and what to try next. Every
  `analyze` flag works on bare `vernier`; `vernier analyze` still works.
- **`--explain`**: a plain-English summary of the blast radius from your own
  LLM key. Anthropic, OpenAI, Gemini, Ollama and any OpenAI-compatible API,
  picked with `--llm`, `--model` or `VERNIER_LLM*` variables. Only the blast
  radius is sent; no source code.
- **`--full`** prints every service, edge and finding, the old default.
- **`--include-tests`** reads test, fixture and example directories.

### Changed

- Test, fixture and example directories (`test`, `tests`, `__tests__`,
  `e2e`, `fixtures`, `__fixtures__`, `testdata`, `examples`) are skipped by
  default, so fixtures are not reported as services and test code adds no
  edges.
- The unmatched-target line counts distinct targets, and the references
  separately when they differ.

## [0.1.0] - 2026-10-04

The first release. Install with `npx @go-vernier/cli`, `brew install
go-vernier/tap/vernier`, or the shell and PowerShell installers; see the
README.

### Added

- **Service discovery** with no configuration: docker-compose (with `.env`
  resolution), Kubernetes manifests, monorepo layouts (`services/`, `apps/`,
  `packages/`, ...) and workspace config (npm, pnpm, Cargo, Nx). A
  single-service or deploy-only repository is named for what it is.
- **Static edges** between services: HTTP calls, gRPC stubs, message topics
  and typed events, shared databases and cross-package imports, read from
  JavaScript, TypeScript, Python, Go, Java, C# and PHP through tree-sitter,
  and from configuration files. Every edge points to a file and line.
- **Runtime join**: production edges from OpenTelemetry (`--otel`,
  servicegraph scrape or OTLP JSON) and Datadog (`--datadog`), with a report
  of how trace names matched services.
- **Blast radius** of a change: `--pr`, `--diff`, `--files`, and `--history`
  for the last N pull requests, with Observed, Static, Inferred and
  Uncertain confidence labels and `--depth`.
- **Reports**: terminal, `--json`, and a self-contained `--html` report with
  the graph.
- **`vernier tui`**: explore the services, changes and blast radius
  interactively.
- **Prebuilt binaries** for macOS (Apple silicon, Intel), Linux (x64, arm64;
  static, any distribution) and Windows (x64).

[0.1.0]: https://github.com/Go-Vernier/Vernier-OSS/releases/tag/v0.1.0
