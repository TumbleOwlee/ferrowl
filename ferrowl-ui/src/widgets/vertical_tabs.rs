use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters, WithSetters};
use ratatui::{buffer::Buffer, layout::Rect, widgets::StatefulWidget};
use std::marker::PhantomData;

use crate::state::VerticalTabsState;
use crate::style::ScrollingTabsStyle;
use crate::traits::ToLabel;

/// A tab bar that lays its tabs out vertically, one per row, showing only the
/// first character of each label, and scrolls to keep the active row visible.
///
/// Style is shared with [`crate::widgets::ScrollingTabs`] via
/// [`ScrollingTabsStyle`]; tab data and the active index live in
/// [`VerticalTabsState`], which the widget only ever reads, aside from the
/// scroll offset it maintains itself.
#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters, WithSetters)]
#[getset(set = "pub")]
pub struct VerticalTabs<T: ToLabel + Clone> {
    #[getset(get = "pub")]
    #[builder(default = "ScrollingTabsStyle::default()")]
    style: ScrollingTabsStyle,
    #[builder(setter(skip))]
    #[builder(default = "PhantomData")]
    marker: PhantomData<T>,
}

impl<T: ToLabel + Clone> StatefulWidget for VerticalTabs<T> {
    type State = VerticalTabsState<T>;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        StatefulWidget::render(&self, area, buf, state);
    }
}

impl<T: ToLabel + Clone> StatefulWidget for &VerticalTabs<T> {
    type State = VerticalTabsState<T>;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        if state.titles.is_empty() {
            state.offset = 0;
            return;
        }

        let h = area.height as usize;
        if state.active < state.titles.len() {
            if state.active < state.offset {
                state.offset = state.active;
            } else if state.active >= state.offset + h {
                state.offset = state.active + 1 - h;
            }
        }

        let end = (state.offset + h).min(state.titles.len());
        for i in state.offset..end {
            let y = area.y + (i - state.offset) as u16;
            let x = area.x;
            let label = state.titles[i].to_label();
            let sym = label
                .chars()
                .next()
                .map_or_else(|| " ".to_string(), String::from);
            let style = if i == state.active {
                self.style.selected
            } else {
                self.style.general
            };
            buf[(x, y)].set_symbol(&sym).set_style(style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(w: u16, h: u16) -> Buffer {
        Buffer::empty(Rect::new(0, 0, w, h))
    }

    fn titles(labels: &[&str]) -> Vec<String> {
        labels.iter().map(|s| s.to_string()).collect()
    }

    fn cell_has_style(cell: &ratatui::buffer::Cell, style: ratatui::style::Style) -> bool {
        cell.fg == style.fg.unwrap_or(ratatui::style::Color::Reset)
            && cell.bg == style.bg.unwrap_or(ratatui::style::Color::Reset)
            && cell.modifier == style.add_modifier
    }

    /// UI-R-114, UI-R-115 — one first char per row, top-down in list order.
    #[test]
    fn ut_renders_one_first_char_per_row() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["alpha", "beta", "gamma"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(1, 3);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 3), &mut b, &mut st);
        assert_eq!(b[(0, 0)].symbol(), "a");
        assert_eq!(b[(0, 1)].symbol(), "b");
        assert_eq!(b[(0, 2)].symbol(), "g");
    }

