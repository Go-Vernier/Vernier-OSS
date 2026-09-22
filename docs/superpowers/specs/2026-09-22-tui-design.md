# The TUI: explore the graph, walk a change — design

Date: 2026-09-22. Status: approved in conversation, implementation follows.
Builds on the four built stages (`docs/build-spec.md`) and the Stage 4
design (`docs/superpowers/specs/2026-09-10-blast-radius-design.md`).

## Decision

The terminal report answers one question per run. The TUI keeps the graph
in memory and lets the reader ask many: which service is this, who calls it,
what evidence says so, and what would a change here reach? It is an
interactive view over the same `Analysis` the reports read. It computes
nothing the engine does not already compute, and it words results the way
the terminal report words them.

`vernier tui [PATH]` is a new subcommand in the same binary. It is built with
ratatui 0.30 and the crossterm 0.29 ratatui re-exports, in a new library
crate, `crates/vernier-tui`. The engine crate takes no new dependency.

## CLI

```bash
vernier tui                          # the current directory
vernier tui path/to/repo --depth 2   # start the walk at two hops
vernier tui . --otel traces.prom     # observed edges take part, as in analyze
vernier tui . --datadog deps.json
vernier tui . --history 100          # how many recent PRs the Changes tab lists (default 50)
vernier tui . --pr 481               # open straight on one change
vernier tui . --diff main...HEAD
vernier tui . --files a/b.js c/d.py
```

The runtime and change flags have the same meaning, conflicts and errors as
they do on `analyze`. The flag parsing and the `runtime_input` and
`change_input` helpers are shared, not copied. `--json`, `--html` and
`--no-color` stay on `analyze`. The TUI respects `NO_COLOR`.

When stdout is not a terminal, `vernier tui` fails with
`vernier: tui needs a terminal; use vernier analyze for a report`.

Analysis runs before the alternate screen opens, with `vernier: analysing
<path>` on stderr, so a slow repository or a failed runtime load prints
its error in the normal terminal.

## Screens

A header line runs across every tab, with the report's own words:
`robot-shop · 11 services · 23 edges · static only · depth 3`. When runtime
data was joined, "static only" becomes `OTel 9/11 matched`, and the partial
join is shown in yellow as it is in the report header.

Tabs, switched with `1`–`4` or `Tab`:

**1 Overview.** The repository report (`format_repo_report`, no colour),
in a scrollable pane. It reuses the findings rather than rebuilding them,
so the TUI and the terminal can never disagree.

**2 Services.** On the left, a filterable list of services: name, language,
inbound and outbound edge counts, with infrastructure dimmed and listed
after code. On the right, the selected service's detail:

- its declaration evidence (`docker-compose.yaml:57`), root, image and
  package name
- its edges, `out` (what it depends on) then `in` (what depends on it), each
  with its type and confidence label, and the call count when observed
- the selected edge's evidence: every `file:line detail`

`Enter` on a service walks the blast radius of a change to that service,
as if one of its files had changed, and opens tab 4. That gives the report's
"widest change surface" finding for every service, not only the widest.

**3 Changes.** The recent pull requests (or commits, with the history's
own wording when the log has no pull request markers): reference, date,
files, changed, reached, title. The CHANGE HISTORY numbers (average, median,
largest, over 10) sit above the list. `Enter` opens that change in tab 4.
`d` prompts for a diff range and `f` for a list of files, both parsed the
way `--diff` and `--files` parse them.

**4 Blast.** One change, the report's BLAST RADIUS section made navigable:

- the headline `1 service changed -> 7 services in the blast radius`, then
  the confidence breakdown and the depth
- CHANGED: services and their files
- REACHED: grouped by depth, coloured by confidence, uncertain dimmed
- for the selected reached service, its path hop by hop, in the report's
  words (`cart calls catalogue (http)`)
- infrastructure on the path, unowned files, and the not-reached list
  under the fixed `NOT_REACHED` wording

`+` and `-` change the depth and re-run the walk; the walk is cheap enough
to run on every key press. `Esc` goes back to the tab that opened it.

## Keys

| Key | Does |
| --- | --- |
| `1`–`4`, `Tab`, `Shift-Tab` | switch tab |
| `j`/`k`, arrows, `PgUp`/`PgDn`, `g`/`G` | move, scroll |
| `h`/`l`, `←`/`→` | move focus between panes |
| `/` | filter the focused list; `Esc` clears it |
| `Enter` | open: service to blast, change to blast, edge to its evidence |
| `+` / `-` | depth, on the Blast tab |
| `d`, `f` | diff range, files, on the Changes tab |
| `?` | key help |
| `q`, `Ctrl-C` | quit |

## Colours

Confidence colours match the HTML report so the two read as one product:
observed `#3ddc97`, static `#5aa9ff`, inferred `#ffb454`, uncertain
`#8b93a7`, changed services `#ff5ca8`. These are true colour, and fall back
to green, blue, yellow, dark grey and magenta when `COLORTERM` does not say
`truecolor`/`24bit`. Warnings are yellow, as in the terminal report.
`NO_COLOR` turns all of it off, leaving bold and reverse video for the
selection.

## Architecture

