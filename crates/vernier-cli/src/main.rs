//! `vernier`: which services can this change reach?
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use vernier::{Change, RuntimeInput, agent, blast, explain, git, history, html, judge};

const EXAMPLES: &str = "\
Examples:
  vernier                          what your changes on this branch can reach
  vernier --pr 481                 what pull request #481 can reach
  vernier --files cart/server.js   what a change to these files can reach
  vernier --explain                add a plain-English summary from your own LLM key
  vernier --judge                  have your Claude Code or Codex check the links against the code
  vernier --full                   every service, edge and finding
  vernier agent                    ask your Claude Code or Codex follow-ups, with Vernier's tools
  vernier tui                      explore it interactively";

#[derive(Parser)]
#[command(
    name = "vernier",
    version,
    about = "Which services can this change reach?",
    long_about = "Which services can this change reach?\n\nRun it inside a repository: with no arguments it walks your changes on this branch (commits since the default branch, uncommitted edits and new files) and prints their blast radius.",
    after_help = EXAMPLES,
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
    #[command(flatten)]
    analyze: AnalyzeArgs,
}

#[derive(Subcommand)]
enum Cmd {
    /// The same as `vernier` with no command: the blast radius of your changes, or of the change you name
    #[command(after_help = EXAMPLES)]
    Analyze(AnalyzeArgs),
    /// Explore the services and edges, and walk a change, interactively
    Tui {
        /// Repository root
        #[arg(default_value = ".")]
        path: PathBuf,
        #[command(flatten)]
        runtime: RuntimeArgs,
        #[command(flatten)]
        change: ChangeArgs,
        /// How many recent pull requests the Changes tab lists
        #[arg(long, value_name = "N", default_value_t = 50)]
        history: usize,
        /// Also read test, fixture and example directories
        #[arg(long = "include-tests")]
        include_tests: bool,
    },
    /// Open Claude Code or Codex here with Vernier's tools attached
    Agent {
        /// Repository root
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Which agent: claude or codex [env: `VERNIER_AGENT`]
        #[arg(long, value_name = "AGENT")]
        with: Option<String>,
        /// Model to ask the agent to use
        #[arg(long, value_name = "NAME")]
        model: Option<String>,
        /// First message to send
        #[arg(long, value_name = "TEXT")]
        prompt: Option<String>,
    },
    /// Serve the blast radius to a coding agent over MCP on stdio
    #[command(hide = true)]
    Mcp {
        /// Repository root
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Also read test, fixture and example directories
        #[arg(long = "include-tests")]
        include_tests: bool,
    },
}

#[derive(Args)]
struct AnalyzeArgs {
    /// Repository root
    #[arg(default_value = ".")]
    path: PathBuf,
    #[command(flatten)]
    change: ChangeArgs,
    #[command(flatten)]
    explain: ExplainArgs,
    #[command(flatten)]
    judge: JudgeArgs,
    #[command(flatten)]
    runtime: RuntimeArgs,
    #[command(flatten)]
    output: OutputArgs,
}

/// Command-line switches, so each is a bool.
#[derive(Args)]
#[command(next_help_heading = "Output")]
#[allow(clippy::struct_excessive_bools)]
struct OutputArgs {
    /// Every service, edge and finding, instead of your changes
    #[arg(long, conflicts_with_all = ["pr", "diff", "files"])]
    full: bool,
    /// Print the graph as JSON instead of the report
    #[arg(long)]
    json: bool,
    /// Write the self-contained HTML report to this file
    #[arg(long, value_name = "PATH")]
    html: Option<PathBuf>,
    /// Blast radius of the last N pull requests, as a CHANGE HISTORY section
    #[arg(long, value_name = "N")]
    history: Option<usize>,
    /// Also read test, fixture and example directories
    #[arg(long = "include-tests")]
    include_tests: bool,
    /// Disable colours
    #[arg(long = "no-color")]
    no_color: bool,
}

