//! Tab 2: every service, and for the selected one its declaration, its
//! edges both ways, and the evidence for the selected edge.
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Row, Table, TableState, Wrap};
use vernier::{Edge, Evidence, Service, ServiceRole};

use crate::app::{App, Direction, Pane};

/// Rows the edge table keeps, header included, however short the screen.
const TABLE_MIN: u16 = 4;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let (list_w, language) = list_width(app, area.width);
    let [left, right] =
        Layout::horizontal([Constraint::Length(list_w), Constraint::Min(0)]).areas(area);
    list(frame, left, app, language);
    detail(frame, right, app);
}

/// The list takes what its names need, up to half the screen. When the
/// names are too long for that, the language column goes rather than the
/// names being cut.
fn list_width(app: &App, width: u16) -> (u16, bool) {
    let name_w = super::width("NAME", app.services.iter().map(|s| s.name.as_str()));
    let lang_w = super::width(
        "LANGUAGE",
        app.services.iter().filter_map(|s| s.language.as_deref()),
    );
    // Borders, IN, OUT and the gaps between columns.
    let rest = 2 + 3 + 3 + 2;
    let half = width / 2;
    let full = name_w + 1 + lang_w + rest;
    if full <= half {
        (full.max(width * 3 / 10), true)
    } else {
        ((name_w + rest).min(half), false)
    }
}

fn list(frame: &mut Frame, area: Rect, app: &App, language: bool) {
    let t = app.theme;
    let services = app.filtered_services();
    let focused = app.pane == Pane::List;
    let title = if app.service_filter.is_empty() {
        format!(" Services {} ", services.len())
    } else {
        format!(
            " Services {} of {}  /{} ",
            services.len(),
            app.services.len(),
            app.service_filter
        )
    };
    let graph = &app.analysis.graph;
    let rows: Vec<Row> = services
        .iter()
        .map(|s| {
            let mut cells = vec![s.name.clone()];
            if language {
                cells.push(s.language.clone().unwrap_or_else(|| "-".into()));
            }
            cells.push(graph.inbound(&s.name).len().to_string());
            cells.push(graph.outbound(&s.name).len().to_string());
            let row = Row::new(cells);
            if s.role == ServiceRole::Infrastructure {
                row.style(t.dim())
            } else {
                row
            }
        })
        .collect();
    let lang_w = super::width(
        "LANGUAGE",
        app.services.iter().filter_map(|s| s.language.as_deref()),
    );
    let mut widths = vec![Constraint::Fill(1)];
    let mut header = vec!["NAME"];
    if language {
        widths.push(Constraint::Length(lang_w));
        header.push("LANGUAGE");
    }
    widths.extend([Constraint::Length(3), Constraint::Length(3)]);
    header.extend(["IN", "OUT"]);
    let table = Table::new(rows, widths)
        .header(Row::new(header).style(t.dim()))
        .row_highlight_style(super::highlight(focused, app))
        .block(super::pane(title, focused, app));
    let visible = usize::from(area.height.saturating_sub(3));
    let off = super::offset(app.service_sel, &app.offsets.services, visible);
    let mut state = TableState::new()
        .with_offset(off)
        .with_selected((!services.is_empty()).then_some(app.service_sel));
    frame.render_stateful_widget(table, area, &mut state);
}

fn detail(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.pane == Pane::Edges;
    let Some(service) = app.selected_service() else {
        frame.render_widget(
            Paragraph::new("No service matches the filter.").block(super::pane(
                " Service ".into(),
                false,
                app,
            )),
            area,
        );
        return;
    };
    let block = super::pane(format!(" {} ", service.name), focused, app);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let info = info(service, app);
    let edges = app.edges_of_selected();
    let evidence_lines = app
        .selected_edge()
        .map_or_else(Vec::new, |(_, e)| edge_evidence(e, app));
    // One row per fact, cut at the edge; the Overview has them in full.
    let info_h = u16::try_from(info.len() + 1).unwrap_or(u16::MAX);
    let ev_h = super::rows(&evidence_lines, inner.width).min(inner.height / 3);
    // The diagram gives way first: it shrinks to fit what is left after the
    // edge table's minimum and some evidence, and goes when nothing fits.
    let budget = inner
        .height
        .saturating_sub(info_h + TABLE_MIN + ev_h.min(3));
    let graph = &app.analysis.graph;
    let (inbound, outbound) = (graph.inbound(&service.name), graph.outbound(&service.name));
    let mut diagram = Vec::new();
    for cap in (1..=7).rev() {
        let lines = super::ego::lines(&service.name, &inbound, &outbound, inner.width, cap, app);
        if u16::try_from(lines.len() + 1).is_ok_and(|h| h <= budget) {
            diagram = lines;
            break;
        }
    }
    let diagram_h = if diagram.is_empty() {
        0
    } else {
        u16::try_from(diagram.len() + 1).unwrap_or(u16::MAX)
    };
    // Evidence takes what the rest leaves, up to a third of the pane.
    let ev_h = ev_h.min(inner.height.saturating_sub(info_h + diagram_h + TABLE_MIN));
    let [info_area, diagram_area, edges_area, ev_area] = Layout::vertical([
        Constraint::Length(info_h),
        Constraint::Length(diagram_h),
        Constraint::Min(TABLE_MIN),
        Constraint::Length(ev_h),
    ])
    .areas(inner);
    frame.render_widget(Paragraph::new(info), info_area);
    frame.render_widget(Paragraph::new(diagram), diagram_area);

    edges_table(frame, edges_area, &edges, focused, app);
    frame.render_widget(
        Paragraph::new(evidence_lines).wrap(Wrap { trim: false }),
        ev_area,
    );
}

