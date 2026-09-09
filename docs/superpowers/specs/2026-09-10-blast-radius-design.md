# Stage 4: blast radius and the reports — design

Date: 2026-09-10. Status: approved in conversation, implementation follows.
Builds on `docs/build-spec.md` § "Stage 4 — Blast radius computation" and
§ "The reports", and on the Stage 3 engine
(`docs/superpowers/specs/2026-09-09-runtime-join-design.md`).

## Decision

Stages 1 to 3 build a graph. Stage 4 asks it the question the tool exists
to answer: given a change, which services can it reach? A change is a set
of files, taken from a pull request in the local git history, from a git
diff range, or listed on the command line. Each file is mapped to the
service that owns its directory; those services seed a walk that follows
dependency edges outward to a bounded depth. Every reached service carries
the depth it was reached at, the weakest confidence on the path that
reached it, and that path. Everything else is "not in the computed blast
radius"; the tool never says a service cannot be affected.

The same walk, run once per service, gives the repository report its
"widest change surface" finding; run once per pull request in the recent
history, it gives the CHANGE HISTORY section. A self-contained HTML report
draws the graph and the radius.

No new dependency. Git is called as a subprocess, as `repository_name`
already does; nothing is fetched from the network.

## CLI

```bash
vernier analyze . --pr 481                  # one pull request, from the local git history
vernier analyze . --diff main...HEAD        # any git diff range
vernier analyze . --files a/b.js c/d.py     # explicit files, repository-relative
vernier analyze . --history 50              # the last 50 pull requests
vernier analyze . --pr 481 --depth 2        # walk two hops instead of three
vernier analyze . --html out.html           # write the self-contained HTML report
```

`--pr`, `--diff` and `--files` are mutually exclusive: one change at a time.
`--history` adds a section to the repository report and may be combined with
any of them. `--depth` defaults to 3. `--html` writes the file and still
prints the terminal report (or the JSON with `--json`). `--otel` and
`--datadog` combine with all of these: observed edges take part in the walk
like any other.

## Where a change comes from

**`--pr N`.** The pull request must be in the local history or fetched into
a local ref; the tool never calls a forge API. It is looked for in this
order, first hit wins:

1. A first-parent commit on `HEAD` whose subject carries the number: a merge
   commit `Merge pull request #N ...` (GitHub), `... (#N)` at the end of the
   subject (squash merge), or `(pull request #N)` (Bitbucket). Its files are
   `git diff --name-only <commit>^1 <commit>`; a root commit uses
   `git diff-tree --root`.
2. A ref holding the pull request's head: `refs/pull/N/head`,
   `refs/pull/N/merge`, `refs/remotes/origin/pr/N`,
   `refs/remotes/origin/pull/N/head`, `refs/heads/pr-N`, `refs/heads/pull/N`.
   Its files are the diff from `git merge-base HEAD <ref>` to the ref.

Anything else is an error that says how to get the pull request locally
(`git fetch origin pull/N/head:refs/pull/N/head`) and names `--diff` and
`--files` as alternatives. The first 10 000 first-parent commits are searched.

**`--diff RANGE`.** `git diff --name-only RANGE`, verbatim; whatever git
accepts (`main...HEAD`, `HEAD~3`, `a..b`, `--cached` is not supported).

**`--files ...`.** Paths relative to the analysed root, as given. Nothing is
checked against the file system: a deleted file still belongs to the
service that owned it.

When the analysed directory is a subdirectory of the git repository, paths
from git are made relative to the analysed root and files outside it are
dropped and counted.

**`--history N`.** The first-parent commits on `HEAD`, newest first, keeping
those whose subject carries a pull request number by the rules above, until
N are found. When no commit carries one, the last N first-parent commits are
used instead and the report says `(last N commits)` and that no pull request
markers were found. Each entry's files come from its diff to its first
parent.

## Mapping files to services

A service owns a file when the file's path equals the service root or lies
under it. The longest matching root wins, so `services/checkout/lib` beats
`services`. Several services that share a root all own the file. A file
under no root is *unowned*: it is listed, counted and seeds nothing. A
service whose root is `.` (the single-service fallback) owns everything;
the radius is then empty and the report repeats the single-service message.

## The walk

Edges point from the dependent to the dependency (`checkout -> payment`:
checkout calls payment), except event edges, which the mapping stage
writes producer → consumer (`payment -> dispatch`: payment publishes,
dispatch consumes). The walk starts from the changed services at depth 0
and, from each service it holds, takes these steps to depth `--depth`:

| From service N, follow | Reaches | Relation | Confidence of the hop |
| --- | --- | --- | --- |
| inbound `http`, `grpc` edge `S -> N`, S code | S | S calls N | the edge's |
| inbound `import` edge `S -> N`, S code | S | S imports N | the edge's |
| inbound `database` edge `S -> N`, S code (a shared database) | S | S shares a database with N | the edge's |
| outbound `event` edge `N -> T`, T code (a consumer) | T | T consumes events from N | the edge's |
| outbound `event` edge `N -> B`, B infrastructure (a broker), **N a changed service only** | every code C with an edge `C -> B`, C ≠ N | C shares broker B with N | Uncertain |

