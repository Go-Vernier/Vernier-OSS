//! Tab 3: the recent pull requests and the numbers across them.
use blastradius::history::{History, Unit};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState, Wrap};

use super::charts;
use crate::app::App;

/// Cells the REACHED bar takes at its longest.
const REACH_BAR: usize = 12;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let t = app.theme;
    let recent = match &app.recent {
        Ok(recent) => recent,
        Err(e) => {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled(e.clone(), t.warn()),
                    Line::raw(""),
                    Line::styled("f walks a list of files without the history.", t.dim()),
                ])
                .wrap(Wrap { trim: false })
                .block(super::pane(" Changes ".into(), true, app)),
                area,
            );
            return;
        }
    };
    let h = &recent.history;
    let summary = summary(h, app);
    let [top, list] = Layout::vertical([
        Constraint::Length(u16::try_from(summary.len()).unwrap_or(u16::MAX)),
        Constraint::Min(3),
    ])
    .areas(area);
    frame.render_widget(Paragraph::new(summary).wrap(Wrap { trim: false }), top);

    let shown = app.filtered_changes();
    let title = if app.change_filter.is_empty() {
        format!(" {} ", super::plural(shown.len(), "change", "changes"))
    } else {
        format!(
            " {} of {}  /{} ",
            shown.len(),
            h.entries.len(),
            app.change_filter
        )
    };
    let most = h.entries.iter().map(|e| e.reached).max().unwrap_or(0);
    let rows: Vec<Row> = shown
        .iter()
        .map(|&i| {
            let e = &h.entries[i];
            Row::new(vec![
                Cell::from(e.reference.clone()),
                Cell::from(Span::styled(e.date.clone(), t.dim())),
                Cell::from(e.files.to_string()),
                Cell::from(e.changed.to_string()),
                Cell::from(Line::from(vec![
                    Span::styled(
                        format!("{:>3} ", e.reached),
                        if e.reached > 10 { t.warn() } else { t.bold() },
                    ),
                    Span::styled(charts::bar(e.reached, most, REACH_BAR), t.changed()),
                ])),
                Cell::from(e.title.clone()),
            ])
        })
        .collect();
    let ref_w = super::width("REF", h.entries.iter().map(|e| e.reference.as_str()));
    let table = Table::new(
        rows,
        [
            Constraint::Length(ref_w),
            Constraint::Length(10),
            Constraint::Length(5),
            Constraint::Length(7),
            Constraint::Length(u16::try_from(REACH_BAR + 4).unwrap_or(u16::MAX)),
            Constraint::Min(10),
        ],
    )
    .header(Row::new(vec!["REF", "DATE", "FILES", "CHANGED", "REACHED", "TITLE"]).style(t.dim()))
    .row_highlight_style(super::highlight(true, app))
    .block(super::pane(title, true, app));
    let visible = usize::from(list.height.saturating_sub(3));
    let off = super::offset(app.change_sel, &app.offsets.changes, visible);
    let mut state = TableState::new()
        .with_offset(off)
        .with_selected((!shown.is_empty()).then_some(app.change_sel));
    frame.render_stateful_widget(table, list, &mut state);
}

/// The CHANGE HISTORY numbers, in the terminal report's words.
fn summary(h: &History, app: &App) -> Vec<Line<'static>> {
    let t = app.theme;
    let scope = if h.found < h.requested {
        format!(
            "(last {} {}, {} asked for, depth {})",
            h.found,
            h.unit.noun(h.found),
            h.requested,
            h.depth
        )
    } else {
        format!(
            "(last {} {}, depth {})",
            h.found,
            h.unit.noun(h.found),
            h.depth
        )
    };
    let mut lines = vec![Line::from(vec![
        Span::styled("CHANGE HISTORY  ", t.bold()),
        Span::styled(scope, t.dim()),
    ])];
    if h.unit == Unit::Commits {
        lines.push(Line::styled(
            "No pull request markers in the history; each first-parent commit counts as one change.",
            t.warn(),
        ));
    }
    if h.found == 0 {
        lines.push(Line::styled("No commits found.", t.warn()));
        return lines;
    }
    let dot = || Span::styled(" · ", t.dim());
    let mut numbers = vec![
        Span::styled("Average ", t.dim()),
        Span::raw(format!(
            "{:.1} {}",
            h.average,
            if (h.average - 1.0).abs() < f64::EPSILON {
                "service"
            } else {
                "services"
            }
        )),
        dot(),
        Span::styled("Median ", t.dim()),
        Span::raw(number(h.median)),
    ];
    if let Some(largest) = &h.largest {
        let label = match h.unit {
            Unit::PullRequests => format!("PR {}", largest.reference),
            Unit::Commits => format!("commit {}", largest.reference),
        };
        numbers.push(dot());
        numbers.push(Span::styled("Largest ", t.dim()));
        numbers.push(Span::raw(format!(
            "{label} - {}",
            super::plural(largest.reached, "service", "services")
        )));
    }
    let unit_noun = match h.unit {
        Unit::PullRequests => "PRs",
        Unit::Commits => "Commits",
    };
    numbers.push(dot());
    numbers.push(Span::styled(format!("{unit_noun} reaching >10 "), t.dim()));
    numbers.push(Span::raw(format!(
        "{} ({}%)",
        h.over_10.count, h.over_10.percent
    )));
    if h.touching_no_service > 0 {
        numbers.push(dot());
        numbers.push(Span::styled(
            format!("{unit_noun} touching no service "),
            t.dim(),
        ));
        numbers.push(Span::raw(h.touching_no_service.to_string()));
    }
    lines.push(Line::from(numbers));
    lines
}

/// `3` for a whole number, `3.5` otherwise, as the report prints it.
fn number(x: f64) -> String {
    if x.fract().abs() < f64::EPSILON {
        format!("{x:.0}")
    } else {
        format!("{x:.1}")
    }
}
