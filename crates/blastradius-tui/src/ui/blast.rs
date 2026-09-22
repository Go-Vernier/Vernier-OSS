//! Tab 4: one change's blast radius. The walk is drawn as a tree from each
//! changed service; beside it, the reach by depth and by confidence, and the
//! selected service's path in the report's words.
use blastradius::{Blast, ChangeKind, Confidence, Hop, blast::NOT_REACHED};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};

use super::charts;
use crate::app::App;
use crate::tree::{Kind, Row};

/// Cells the reach meter under the headline takes.
const METER: usize = 24;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let Some(b) = &app.blast else {
        frame.render_widget(
            Paragraph::new(vec![
                Line::raw("No change walked yet."),
                Line::raw(""),
                Line::styled(
                    "Enter on a service (2) or a pull request (3), or d for a diff range and f for files on the Changes tab.",
                    app.theme.dim(),
                ),
            ])
            .wrap(Wrap { trim: false })
            .block(super::pane(" Blast radius ".into(), true, app)),
            area,
        );
        return;
    };
    let top = summary(b, app);
    let bottom = outside(b, app, area.width, usize::from(area.height / 8).max(1));
    let bottom_h = super::rows(&bottom, area.width);
    let [top_area, middle, bottom_area] = Layout::vertical([
        Constraint::Length(super::rows(&top, area.width)),
        Constraint::Min(6),
        Constraint::Length(bottom_h),
    ])
    .areas(area);
    frame.render_widget(Paragraph::new(top).wrap(Wrap { trim: false }), top_area);
    let tree_w = tree_width(app).min(area.width * 6 / 10);
    let [left, right] =
        Layout::horizontal([Constraint::Length(tree_w), Constraint::Min(0)]).areas(middle);
    tree(frame, left, app);
    side(frame, right, b, app);
    frame.render_widget(
        Paragraph::new(bottom).wrap(Wrap { trim: false }),
        bottom_area,
    );
}

/// The headline, a meter of how much of the repository it reaches, the
/// confidence breakdown, and where the change came from.
fn summary(b: &Blast, app: &App) -> Vec<Line<'static>> {
    let t = app.theme;
    let s = &b.summary;
    let mut lines = vec![Line::styled(
        format!(
            "{} changed -> {} in the blast radius",
            super::plural(s.changed, "service", "services"),
            super::plural(s.reached, "service", "services")
        ),
        t.bold(),
    )];
    // Code services the change could reach: every one it did not change.
    let others = s.reached + b.not_reached.len();
    let mut meter = charts::meter(s.reached, others, METER, t.changed(), app);
    meter.push(Span::styled(
        format!("  {} of {} other code services", s.reached, others),
        t.dim(),
    ));
    meter.push(Span::styled(format!(" · depth {}", b.depth), t.dim()));
    lines.push(Line::from(meter));

    let ch = &b.change;
    let mut first = vec![match ch.kind {
        ChangeKind::Pr => format!("PR {}", ch.reference),
        ChangeKind::Commit => format!("commit {}", ch.reference),
        ChangeKind::Diff => format!("diff {}", ch.reference),
        ChangeKind::Files if ch.reference.starts_with("service ") => ch.reference.clone(),
        ChangeKind::Files => format!("{} given", ch.reference),
    }];
    first.extend(ch.how.clone());
    first.extend(ch.date.clone());
    let mut change = vec![
        Span::styled("Change  ", t.dim()),
        Span::raw(first.join("  ")),
    ];
    if let Some(title) = &ch.title {
        change.push(Span::styled(format!("  {title}"), t.dim()));
    }
    lines.push(Line::from(change));

    if b.changed.is_empty() {
        let noun = if ch.files.len() == 1 {
            "file belongs"
        } else {
            "files belong"
        };
        lines.push(Line::styled(
            format!(
                "None of the {} changed {noun} to a discovered service: {}",
                ch.files.len(),
                b.unowned.join(", ")
            ),
            t.warn(),
        ));
        if ch.files.is_empty() {
            lines.push(Line::styled("The change lists no files.", t.warn()));
        }
    }
    lines
}

/// What a tree row says after its name: `calls catalogue (http)`, read from
/// the reached service's side as the report reads a hop.
fn hop_tail(hop: &Hop) -> String {
    let words = blastradius::hop_words(hop);
    words
        .strip_prefix(&format!("{} ", hop.to))
        .map_or(words.clone(), str::to_string)
}

