//! Bars drawn with block characters, for counts the tabs chart.
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::app::App;

const EIGHTHS: [char; 8] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// `value` out of `max` as a bar at most `width` cells long, to an eighth of
/// a cell. Any value above zero draws at least an eighth, so a small count
/// never looks like none.
pub fn bar(value: usize, max: usize, width: usize) -> String {
    if value == 0 || max == 0 || width == 0 {
        return String::new();
    }
    let eighths = (value * width * 8).div_ceil(max).max(1);
    let full = eighths / 8;
    let mut s = "█".repeat(full);
    if eighths % 8 > 0 {
        s.push(EIGHTHS[eighths % 8 - 1]);
    }
    s
}

/// `████████░░░░`: `part` of `whole` filled in `width` cells.
pub fn meter(
    part: usize,
    whole: usize,
    width: usize,
    fill: Style,
    app: &App,
) -> Vec<Span<'static>> {
    let filled = if whole == 0 {
        0
    } else {
        ((part * width).div_ceil(whole)).min(width)
    };
    vec![
        Span::styled("█".repeat(filled), fill),
        Span::styled("░".repeat(width - filled), app.theme.dim()),
    ]
}

/// One labelled bar per item, the bars sharing one scale, the count after
/// each: `static    ██████▍ 12`.
pub fn bars(items: &[(String, usize, Style)], width: u16, app: &App) -> Vec<Line<'static>> {
    let label_w = items
        .iter()
        .map(|(l, _, _)| l.chars().count())
        .max()
        .unwrap_or(0);
    let max = items.iter().map(|(_, n, _)| *n).max().unwrap_or(0);
    let value_w = max.to_string().len();
    let bar_w = usize::from(width).saturating_sub(label_w + value_w + 3);
    items
        .iter()
        .map(|(label, n, style)| {
            let b = bar(*n, max, bar_w);
            let pad = bar_w.saturating_sub(b.chars().count());
            Line::from(vec![
                Span::styled(format!("{label:<label_w$} "), app.theme.dim()),
                Span::styled(b, *style),
                Span::raw(format!("{} {n:>value_w$}", " ".repeat(pad))),
            ])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::bar;

    #[test]
    fn bars_scale_to_eighths_and_never_hide_a_count() {
        assert_eq!(bar(0, 10, 10), "");
        assert_eq!(bar(10, 10, 10), "██████████");
        assert_eq!(bar(5, 10, 10), "█████");
        assert_eq!(bar(1, 1000, 10), "▏");
        assert_eq!(bar(3, 16, 4), "▊");
    }
}
