//! Tab 1: a dashboard of the graph, then the repository report as the
//! terminal prints it.
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use vernier::{Confidence, EdgeType};

use super::charts;
use crate::app::App;

/// Services each ranking lists on a tall screen, and on a short one.
const TOP: usize = 5;
const TOP_SHORT: usize = 3;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let top = if area.height >= 30 { TOP } else { TOP_SHORT };
    let t = app.theme;
    let edges = app.analysis.graph.edges();

    let by_confidence: Vec<(String, usize, Style)> = [
        Confidence::Observed,
        Confidence::Static,
        Confidence::Inferred,
        Confidence::Uncertain,
    ]
    .into_iter()
    .map(|c| {
        (
            c.as_str().to_string(),
            edges.iter().filter(|e| e.confidence == c).count(),
            t.confidence(c),
        )
    })
    .collect();
    let by_type: Vec<(String, usize, Style)> = [
        EdgeType::Http,
        EdgeType::Grpc,
        EdgeType::Event,
        EdgeType::Database,
        EdgeType::Import,
    ]
    .into_iter()
    .map(|ty| {
        (
            ty.as_str().to_string(),
            edges.iter().filter(|e| e.edge_type == ty).count(),
            t.accent(),
        )
    })
    .collect();

    let mut depended: Vec<(String, usize, Style)> = app
        .services
        .iter()
        .map(|s| {
            (
                s.name.clone(),
                app.analysis.graph.inbound(&s.name).len(),
                t.accent(),
            )
        })
        .filter(|(_, n, _)| *n > 0)
        .collect();
    // Stable: ties keep the services' order.
    depended.sort_by_key(|d| std::cmp::Reverse(d.1));
    depended.truncate(top);
    let surface: Vec<(String, usize, Style)> = app
        .surface
        .iter()
        .filter(|(_, n)| *n > 0)
        .take(top)
        .map(|(name, n)| (name.clone(), *n, t.changed()))
        .collect();

    let left_h = 1 + by_confidence.len() + 1 + 1 + by_type.len();
    let right_h = 1 + depended.len().max(1) + 1 + 1 + surface.len().max(1);
    let dash_h = u16::try_from(left_h.max(right_h) + 2).unwrap_or(u16::MAX);
    let [dash, report] =
        Layout::vertical([Constraint::Length(dash_h), Constraint::Min(3)]).areas(area);
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(dash);

    let block = super::pane(" Edges ".into(), false, app);
    let inner = block.inner(left);
    frame.render_widget(block, left);
    let mut lines = vec![Line::styled("BY CONFIDENCE", t.dim())];
    lines.extend(charts::bars(&by_confidence, inner.width, app));
    lines.push(Line::raw(""));
    lines.push(Line::styled("BY TYPE", t.dim()));
    lines.extend(charts::bars(&by_type, inner.width, app));
    frame.render_widget(Paragraph::new(lines), inner);

    let block = super::pane(" Services ".into(), false, app);
    let inner = block.inner(right);
    frame.render_widget(block, right);
    let mut lines = vec![Line::styled("MOST DEPENDED ON  (inbound edges)", t.dim())];
    if depended.is_empty() {
        lines.push(Line::styled("no edges between services", t.dim()));
    }
    lines.extend(charts::bars(&depended, inner.width, app));
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        format!("WIDEST CHANGE SURFACE  (reach at depth {})", app.depth),
        t.dim(),
    ));
    if surface.is_empty() {
        lines.push(Line::styled("no change reaches another service", t.dim()));
    }
    lines.extend(charts::bars(&surface, inner.width, app));
    frame.render_widget(Paragraph::new(lines), inner);

    let text: Vec<&str> = app.overview.iter().map(String::as_str).collect();
    let scroll = u16::try_from(app.scroll).unwrap_or(u16::MAX);
    frame.render_widget(
        Paragraph::new(text.join("\n"))
            .scroll((scroll, 0))
            .block(super::pane(" Report  j/k scroll ".into(), true, app)),
        report,
    );
}