fn row_line(row: &Row, b: &Blast, app: &App) -> Line<'static> {
    let t = app.theme;
    let mut spans = vec![Span::styled(row.prefix.clone(), t.dim())];
    match row.kind {
        Kind::Root => {
            let files = b
                .changed
                .iter()
                .find(|c| c.service == row.service)
                .map_or(0, |c| c.files.len());
            spans.push(Span::styled(format!("◆ {}", row.service), t.changed()));
            spans.push(Span::styled(
                format!("  changed · {}", super::plural(files, "file", "files")),
                t.dim(),
            ));
        }
        Kind::Reached(i) => {
            let c = b.reached[i].confidence;
            spans.push(Span::styled(
                format!("● {}", row.service),
                t.confidence(c).add_modifier(ratatui::style::Modifier::BOLD),
            ));
            if let Some(hop) = &row.hop {
                spans.push(Span::styled(format!("  {}", hop_tail(hop)), t.dim()));
            }
        }
        Kind::Via => {
            spans.push(Span::styled(format!("○ {}", row.service), t.dim()));
            if let Some(hop) = &row.hop {
                spans.push(Span::styled(format!("  {}", hop_tail(hop)), t.dim()));
            }
        }
    }
    Line::from(spans)
}

/// Wide enough for the longest row and its borders.
fn tree_width(app: &App) -> u16 {
    let Some(b) = &app.blast else {
        return 0;
    };
    let w = app
        .tree
        .iter()
        .map(|r| row_line(r, b, app).width())
        .max()
        .unwrap_or(0)
        + 3;
    u16::try_from(w.max(24)).unwrap_or(u16::MAX)
}

fn tree(frame: &mut Frame, area: Rect, app: &App) {
    let Some(b) = &app.blast else {
        return;
    };
    let items: Vec<ListItem> = app
        .tree
        .iter()
        .map(|r| ListItem::new(row_line(r, b, app)))
        .collect();
    let list = List::new(items)
        .highlight_style(super::highlight(true, app))
        .block(super::pane(" Walk ".into(), true, app));
    let visible = usize::from(area.height.saturating_sub(2));
    let off = super::offset(app.reached_sel, &app.offsets.reached, visible);
    let mut state = ListState::default()
        .with_offset(off)
        .with_selected((!app.tree.is_empty()).then_some(app.reached_sel));
    frame.render_stateful_widget(list, area, &mut state);
}

/// Reach by depth and by confidence, then what the cursor is on.
fn side(frame: &mut Frame, area: Rect, b: &Blast, app: &App) {
    let t = app.theme;
    let block = super::pane(" Reach ".into(), false, app);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    if b.reached.is_empty() {
        lines.push(Line::styled(
            format!(
                "No service reached within depth {}. + walks further.",
                b.depth
            ),
            t.dim(),
        ));
        lines.push(Line::raw(""));
    } else {
        let deepest = b.reached.iter().map(|r| r.depth).max().unwrap_or(0);
        let by_depth: Vec<(String, usize, ratatui::style::Style)> = (1..=deepest)
            .map(|d| {
                (
                    format!("depth {d}"),
                    b.reached.iter().filter(|r| r.depth == d).count(),
                    t.changed(),
                )
            })
            .collect();
        lines.push(Line::styled("BY DEPTH", t.dim()));
        lines.extend(charts::bars(&by_depth, inner.width, app));
        lines.push(Line::raw(""));
        let by = b.summary.by_confidence;
        let by_conf: Vec<(String, usize, ratatui::style::Style)> = [
            (by.observed, Confidence::Observed),
            (by.static_, Confidence::Static),
            (by.inferred, Confidence::Inferred),
            (by.uncertain, Confidence::Uncertain),
        ]
        .into_iter()
        .map(|(n, c)| (c.as_str().to_string(), n, t.confidence(c)))
        .collect();
        lines.push(Line::styled("BY CONFIDENCE", t.dim()));
        lines.extend(charts::bars(&by_conf, inner.width, app));
        lines.push(Line::raw(""));
    }
    lines.extend(selection(b, app));
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// The row under the cursor: a changed service's files, a reached
/// service's path hop by hop, or where a service on the way is reported.
fn selection(b: &Blast, app: &App) -> Vec<Line<'static>> {
    let t = app.theme;
    let Some(row) = app.selected_row() else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    match row.kind {
        Kind::Root => {
            lines.push(Line::styled(format!("CHANGED  {}", row.service), t.bold()));
            if let Some(c) = b.changed.iter().find(|c| c.service == row.service) {
                lines.extend(c.files.iter().map(|f| Line::styled(f.clone(), t.dim())));
            }
        }
        Kind::Reached(i) => {
            let r = &b.reached[i];
            lines.push(Line::styled(format!("PATH TO  {}", r.service), t.bold()));
            lines.extend(r.path.iter().enumerate().map(|(n, hop)| {
                Line::from(vec![
                    Span::styled(format!("{}. ", n + 1), t.dim()),
                    Span::styled(blastradius::hop_words(hop), t.confidence(hop.confidence)),
                ])
            }));
            lines.push(Line::styled(
                format!(
                    "depth {} · the weakest hop on the path is {}",
                    r.depth,
                    r.confidence.as_str()
                ),
                t.dim(),
            ));
        }
        Kind::Via => {
            lines.push(Line::styled(
                format!("ON THE WAY  {}", row.service),
                t.bold(),
            ));
            lines.push(Line::styled(
                "A step on another service's path. Its own path, the strongest one, is listed where it is marked ●.",
                t.dim(),
            ));
        }
    }
    lines
}

