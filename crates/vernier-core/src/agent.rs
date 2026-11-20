//! Run the user's own signed-in coding agent (Claude Code or Codex) as a
//! subprocess, so vernier needs no API key of its own. The agent bills the
//! user's subscription, which is why every child starts from a scrubbed
//! environment: a stray API key must never silently switch billing, and a
//! nested-session variable must never make the agent think it is already
//! running inside another one. Prompts go on stdin, never argv, so they stay
//! out of `ps`.
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use thiserror::Error;

const STATUS_TIMEOUT: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(50);
/// Extra time for the readers to drain after the child is gone.
const GRACE: Duration = Duration::from_secs(2);

/// Variables that would change which account pays, or that mark a nested
/// agent session.
const SCRUB_NAMES: [&str; 4] = [
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "OPENAI_API_KEY",
    "CODEX_API_KEY",
];
const SCRUB_PREFIXES: [&str; 2] = ["CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT"];

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AgentError {
    /// No usable agent: not installed, signed out, or an unknown name.
    #[error("{0}")]
    NotReady(String),
    #[error("{0}")]
    Failed(String),
    #[error("{0}")]
    Timeout(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    ClaudeCode,
    Codex,
}

/// Preference order when no agent is named.
const ALL: [Agent; 2] = [Agent::ClaudeCode, Agent::Codex];

impl Agent {
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "claude" | "claude-code" => Some(Self::ClaudeCode),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }

    pub fn binary(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
        }
    }

    /// The name the JSON carries and `--judge=` accepts.
    pub fn slug(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
        }
    }

    pub fn install_hint(self) -> &'static str {
        match self {
            Self::ClaudeCode => {
                "install from https://claude.com/claude-code, then run: claude auth login"
            }
            Self::Codex => "npm install -g @openai/codex, then run: codex login",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    pub agent: Agent,
    pub installed: bool,
    pub signed_in: bool,
}

/// `claude auth status --json` prints `{"loggedIn": true, ...}`.
pub fn claude_signed_in(stdout: &str) -> bool {
    serde_json::from_str::<Value>(stdout)
        .ok()
        .and_then(|v| v.get("loggedIn").and_then(Value::as_bool))
        .unwrap_or(false)
}

/// `codex login status` exits 0 and prints "Logged in ...". Some versions
/// print on stderr, so callers pass both streams joined.
pub fn codex_signed_in(exit_ok: bool, output: &str) -> bool {
    exit_ok
        && output
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("logged in")
}

/// Drop the variables listed above from a parent environment.
pub fn scrub(vars: impl Iterator<Item = (String, String)>) -> Vec<(String, String)> {
    vars.filter(|(name, _)| {
        let upper = name.to_ascii_uppercase();
        !SCRUB_NAMES.contains(&upper.as_str())
            && !SCRUB_PREFIXES.iter().any(|p| upper.starts_with(p))
    })
    .collect()
}

/// Resolve a binary on `PATH` to its full path. Windows needs this: npm
/// installs agents as `.cmd` shims, which `Command::new("codex")` will not
/// find but a full path spawns fine.
pub fn which(binary: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let names: Vec<String> = if cfg!(windows) {
        ["exe", "cmd", "bat"]
            .iter()
            .map(|e| format!("{binary}.{e}"))
            .collect()
    } else {
        vec![binary.to_string()]
    };
    std::env::split_paths(&path)
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|candidate| candidate.is_file())
}

/// Ask each agent whether it is installed and signed in. A status command
/// that fails or times out counts as signed out.
pub fn detect() -> Vec<Status> {
    ALL.iter()
        .map(|&agent| {
            let Some(exe) = which(agent.binary()) else {
                return Status {
                    agent,
                    installed: false,
                    signed_in: false,
                };
            };
            let mut argv = vec![exe.to_string_lossy().into_owned()];
            argv.extend(
                match agent {
                    Agent::ClaudeCode => ["auth", "status", "--json"].as_slice(),
                    Agent::Codex => ["login", "status"].as_slice(),
                }
                .iter()
                .map(|a| (*a).to_string()),
            );
            let signed_in =
                run(&argv, "", Path::new("."), STATUS_TIMEOUT).is_ok_and(|out| match agent {
                    Agent::ClaudeCode => claude_signed_in(&out.stdout),
                    Agent::Codex => {
                        codex_signed_in(out.status_ok, &format!("{}{}", out.stdout, out.stderr))
                    }
                });
            Status {
                agent,
                installed: true,
                signed_in,
            }
        })
        .collect()
}