/// Every edge of the selected service, one row each; the selected row's
/// evidence shows under the table.
fn edges_table(
    frame: &mut Frame,
    edges_area: Rect,
    edges: &[(Direction, &Edge)],
    focused: bool,
    app: &App,
) {
    let t = app.theme;
    if edges.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "No edges found to or from this service.",
                t.dim(),
            )),
            edges_area,
        );
        return;
    }
    // CALLS only when production saw at least one of these edges.
    let observed = edges.iter().any(|(_, e)| e.observed.is_some());
    let rows: Vec<Row> = edges
        .iter()
        .map(|(dir, e)| {
            let (words, other) = match dir {
                Direction::DependsOn => ("out", &e.target),
                Direction::DependedOnBy => ("in", &e.source),
            };
            let mut cells = vec![
                Span::styled(words, t.dim()),
                Span::raw(other.clone()),
                Span::raw(e.edge_type.as_str()),
                Span::styled(e.confidence.as_str(), t.confidence(e.confidence)),
            ];
            if observed {
                cells.push(Span::raw(
                    e.observed
                        .as_ref()
                        .and_then(|o| o.calls)
                        .map(|n| n.to_string())
                        .unwrap_or_default(),
                ));
            }
            Row::new(cells)
        })
        .collect();
    let mut widths = vec![
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(8),
        Constraint::Length(10),
    ];
    let mut header = vec!["DIR", "SERVICE", "TYPE", "CONFIDENCE"];
    if observed {
        widths.push(Constraint::Length(6));
        header.push("CALLS");
    }
    let table = Table::new(rows, widths)
        .header(Row::new(header).style(t.dim()))
        .row_highlight_style(super::highlight(focused, app));
    let visible = usize::from(edges_area.height.saturating_sub(1));
    let off = super::offset(app.edge_sel, &app.offsets.edges, visible);
    let mut state = TableState::new()
        .with_offset(off)
        .with_selected(Some(app.edge_sel));
    frame.render_stateful_widget(table, edges_area, &mut state);
}

/// Where the service is and what declared it.
fn info(service: &Service, app: &App) -> Vec<Line<'static>> {
    let t = app.theme;
    let role = match service.role {
        ServiceRole::Code => "code",
        ServiceRole::Infrastructure => "infrastructure",
    };
    let mut info = vec![
        Line::from(vec![
            Span::raw(service.language.clone().unwrap_or_else(|| "-".into())),
            Span::styled(" · ", t.dim()),
            Span::raw(role),
            Span::styled(" · root ", t.dim()),
            Span::raw(service.root.clone().unwrap_or_else(|| "-".into())),
            Span::styled(format!(" · by {}", service.discovered_by.as_str()), t.dim()),
        ]),
        labelled("declared", &evidence(&service.evidence), app),
    ];
    if let Some(image) = &service.image {
        info.push(labelled("image", image, app));
    }
    if let Some(package) = &service.package_name {
        info.push(labelled("package", package, app));
    }
    info
}
/// The selected edge, every piece of evidence for it, and what production saw.
fn edge_evidence(e: &Edge, app: &App) -> Vec<Line<'static>> {
    let t = app.theme;
    let mut lines = vec![Line::from(vec![
        Span::styled("EVIDENCE  ", t.dim()),
        Span::raw(format!(
            "{} -> {} ({})",
            e.source,
            e.target,
            e.edge_type.as_str()
        )),
    ])];
    lines.extend(e.evidence.iter().map(|ev| Line::raw(evidence(ev))));
    if let Some(o) = &e.observed {
        lines.push(Line::raw(match o.calls {
            Some(n) => format!("observed by {}: {n} calls", o.source.label()),
            None => format!("observed by {}", o.source.label()),
        }));
    }
    lines
}

fn labelled(label: &str, value: &str, app: &App) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<10}"), app.theme.dim()),
        Span::raw(value.to_string()),
    ])
}

/// `docker-compose.yaml:57  image redis`
fn evidence(e: &Evidence) -> String {
    let at = match e.line {
        Some(line) => format!("{}:{line}", e.file),
        None => e.file.clone(),
    };
    match &e.detail {
        Some(detail) => format!("{at}  {detail}"),
        None => at,
    }
}
