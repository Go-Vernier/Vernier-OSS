//! `vernier`: which services can this change reach?
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use vernier::{Change, RuntimeInput, blast, explain, git, history, html};

const EXAMPLES: &str = "\
Examples:
  vernier                          what your changes on this branch can reach
  vernier --pr 481                 what pull request #481 can reach
  vernier --files cart/server.js   what a change to these files can reach
  vernier --explain                add a plain-English summary from your own LLM key
  vernier --full                   every service, edge and finding
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
