//! One service and its neighbours, drawn: what depends on it on the left,
//! what it depends on on the right, arrows pointing the way the dependency
//! runs.
//!
//! ```text
//!     web ─┐                ┌─▶ catalogue
//! payment ─┼─▶[ cart ]──────┤
//!          ┘                └─▶ redis
//! ```
use blastradius::{Confidence, Edge};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::app::App;

/// One neighbour: its name and the weakest confidence among its edges with
/// the service, which is what a walk through it would carry.
struct Side {
    name: String,
    confidence: Confidence,
    /// A count standing in for names that did not fit.
    more: bool,
}

fn sides(edges: &[&Edge], name: impl Fn(&Edge) -> &str) -> Vec<Side> {
    let mut out: Vec<Side> = Vec::new();
    for e in edges {
        let n = name(e);
        match out.iter_mut().find(|s| s.name == n) {
            Some(s) if e.confidence.rank() < s.confidence.rank() => s.confidence = e.confidence,
            Some(_) => {}
            None => out.push(Side {
                name: n.to_string(),
                confidence: e.confidence,
                more: false,
            }),
        }
    }
    out
}

/// The box-drawing piece where a side's vertical spine meets a row: `up` and
/// `down` when the spine continues, `left` and `right` when a line leaves the
/// cell that way.
// Four directions are four bools; an enum per direction would say less.
#[allow(clippy::fn_params_excessive_bools)]
fn joint(up: bool, down: bool, left: bool, right: bool) -> char {
    match (up, down, left, right) {
        (false, false, false, false) => ' ',
        (false, false, _, _) => '─',
        (_, _, false, false) => '│',
        (false, true, true, false) => '┐',
        (true, false, true, false) => '┘',
        (true, true, true, false) => '┤',
        (false, true, false, true) => '┌',
        (true, false, false, true) => '└',
        (true, true, false, true) => '├',
        (true, true, true, true) => '┼',
        (false, true, true, true) => '┬',
        (true, false, true, true) => '┴',
    }
}

fn clip(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return s.to_string();
    }
    let mut out: String = s.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// The rows a side takes, `max_rows` at most; what does not fit is one row,
/// `+N more`, or `N services` when there is room for nothing else.
fn fit(mut sides: Vec<Side>, max_rows: usize) -> Vec<Side> {
    if sides.len() > max_rows {
        let total = sides.len();
        let hidden = total - (max_rows - 1);
        sides.truncate(max_rows - 1);
        sides.push(Side {
            name: if sides.is_empty() {
                format!("{total} services")
            } else {
                format!("+{hidden} more")
            },
            confidence: Confidence::Uncertain,
            more: true,
        });
    }
    sides
}

/// The narrow form, when names do not fit side by side: what depends on the
/// service above it, flowing down into it, and what it depends on below.
///
/// ```text
/// ┌─ ts-gateway-service
/// ├─ +3 more
/// ▼
/// [ ts-basic-service ]
/// ├─▶ ts-price-service
/// └─▶ +2 more
/// ```
fn stacked(
    center: &str,
    left: &[Side],
    right: &[Side],
    width: usize,
    app: &App,
) -> Vec<Line<'static>> {
    let t = app.theme;
    let style = |s: &Side| -> Style {
        if s.more {
            t.dim()
        } else {
            t.confidence(s.confidence)
        }
    };
    let name_w = width.saturating_sub(4);
    let mut out = Vec::new();
    for (i, s) in left.iter().enumerate() {
        out.push(Line::from(vec![
            Span::styled(if i == 0 { "┌─ " } else { "├─ " }, t.dim()),
            Span::styled(clip(&s.name, name_w), style(s)),
        ]));
    }
    if !left.is_empty() {
        out.push(Line::styled("▼", t.dim()));
    }
    out.push(Line::styled(center.to_string(), t.accent()));
    for (i, s) in right.iter().enumerate() {
        out.push(Line::from(vec![
            Span::styled(
                if i + 1 == right.len() {
                    "└─▶ "
                } else {
                    "├─▶ "
                },
                t.dim(),
            ),
            Span::styled(clip(&s.name, name_w), style(s)),
        ]));
    }
    out
}

