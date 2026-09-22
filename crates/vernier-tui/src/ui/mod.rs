//! Drawing. Every function reads `&App`; the only thing written back is a
//! list's scroll offset, through a `Cell`.
use std::cell::Cell;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Row, Table, Tabs, Wrap};
use vernier::ServiceRole;

use crate::app::{App, Tab};

mod blast;
mod changes;
mod charts;
mod ego;
mod overview;
mod services;

pub fn draw(frame: &mut Frame, app: &App) {
    let [header, tabs, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    frame.render_widget(Paragraph::new(header_line(app)), header);
    frame.render_widget(tab_bar(app), tabs);
    match app.tab {
        Tab::Overview => overview::draw(frame, body, app),
        Tab::Services => services::draw(frame, body, app),
        Tab::Changes => changes::draw(frame, body, app),
        Tab::Blast => blast::draw(frame, body, app),
    }
    frame.render_widget(Paragraph::new(footer_line(app)), footer);
    if app.help {
        help(frame, app);
    }
}

/// `robot-shop · 11 services · static only · depth 3 · 20 edges · 2 infrastructure`
fn header_line(app: &App) -> Line<'static> {
    let t = app.theme;
    let code = app
        .services
        .iter()
        .filter(|s| s.role == ServiceRole::Code)
        .count();
    let infra = app.services.len() - code;
    let (_, edges) = app.analysis.graph.size();
    let dot = || Span::styled(" · ", t.dim());
    // Most important first: a narrow terminal cuts from the end.
    let mut spans = vec![
        Span::styled(app.analysis.repository.clone(), t.bold()),
        dot(),
        Span::raw(plural(code, "service", "services")),
        dot(),
        match vernier::runtime_words(&app.analysis) {
            None => Span::styled("static only", t.dim()),
            Some((words, true)) => Span::styled(words, t.warn()),
            Some((words, false)) => Span::raw(words),
        },
        dot(),
        Span::raw(format!("depth {}", app.depth)),
        dot(),
        Span::raw(plural(edges, "edge", "edges")),
    ];
    if infra > 0 {
        spans.push(dot());
        spans.push(Span::raw(format!("{infra} infrastructure")));
    }
    Line::from(spans)
}

fn tab_bar(app: &App) -> Tabs<'static> {
    let titles: Vec<String> = Tab::ALL
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{} {}", i + 1, t.title()))
        .collect();
    Tabs::new(titles)
        .select(Tab::ALL.iter().position(|t| *t == app.tab))
        .style(app.theme.dim())
        .highlight_style(app.theme.accent())
        .divider(" ")
}

fn footer_line(app: &App) -> Line<'static> {
    let t = app.theme;
    if let Some(prompt) = &app.prompt {
        return Line::from(vec![
            Span::styled(format!("{}: ", prompt.kind.label()), t.accent()),
            Span::raw(format!("{}_", prompt.text)),
            Span::styled("   Enter walk · Esc cancel", t.dim()),
        ]);
    }
    if app.filtering {
        let text = if app.tab == Tab::Changes {
            &app.change_filter
        } else {
            &app.service_filter
        };
        return Line::from(vec![
            Span::styled("/", t.accent()),
            Span::raw(format!("{text}_")),
            Span::styled("   Enter keep · Esc clear", t.dim()),
        ]);
    }
    if let Some(status) = &app.status {
        return Line::from(Span::styled(status.clone(), t.warn()));
    }
    let hints = match app.tab {
        Tab::Overview => "j/k scroll · 1-4 tabs · ? help · q quit",
        Tab::Services => "j/k move · h/l pane · / filter · Enter blast radius · ? help · q quit",
        Tab::Changes => {
            "j/k move · / filter · Enter blast radius · d diff · f files · ? help · q quit"
        }
        Tab::Blast => "j/k move · +/- depth · Esc back · ? help · q quit",
    };
    Line::from(Span::styled(hints, t.dim()))
}

const KEYS: [(&str, &str); 11] = [
    ("1-4  Tab  Shift-Tab", "switch tab"),
    ("j/k  arrows", "move, scroll"),
    ("PgUp/PgDn  g/G", "page, top, bottom"),
    ("h/l  left/right", "move between panes"),
    ("/", "filter the list; Esc clears it"),
    ("Enter", "blast radius of the service or change"),
    ("+ / -", "depth, on the Blast tab"),
    ("d  f", "diff range, files, on the Changes tab"),
    ("Esc", "back to the tab that opened the blast"),
    ("?", "this help"),
    ("q  Ctrl-C", "quit"),
];

fn help(frame: &mut Frame, app: &App) {
    let area = centered(frame.area(), 64, u16::try_from(KEYS.len()).unwrap_or(0) + 4);
    frame.render_widget(Clear, area);
    let rows = KEYS
        .iter()
        .map(|(k, v)| Row::new(vec![Span::styled(*k, app.theme.bold()), Span::raw(*v)]));
    let table = Table::new(rows, [Constraint::Length(22), Constraint::Min(0)]).block(
        Block::bordered()
            .title(" Keys ")
            .title_bottom(Line::from(" any key closes ").right_aligned())
            .border_style(app.theme.accent()),
    );
    frame.render_widget(table, area.inner(ratatui::layout::Margin::new(0, 0)));
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

/// A bordered block whose border shows whether it has the keys.
pub(crate) fn pane(title: String, focused: bool, app: &App) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(if focused {
            app.theme.accent()
        } else {
            app.theme.dim()
        })
}

/// The first visible row that keeps `selected` in a window of `visible`
/// rows, starting from the last offset so the list does not jump.
pub(crate) fn offset(selected: usize, last: &Cell<usize>, visible: usize) -> usize {
    let mut off = last.get();
    if visible > 0 {
        if selected < off {
            off = selected;
        } else if selected >= off + visible {
            off = selected + 1 - visible;
        }
    }
    last.set(off);
    off
}

/// How a selected row looks: reversed when its pane has the keys, bold
/// otherwise.
pub(crate) fn highlight(focused: bool, app: &App) -> Style {
    if focused {
        app.theme.selected()
    } else {
        app.theme.bold()
    }
}

pub(crate) fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Rows `lines` take when word-wrapped to `width`, as a wrapped
/// `Paragraph` draws them.
pub(crate) fn rows(lines: &[Line<'static>], width: u16) -> u16 {
    let count = Paragraph::new(lines.to_vec())
        .wrap(Wrap { trim: false })
        .line_count(width.max(1));
    u16::try_from(count).unwrap_or(u16::MAX)
}

/// Width for a column that holds `values`, never narrower than its header.
pub(crate) fn width<'a>(header: &str, values: impl Iterator<Item = &'a str>) -> u16 {
    let w = values
        .map(|v| v.chars().count())
        .max()
        .unwrap_or(0)
        .max(header.chars().count());
    u16::try_from(w).unwrap_or(u16::MAX)
}