/// Pick the agent: an explicit name beats `VERNIER_AGENT`, which beats the
/// first one that is installed and signed in. An explicit choice that is not
/// ready is an error rather than a silent fallback, so the user never gets
/// an answer from an agent they did not ask for.
pub fn choose(
    named: Option<&str>,
    env_choice: Option<&str>,
    statuses: &[Status],
) -> Result<Agent, AgentError> {
    let wanted = named
        .or(env_choice)
        .map(str::trim)
        .filter(|n| !n.is_empty());
    if let Some(name) = wanted {
        let agent = Agent::parse(name).ok_or_else(|| {
            AgentError::NotReady(format!(
                "unknown agent `{name}`; choose one of: claude, codex"
            ))
        })?;
        let ready = statuses
            .iter()
            .any(|s| s.agent == agent && s.installed && s.signed_in);
        return if ready {
            Ok(agent)
        } else {
            Err(AgentError::NotReady(format!(
                "{} is not ready: {}",
                agent.label(),
                agent.install_hint()
            )))
        };
    }
    ALL.iter()
        .copied()
        .find(|a| {
            statuses
                .iter()
                .any(|s| s.agent == *a && s.installed && s.signed_in)
        })
        .ok_or_else(|| {
            AgentError::NotReady(format!(
                "no coding agent is ready. {}: {}. {}: {}. Or use --explain with an API key.",
                Agent::ClaudeCode.label(),
                Agent::ClaudeCode.install_hint(),
                Agent::Codex.label(),
                Agent::Codex.install_hint(),
            ))
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutput {
    pub status_ok: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Run `argv` with the current environment, scrubbed.
pub fn run(
    argv: &[String],
    stdin: &str,
    cwd: &Path,
    timeout: Duration,
) -> Result<RunOutput, AgentError> {
    run_in(argv, stdin, cwd, timeout, std::env::vars())
}

/// Like [`run`] with the parent environment passed in, so tests need not
/// touch the real one.
pub fn run_in(
    argv: &[String],
    stdin: &str,
    cwd: &Path,
    timeout: Duration,
    parent_env: impl IntoIterator<Item = (String, String)>,
) -> Result<RunOutput, AgentError> {
    let (program, rest) = argv
        .split_first()
        .ok_or_else(|| AgentError::Failed("no command to run".into()))?;
    let mut command = Command::new(program);
    command
        .args(rest)
        .current_dir(cwd)
        .env_clear()
        .envs(scrub(parent_env.into_iter()))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Own process group, so a timeout kills the agent's helper processes too.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command
        .spawn()
        .map_err(|e| AgentError::Failed(format!("could not start {program}: {e}")))?;

    // Feed stdin from its own thread: a child that never reads it must not
    // block us. A closed pipe just means it did not want the input.
    let input = stdin.as_bytes().to_vec();
    let mut pipe = child.stdin.take();
    thread::spawn(move || {
        if let Some(pipe) = pipe.as_mut() {
            let _ = pipe.write_all(&input);
        }
    });
    let out = reader(child.stdout.take());
    let err = reader(child.stderr.take());

    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() >= timeout => {
                kill_tree(&mut child);
                let _ = child.wait();
                return Err(AgentError::Timeout(timed_out(program, timeout)));
            }
            Ok(None) => thread::sleep(POLL),
            Err(e) => {
                kill_tree(&mut child);
                let _ = child.wait();
                return Err(AgentError::Failed(format!("waiting for {program}: {e}")));
            }
        }
    };
    // A descendant that outlived the child may still hold the pipes open and
    // would stall the readers; end the whole group, then bound the wait.
    kill_tree(&mut child);
    let deadline = (start + timeout).max(Instant::now()) + GRACE;
    let (Some(stdout), Some(stderr)) = (collect(&out, deadline), collect(&err, deadline)) else {
        return Err(AgentError::Timeout(timed_out(program, timeout)));
    };
    Ok(RunOutput {
        status_ok: status.success(),
        stdout,
        stderr,
    })
}

fn timed_out(program: &str, timeout: Duration) -> String {
    format!("{program} timed out after {}s", timeout.as_secs())
}

fn collect(rx: &mpsc::Receiver<String>, deadline: Instant) -> Option<String> {
    rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .ok()
}

fn reader<R: Read + Send + 'static>(stream: Option<R>) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut stream) = stream {
            let _ = stream.read_to_end(&mut bytes);
        }
        let _ = tx.send(String::from_utf8_lossy(&bytes).into_owned());
    });
    rx
}

