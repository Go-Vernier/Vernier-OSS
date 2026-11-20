# Agent judge and hand-off Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `vernier --judge` has the user's signed-in Claude Code or Codex check the uncertain and inferred links against the code and return a structured verdict. `vernier agent` hands the terminal to that agent with Vernier's tools attached over MCP.

**Architecture:** A new `agent` module in vernier-core runs the user's `claude` or `codex` CLI. It scrubs API keys from the environment, sends the prompt on stdin, kills the process group on timeout, and builds argv in pure, tested functions. `judge` builds the prompt and schema and validates the answer. `mcp` is a hand-written stdio JSON-RPC server over the existing analysis API. The CLI wires `--judge`, `mcp` and `agent`.

**Tech Stack:** Rust 2024, clap 4, serde_json; no new dependencies.

**Spec:** `docs/superpowers/specs/2026-11-20-agent-judge-design.md`

## Global Constraints

- No new crate dependencies. No `libc`; to kill a process group on unix, run `kill -KILL -<pgid>`, and on Windows use `Child::kill`.
- Every child env drops: `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `OPENAI_API_KEY`, `CODEX_API_KEY`, and names starting `CLAUDECODE` or `CLAUDE_CODE_ENTRYPOINT`.
- Prompts go on stdin only. Never `--bare`. Never read or write `~/.claude` / `~/.codex`.
- Judge argv. Claude: `-p --output-format json --tools Read,Grep,Glob --permission-mode dontAsk --strict-mcp-config --no-session-persistence --disable-slash-commands --system-prompt <SYSTEM> --json-schema <SCHEMA>`. Codex: `exec --sandbox read-only --skip-git-repo-check --ephemeral --color never -C <root> --output-schema <tmp>/schema.json -o <tmp>/answer.json -c developer_instructions=<json-quoted SYSTEM> -`.
- `JUDGE_TIMEOUT_SECONDS = 600`; at most `MAX_LINKS = 20` links judged.
- CI runs `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings` (pedantic), `cargo test` on macOS, Linux, Windows. Tests that run fake agent scripts are `#[cfg(unix)]`.
- Locally: `export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"`.
- Match the house style: doc comments say *why*, terse imperative errors prefixed by the binary's `vernier:`.

## Review Focus

- The agent replies with prose or malformed JSON instead of the schema. Expected: a clear error naming the agent and the first 300 characters, exit 1, and no partial report on stdout. Test in Task 2.
- The agent invents a link Vernier didn't ask about, or judges an asked link twice. Expected: unknown links are dropped, the first verdict wins, and a link the agent skipped shows as `unclear` with "not judged". Test in Task 2.
- The blast radius has no inferred or uncertain links. Expected: no agent run; stderr says "nothing uncertain to judge" and the report prints as normal. Test in Task 3.
- `claude` is installed but signed out while `codex` is ready, and `--judge` was given with no value. Expected: codex is chosen. Test in Task 1.
- The agent hangs. Expected: killed after the timeout with `<agent> timed out after Ns`. Test in Task 1 with a 1 s timeout and a fake script that sleeps.

---

### Task 1: `agent` module — detect, choose, run

**Files:**
- Create: `crates/vernier-core/src/agent.rs`
- Modify: `crates/vernier-core/src/lib.rs` (add `pub mod agent;`)

