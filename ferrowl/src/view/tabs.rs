//! Module tab bar across the top. Uses the scrolling-aware `ScrollingTabs` widget.

use ferrowl_ui::{state::ScrollingTabsState, widgets::ScrollingTabsBuilder};
use ratatui::{buffer::Buffer, layout::Rect, widgets::StatefulWidget};

/// Render the tab bar with `names`, scrolling as needed to keep `active` visible.
pub fn render_tabs(names: &[String], active: usize, area: Rect, buf: &mut Buffer) {
    let widget = ScrollingTabsBuilder::<String>::default().build().unwrap();
    let mut state = ScrollingTabsState {
        titles: names.to_vec(),
        selected: active,
    };
    StatefulWidget::render(&widget, area, buf, &mut state);
}