#[cfg(unix)]
fn kill_tree(child: &mut Child) {
    let group = format!("-{}", child.id());
    let killed = Command::new("kill")
        .args(["-KILL", "--", &group])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !killed {
        let _ = child.kill();
    }
}

#[cfg(not(unix))]
fn kill_tree(child: &mut Child) {
    let _ = child.kill();
}

/// Told to the agent when vernier hands the terminal over. No double quotes
/// or backslashes, so it survives being quoted as a TOML string for Codex.
pub const HANDOFF_INSTRUCTIONS: &str = "Vernier's tools are attached to this session: blast_radius, with no arguments, gives the services your changes on this branch can reach, and it also takes files, pr or diff to name another change; services lists what Vernier found; edges lists how they connect. Use them to answer questions about what a change can affect. Read the code to verify any link marked uncertain or inferred, and cite file:line. Never call a service safe just because no path to it was found.";

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A new, empty directory under the system temp dir. Unique and freshly
/// created: a path that already exists is an error, so another process (or a
/// planted directory) is never reused.
pub fn fresh_temp_dir(prefix: &str) -> std::io::Result<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!(
        "{prefix}-{}-{nanos}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir)?;
    Ok(dir)
}

/// The MCP server entry that attaches `vernier mcp <root>` to one session.
pub fn mcp_config_json(vernier: &Path, root: &Path) -> Value {
    json!({"mcpServers": {"vernier": {
        "command": vernier.to_string_lossy(),
        "args": ["mcp", root.to_string_lossy()],
    }}})
}

/// argv for an interactive session with vernier's tools attached for this
/// session only (nothing is installed into the agent's own config). argv[0]
/// is the bare binary name; the caller swaps in the resolved path.
pub fn handoff_argv(
    agent: Agent,
    vernier: &Path,
    root: &Path,
    mcp_config: &Path,
    model: Option<&str>,
    prompt: Option<&str>,
) -> Vec<String> {
    let mut argv = vec![agent.binary().to_string()];
    match agent {
        Agent::ClaudeCode => {
            argv.extend([
                "--mcp-config".into(),
                mcp_config.to_string_lossy().into_owned(),
                "--append-system-prompt".into(),
                HANDOFF_INSTRUCTIONS.into(),
            ]);
            if let Some(m) = model {
                argv.extend(["--model".into(), m.into()]);
            }
        }
        Agent::Codex => {
            // JSON strings and arrays of strings are valid TOML values.
            let quote = |v: &Value| v.to_string();
            let server_args = json!(["mcp", root.to_string_lossy()]);
            for setting in [
                format!(
                    "mcp_servers.vernier.command={}",
                    quote(&json!(vernier.to_string_lossy()))
                ),
                format!("mcp_servers.vernier.args={}", quote(&server_args)),
                "mcp_servers.vernier.tool_timeout_sec=600".to_string(),
                format!(
                    "developer_instructions={}",
                    quote(&json!(HANDOFF_INSTRUCTIONS))
                ),
            ] {
                argv.extend(["-c".into(), setting]);
            }
            if let Some(m) = model {
                argv.extend(["-m".into(), m.into()]);
            }
        }
    }
    if let Some(p) = prompt {
        argv.push(p.into());
    }
    argv
}