    /// UI-R-116 — active row takes the selected style, others the general style.
    #[test]
    fn ut_active_row_uses_selected_style() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["alpha", "beta", "gamma"]),
            active: 1,
            offset: 0,
        };
        let mut b = buffer(1, 3);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 3), &mut b, &mut st);
        let style = ScrollingTabsStyle::default();
        assert!(cell_has_style(&b[(0, 0)], style.general));
        assert!(cell_has_style(&b[(0, 1)], style.selected));
        assert!(cell_has_style(&b[(0, 2)], style.general));
    }

    /// UI-R-117, UI-R-118 — fewer rows than tabs scrolls to keep active visible.
    #[test]
    fn ut_scrolls_to_keep_active_visible() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["alpha", "beta", "gamma", "delta", "epsilon"]),
            active: 4,
            offset: 0,
        };
        let mut b = buffer(1, 2);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 2), &mut b, &mut st);
        assert_eq!(st.offset, 3);
        assert_eq!(b[(0, 0)].symbol(), "d");
        assert_eq!(b[(0, 1)].symbol(), "e");
    }

    /// UI-R-118 — offset moves the minimum distance, unchanged when already visible.
    #[test]
    fn ut_scroll_offset_is_minimal() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["alpha", "beta", "gamma", "delta", "epsilon"]),
            active: 3,
            offset: 3,
        };
        let mut b = buffer(1, 2);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 2), &mut b, &mut st);
        assert_eq!(st.offset, 3);

        st.active = 1;
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 2), &mut b, &mut st);
        assert_eq!(st.offset, 1);
    }

    /// UI-R-119 — widget writes no field but the offset.
    #[test]
    fn ut_render_leaves_titles_and_active_untouched() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let input = titles(&["alpha", "beta", "gamma", "delta", "epsilon"]);
        let mut st = VerticalTabsState {
            titles: input.clone(),
            active: 4,
            offset: 0,
        };
        let mut b = buffer(1, 2);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 2), &mut b, &mut st);
        assert_eq!(st.titles, input);
        assert_eq!(st.active, 4);
        assert_eq!(st.offset, 3);
    }

    /// UI-E-063 — zero-sized area skips drawing, offset unchanged.
    #[test]
    fn ut_zero_sized_area_skips_drawing() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["alpha", "beta", "gamma"]),
            active: 0,
            offset: 2,
        };
        let mut b = buffer(0, 3);
        StatefulWidget::render(&w, Rect::new(0, 0, 0, 3), &mut b, &mut st);
        assert_eq!(st.offset, 2);

        let mut b = buffer(1, 0);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 0), &mut b, &mut st);
        assert_eq!(st.offset, 2);
    }

    /// UI-E-064 — wider area draws into the leftmost column only.
    #[test]
    fn ut_wider_area_uses_leftmost_column_only() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["alpha", "beta"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(4, 2);
        StatefulWidget::render(&w, Rect::new(0, 0, 4, 2), &mut b, &mut st);
        assert_eq!(b[(0, 0)].symbol(), "a");
        for x in 1..4 {
            assert_eq!(b[(x, 0)].symbol(), " ");
            assert!(cell_has_style(&b[(x, 0)], ratatui::style::Style::default()));
        }
    }

    /// UI-E-065 — empty tab list draws nothing and resets the offset.
    #[test]
    fn ut_empty_titles_reset_offset() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st = VerticalTabsState {
            titles: vec![],
            active: 0,
            offset: 3,
        };
        let mut b = buffer(1, 3);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 3), &mut b, &mut st);
        assert_eq!(st.offset, 0);
    }

    /// UI-E-066 — active out of range: no row selected, offset unchanged, no panic.
    #[test]
    fn ut_active_out_of_range_is_inert() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["alpha", "beta", "gamma"]),
            active: 9,
            offset: 1,
        };
        let mut b = buffer(1, 3);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 3), &mut b, &mut st);
        assert_eq!(st.offset, 1);
        let style = ScrollingTabsStyle::default();
        for y in 0..2 {
            assert!(cell_has_style(&b[(0, y)], style.general));
        }
    }

    /// UI-E-067 — empty label draws a blank row, still styled.
    #[test]
    fn ut_empty_label_row_is_blank_but_styled() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["", "beta"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(1, 2);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 2), &mut b, &mut st);
        assert_eq!(b[(0, 0)].symbol(), " ");
        assert!(cell_has_style(
            &b[(0, 0)],
            ScrollingTabsStyle::default().selected
        ));
        assert_eq!(b[(0, 1)].symbol(), "b");
    }

    /// UI-E-068 — double-width first char written as-is, no fallback glyph.
    #[test]
    fn ut_wide_first_char_is_written_as_is() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["日本"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(1, 1);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 1), &mut b, &mut st);
        assert_eq!(b[(0, 0)].symbol(), "日");
    }
}
