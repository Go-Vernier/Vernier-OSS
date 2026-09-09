# Stage 4: Blast Radius Implementation Plan

**Goal:** `vernier analyze . --pr 481` (or `--diff`, `--files`) reports which services a change can reach, with depth, confidence and path per service and the fixed wording for the rest; `--history N` aggregates over recent pull requests; the repository report gains the widest-change-surface finding; `--html` writes a self-contained report.

**Architecture:** `blast.rs` owns the walk and the contract types; `git.rs` reads the local repository; `history.rs` runs the walk per pull request and aggregates; `html.rs` renders the page; `report.rs` gains the change report and two sections; the CLI adds six flags. `analyze()` is unchanged and the JSON contract grows additively.

**Spec:** `docs/superpowers/specs/2026-09-10-blast-radius-design.md`. `docs/build-spec.md` § "Stage 4" and § "The reports" are the parent requirements.

## Global constraints

- `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` green after every task; the corpus test unchanged.
- No network. Git is a subprocess on the local repository; a missing pull request is an error that says how to fetch it.
- The JSON contract is additive: `blast` and `history` are omitted when absent.
- Never claim a service cannot be affected. The wording is `blast::NOT_REACHED`.

## Tasks

- [x] **1. Contract and the walk** — `blast.rs`: `Change`, `Blast`, `Reached`, `Hop`, `Relation`, `Touched`; `owners()`, `seeds()`, `radius()`, `of_change()`, `widest()`; unit tests on a hand-built graph for every row of the walk table, the steps not taken, depth, weakest-on-path and strongest-path-wins. `Analysis` and `AnalysisJson` gain `blast` and `history`.
- [x] **2. Git** — `git.rs`: first-parent log with pull request markers, files in a commit or range, `pull_request()` through commits then refs, `diff()`, `recent()`, subdirectory prefixes; unit tests on the markers and prefixes.
- [x] **3. History** — `history.rs`: `run()` and `summarise()`; unit tests on the numbers.
- [x] **4. Reports** — `report.rs`: shared header with the `Change` rows, `format_change_report`, `format_report`, BLAST RADIUS, widest change surface, CHANGE HISTORY.
- [x] **5. HTML** — `html.rs`: template with embedded contract, force layout, panel, findings; escaping tests.
- [x] **6. CLI** — `--pr`, `--diff`, `--files`, `--history`, `--depth`, `--html`; conflicts; errors exit 1.
- [x] **7. Tests** — `tests/blast.rs` on the fixtures and on temporary git repositories built by `tests/common`; CLI tests per flag; the HTML is self-contained and parses.
- [x] **8. Documentation** — README status, usage, the change report, how the blast radius works; spec decisions; this plan.