Not followed: outbound `http`, `grpc`, `import` and `database` edges (what
N depends on is not affected by a change to N); inbound `event` edges (a
consumer changing does not reach its producer); the broker step from a
service that was itself reached rather than changed, because that step
carries no topic evidence and chaining it would join the whole repository
through one broker. Infrastructure is never "reached": a broker on the
path is listed under the radius as touched, with the service that publishes
to it, and is not counted in the headline number.

A service's confidence is the weakest hop on the path that reached it; when
several paths reach it, the strongest path wins and, among equally strong
paths, the shortest. The search keeps every non-dominated (confidence,
depth) label per service, so a depth-2 static path beats a depth-1
uncertain one and both are found. The headline number counts reached code
services; the changed services are not part of it.

The wording for everything else is fixed:
*Not in the computed blast radius - no static or observed runtime path found.*

## JSON contract

Additive. `blast` and `history` are present only when asked for.

```json
"blast": {
  "change": { "kind": "pr", "reference": "#481", "how": "merge commit 7d13248",
              "title": "Merge pull request #481 from acme/checkout-retry",
              "date": "2026-09-09", "files": ["services/checkout/src/pay.ts"], "outsideRoot": 0 },
  "depth": 3,
  "changed": [{ "service": "checkout", "files": ["services/checkout/src/pay.ts"] }],
  "unowned": ["README.md"],
  "reached": [{ "service": "payment", "depth": 1, "confidence": "observed",
                "path": [{ "from": "checkout", "to": "payment", "relation": "calls",
                           "type": "http", "confidence": "observed" }] },
              { "service": "dispatch", "depth": 1, "confidence": "uncertain",
                "path": [{ "from": "checkout", "to": "dispatch", "relation": "shares-broker",
                           "type": "event", "confidence": "uncertain", "via": "rabbitmq" }] }],
  "infrastructure": [{ "service": "rabbitmq", "via": "checkout" }],
  "notReached": ["catalogue", "ratings"],
  "summary": { "changed": 1, "reached": 7,
               "byConfidence": { "observed": 2, "static": 3, "inferred": 1, "uncertain": 1 } }
},
"history": {
  "requested": 50, "found": 37, "unit": "pull requests",
  "depth": 3,
  "entries": [{ "reference": "#481", "commit": "7d13248", "date": "2026-09-09",
                "title": "...", "files": 12, "changed": 1, "reached": 7 }],
  "average": 4.2, "median": 3,
  "largest": { "reference": "#388", "reached": 31 },
  "over10": { "count": 7, "percent": 14 }
}
```

`change.kind` is `pr`, `diff` or `files`; `how` says which rule found a
pull request. A hop's `from` is the service already in the radius and `to`
the one it reaches; `relation` is `calls`, `imports`, `shares-database`,
`consumes` or `shares-broker`, and `via` names the broker for the last.
`reached` is sorted by depth, then confidence (strongest first), then name.
`history.unit` is `pull requests` or `commits`.

## Reports

**Change report** (`--pr`, `--diff`, `--files`). The header gains a
`Change` row; the body is the radius, then the RUNTIME section when a
source was joined. Services, structure, edges and findings belong to the
repository report and are not repeated.

```
VERNIER

  Repository    acme/platform
  Services      42 detected  (docker-compose)
  Runtime       not connected - static only
  Change        PR #481  merge commit 7d13248  2026-09-09
                12 files in 1 service

BLAST RADIUS

  1 service changed -> 7 services in the blast radius
  2 observed · 3 static · 1 inferred · 1 uncertain · depth 3

  CHANGED
  checkout       12 files   services/checkout/src/pay.ts, services/checkout/src/index.ts, ...

  REACHED
  SERVICE        DEPTH  CONFIDENCE  PATH
  payment        1      observed    payment calls checkout (http, 1204 calls)
  web            1      uncertain   web calls checkout (http)
  notifications  1      inferred    notifications consumes events from checkout
  orders         2      static      orders calls payment; payment calls checkout
  dispatch       1      uncertain   dispatch shares broker rabbitmq with checkout

  Infrastructure on the path    rabbitmq (published to by checkout)

  3 changed files belong to no service: README.md, Makefile, docs/adr/0007.md

  Not in the computed blast radius - no static or observed runtime path found
    34 services  catalogue, cart, ...
```

A change that touches no service says so:
`0 services changed -> 0 services in the blast radius`, and lists the files.

**Repository report.** FINDINGS gains

```
  Widest change surface                  checkout
    a change here reaches 14 services
```

computed by running the walk from each code service at the default depth.
With `--history N`, a CHANGE HISTORY section follows FINDINGS:

```
CHANGE HISTORY  (last 37 PRs)

  Average blast radius     4.2 services
  Median                   3
  Largest                  PR #388 - 31 services
  PRs reaching >10         7  (19%)
```