/// `--explain` and the LLM it asks. Keys are read from the environment.
#[derive(Args)]
#[command(next_help_heading = "Explain with your own LLM key")]
struct ExplainArgs {
    /// Explain the blast radius in plain English with your own LLM key
    /// (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY` or `GEMINI_API_KEY`)
    #[arg(long)]
    explain: bool,
    /// LLM provider: anthropic, openai, gemini, ollama or openai-compatible
    /// [env: `VERNIER_LLM`]
    #[arg(long, value_name = "PROVIDER", requires = "explain")]
    llm: Option<String>,
    /// Model name, instead of the provider's default [env: `VERNIER_LLM_MODEL`]
    #[arg(long, value_name = "NAME", requires = "explain")]
    model: Option<String>,
}

/// `--judge` and the coding agent it asks. It is the user's own signed-in
/// Claude Code or Codex, so no key is read.
#[derive(Args)]
#[command(next_help_heading = "Judge with Claude Code or Codex")]
struct JudgeArgs {
    /// Have your signed-in Claude Code or Codex read the code and judge the
    /// uncertain links; `--judge=codex` picks one [env: `VERNIER_AGENT`]
    #[arg(
        long,
        value_name = "AGENT",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "",
        conflicts_with = "explain"
    )]
    judge: Option<String>,
    /// Model name for the agent, instead of its default
    #[arg(long = "judge-model", value_name = "NAME", requires = "judge")]
    judge_model: Option<String>,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("vernier: {err}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        None => analyze(cli.analyze),
        Some(Cmd::Analyze(args)) => analyze(args),
        Some(Cmd::Agent {
            path,
            with,
            model,
            prompt,
        }) => {
            agent::refuse_dash("--model", model.as_deref())?;
            agent::refuse_dash("--prompt", prompt.as_deref())?;
            if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
                anyhow::bail!("agent needs a terminal; for a one-shot verdict use vernier --judge");
            }
            let agent = agent::choose(
                with.as_deref(),
                std::env::var("VERNIER_AGENT").ok().as_deref(),
                &agent::detect(),
            )?;
            let root = path
                .canonicalize()
                .map_err(|e| anyhow::anyhow!("can't read {}: {e}", path.display()))?;
            anyhow::ensure!(root.is_dir(), "{} is not a directory", root.display());
            let code = agent::handoff(agent, &root, model.as_deref(), prompt.as_deref())?;
            std::process::exit(code);
        }
        Some(Cmd::Mcp {
            path,
            include_tests,
        }) => {
            let analysis = vernier::analyze_with(&path, vernier::Options { include_tests })?;
            // Stdout is the protocol; the progress line goes to stderr.
            eprintln!("vernier: serving {} over MCP", analysis.root.display());
            vernier::mcp::Server::new(analysis)
                .serve(std::io::stdin().lock(), std::io::stdout().lock())?;
            Ok(())
        }
        Some(Cmd::Tui {
            path,
            runtime,
            change,
            history,
            include_tests,
        }) => {
            if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
                anyhow::bail!("tui needs a terminal; use vernier analyze for a report");
            }
            eprintln!("vernier: analysing {}", path.display());
            let analysis = load(&path, runtime, include_tests)?;
            let depth = change.depth;
            let change = change_input(&analysis.root, change)?;
            vernier_tui::run(
                analysis,
                vernier_tui::Options {
                    depth,
                    history,
                    color: std::env::var_os("NO_COLOR").is_none(),
                    change,
                },
            )
        }
    }
}

