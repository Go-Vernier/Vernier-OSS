//! `blast-radius`: which services can this change reach?
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

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
        } => {
            let analysis = blastradius::analyze(&path)?;
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