```
crates/vernier-tui/src/
  lib.rs      run(analysis, options) -> Result<()>: terminal setup, loop, restore
  app.rs      App state and update(Key) -> Action; no terminal, no I/O
  ui/         one render function per tab and the header; pure over &App
  input.rs    the one-line prompt for d and f
```

- `App` owns the `Analysis` and one `Option<Blast>`. The walk re-runs
  through `blast::of_change`, and the history comes from `history::run`,
  computed once at start-up, because it is one git call per entry.
- Updating state is separate from drawing: `update` is a pure function of
  state and key, and the render functions read `&App`. Both are unit-tested
  without a terminal.
- `ratatui::init()` and `ratatui::restore()` handle the alternate screen,
  raw mode and restoring the terminal on panic.
- `report::hop_words` becomes `pub`, so the path reads exactly as it does
  in the terminal report.
- The engine's blocking git calls for `d` stay on the UI thread. A diff
  range takes milliseconds; a spinner is not worth a thread.

## Testing

- `update`: key sequences in, state out (tab, selection, filter, depth,
  back stack).
- Rendering: ratatui's `TestBackend` at 100×30, against the fixture repos
  in `test/fixtures` (`edges-http-app`, `edges-events-app`), with the
  buffer compared to an expected text snapshot per tab. That is the same
  approach as the expected corpus files.
- One run over every corpus repository, that renders every tab at 80×24 and
  120×40 without a panic, and checks that the Blast tab's headline equals
  the terminal report's line for the widest service.
- By hand, the TUI is run on robot-shop and train-ticket (45 services) to
  check the lists scroll and stay legible.

## Out of scope

Editing or opening evidence files in `$EDITOR`, mouse support, a graph
drawn in the terminal, live reload on file change, watching a runtime URL,
exporting from the TUI (that is `analyze --html` / `--json`), themes.

## Decisions

1. `Enter` on a service seeds the walk with the service's root as a changed
   file, `Change::from_files(&[root])` with the reference `service <name>`.
   It goes through `owners` like a real change, so two services declaring the
   same root are both changed, as they would be for a real file there. There
   is no engine change. An infrastructure service has no root, so `Enter`
   says `infrastructure has no files here; the walk starts from code`.
2. The Changes tab reads the last 50 pull requests by default (`--history`).
   Outside a git repository, the tab shows the engine's error instead of a
   list.

## Decisions made while building

- `history::run` is now `history::changes` (the commits and their changes),
  then `history::entry` for each, then `summarise`. The Changes tab keeps each
  `Change`, so `Enter` walks it without a second git call. `--history` output
  is unchanged.
- `report::runtime_words` returns the Runtime row's words and whether the
  join is partial, and `report::hop_words` is public, so the header and the
  path pane cannot drift from the report.
- The header runs most important first: repository, services, runtime,
  depth, edges, infrastructure. A narrow terminal cuts it from the end, so
  a partial join stays visible.
- The Services list takes what its names need, up to half the screen. When
  that is not enough, the LANGUAGE column goes rather than cutting names
  (train-ticket, whose names run to 29 characters).
- The edge table's direction column says `out` and `in`, matching the list's
  IN and OUT columns. At 80 columns, `depended on by` left no room for the
  service name. CALLS appears only when an edge was observed.
- The service's declaration facts take one row each, cut at the pane's edge;
  the Overview has them in full. Edge evidence wraps, because it is what the
  reader opened the pane for.
- The not-reached names get at most an eighth of the screen, and end in
  `and N more` when they do not fit. Cutting them off silently would have
  looked like a complete list.
- Wrapped heights come from ratatui's `Paragraph::line_count`, behind the
  `unstable-rendered-line-info` feature. Counting characters undercounted
  word-wrapped lines and cut off the last one.
- The snapshot tests build a throwaway repository named `acme/shop`, with a
  fixed commit date, so the header, the dates and the commit hashes are the
  same on every machine. The fixtures sit inside this repository, and their
  own history would change with every commit.
- The CLI refuses to start unless both stdin and stdout are terminals:
  keys come from one and frames go to the other.

### The visual pass

- The Blast tab draws the walk as a tree (`tree.rs`), with the changed
  services as roots. The engine keeps one path per reached service, and the
  tree is the union of those paths. When a service lies on another's path
  but was reported with a stronger path of its own, it appears there as a
  dimmed step (`○`), and as `●` where its own path ends. The cursor moves
  over tree rows, so `+`/`-` keeps it on the same service by name.
- A reach meter under the headline shows reached out of every code service
  the change did not touch. The confidence counts moved to a bar chart
  beside the tree, next to one for depth.
- The Overview's WIDEST CHANGE SURFACE list runs the same walk
  `blast::widest` runs, for every code service, and breaks ties the same
  way. It is recomputed when the depth changes.
- The Services diagram puts dependents on the left and dependencies on the
  right, each coloured by the weakest of its edges. When the names do not
  fit side by side, it stacks vertically. It gives up rows before the edge
  table and the evidence do: its cap drops until it fits, down to
  `N services`, and it goes entirely when nothing fits. It never cuts a
  line off silently.
- Bars use eighth blocks, and any count above zero draws at least an
  eighth, so a small count never looks like none.