When fewer than N were found, the heading says how many were. When the
history has no pull request markers, the heading says `(last 50 commits)`
and a line under it says why.

**HTML report** (`--html`). One file, no external resource, so it works
offline and can be emailed. It embeds the JSON contract and draws a
force-directed graph: nodes sized by inbound edges, coloured by the
strongest confidence of their inbound edges, infrastructure drawn hollow;
edges coloured by confidence, uncertain ones dashed. When a change was
analysed the changed services are ringed, reached services keep their
colour and everything else is dimmed. Clicking a node opens a panel with
its declaration, its edges and their evidence (file and line, detail, call
count). The header repeats the report header and the findings. The layout
is computed by a small force simulation written for this file; there is no
library.

## Architecture

```
crates/blastradius-core/src/blast.rs     Change, ChangeKind, Blast, Changed, Reached, Hop, Relation, Touched; owners(), radius(), of_change(), widest()
crates/blastradius-core/src/git.rs       GitError; changed files for a commit, a range, a pull request; first-parent log with PR markers
crates/blastradius-core/src/history.rs   History, Entry; run(analysis, n, depth)
crates/blastradius-core/src/html.rs      render(analysis) -> String
crates/blastradius-core/src/analyze.rs   Analysis.blast, Analysis.history; AnalysisJson gains blast, history (omitted when None)
crates/blastradius-core/src/report.rs    Change row, BLAST RADIUS, Widest change surface, CHANGE HISTORY; format_report dispatches
crates/blastradius-cli/src/main.rs       --pr, --diff, --files, --history, --depth, --html
```

`analyze(root)` keeps its signature. The CLI calls `analyze`, joins the
runtime when asked, then `blast::of_change` and `history::run` when asked,
and picks the report from what the analysis holds. The library exposes the
same calls.

## Validation

- Unit tests in `blast.rs` on a hand-built graph: file ownership (longest
  root, shared roots, `.`, unowned), every row of the walk table, the steps
  not taken, the depth cut, weakest-on-path, strongest-path-wins, broker
  step from changed services only, the fixed wording.
- Integration tests on the fixtures: `edges-http-app` (calls, depth 2 via
  cart, `--depth 1` cuts it), `edges-events-app` (consumers, broker step,
  depth 3), `edges-db-app` (shared database), `edges-import-app` (imports),
  `runtime-app` with `traces.prom` (observed hop with its call count).
- Git tests build a temporary repository from a fixture with a merge commit
  carrying `Merge pull request #7`, a squash commit carrying `(#8)` and a
  plain commit: `--pr 7`, `--pr 8`, a missing number's error text, `--diff`,
  `--history` finding two pull requests, and the commits-only fallback.
- CLI tests per flag and for the conflicts; an `--html` test that the file
  is written, embeds the data and loads no external resource.
- Corpus: the report's new finding runs on every repository; the corpus
  test's expectations are unchanged.

## Out of scope

Forge APIs (`gh`, GitLab), `--pr` for an unfetched pull request, weighting
by call frequency, per-file ownership finer than a directory, a "last
observed" date on static edges, publishing the npm package.

## Decisions made while building

- `ChangeKind` gained `Commit` for a history without pull request markers,
  so an entry's kind says what it is rather than borrowing `diff`.
- `History` holds `f64` for the average and median, so `AnalysisJson` derives
  `PartialEq` but no longer `Eq`. Nothing needed `Eq`.
- `widest()` breaks a tie on the first name in service order; in the unit
  graph `dispatch` and `payment` both reach seven services and `dispatch` is
  reported. A finding that flips between runs would be worse than one that
  picks a side.
- The walk runs on the graph at `HEAD`, also for older pull requests in
  `--history`. Re-analysing the tree at each merge commit would need a
  checkout per entry; the report says which graph it used. The test
  repositories append to files rather than overwrite them, because
  overwriting `web/default.conf.template` silently removed web's edges and
  the history numbers changed for a reason that had nothing to do with git.
- `--diff` splits its value on whitespace, so `HEAD~3 HEAD~1` works as well
  as `main...HEAD`.
- `--files` is greedy (`num_args = 1..`); the repository path goes before it.
- The change report repeats the RUNTIME section when a source was joined,
  because a partial join changes the radius and the reader should see the
  mapping next to the result. It does not repeat SERVICES, STRUCTURE, EDGES
  or FINDINGS.
- `--html` reports the written path on stderr, so stdout stays the report or
  the JSON.
- The HTML page computes its findings in the browser from the embedded
  contract, with the same rules as the terminal: the shared-database finding
  reads the `shared database <key> with ...` evidence detail rather than
  counting every database edge, which on robot-shop would have said 4 where
  the terminal says 0. The widest-change-surface value travels with the data
  because it needs the walk.
- The HTML layout uses a seeded generator so the same graph draws the same
  picture; positions are not stored in the contract.
- Chrome's extension could not open the generated file during the build; the
  page was checked with headless Chrome instead: the script runs to the end,
  the panel and headline are filled and every node and edge is drawn.