**Interfaces:**
- Produces:
  - `pub enum Agent { ClaudeCode, Codex }` with `parse(&str) -> Option<Agent>` (`claude`, `claude-code`, `codex`), `binary() -> &'static str`, `label() -> &'static str` ("Claude Code", "Codex"), `install_hint() -> &'static str`.
  - `pub struct Status { pub agent: Agent, pub installed: bool, pub signed_in: bool }`.
  - `pub fn claude_signed_in(stdout: &str) -> bool` (JSON `loggedIn == true`); `pub fn codex_signed_in(exit_ok: bool, output: &str) -> bool` (case-insensitive prefix "logged in").
  - `pub fn detect() -> Vec<Status>`: runs the status commands with scrubbed env and a 20 s timeout.
  - `pub fn choose(named: Option<&str>, env_choice: Option<&str>, statuses: &[Status]) -> Result<Agent, AgentError>`.
  - `pub fn scrub(vars: impl Iterator<Item=(String,String)>) -> Vec<(String,String)>`.
  - `pub fn run(argv: &[String], stdin: &str, cwd: &Path, timeout: Duration) -> Result<RunOutput, AgentError>`, where `RunOutput { status_ok: bool, stdout: String, stderr: String }`. `env_clear()` + scrubbed env, own process group on unix (`CommandExt::process_group(0)`), stdout and stderr read on threads, a `try_wait` loop every 50 ms.
  - `#[derive(Debug, thiserror::Error)] pub enum AgentError { NotReady(String), Failed(String), Timeout(String) }`.