/// The diagram for `service` in `width` cells, at most `max_rows` rows.
pub fn lines(
    service: &str,
    inbound: &[&Edge],
    outbound: &[&Edge],
    width: u16,
    max_rows: usize,
    app: &App,
) -> Vec<Line<'static>> {
    let t = app.theme;
    let max_rows = max_rows.max(1);
    let left = fit(sides(inbound, |e| &e.source), max_rows);
    let right = fit(sides(outbound, |e| &e.target), max_rows);
    let h = left.len().max(right.len()).max(1);
    let mid = (h - 1) / 2;

    let width = usize::from(width);
    let center = format!("[ {service} ]");
    let center_w = center.chars().count();
    // `─▶` before the centre and `──` after it, a joint each side, and `─▶ `
    // before a name on the right.
    let names_w = width.saturating_sub(center_w + 2 + 2 + 2 + 3);
    let lw = left
        .iter()
        .map(|s| s.name.chars().count())
        .max()
        .unwrap_or(0)
        .min(names_w / 2);
    let rw = names_w.saturating_sub(lw + 1);
    let longest = |side: &[Side]| {
        side.iter()
            .map(|s| s.name.chars().count())
            .max()
            .unwrap_or(0)
    };
    if longest(&left) > lw || longest(&right) > rw {
        return stacked(&center, &left, &right, width, app);
    }

    // Each side is centred on the middle row.
    let start = |n: usize| mid.saturating_sub(n.saturating_sub(1) / 2);
    let (l0, r0) = (start(left.len()), start(right.len()));
    let span = |first: usize, n: usize| {
        if n == 0 {
            None
        } else {
            Some((first.min(mid), (first + n - 1).max(mid)))
        }
    };
    let (lspan, rspan) = (span(l0, left.len()), span(r0, right.len()));

    let style = |s: &Side| -> Style {
        if s.more {
            t.dim()
        } else {
            t.confidence(s.confidence)
        }
    };
    let mut out = Vec::with_capacity(h);
    for row in 0..h {
        let mut spans: Vec<Span> = Vec::new();
        // Left: name, then its line into the spine.
        let l = row.checked_sub(l0).and_then(|i| left.get(i));
        match l {
            Some(s) => {
                spans.push(Span::styled(
                    format!("{:>lw$}", clip(&s.name, lw)),
                    style(s),
                ));
                spans.push(Span::styled(" ─", t.dim()));
            }
            None => spans.push(Span::raw(" ".repeat(lw + 2))),
        }
        let lj = match lspan {
            Some((top, bot)) if (top..=bot).contains(&row) => {
                joint(row > top, row < bot, l.is_some(), row == mid)
            }
            _ => ' ',
        };
        spans.push(Span::styled(lj.to_string(), t.dim()));
        // Centre.
        if row == mid {
            let into = if left.is_empty() { "  " } else { "─▶" };
            spans.push(Span::styled(into, t.dim()));
            spans.push(Span::styled(center.clone(), t.accent()));
            spans.push(Span::styled(
                if right.is_empty() { "  " } else { "──" },
                t.dim(),
            ));
        } else {
            spans.push(Span::raw(" ".repeat(center_w + 4)));
        }
        // Right: the spine, then a line to the name.
        let r = row.checked_sub(r0).and_then(|i| right.get(i));
        let rj = match rspan {
            Some((top, bot)) if (top..=bot).contains(&row) => {
                joint(row > top, row < bot, row == mid, r.is_some())
            }
            _ => ' ',
        };
        spans.push(Span::styled(rj.to_string(), t.dim()));
        if let Some(s) = r {
            spans.push(Span::styled("─▶ ", t.dim()));
            spans.push(Span::styled(clip(&s.name, rw), style(s)));
        }
        out.push(Line::from(spans));
    }
    if left.is_empty() && right.is_empty() {
        return vec![Line::from(vec![
            Span::styled(center, t.accent()),
            Span::styled("  no edges found to or from it", t.dim()),
        ])];
    }
    out
}
