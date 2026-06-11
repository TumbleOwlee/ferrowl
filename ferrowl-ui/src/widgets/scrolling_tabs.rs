use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters, WithSetters};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::Line,
    widgets::{StatefulWidget, Tabs, Widget},
};
use std::marker::PhantomData;

use crate::state::ScrollingTabsState;
use crate::style::ScrollingTabsStyle;
use crate::traits::ToLabel;

/// A tab bar that scrolls horizontally to keep the selected tab visible.
///
/// The visible range is computed each render by anchoring on the selected tab
/// and keeping it centered: the left side fills at most half the remaining
/// width, the right side fills the rest, and unused width on either side
/// rolls over to the other.
/// Configure style and divider via [`ScrollingTabsBuilder`]; tab data lives in
/// [`ScrollingTabsState`].
#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters, WithSetters)]
#[getset(set = "pub")]
pub struct ScrollingTabs<T: ToLabel + Clone> {
    #[getset(get = "pub")]
    #[builder(default = "ScrollingTabsStyle::default()")]
    style: ScrollingTabsStyle,
    #[getset(get = "pub")]
    #[builder(default = r#""│".to_string()"#)]
    divider: String,
    #[builder(setter(skip))]
    #[builder(default = "PhantomData")]
    marker: PhantomData<T>,
}

impl<T: ToLabel + Clone> StatefulWidget for ScrollingTabs<T> {
    type State = ScrollingTabsState<T>;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        StatefulWidget::render(&self, area, buf, state);
    }
}

impl<T: ToLabel + Clone> StatefulWidget for &ScrollingTabs<T> {
    type State = ScrollingTabsState<T>;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        if state.titles.is_empty() || area.height == 0 {
            return;
        }
        let n = state.titles.len();
        let sel = state.selected.min(n - 1);
        let div_w = self.divider.chars().count() as u16;
        let widths: Vec<u16> = state
            .titles
            .iter()
            .map(|t| t.to_label().chars().count() as u16 + 2)
            .collect();

        let mut remaining = area.width.saturating_sub(widths[sel]);
        let (mut start, mut end) = (sel, sel);

        // Keep the selected tab centered: the left side may take at most half
        // the leftover width, the right side takes whatever remains, and any
        // width the right side can't use rolls back to the left.
        let mut left_budget = remaining / 2;
        for i in (0..sel).rev() {
            let cost = widths[i] + div_w;
            if left_budget < cost {
                break;
            }
            left_budget -= cost;
            remaining -= cost;
            start = i;
        }
        for i in (sel + 1)..n {
            let cost = div_w + widths[i];
            if remaining < cost {
                break;
            }
            remaining -= cost;
            end = i;
        }
        for i in (0..start).rev() {
            let cost = widths[i] + div_w;
            if remaining < cost {
                break;
            }
            remaining -= cost;
            start = i;
        }

        // Tabs adds one space of padding on each side, matching the +2 in `widths`.
        let visible: Vec<Line> = state.titles[start..=end]
            .iter()
            .map(|t| Line::from(t.to_label()))
            .collect();
        Tabs::new(visible)
            .select(sel - start)
            .style(self.style.general)
            .highlight_style(self.style.selected)
            .divider(self.divider.as_str())
            .render(area, buf);
    }
}
