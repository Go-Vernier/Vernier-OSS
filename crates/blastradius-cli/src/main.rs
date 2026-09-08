//! `blast-radius`: which services can this change reach?
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

use blastradius::RuntimeInput;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "blast-radius",
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
    },
}

fn main() {
    if let Err(err) = run() {
        eprintln!("blast-radius: {err}");
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
        } => {
            let mut analysis = blastradius::analyze(&path)?;
            if let Some(input) = runtime_input(otel, datadog, dd_env, dd_site)? {
                let config = blastradius::config::load(&analysis.root)?.runtime;
                let graph = blastradius::runtime::load(&input)?;
                blastradius::runtime::join(&mut analysis, graph, &config)?;
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
            writeln!(out, "{}", blastradius::format_repo_report(&analysis, color))?;
            Ok(())
        }
    }
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