/// Run the agent interactively in `root` with vernier attached, and return
/// its exit code (1 if a signal killed it). It shares our terminal and
/// process group, so job control and Ctrl-C reach it directly.
pub fn handoff(
    agent: Agent,
    root: &Path,
    model: Option<&str>,
    prompt: Option<&str>,
) -> Result<i32, AgentError> {
    refuse_dash("--model", model)?;
    refuse_dash("--prompt", prompt)?;
    let binary =
        which(agent.binary()).ok_or_else(|| AgentError::NotReady(agent.install_hint().into()))?;
    handoff_with(agent, &binary, "vernier-agent", root, model, prompt)
}

/// A value that starts with `-` would reach the agent as one of its own
/// flags (say `--dangerously-skip-permissions`), so it is refused.
pub fn refuse_dash(flag: &str, value: Option<&str>) -> Result<(), AgentError> {
    match value {
        Some(v) if v.starts_with('-') => Err(AgentError::NotReady(format!(
            "{flag} must not start with -"
        ))),
        _ => Ok(()),
    }
}

/// `handoff` with the agent binary already resolved; the temp directory is
/// named from `temp_prefix` and always removed afterwards.
fn handoff_with(
    agent: Agent,
    binary: &Path,
    temp_prefix: &str,
    root: &Path,
    model: Option<&str>,
    prompt: Option<&str>,
) -> Result<i32, AgentError> {
    let vernier = std::env::current_exe()
        .map_err(|e| AgentError::Failed(format!("can't locate this executable: {e}")))?;
    let tmp = fresh_temp_dir(temp_prefix)
        .map_err(|e| AgentError::Failed(format!("can't create a temp directory: {e}")))?;
    let result = (|| {
        let config = tmp.join("mcp.json");
        std::fs::write(&config, mcp_config_json(&vernier, root).to_string())
            .map_err(|e| AgentError::Failed(format!("can't write {}: {e}", config.display())))?;
        let argv = handoff_argv(agent, &vernier, root, &config, model, prompt);
        let status = Command::new(binary)
            .args(&argv[1..])
            .env_clear()
            .envs(scrub(std::env::vars()))
            .current_dir(root)
            .status()
            .map_err(|e| AgentError::Failed(format!("can't start {}: {e}", agent.label())))?;
        Ok(status.code().unwrap_or(1))
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn vars(names: &[&str]) -> Vec<(String, String)> {
        names
            .iter()
            .map(|n| ((*n).to_string(), "x".to_string()))
            .collect()
    }

    fn status(agent: Agent, installed: bool, signed_in: bool) -> Status {
        Status {
            agent,
            installed,
            signed_in,
        }
    }

    #[test]
    fn scrub_drops_keys_and_nested_session_vars() {
        let kept = scrub(
            vars(&[
                "ANTHROPIC_API_KEY",
                "OPENAI_API_KEY",
                "CLAUDECODE",
                "CLAUDE_CODE_ENTRYPOINT",
                "Anthropic_Api_Key",
                "PATH",
            ])
            .into_iter(),
        );
        assert_eq!(kept, vars(&["PATH"]));
    }

    #[test]
    fn sign_in_parsers() {
        assert!(claude_signed_in(r#"{"loggedIn":true}"#));
        assert!(!claude_signed_in(r#"{"loggedIn":false}"#));
        assert!(!claude_signed_in("garbage"));
        assert!(codex_signed_in(true, "Logged in using ChatGPT"));
        assert!(!codex_signed_in(false, "Logged in"));
        assert!(!codex_signed_in(true, "Not logged in"));
    }

    #[test]
    fn choose_prefers_flag_then_env_then_first_ready() {
        let statuses = [
            status(Agent::ClaudeCode, true, false),
            status(Agent::Codex, true, true),
        ];
        assert_eq!(choose(None, None, &statuses), Ok(Agent::Codex));
        let Err(AgentError::NotReady(message)) = choose(Some("claude"), None, &statuses) else {
            panic!("a signed-out agent must not be chosen");
        };
        assert!(message.contains("claude auth login"), "{message}");
        assert_eq!(choose(None, Some("codex"), &statuses), Ok(Agent::Codex));

        let none_ready = [
            status(Agent::ClaudeCode, false, false),
            status(Agent::Codex, true, false),
        ];
        let Err(AgentError::NotReady(message)) = choose(None, None, &none_ready) else {
            panic!("nothing is ready");
        };
        assert!(
            message.contains(Agent::ClaudeCode.install_hint()),
            "{message}"
        );
        assert!(message.contains(Agent::Codex.install_hint()), "{message}");
        assert!(message.contains("--explain"), "{message}");

        let Err(AgentError::NotReady(message)) = choose(Some("gemini"), None, &statuses) else {
            panic!("unknown agent");
        };
        assert!(message.contains("claude, codex"), "{message}");
    }

    #[test]
    fn handoff_argv_attaches_vernier_for_this_session_only() {
        let vernier = Path::new("/opt/my tools/vernier");
        let root = Path::new("/Users/a b/Dream YC/repo");
        let cfg = Path::new("/tmp/x y/mcp.json");

        let claude = handoff_argv(Agent::ClaudeCode, vernier, root, cfg, None, None);
        assert_eq!(claude[0], "claude");
        let i = claude.iter().position(|a| a == "--mcp-config").unwrap();
        assert_eq!(claude[i + 1], cfg.to_str().unwrap());
        let j = claude
            .iter()
            .position(|a| a == "--append-system-prompt")
            .unwrap();
        assert_eq!(claude[j + 1], HANDOFF_INSTRUCTIONS);
        for banned in ["mcp", "add", "--bare", "--strict-mcp-config"] {
            assert!(!claude.iter().any(|a| a == banned), "{banned}");
        }
        assert!(!claude.iter().any(|a| a == "--model"));
        let with = handoff_argv(
            Agent::ClaudeCode,
            vernier,
            root,
            cfg,
            Some("opus"),
            Some("what breaks?"),
        );
        let m = with.iter().position(|a| a == "--model").unwrap();
        assert_eq!(with[m + 1], "opus");
        assert_eq!(with.last().unwrap(), "what breaks?");

        let codex = handoff_argv(
            Agent::Codex,
            vernier,
            root,
            cfg,
            Some("gpt-x"),
            Some("hello"),
        );
        assert_eq!(codex[0], "codex");
        let mut values = std::collections::HashMap::new();
        for (k, a) in codex.iter().enumerate() {
            if a == "-c" {
                let (key, value) = codex[k + 1].split_once('=').unwrap();
                let parsed: toml::Value = toml::from_str(&format!("x = {value}")).unwrap();
                values.insert(key.to_string(), parsed["x"].clone());
            }
        }
        assert_eq!(
            values["mcp_servers.vernier.command"].as_str().unwrap(),
            vernier.to_str().unwrap()
        );
        let args: Vec<&str> = values["mcp_servers.vernier.args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(args, ["mcp", root.to_str().unwrap()]);
        assert_eq!(
            values["mcp_servers.vernier.tool_timeout_sec"].as_integer(),
            Some(600)
        );
        assert_eq!(
            values["developer_instructions"].as_str().unwrap(),
            HANDOFF_INSTRUCTIONS
        );
        let m = codex.iter().position(|a| a == "-m").unwrap();
        assert_eq!(codex[m + 1], "gpt-x");
        assert_eq!(codex.last().unwrap(), "hello");
    }

    #[test]
    fn handoff_instructions_survive_toml_quoting() {
        assert!(!HANDOFF_INSTRUCTIONS.contains('"'));
        assert!(!HANDOFF_INSTRUCTIONS.contains('\\'));
        for tool in ["blast_radius", "services", "edges"] {
            assert!(HANDOFF_INSTRUCTIONS.contains(tool));
        }
    }

    #[test]
    fn mcp_config_points_at_this_binary() {
        let json = mcp_config_json(Path::new("/abs/vernier"), Path::new("/abs/root"));
        assert_eq!(
            json,
            serde_json::json!({"mcpServers":{"vernier":{
                "command":"/abs/vernier","args":["mcp","/abs/root"]}}})
        );
    }

    #[test]
    fn fresh_temp_dir_is_unique_and_created() {
        let a = fresh_temp_dir("vernier-agent-test").unwrap();
        let b = fresh_temp_dir("vernier-agent-test").unwrap();
        assert_ne!(a, b);
        assert!(a.is_dir() && b.is_dir());
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    #[test]
    fn dash_leading_values_are_refused() {
        assert!(refuse_dash("--prompt", Some("what breaks?")).is_ok());
        assert!(refuse_dash("--prompt", None).is_ok());
        let Err(AgentError::NotReady(m)) = refuse_dash("--model", Some("-x")) else {
            panic!("should refuse");
        };
        assert_eq!(m, "--model must not start with -");
        let r = handoff(Agent::Codex, Path::new("."), None, Some("--yolo"));
        assert!(matches!(r, Err(AgentError::NotReady(m)) if m.contains("--prompt")));
    }

    #[cfg(unix)]
    fn leftover_dirs(prefix: &str) -> Vec<PathBuf> {
        std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with(prefix))
            })
            .collect()
    }

    #[cfg(unix)]
    #[test]
    fn handoff_returns_the_exit_code_and_cleans_up() {
        use std::os::unix::fs::PermissionsExt;
        let dir = test_dir("handoff");
        let script = dir.join("fake-agent");
        std::fs::write(&script, "#!/bin/sh\nexit 3\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let prefix = "vernier-handoff-ok";
        let code = handoff_with(Agent::ClaudeCode, &script, prefix, &dir, None, None);
        assert_eq!(code, Ok(3));
        assert!(leftover_dirs(prefix).is_empty());

        let plain = dir.join("not-executable");
        std::fs::write(&plain, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o644)).unwrap();
        let prefix = "vernier-handoff-err";
        let r = handoff_with(Agent::ClaudeCode, &plain, prefix, &dir, None, None);
        assert!(matches!(r, Err(AgentError::Failed(_))), "{r:?}");
        assert!(leftover_dirs(prefix).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    fn test_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("vernier-agent-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(unix)]
    fn process_alive(pid: &str) -> bool {
        Command::new("kill")
            .args(["-0", pid])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    #[cfg(unix)]
    #[test]
    fn run_kills_a_hung_child() {
        let dir = test_dir("hung");
        let script = format!("sleep 30 & echo $! > '{}'; wait", dir.join("pid").display());
        let argv = ["sh", "-c", &script].map(String::from);
        let start = Instant::now();
        let result = run(&argv, "", Path::new("."), Duration::from_secs(1));
        assert!(matches!(result, Err(AgentError::Timeout(_))), "{result:?}");
        assert!(start.elapsed() < Duration::from_secs(5));
        // The backgrounded grandchild must die with the group, not just the shell.
        let pid = std::fs::read_to_string(dir.join("pid"))
            .unwrap()
            .trim()
            .to_string();
        let deadline = Instant::now() + Duration::from_secs(2);
        while process_alive(&pid) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
        let alive = process_alive(&pid);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(!alive, "grandchild {pid} survived the timeout");
    }

    #[cfg(unix)]
    #[test]
    fn run_returns_when_a_grandchild_holds_the_pipe() {
        let argv = ["sh", "-c", "(sleep 30 &) ; echo done"].map(String::from);
        let start = Instant::now();
        let out = run(&argv, "", Path::new("."), Duration::from_secs(10)).unwrap();
        assert_eq!(out.stdout, "done\n");
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[cfg(unix)]
    #[test]
    fn run_feeds_stdin_and_hides_keys() {
        let argv = ["sh", "-c", r#"cat; echo "[${OPENAI_API_KEY:-none}]""#].map(String::from);
        let parent = [
            ("OPENAI_API_KEY".to_string(), "sk-x".to_string()),
            (
                "PATH".to_string(),
                std::env::var("PATH").unwrap_or_default(),
            ),
        ];
        let out = run_in(&argv, "hi", Path::new("."), Duration::from_secs(10), parent).unwrap();
        assert!(out.status_ok);
        assert_eq!(out.stdout, "hi[none]\n");
    }
}
