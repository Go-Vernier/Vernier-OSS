//! `vernier`: which services can this change reach?
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

use blastradius::{Change, RuntimeInput, blast, git, history, html};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "vernier",
    version,
    about = "Which services can this change reach?"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Analyse a repository: its services, and what a change can reach
    Analyze {
        /// Repository root
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Print the graph as JSON instead of the report
        #[arg(long)]
        json: bool,
        /// Disable colours
        #[arg(long = "no-color")]
        no_color: bool,
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
        /// Blast radius of the last N pull requests, as a CHANGE HISTORY section
        #[arg(long, value_name = "N")]
        history: Option<usize>,
        /// How many hops the walk follows from a changed service
        #[arg(long, value_name = "N", default_value_t = blast::DEFAULT_DEPTH)]
        depth: usize,
        /// Write the self-contained HTML report to this file
        #[arg(long, value_name = "PATH")]
        html: Option<PathBuf>,
    },
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
        Cmd::Analyze {
            path,
            json,
            no_color,
            otel,
            datadog,
            dd_env,
            dd_site,
            pr,
            diff,
            files,
            history,
            depth,
            html,
        } => {
            let mut analysis = blastradius::analyze(&path)?;
            if let Some(input) = runtime_input(otel, datadog, dd_env, dd_site)? {
                let config = blastradius::config::load(&analysis.root)?.runtime;
                let graph = blastradius::runtime::load(&input)?;
                blastradius::runtime::join(&mut analysis, graph, &config)?;
            }
            if let Some(change) = change_input(&analysis.root, pr, diff, &files)? {
                analysis.blast = Some(blast::of_change(&analysis.graph, change, depth));
            }
            if let Some(n) = history {
                analysis.history = Some(history::run(&analysis, n, depth)?);
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
            let color = !no_color
                && std::io::stdout().is_terminal()
                && std::env::var_os("NO_COLOR").is_none();
            writeln!(out, "{}", blastradius::format_report(&analysis, color))?;
            Ok(())
        }
    }
}

/// The change the flags describe, if any. `--pr` and `--diff` read git;
/// `--files` reads nothing.
fn change_input(
    root: &std::path::Path,
    pr: Option<u64>,
    diff: Option<String>,
    files: &[String],
) -> anyhow::Result<Option<Change>> {
    if let Some(number) = pr {
        return Ok(Some(git::pull_request(root, number)?));
    }
    if let Some(range) = diff {
        return Ok(Some(git::diff(root, &range)?));
    }
    if !files.is_empty() {
        return Ok(Some(Change::from_files(files)));
    }
    Ok(None)
}

/// Which runtime source the flags ask for, if any. A bare `--datadog` means a
/// live call, which needs `--dd-env`.
fn runtime_input(
    otel: Option<String>,
    datadog: Option<String>,
    dd_env: Option<String>,
    dd_site: String,
) -> anyhow::Result<Option<RuntimeInput>> {
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