/// The report. With no change named, your changes on this branch; with
/// none of those, a short summary. `--full` is the whole inventory.
fn analyze(args: AnalyzeArgs) -> anyhow::Result<()> {
    let AnalyzeArgs {
        path,
        change,
        explain: explain_args,
        judge: judge_args,
        runtime,
        output:
            OutputArgs {
                full,
                json,
                html,
                history,
                include_tests,
                no_color,
            },
    } = args;
    // A missing key fails before the analysis, not after it.
    let llm = if explain_args.explain {
        Some(explain::resolve(
            explain_args.llm.as_deref(),
            explain_args.model.as_deref(),
            &|name| std::env::var(name).ok(),
        )?)
    } else {
        None
    };
    agent::refuse_dash("--judge-model", judge_args.judge_model.as_deref())?;
    // Likewise a missing or signed-out agent.
    let agent = match &judge_args.judge {
        Some(named) => Some(agent::choose(
            Some(named.as_str()),
            std::env::var("VERNIER_AGENT").ok().as_deref(),
            &agent::detect(),
        )?),
        None => None,
    };
    let mut analysis = load(&path, runtime, include_tests)?;
    let depth = change.depth;
    let named = change_input(&analysis.root, change)?;
    let change = match named {
        Some(change) => Some(change),
        None if full => None,
        None => git::working_change(&analysis.root)?,
    };
    if let Some(change) = change {
        analysis.blast = Some(blast::of_change(&analysis.graph, change, depth));
    }
    if let Some(n) = history {
        analysis.history = Some(history::run(&analysis, n, depth)?);
    }
    if let Some(config) = &llm {
        match &analysis.blast {
            Some(b) => {
                eprintln!(
                    "vernier: asking {} ({}) to explain; it is sent service names and changed file paths, no source code",
                    config.provider.as_str(),
                    config.model
                );
                analysis.explanation = Some(explain::explain(config, &analysis.repository, b)?);
            }
            None => eprintln!(
                "vernier: nothing to explain: no change to walk; name one with --pr, --diff or --files"
            ),
        }
    }
    if let Some(agent) = agent {
        run_judge(&mut analysis, agent, judge_args.judge_model.as_deref())?;
    }
    if let Some(out) = &html {
        std::fs::write(out, html::render(&analysis))
            .map_err(|e| anyhow::anyhow!("cannot write {}: {e}", out.display()))?;
        eprintln!("vernier: wrote {}", out.display());
    }
    let mut out = std::io::stdout().lock();
    if json {
        writeln!(
            out,
            "{}",
            serde_json::to_string_pretty(&analysis.to_json())?
        )?;
        return Ok(());
    }
    let color =
        !no_color && std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let report = if analysis.blast.is_some() || full {
        vernier::format_report(&analysis, color)
    } else {
        vernier::format_summary_report(&analysis, color, git::is_repository(&analysis.root))
    };
    writeln!(out, "{report}")?;
    if let Some(e) = &analysis.explanation {
        writeln!(out, "\n{}", vernier::format_explanation(e, color))?;
    }
    if let Some(j) = &analysis.judgment {
        writeln!(out, "\n{}", vernier::format_judgment(j, color))?;
    }
    Ok(())
}

/// Asks the agent about the inferred and uncertain links, if there are any.
fn run_judge(
    analysis: &mut vernier::Analysis,
    agent: agent::Agent,
    model: Option<&str>,
) -> anyhow::Result<()> {
    let Some(blast) = &analysis.blast else {
        eprintln!(
            "vernier: nothing to judge: no change to walk; name one with --pr, --diff or --files"
        );
        return Ok(());
    };
    let links = judge::links_to_judge(blast, &analysis.graph);
    if links.is_empty() {
        eprintln!(
            "vernier: nothing uncertain to judge: every link in this blast radius is observed or static"
        );
        return Ok(());
    }
    eprintln!(
        "vernier: asking {} (your subscription) to judge {} link{}; it reads this repository's code, read-only",
        agent.label(),
        links.len(),
        if links.len() == 1 { "" } else { "s" }
    );
    analysis.judgment = Some(judge::judge(agent, model, analysis, &links)?);
    Ok(())
}