/// Infrastructure on the path, unowned files, and what the walk did not
/// reach, in the report's fixed wording. The not-reached names get at most
/// `max_rows` rows and end in `and N more` rather than being cut off.
fn outside(b: &Blast, app: &App, width: u16, max_rows: usize) -> Vec<Line<'static>> {
    let t = app.theme;
    let mut lines = Vec::new();
    if !b.infrastructure.is_empty() {
        let list = b
            .infrastructure
            .iter()
            .map(|x| format!("{} (published to by {})", x.service, x.via))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(Line::from(vec![
            Span::styled("Infrastructure on the path  ", t.dim()),
            Span::raw(list),
        ]));
    }
    if !b.unowned.is_empty() && !b.changed.is_empty() {
        lines.push(Line::styled(
            format!(
                "{} changed {} to no service: {}",
                b.unowned.len(),
                if b.unowned.len() == 1 {
                    "file belongs"
                } else {
                    "files belong"
                },
                b.unowned.join(", ")
            ),
            t.dim(),
        ));
    }
    lines.push(Line::raw(NOT_REACHED));
    if b.not_reached.is_empty() {
        lines.push(Line::styled(
            "  0 services  (every other service is in the computed blast radius)",
            t.dim(),
        ));
        return lines;
    }
    let lead = format!(
        "  {}  ",
        super::plural(b.not_reached.len(), "service", "services")
    );
    let rows = fit_names(&lead, &b.not_reached, usize::from(width), max_rows);
    for (i, row) in rows.into_iter().enumerate() {
        if i == 0 {
            let rest = row[lead.len()..].to_string();
            lines.push(Line::from(vec![
                Span::raw(lead.clone()),
                Span::styled(rest, t.dim()),
            ]));
        } else {
            lines.push(Line::styled(row, t.dim()));
        }
    }
    lines
}

/// `names`, comma separated after `lead`, wrapped to `width` in at most
/// `max_rows` rows. What does not fit is counted: `a, b, and 30 more`.
fn fit_names(lead: &str, names: &[String], width: usize, max_rows: usize) -> Vec<String> {
    let indent = " ".repeat(lead.chars().count().min(width / 2));
    let mut rows: Vec<String> = Vec::new();
    let mut current = lead.to_string();
    let mut on_row = 0;
    for (i, name) in names.iter().enumerate() {
        let piece = if i + 1 < names.len() {
            format!("{name},")
        } else {
            name.clone()
        };
        let add = piece.chars().count() + usize::from(on_row > 0);
        if on_row > 0 && current.chars().count() + add > width {
            rows.push(std::mem::replace(&mut current, indent.clone()));
            on_row = 0;
            if rows.len() == max_rows {
                return with_more(rows, names.len() - i, width);
            }
        }
        if on_row > 0 {
            current.push(' ');
        }
        current.push_str(&piece);
        on_row += 1;
    }
    rows.push(current);
    rows
}

/// Drops names from the end of the last row until `and N more` fits.
fn with_more(mut rows: Vec<String>, mut hidden: usize, width: usize) -> Vec<String> {
    let Some(last) = rows.last_mut() else {
        return rows;
    };
    loop {
        let suffix = format!(" and {hidden} more");
        if last.chars().count() + suffix.chars().count() <= width {
            last.push_str(&suffix);
            return rows;
        }
        match last.trim_end().rfind(' ') {
            Some(cut) if !last[..cut].trim().is_empty() => {
                last.truncate(cut);
                hidden += 1;
            }
            _ => {
                last.push_str(&suffix);
                return rows;
            }
        }
    }
}
