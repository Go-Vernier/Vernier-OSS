# Agent judge and hand-off: the user's Claude Code or Codex as Vernier's reviewer — design

Date: 2026-11-20. Status: approved in conversation ("Judge + hand-off").
Builds on `--explain` (`crates/vernier-core/src/explain.rs`), which stays as is.

## Decision

Vernier's static analysis reports every possible link, including uncertain
ones (computed URLs, fuzzy name matches) and inferred ones (shared topic or
database). `--explain` only rewrites that output in plain words; it never sees
code, so it cannot tell a real call from a string that looks like one.

Two additions, both run on the user's own signed-in `claude` (Claude Code) or
`codex` (Codex CLI), so no API key is needed and nothing is billed by Vernier:

1. **`vernier --judge`**: after the blast radius is computed, one read-only
   agent run inside the repository checks the uncertain and inferred links
   against the code, then returns a structured verdict: overall risk, a
   short summary, a verdict per link, and what to test. Printed as a
   JUDGMENT section, carried in `--json` as `judgment`, drawn in `--html`.
2. **`vernier agent`**: hands the terminal to Claude Code or Codex in the
   repository with Vernier's tools attached for this session only, over MCP
   (`vernier mcp`), so the user can ask follow-ups ("what breaks if I change
   the cart API?").

## Rules (from the using-coding-agents-as-llm skill, verified 2026-11-20 against Claude Code 2.1.328 and codex-cli 0.160.0)

- Reach the subscription only by running the user's CLI. Never read
  `~/.claude` or `~/.codex`.
- Remove `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `OPENAI_API_KEY`,
  `CODEX_API_KEY` and every `CLAUDECODE*` / `CLAUDE_CODE_ENTRYPOINT*` variable
  from the child's environment.
- Prompt on stdin, never in argv. The child gets its own process group and is
  killed on timeout (`JUDGE_TIMEOUT_SECONDS = 600`).
- The judge is read-only: Claude `--tools "Read,Grep,Glob" --permission-mode
  dontAsk`; Codex `--sandbox read-only`. Its cwd is the repository root.
- Hand-off is session-only: a temp `--mcp-config` file for Claude, `-c
  mcp_servers.vernier.*` for Codex. No user config is written.
- Never `claude --bare`.

Verified in a spike: `claude -p --output-format json --json-schema S` returns
the object in `structured_output`; `codex exec --output-schema F -o out` writes
the object as JSON to `out`.

## The judgment

The JSON schema both agents get (strict: every property required,
`additionalProperties: false`, as Codex requires):

```json
{
  "risk": "low | medium | high",
  "summary": "string, at most 3 sentences",
  "links": [{"from": "svc", "to": "svc", "verdict": "confirmed | doubtful | unclear",
             "why": "string", "file": "string", "line": "integer | null"}],
  "check": ["string: one thing the reviewer should test"]
}
```

Which links are judged: each distinct `from → to` hop on a reached
service's path whose confidence is `inferred` or `uncertain`, at most 20,
in report order. Each one goes into the prompt with its evidence file and line
from the graph. Observed and static links are listed as context, not judged.

Vernier rejects a `links` entry whose `from → to` it did not ask about, so the
agent cannot invent a link.

## Choosing the agent

`--judge` takes an optional value: `claude` or `codex`. With no value it uses
`VERNIER_AGENT` if set; otherwise the first of claude, codex that is installed
and signed in. Sign-in checks: `claude auth status --json` → `loggedIn`;
`codex login status` exit 0 and stdout+stderr starting "Logged in". If
neither is ready, the error names both install hints and suggests
`--explain` with an API key. `--judge-model <NAME>` is passed through as
`--model` / `-m`. `--judge` conflicts with `--explain`.

## Disclosure

Unlike `--explain`, the judge reads source code through the user's own agent.
stderr says so before the run: `vernier: asking Claude Code (your
subscription) to judge 4 links; it reads this repository's code, read-only`.
The README states that code goes to the user's agent provider under their
plan's terms, and that consumer plans are not for regulated data.

## MCP server (`vernier mcp [PATH]`)

Newline-delimited JSON-RPC 2.0 on stdio, protocol version `2025-06-18`,
hand-written on `serde_json` (no new dependency). The analysis is loaded once
at startup. Methods: `initialize`, `notifications/initialized` (no reply),
`ping`, `tools/list`, `tools/call`. Tools:

| Tool | Arguments | Returns |
| --- | --- | --- |
| `blast_radius` | `files: [string]` or `pr: integer` or `diff: string`, or none for the working change; `depth?: integer` | the `Blast` JSON |
| `services` | none | services with role, language, root |
| `edges` | `service?: string` | edges touching that service (or all), with evidence |

Tool errors are results with `isError: true`, not JSON-RPC errors.

## Hand-off (`vernier agent [PATH] [--with claude|codex] [PROMPT]`)

Same agent choice as `--judge`. Starts the agent interactively in the
repository root with the scrubbed environment, the MCP server
`vernier mcp <root>` (the current executable's absolute path) and appended
instructions that say what Vernier is and which tools to use. Exits with the
agent's exit code. Needs a terminal.

## Out of scope

TUI integration, a GitHub Action that posts the judgment, diff text in the
prompt (the agent reads current files), caching judgments.