- [ ] **Step 1: Failing tests** in `agent.rs` `mod tests`:
  - `scrub_drops_keys_and_nested_session_vars`: input with `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `CLAUDECODE`, `CLAUDE_CODE_ENTRYPOINT`, `PATH` → only `PATH` remains.
  - `sign_in_parsers`: `claude_signed_in(r#"{"loggedIn":true}"#)`, `!claude_signed_in(r#"{"loggedIn":false}"#)`, `!claude_signed_in("garbage")`, `codex_signed_in(true, "Logged in using ChatGPT")`, `!codex_signed_in(false, "Logged in")`, `!codex_signed_in(true, "Not logged in")`.
  - `choose_prefers_flag_then_env_then_first_ready`: claude installed but signed out and codex ready, no flag → `Codex`; flag `claude` while signed out → `Err(NotReady)` whose message contains `claude auth login`; env `codex` → `Codex`; none ready → error containing both install hints and `--explain`; unknown name → error listing `claude, codex`.
  - `#[cfg(unix)] run_kills_a_hung_child`: argv `["sh","-c","sleep 30"]` with a 1 s timeout → `Err(Timeout)` within 5 s.
  - `#[cfg(unix)] run_feeds_stdin_and_hides_keys`: set `OPENAI_API_KEY` in this process with `std::env::set_var` in an `unsafe` block, then run `sh -c 'cat; echo "[${OPENAI_API_KEY:-none}]"'` with stdin `hi` → stdout `hi[none]\n`.
- [ ] **Step 2:** `cargo test -p vernier-core agent::` → fails to compile.
- [ ] **Step 3:** Implement the interfaces above.
- [ ] **Step 4:** `cargo test -p vernier-core agent::` → pass; clippy clean.
- [ ] **Step 5:** `git commit -m "feat(agent): run the user's signed-in Claude Code or Codex"`

### Task 2: `judge` module — links, prompt, schema, parse, report section

**Files:**
- Create: `crates/vernier-core/src/judge.rs`
- Modify: `crates/vernier-core/src/lib.rs` (`pub mod judge;`, re-export `Judgment`, `format_judgment`), `crates/vernier-core/src/analyze.rs:92,111,126,175` (add `judgment: Option<Judgment>` after `explanation` in `Analysis` and `AnalysisJson`, `skip_serializing_if`), `crates/vernier-core/src/report.rs` (add `format_judgment`), `crates/vernier-core/tests/runtime.rs:459` (add `judgment: None`)
- Test: `crates/vernier-core/src/judge.rs` (unit), `crates/vernier-core/tests/analyze.rs` (rendering)

**Interfaces:**
- Consumes: `agent::{Agent, run, AgentError}`, `blast::Blast`, `graph::BlastGraph` (edge evidence lookup).
- Produces:
  - `pub struct Link { pub from: String, pub to: String, pub confidence: Confidence, pub evidence: Vec<Evidence> }`.
  - `pub fn links_to_judge(blast: &Blast, graph: &BlastGraph) -> Vec<Link>`: distinct `(from,to)` hops whose confidence is inferred or uncertain, in report order, capped at `MAX_LINKS`.
  - `pub const SYSTEM: &str`, `pub fn schema() -> serde_json::Value`, `pub fn prompt(repository: &str, blast: &Blast, links: &[Link]) -> String`.
  - `#[serde(rename_all="lowercase")] pub enum Risk { Low, Medium, High }`, `pub enum Verdict { Confirmed, Doubtful, Unclear }`.
  - `pub struct LinkVerdict { from, to, verdict: Verdict, why: String, file: Option<String>, line: Option<u32> }`.
  - `pub struct Judgment { pub agent: String /* "claude-code" | "codex" */, pub model: Option<String>, pub risk: Risk, pub summary: String, pub links: Vec<LinkVerdict>, pub check: Vec<String> }` (Serialize, Deserialize, camelCase).
  - `pub fn parse(agent: Agent, raw: &serde_json::Value, asked: &[Link]) -> Result<Judgment, AgentError>`.
  - `pub fn judge_argv(agent: Agent, model: Option<&str>, root: &Path, tmp: &Path) -> Vec<String>` (Global Constraints values; the codex variant expects `schema.json` written in `tmp`).
  - `pub fn judge(agent: Agent, model: Option<&str>, analysis: &Analysis, links: &[Link]) -> Result<Judgment, AgentError>`: makes a temp dir under `std::env::temp_dir()` named `vernier-judge-<pid>`, writes `schema.json`, calls `agent::run` with cwd = `analysis.root`, reads `structured_output` (claude; error when `is_error`) or `answer.json` (codex), then `parse`, and removes the temp dir.
  - `pub fn format_judgment(j: &Judgment, color: bool) -> String` in report.rs.

SYSTEM must say: you are reviewing links a static analyzer reported between services; read the evidence files and their callers; `confirmed` = code really makes that call/consumes that topic/shares that table at runtime, `doubtful` = the code does not (dead code, an unused string, a test, a different service with a similar name), `unclear` = cannot tell from the code (configuration decides); cite file and line; never claim a service is safe; do not modify files.

- [ ] **Step 1: Failing tests**:
  - `links_to_judge_takes_only_inferred_and_uncertain_once_each`: build a `Blast` whose paths share an uncertain `web→cart` hop twice and also have a static `payment→cart` hop and an inferred `dispatch→payment` hop. Expect `[web→cart, dispatch→payment]` in that order, with evidence copied from the graph's edge.
  - `schema_is_strict`: every object in `schema()` has `additionalProperties: false` and `required` lists all of its `properties` keys.
  - `prompt_lists_judged_links_with_evidence`: the prompt contains `web -> cart (uncertain) evidence: web/app.js:12` and the blast JSON.
  - `parse_drops_unknown_links_keeps_first_and_fills_skipped`: asked `[a→b, c→d]`; raw links `[a→b confirmed, a→b doubtful, x→y confirmed]` → links `[a→b confirmed, c→d unclear "not judged"]`.
  - `parse_rejects_wrong_shape`: `{"risk":"extreme"}` → `Err(Failed)` whose message contains `Codex returned` and the raw text.
  - `judge_argv_is_read_only`: claude argv contains `--tools`, `Read,Grep,Glob`, `dontAsk`, `--json-schema` and not `--bare`; codex argv contains `read-only`, `-C`, root, `--output-schema`, ends with `-`; `--model`/`-m` present only when a model is given.
  - In `tests/analyze.rs`: `judgments_print_risk_summary_links_and_checks` asserts `format_judgment(.., false)` starts with `JUDGMENT  risk medium · by Claude Code`, has a `confirmed  web -> cart  web/app.js:12` row, and `CHECK` bullets.
- [ ] **Step 2:** `cargo test -p vernier-core judge` → fails to compile.
- [ ] **Step 3:** Implement. Keep the `format_judgment` layout consistent with `format_explanation` (bold header, dim attribution, wrapped bullets).
- [ ] **Step 4:** `cargo test -p vernier-core` → pass.
- [ ] **Step 5:** `git commit -m "feat(judge): ask the agent to verify uncertain and inferred links"`

### Task 3: `--judge` on the CLI and HTML

**Files:**
- Modify: `crates/vernier-cli/src/main.rs` (new `JudgeArgs` group "Judge with Claude Code or Codex", flattened into `AnalyzeArgs`; a call after the explain block; print `format_judgment` after the report; add an EXAMPLES line `vernier --judge   have your Claude Code or Codex check the links against the code`), `crates/vernier-core/src/html.rs` (judgment panel)
- Test: `crates/vernier-cli/tests/cli.rs`

**Interfaces:**
- Consumes: `agent::{detect, choose}`, `judge::{links_to_judge, judge}`, `format_judgment`.
- CLI: `--judge[=AGENT]` (`num_args = 0..=1`, `default_missing_value = ""`, `conflicts_with = "explain"`), `--judge-model <NAME>` (`requires = "judge"`), env `VERNIER_AGENT`. The agent is chosen before the analysis, so a missing agent fails fast.

- [ ] **Step 1: Failing tests** (`#[cfg(unix)]`). Use a helper `fake_agents(dir, claude_json: Option<&str>, codex_json: Option<&str>)` that writes executable `claude`/`codex` scripts: they answer `auth status --json` / `login status`, dump argv and stdin to `<dir>/<name>.args` and `<dir>/<name>.stdin`, and print the canned JSON (claude) or write it to the `-o` path (codex). Put `dir` first on `PATH`; run on fixture `compose-app` with `--files` chosen so the blast radius has an uncertain or inferred hop (pick the fixture file during implementation and name it in the test).
  - `judge_with_claude_prints_judgment_and_json`: `--judge --json` → `json["judgment"]["agent"] == "claude-code"`, risk is copied; `claude.args` contains `dontAsk`; `claude.stdin` contains the link; `ANTHROPIC_API_KEY` set on the parent is absent from the child (the script echoes it into `claude.env`).
  - `judge_codex_by_name`: `--judge=codex` → `json["judgment"]["agent"] == "codex"`.
  - `judge_without_agents_fails_fast`: an empty PATH dir (plus `/bin:/usr/bin`) → exit 1, stderr contains `claude auth login` and `--explain`, stdout empty.
  - `judge_with_nothing_uncertain_skips_the_agent`: a change with only static hops → success, stderr contains `nothing uncertain to judge`, no `claude.args` file.
  - `judge_conflicts_with_explain`: `--judge --explain` → clap error, exit 2.
- [ ] **Step 2:** `cargo test -p vernier-cli judge` → fail.
- [ ] **Step 3:** Implement. Disclosure line before the run, exactly: `vernier: asking {label} (your subscription) to judge {n} link{s}; it reads this repository's code, read-only`. In `html.rs`, render `analysis.judgment` (already in the embedded JSON) as a panel above the details: risk badge, summary, per-link verdict rows, checks. All text goes in via `textContent`, never `innerHTML`.
- [ ] **Step 4:** `cargo test` → pass. Run `cargo run -- test/fixtures/compose-app --files <file> --judge --html $SCRATCH/j.html` against the real `claude`, open the HTML, and check the panel.
- [ ] **Step 5:** `git commit -m "feat(cli): --judge with your Claude Code or Codex"`

### Task 4: `vernier mcp` — the stdio server

**Files:**
- Create: `crates/vernier-core/src/mcp.rs`
- Modify: `crates/vernier-core/src/lib.rs`, `crates/vernier-cli/src/main.rs` (subcommand `Mcp { path: PathBuf (default "."), include_tests: bool }`, hidden from the short help: `#[command(hide = true)]`)
- Test: `crates/vernier-core/src/mcp.rs` (unit), `crates/vernier-cli/tests/cli.rs` (pipe)

**Interfaces:**
- Produces: `pub struct Server { analysis: Analysis }`, `Server::new(analysis)`, `pub fn handle(&mut self, msg: &Value) -> Option<Value>` (None for notifications), `pub fn serve(self, input: impl BufRead, output: impl Write) -> io::Result<()>`.
- Tools and arguments are as in the spec table. `blast_radius` uses `git::pull_request`, `git::diff`, `Change::from_files`, `git::working_change`, `blast::of_change`; default depth `blast::DEFAULT_DEPTH`. Text content is pretty JSON. Tool failures return `{"content":[{"type":"text","text":msg}],"isError":true}`. An unknown method returns JSON-RPC error `-32601`. A line that won't parse returns `-32700` with `id: null`.

- [ ] **Step 1: Failing tests**: `initialize_reports_tools_capability` (`result.protocolVersion == "2025-06-18"`, `capabilities.tools` present, `serverInfo.name == "vernier"`); `initialized_notification_gets_no_reply`; `tools_list_names_three_tools` with input schemas; `blast_radius_for_files` on the `compose-app` fixture returns a JSON text whose `changed[0].service` matches; `unknown_service_edges_is_a_tool_error`; `bad_json_is_parse_error`. CLI: pipe `initialize` + `tools/list` lines into `vernier mcp <fixture>` and get two reply lines.
- [ ] **Step 2:** fail. **Step 3:** implement; stderr only for logs, stdout only for protocol. **Step 4:** pass.
- [ ] **Step 5:** `git commit -m "feat(mcp): serve the blast radius to coding agents over stdio"`

### Task 5: `vernier agent` — hand-off

**Files:**
- Modify: `crates/vernier-core/src/agent.rs` (handoff argv), `crates/vernier-cli/src/main.rs` (subcommand `Agent { path, with: Option<String>, model: Option<String>, prompt: Option<String> }`)
- Test: `agent.rs` unit, `cli.rs` with fake agents

**Interfaces:**
- Produces: `pub fn handoff_argv(agent: Agent, vernier: &Path, root: &Path, mcp_config: &Path, model: Option<&str>, prompt: Option<&str>) -> Vec<String>`, `pub const HANDOFF_INSTRUCTIONS: &str`, `pub fn mcp_config_json(vernier: &Path, root: &Path) -> Value` (`{"mcpServers":{"vernier":{"command":<abs exe>,"args":["mcp",<root>]}}}`).
- Claude argv: `--mcp-config <file> --append-system-prompt <HANDOFF_INSTRUCTIONS> [--model M] [PROMPT]`. Codex argv: `-c mcp_servers.vernier.command=<json str> -c mcp_servers.vernier.args=<json array> -c mcp_servers.vernier.tool_timeout_sec=600 -c developer_instructions=<json str> [-m M] [PROMPT]`.
- The CLI requires a terminal on stdin and stdout and uses `std::env::current_exe()`. It writes the MCP config to a temp file that's removed afterwards, runs with scrubbed env and inherited stdio in cwd = root, and exits with the child's code.

- [ ] **Step 1: Failing tests**: `handoff_argv_attaches_vernier_for_this_session_only` (claude contains `--mcp-config` and never `mcp add`; codex `-c` values parse as TOML strings and arrays, so check that the JSON quoting round-trips through `toml::from_str`); `mcp_config_points_at_this_binary`. CLI: `agent` with stdin not a TTY → exit 1, `agent needs a terminal`.
- [ ] **Step 2–4:** fail → implement → pass. Then run `cargo run -- agent test/fixtures/compose-app` with real claude, ask "which services does catalogue reach?", and confirm the `vernier` tools are called.
- [ ] **Step 5:** `git commit -m "feat(cli): vernier agent hands off to Claude Code or Codex with Vernier's tools"`

### Task 6: Docs and end-to-end check

**Files:** `README.md` (Commands table rows for `--judge`, `vernier agent`; a "Judge with your coding agent" section with the data/terms note), `CHANGELOG.md` `[Unreleased] ### Added`, `docs/how-it-works.md` if it lists the stages.

- [ ] **Step 1:** Write the docs.
- [ ] **Step 2:** Full CI set: `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test && npm run test:packaging` → all pass.
- [ ] **Step 3:** Real runs on a corpus repository with uncertain edges (`ls corpus`), once with `--judge=claude` and once with `--judge=codex`. Record the time taken and whether the verdicts cite real file:line.
- [ ] **Step 4:** `git commit -m "docs: --judge and vernier agent"`
