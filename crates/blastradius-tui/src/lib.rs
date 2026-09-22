//! `vernier tui`: the graph the reports read, kept in memory so the reader
//! can ask many questions of it. Which service is this, who calls it, what
//! evidence says so, and what would a change here reach?
//!
//! The TUI computes nothing the engine does not, and words its results the
//! way the terminal report does.
pub mod app;
pub mod theme;
pub mod tree;
pub mod ui;

use blastradius::Analysis;
use ratatui::crossterm::event::{self, Event};

pub use app::{Action, App, Options};

/// Reads the history, takes over the terminal until the reader quits, and
/// gives it back, also on error or panic.
pub fn run(analysis: Analysis, options: Options) -> anyhow::Result<()> {
    let mut app = App::new(analysis, options);
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn event_loop(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> anyhow::Result<()> {
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;
        if let Event::Key(key) = event::read()? {
            if app.update(key) == Action::Quit {
                return Ok(());
            }
        }
    }
}