/// Where runtime data comes from. The same on every command that reads it.
#[derive(Args)]
#[command(next_help_heading = "Production traces")]
struct RuntimeArgs {
    /// Join with OpenTelemetry runtime data: a Prometheus scrape holding the
    /// servicegraph metric, or an OTLP JSON span export (path or URL)
    #[arg(long, value_name = "PATH|URL", conflicts_with = "datadog")]
    otel: Option<String>,
    /// Join with Datadog's service dependency map: a saved response (path or
    /// URL), or bare to call the API with --dd-env and `DD_API_KEY`/`DD_APP_KEY`
    #[arg(long, value_name = "PATH|URL", num_args = 0..=1, default_missing_value = "")]
    datadog: Option<String>,
    /// Datadog environment for a live call
    #[arg(long = "dd-env", value_name = "ENV", requires = "datadog")]
    dd_env: Option<String>,
    /// Datadog site for a live call
    #[arg(long = "dd-site", value_name = "SITE", default_value = "datadoghq.com")]
    dd_site: String,
}

/// Which change to walk, and how far. With none named, `vernier` walks
/// your changes on this branch.
#[derive(Args)]
#[command(next_help_heading = "Which change (default: your changes on this branch)")]
struct ChangeArgs {
    /// Blast radius of one pull request, found in the local git history or
    /// in a fetched ref
    #[arg(long, value_name = "NUMBER", conflicts_with_all = ["diff", "files"])]
    pr: Option<u64>,
    /// Blast radius of a git diff range, as `git diff --name-only` takes it
    #[arg(long, value_name = "RANGE", conflicts_with = "files")]
    diff: Option<String>,
    /// Blast radius of these files, relative to the repository root
    #[arg(long, value_name = "PATH", num_args = 1..)]
    files: Vec<String>,
    /// How many hops the walk follows from a changed service
    #[arg(long, value_name = "N", default_value_t = blast::DEFAULT_DEPTH)]
    depth: usize,
}

/// Stages 1 to 3: the graph, joined with runtime data when a source is given.
fn load(
    path: &std::path::Path,
    runtime: RuntimeArgs,
    include_tests: bool,
) -> anyhow::Result<vernier::Analysis> {
    let mut analysis = vernier::analyze_with(path, vernier::Options { include_tests })?;
    if let Some(input) = runtime_input(runtime)? {
        let config = vernier::config::load(&analysis.root)?.runtime;
        let graph = vernier::runtime::load(&input)?;
        vernier::runtime::join(&mut analysis, graph, &config)?;
    }
    Ok(analysis)
}

/// The change the flags describe, if any. `--pr` and `--diff` read git;
/// `--files` reads nothing.
fn change_input(root: &std::path::Path, args: ChangeArgs) -> anyhow::Result<Option<Change>> {
    let ChangeArgs {
        pr, diff, files, ..
    } = args;
    if let Some(number) = pr {
        return Ok(Some(git::pull_request(root, number)?));
    }
    if let Some(range) = diff {
        return Ok(Some(git::diff(root, &range)?));
    }
    if !files.is_empty() {
        return Ok(Some(Change::from_files(&files)));
    }
    Ok(None)
}

/// Which runtime source the flags ask for, if any. A bare `--datadog` means a
/// live call, which needs `--dd-env`.
fn runtime_input(args: RuntimeArgs) -> anyhow::Result<Option<RuntimeInput>> {
    let RuntimeArgs {
        otel,
        datadog,
        dd_env,
        dd_site,
    } = args;
    if let Some(source) = otel {
        return Ok(Some(RuntimeInput::Otel(source)));
    }
    match datadog {
        None => Ok(None),
        Some(source) if !source.is_empty() => Ok(Some(RuntimeInput::DatadogFile(source))),
        Some(_) => {
            let env = dd_env.ok_or_else(|| {
                anyhow::anyhow!(
                    "--datadog without a file calls the Datadog API and needs --dd-env <ENV>"
                )
            })?;
            Ok(Some(RuntimeInput::DatadogLive { site: dd_site, env }))
        }
    }
}
