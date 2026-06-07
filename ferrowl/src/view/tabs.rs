//! Module tab bar across the top. Uses ratatui's built-in `Tabs` widget.

use ferrowl_ui::COLOR_SCHEME;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Tabs, Widget},
};

/// Render the tab bar with `names`, highlighting the `active` tab.
pub fn render_tabs(names: &[String], active: usize, area: Rect, buf: &mut Buffer) {
    let titles: Vec<Line> = names.iter().map(|n| Line::from(format!(" {n} "))).collect();
    let tabs = Tabs::new(titles)
        .select(active)
        .style(
            Style::default()
                .fg(COLOR_SCHEME.hi)
                .bg(COLOR_SCHEME.bg)
                .bold(),
        )
        .highlight_style(
            Style::default()
                .bg(COLOR_SCHEME.hi)
                .fg(COLOR_SCHEME.hi_bg)
                .bold(),
        )
        .divider("│");
    Widget::render(tabs, area, buf);
}
