use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters, WithSetters};
use ratatui::{
    buffer::Buffer,
    layout::{Margin, Rect, VerticalAlignment},
    widgets::StatefulWidget,
};
use std::marker::PhantomData;

use crate::state::VerticalTabsState;
use crate::style::ScrollingTabsStyle;
use crate::traits::ToLabel;

/// A tab bar that lays its tabs out vertically, writing each title downward
/// one character per row and scrolling to keep the active tab's block of
/// rows visible. An optional padding of a horizontal count H and a vertical
/// count V frames every tab, making the widget's rendered width `1 + 2H`
/// columns. When the tabs' natural height is less than the area, the spare
/// rows stretch the tabs to fill it and `alignment` places each tab's
/// character and padding rows at the top, middle or bottom of the resulting
/// extent.
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
    #[getset(get_copy = "pub")]
    #[builder(default = "Margin::new(0, 0)")]
    padding: Margin,
    #[getset(get_copy = "pub")]
    #[builder(default = "VerticalAlignment::Center")]
    alignment: VerticalAlignment,
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

/// Resolves row `row` (counted across the whole stacked tab list) to the
/// owning tab's index and, if the row is a character row rather than a
/// vertical padding or gained row, the character it carries. `heights`
/// holds each tab's rendered height, natural or stretched; `leads` holds
/// each tab's gained rows placed before its padding, per its alignment.
fn resolve_row<T: ToLabel>(
    titles: &[T],
    heights: &[usize],
    leads: &[usize],
    vertical: usize,
    row: usize,
) -> Option<(usize, Option<char>)> {
    let mut start = 0usize;
    for (idx, title) in titles.iter().enumerate() {
        let label = title.to_label();
        let chars_count = label.chars().count();
        let height = heights[idx];
        if row < start + height {
            let local = row - start;
            let lead = leads[idx];
            if local < lead + vertical || local >= lead + vertical + chars_count {
                return Some((idx, None));
            }
            return Some((idx, label.chars().nth(local - lead - vertical)));
        }
        start += height;
    }
    None
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

        let horizontal = self.padding.horizontal as usize;
        let vertical = self.padding.vertical as usize;
        let h = area.height as usize;

        let natural: Vec<usize> = state
            .titles
            .iter()
            .map(|t| 2 * vertical + t.to_label().chars().count())
            .collect();
        let total: usize = natural.iter().sum();
        let heights: Vec<usize> = if total < h {
            let s = h - total;
            let n = natural.len();
            natural
                .iter()
                .enumerate()
                .map(|(i, nat)| nat + s / n + usize::from(i < s % n))
                .collect()
        } else {
            natural.clone()
        };
        let leads: Vec<usize> = heights
            .iter()
            .zip(natural.iter())
            .map(|(height, nat)| {
                let g = height - nat;
                match self.alignment {
                    VerticalAlignment::Top => 0,
                    VerticalAlignment::Center => g / 2,
                    VerticalAlignment::Bottom => g,
                }
            })
            .collect();

        if total < h && state.active < state.titles.len() {
            state.offset = 0;
        }

        let mut start = 0usize;
        let mut active_block = None;
        for (idx, height) in heights.iter().enumerate() {
            if idx == state.active {
                active_block = Some((start, *height));
            }
            start += height;
        }
        let total_rows = start;

        if let Some((block_start, block_height)) = active_block {
            if block_start + block_height > state.offset + h {
                state.offset = block_start + block_height - h;
            }
            if state.offset > block_start {
                state.offset = block_start;
            }
        }

        let end = (state.offset + h).min(total_rows);
        for row in state.offset..end {
            let Some((tab_idx, ch)) = resolve_row(&state.titles, &heights, &leads, vertical, row)
            else {
                continue;
            };
            let y = area.y + (row - state.offset) as u16;
            let style = if tab_idx == state.active {
                self.style.selected
            } else {
                self.style.general
            };
            for col in 0..=(2 * horizontal) {
                let x = area.x + col as u16;
                if x >= area.x + area.width {
                    break;
                }
                let sym = if col == horizontal {
                    ch.map_or_else(|| " ".to_string(), String::from)
                } else {
                    " ".to_string()
                };
                buf[(x, y)].set_symbol(&sym).set_style(style);
            }
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

    /// UI-R-120, UI-R-121 — horizontal padding alone widens the render.
    #[test]
    fn ut_horizontal_padding_widens_render() {
        let w = VerticalTabsBuilder::<String>::default()
            .padding(Margin::new(2, 0))
            .build()
            .unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["Tab"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(7, 3);
        StatefulWidget::render(&w, Rect::new(0, 0, 7, 3), &mut b, &mut st);
        assert_eq!(b[(2, 0)].symbol(), "T");
        assert_eq!(b[(2, 1)].symbol(), "a");
        assert_eq!(b[(2, 2)].symbol(), "b");
        let style = ScrollingTabsStyle::default();
        for y in 0..3 {
            for x in [0usize, 1, 3, 4] {
                assert_eq!(b[(x as u16, y)].symbol(), " ");
                assert!(cell_has_style(&b[(x as u16, y)], style.selected));
            }
        }
        for y in 0..3 {
            for x in 5..7 {
                assert_eq!(b[(x, y)].symbol(), " ");
                assert!(cell_has_style(&b[(x, y)], ratatui::style::Style::default()));
            }
        }
    }

    /// UI-R-120 — vertical padding alone frames the title with blank rows,
    /// without changing the rendered width.
    #[test]
    fn ut_vertical_padding_frames_rows() {
        let w = VerticalTabsBuilder::<String>::default()
            .padding(Margin::new(0, 1))
            .build()
            .unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["Tab"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(1, 5);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 5), &mut b, &mut st);
        assert_eq!(b[(0, 0)].symbol(), " ");
        assert_eq!(b[(0, 1)].symbol(), "T");
        assert_eq!(b[(0, 2)].symbol(), "a");
        assert_eq!(b[(0, 3)].symbol(), "b");
        assert_eq!(b[(0, 4)].symbol(), " ");
    }

    /// UI-R-114, UI-R-115 — a title is written one character per row, top-down.
    #[test]
    fn ut_renders_title_one_char_per_row() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["Tab", "Two"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(1, 6);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 6), &mut b, &mut st);
        assert_eq!(b[(0, 0)].symbol(), "T");
        assert_eq!(b[(0, 1)].symbol(), "a");
        assert_eq!(b[(0, 2)].symbol(), "b");
        assert_eq!(b[(0, 3)].symbol(), "T");
        assert_eq!(b[(0, 4)].symbol(), "w");
        assert_eq!(b[(0, 5)].symbol(), "o");
    }

    /// UI-R-116, UI-R-121 — every cell of the active tab's block takes the
    /// selected style, including its padding rows and columns.
    #[test]
    fn ut_active_block_cells_use_selected_style() {
        let w = VerticalTabsBuilder::<String>::default()
            .padding(Margin::new(1, 1))
            .build()
            .unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["Tab", "Two"]),
            active: 1,
            offset: 0,
        };
        let mut b = buffer(3, 10);
        StatefulWidget::render(&w, Rect::new(0, 0, 3, 10), &mut b, &mut st);
        let style = ScrollingTabsStyle::default();
        for y in 0..5 {
            for x in 0..3 {
                assert!(cell_has_style(&b[(x, y)], style.general));
            }
        }
        for y in 5..10 {
            for x in 0..3 {
                assert!(cell_has_style(&b[(x, y)], style.selected));
            }
        }
    }

    /// UI-R-120, UI-R-121 — default padding is zero, rendering exactly one column.
    #[test]
    fn ut_default_padding_is_zero() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["Tab"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(3, 3);
        StatefulWidget::render(&w, Rect::new(0, 0, 3, 3), &mut b, &mut st);
        assert_eq!(b[(0, 0)].symbol(), "T");
        assert_eq!(b[(0, 1)].symbol(), "a");
        assert_eq!(b[(0, 2)].symbol(), "b");
        for y in 0..3 {
            assert_eq!(b[(1, y)].symbol(), " ");
            assert_eq!(b[(2, y)].symbol(), " ");
            assert!(cell_has_style(&b[(1, y)], ratatui::style::Style::default()));
            assert!(cell_has_style(&b[(2, y)], ratatui::style::Style::default()));
        }
    }

    /// UI-R-120, UI-R-121 — H and V together frame the title with blank rows and columns.
    #[test]
    fn ut_padding_frames_each_title() {
        let w = VerticalTabsBuilder::<String>::default()
            .padding(Margin::new(1, 1))
            .build()
            .unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["Tab"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(3, 5);
        StatefulWidget::render(&w, Rect::new(0, 0, 3, 5), &mut b, &mut st);
        let row = |y: u16| {
            format!(
                "{}{}{}",
                b[(0, y)].symbol(),
                b[(1, y)].symbol(),
                b[(2, y)].symbol()
            )
        };
        assert_eq!(row(0), "   ");
        assert_eq!(row(1), " T ");
        assert_eq!(row(2), " a ");
        assert_eq!(row(3), " b ");
        assert_eq!(row(4), "   ");
    }

    /// UI-R-114, UI-R-115, UI-R-117 — fewer rows than the tabs' total height
    /// scrolls to keep the active tab's block visible.
    #[test]
    fn ut_scrolls_to_keep_active_visible() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["Tab", "Two"]),
            active: 1,
            offset: 0,
        };
        let mut b = buffer(1, 3);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 3), &mut b, &mut st);
        assert_eq!(st.offset, 3);
        assert_eq!(b[(0, 0)].symbol(), "T");
        assert_eq!(b[(0, 1)].symbol(), "w");
        assert_eq!(b[(0, 2)].symbol(), "o");
    }

    /// UI-R-118 — offset moves the minimum distance, unchanged when the whole
    /// block is already visible.
    #[test]
    fn ut_scroll_offset_is_minimal() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["Tab", "Two"]),
            active: 1,
            offset: 3,
        };
        let mut b = buffer(1, 3);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 3), &mut b, &mut st);
        assert_eq!(st.offset, 3);

        st.active = 0;
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 3), &mut b, &mut st);
        assert_eq!(st.offset, 0);
    }

    /// UI-R-118, UI-R-119 — widget writes no field but the offset, moved the
    /// minimal distance.
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
        assert_eq!(st.offset, 19);
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
        let mut b = buffer(3, 3);
        StatefulWidget::render(&w, Rect::new(0, 0, 0, 3), &mut b, &mut st);
        assert_eq!(st.offset, 2);
        assert_eq!(b[(0, 0)].symbol(), " ");
        assert!(cell_has_style(&b[(0, 0)], ratatui::style::Style::default()));

        let mut b = buffer(3, 3);
        StatefulWidget::render(&w, Rect::new(0, 0, 3, 0), &mut b, &mut st);
        assert_eq!(st.offset, 2);
        assert_eq!(b[(0, 0)].symbol(), " ");
        assert!(cell_has_style(&b[(0, 0)], ratatui::style::Style::default()));
    }

    /// UI-E-064 — an area wider than the rendered width draws into the
    /// leftmost `1 + 2N` columns only.
    #[test]
    fn ut_wider_area_uses_rendered_width_only() {
        let w = VerticalTabsBuilder::<String>::default()
            .padding(Margin::new(1, 1))
            .build()
            .unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["Tab"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(6, 5);
        StatefulWidget::render(&w, Rect::new(0, 0, 6, 5), &mut b, &mut st);
        assert_eq!(b[(1, 1)].symbol(), "T");
        assert_eq!(b[(1, 2)].symbol(), "a");
        assert_eq!(b[(1, 3)].symbol(), "b");
        for x in 3..6 {
            for y in 0..5 {
                assert_eq!(b[(x, y)].symbol(), " ");
                assert!(cell_has_style(&b[(x, y)], ratatui::style::Style::default()));
            }
        }
    }

    /// UI-E-069 — an area narrower than the rendered width clips at the
    /// right edge, no reflow, no panic.
    #[test]
    fn ut_narrower_area_clips_rendered_columns() {
        let w = VerticalTabsBuilder::<String>::default()
            .padding(Margin::new(1, 1))
            .build()
            .unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["Tab"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(2, 5);
        StatefulWidget::render(&w, Rect::new(0, 0, 2, 5), &mut b, &mut st);
        assert_eq!(b[(1, 1)].symbol(), "T");
        assert_eq!(b[(1, 2)].symbol(), "a");
        assert_eq!(b[(1, 3)].symbol(), "b");
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

    /// UI-E-066 — active out of range: no cell selected, offset unchanged, no panic.
    #[test]
    fn ut_active_out_of_range_is_inert() {
        let w = VerticalTabsBuilder::<String>::default()
            .padding(Margin::new(1, 1))
            .build()
            .unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["alpha", "beta", "gamma"]),
            active: 9,
            offset: 1,
        };
        let mut b = buffer(3, 3);
        StatefulWidget::render(&w, Rect::new(0, 0, 3, 3), &mut b, &mut st);
        assert_eq!(st.offset, 1);
        let style = ScrollingTabsStyle::default();
        for y in 0..3 {
            for x in 0..3 {
                assert!(cell_has_style(&b[(x, y)], style.general));
            }
        }
    }

    /// UI-R-120, UI-E-067 — an empty title occupies only its padding rows.
    #[test]
    fn ut_empty_title_occupies_padding_only() {
        let w = VerticalTabsBuilder::<String>::default()
            .padding(Margin::new(1, 1))
            .build()
            .unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["", "Two"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(3, 7);
        StatefulWidget::render(&w, Rect::new(0, 0, 3, 7), &mut b, &mut st);
        let style = ScrollingTabsStyle::default();
        assert!(cell_has_style(&b[(1, 0)], style.selected));
        assert!(cell_has_style(&b[(1, 1)], style.selected));
        assert_eq!(b[(1, 3)].symbol(), "T");

        let w0 = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st0 = VerticalTabsState {
            titles: titles(&["", "Two"]),
            active: 0,
            offset: 0,
        };
        let mut b0 = buffer(1, 3);
        StatefulWidget::render(&w0, Rect::new(0, 0, 1, 3), &mut b0, &mut st0);
        assert_eq!(b0[(0, 0)].symbol(), "T");
    }

    /// UI-E-068 — double-width title characters written as-is, no fallback glyph.
    #[test]
    fn ut_wide_title_char_is_written_as_is() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["日本"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(1, 2);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 2), &mut b, &mut st);
        assert_eq!(b[(0, 0)].symbol(), "日");
        assert_eq!(b[(0, 1)].symbol(), "本");
    }

    /// UI-E-070 — an active block taller than the area places its first row
    /// at the top edge; the rest of the block is clipped.
    #[test]
    fn ut_block_taller_than_area_starts_at_top() {
        let w = VerticalTabsBuilder::<String>::default()
            .padding(Margin::new(1, 1))
            .build()
            .unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["Ab", "Longtitle"]),
            active: 1,
            offset: 0,
        };
        let mut b = buffer(3, 3);
        StatefulWidget::render(&w, Rect::new(0, 0, 3, 3), &mut b, &mut st);
        assert_eq!(st.offset, 4);
        assert_eq!(b[(1, 0)].symbol(), " ");
        assert_eq!(b[(1, 1)].symbol(), "L");
        assert_eq!(b[(1, 2)].symbol(), "o");
    }

    /// UI-R-122 — spare rows below the tabs' natural height are divided
    /// among the tabs so they together cover the whole area.
    #[test]
    fn ut_spare_rows_stretch_tabs_to_cover_area() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["A", "B"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(1, 6);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 6), &mut b, &mut st);
        let style = ScrollingTabsStyle::default();
        for y in 0..6 {
            assert!(
                cell_has_style(&b[(0, y)], style.general)
                    || cell_has_style(&b[(0, y)], style.selected)
            );
        }
    }

    /// UI-R-123, UI-R-116 — spare rows are divided evenly, remainder to the
    /// topmost tabs, and a gained row of the active tab is styled selected.
    #[test]
    fn ut_spare_rows_divided_evenly_remainder_to_top() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["A", "B", "C"]),
            active: 1,
            offset: 0,
        };
        let mut b = buffer(1, 8);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 8), &mut b, &mut st);
        let style = ScrollingTabsStyle::default();
        for y in 0..3 {
            assert!(cell_has_style(&b[(0, y)], style.general));
        }
        for y in 3..6 {
            assert!(cell_has_style(&b[(0, y)], style.selected));
        }
        for y in 6..8 {
            assert!(cell_has_style(&b[(0, y)], style.general));
        }
    }

    /// UI-R-124, UI-R-126, UI-R-116 — under `Top` alignment gained rows sit
    /// outside the padding, below it, and the whole extent stays selected.
    #[test]
    fn ut_gained_rows_sit_outside_the_padding() {
        let w = VerticalTabsBuilder::<String>::default()
            .padding(Margin::new(0, 1))
            .alignment(VerticalAlignment::Top)
            .build()
            .unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["A"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(1, 5);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 5), &mut b, &mut st);
        let style = ScrollingTabsStyle::default();
        assert_eq!(b[(0, 0)].symbol(), " ");
        assert_eq!(b[(0, 1)].symbol(), "A");
        assert_eq!(b[(0, 2)].symbol(), " ");
        assert_eq!(b[(0, 3)].symbol(), " ");
        assert_eq!(b[(0, 4)].symbol(), " ");
        for y in 0..5 {
            assert!(cell_has_style(&b[(0, y)], style.selected));
        }
    }

    /// UI-R-125, UI-R-118 — no tab stretches once the natural height
    /// already fills or exceeds the area; the scroll rules govern instead.
    #[test]
    fn ut_no_stretch_when_natural_height_fills_area() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();

        let mut st = VerticalTabsState {
            titles: titles(&["Ab", "Cd"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(1, 4);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 4), &mut b, &mut st);
        assert_eq!(b[(0, 0)].symbol(), "A");
        assert_eq!(b[(0, 1)].symbol(), "b");
        assert_eq!(b[(0, 2)].symbol(), "C");
        assert_eq!(b[(0, 3)].symbol(), "d");

        let mut st = VerticalTabsState {
            titles: titles(&["Ab", "Cd"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(1, 3);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 3), &mut b, &mut st);
        assert_eq!(b[(0, 0)].symbol(), "A");
        assert_eq!(b[(0, 1)].symbol(), "b");
        assert_eq!(b[(0, 2)].symbol(), "C");

        let mut st = VerticalTabsState {
            titles: titles(&["Abc", "Def"]),
            active: 1,
            offset: 3,
        };
        let mut b = buffer(1, 6);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 6), &mut b, &mut st);
        assert_eq!(st.offset, 3);
        assert_eq!(b[(0, 0)].symbol(), "D");
        assert_eq!(b[(0, 1)].symbol(), "e");
        assert_eq!(b[(0, 2)].symbol(), "f");
        for y in 3..6 {
            assert_eq!(b[(0, y)].symbol(), " ");
            assert!(cell_has_style(&b[(0, y)], ratatui::style::Style::default()));
        }
    }

    /// UI-E-071, UI-R-116 — a single tab with spare height takes every
    /// gained row and covers the whole area.
    #[test]
    fn ut_single_tab_covers_whole_area() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["A"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(1, 5);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 5), &mut b, &mut st);
        let style = ScrollingTabsStyle::default();
        for y in 0..5 {
            assert!(cell_has_style(&b[(0, y)], style.selected));
        }
    }

    /// UI-E-072, UI-R-123 — with fewer spare rows than tabs, the topmost
    /// tabs gain one row each and the area is still covered.
    #[test]
    fn ut_fewer_spare_rows_than_tabs() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["A", "B", "C"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(1, 5);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 5), &mut b, &mut st);
        let style = ScrollingTabsStyle::default();
        for y in 0..2 {
            assert!(cell_has_style(&b[(0, y)], style.selected));
        }
        for y in 2..5 {
            assert!(cell_has_style(&b[(0, y)], style.general));
        }
    }

    /// UI-E-066 — an out-of-range active leaves the offset untouched even
    /// while the stretch path applies, and selects no cell.
    #[test]
    fn ut_out_of_range_active_keeps_offset_when_stretching() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["Abc", "Def"]),
            active: 9,
            offset: 2,
        };
        let mut b = buffer(1, 10);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 10), &mut b, &mut st);
        assert_eq!(st.offset, 2);
        let style = ScrollingTabsStyle::default();
        for y in 0..8 {
            assert!(!cell_has_style(&b[(0, y)], style.selected));
        }
    }

    /// UI-E-073 — an empty title with zero vertical padding still takes
    /// part in the spare-row division and so becomes visible.
    #[test]
    fn ut_zero_height_tab_becomes_visible_via_stretch() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["", "A"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(1, 4);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 4), &mut b, &mut st);
        let style = ScrollingTabsStyle::default();
        assert!(cell_has_style(&b[(0, 0)], style.selected));
        assert!(cell_has_style(&b[(0, 1)], style.selected));
    }

    /// UI-R-122 — a scroll offset retained from a smaller area is cleared
    /// once a later render finds spare height to stretch into.
    #[test]
    fn ut_stretch_clears_a_retained_offset() {
        let w = VerticalTabsBuilder::<String>::default().build().unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["Abc", "Def"]),
            active: 1,
            offset: 0,
        };
        let mut small = buffer(1, 2);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 2), &mut small, &mut st);
        assert_eq!(st.offset, 3);

        let mut big = buffer(1, 10);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 10), &mut big, &mut st);
        assert_eq!(st.offset, 0);
        let style = ScrollingTabsStyle::default();
        for y in 0..10 {
            assert!(
                cell_has_style(&big[(0, y)], style.general)
                    || cell_has_style(&big[(0, y)], style.selected)
            );
        }
    }

    /// UI-R-126, UI-R-127, UI-R-124 — `Center` splits the gained rows with
    /// the smaller half above the top padding row.
    #[test]
    fn ut_alignment_center_splits_the_gained_rows() {
        let w = VerticalTabsBuilder::<String>::default()
            .padding(Margin::new(0, 1))
            .alignment(VerticalAlignment::Center)
            .build()
            .unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["A"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(1, 5);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 5), &mut b, &mut st);
        assert_eq!(b[(0, 0)].symbol(), " ");
        assert_eq!(b[(0, 1)].symbol(), " ");
        assert_eq!(b[(0, 2)].symbol(), "A");
        assert_eq!(b[(0, 3)].symbol(), " ");
        assert_eq!(b[(0, 4)].symbol(), " ");
    }

    /// UI-R-126, UI-R-124 — `Bottom` puts every gained row above the
    /// padding, outside it.
    #[test]
    fn ut_alignment_bottom_puts_gained_rows_first() {
        let w = VerticalTabsBuilder::<String>::default()
            .padding(Margin::new(0, 1))
            .alignment(VerticalAlignment::Bottom)
            .build()
            .unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["A"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(1, 5);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 5), &mut b, &mut st);
        assert_eq!(b[(0, 0)].symbol(), " ");
        assert_eq!(b[(0, 1)].symbol(), " ");
        assert_eq!(b[(0, 2)].symbol(), " ");
        assert_eq!(b[(0, 3)].symbol(), "A");
        assert_eq!(b[(0, 4)].symbol(), " ");
    }

    /// UI-R-126 — with no `.alignment` call the builder defaults to `Center`.
    #[test]
    fn ut_alignment_defaults_to_center() {
        let w = VerticalTabsBuilder::<String>::default()
            .padding(Margin::new(0, 1))
            .build()
            .unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["A"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(1, 5);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 5), &mut b, &mut st);
        assert_eq!(b[(0, 0)].symbol(), " ");
        assert_eq!(b[(0, 1)].symbol(), " ");
        assert_eq!(b[(0, 2)].symbol(), "A");
        assert_eq!(b[(0, 3)].symbol(), " ");
        assert_eq!(b[(0, 4)].symbol(), " ");
    }

    /// UI-R-127 — an odd `g` splits with the smaller half above.
    #[test]
    fn ut_center_puts_the_smaller_half_above() {
        let w = VerticalTabsBuilder::<String>::default()
            .alignment(VerticalAlignment::Center)
            .build()
            .unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["A"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(1, 4);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 4), &mut b, &mut st);
        assert_eq!(b[(0, 0)].symbol(), " ");
        assert_eq!(b[(0, 1)].symbol(), "A");
        assert_eq!(b[(0, 2)].symbol(), " ");
        assert_eq!(b[(0, 3)].symbol(), " ");
    }

    /// UI-E-074, UI-R-127 — gaining exactly one row under `Center` puts it
    /// below the bottom padding row; the title sits one row above centre.
    #[test]
    fn ut_center_single_gained_row_goes_below() {
        let w = VerticalTabsBuilder::<String>::default()
            .padding(Margin::new(0, 1))
            .alignment(VerticalAlignment::Center)
            .build()
            .unwrap();
        let mut st = VerticalTabsState {
            titles: titles(&["A"]),
            active: 0,
            offset: 0,
        };
        let mut b = buffer(1, 4);
        StatefulWidget::render(&w, Rect::new(0, 0, 1, 4), &mut b, &mut st);
        assert_eq!(b[(0, 0)].symbol(), " ");
        assert_eq!(b[(0, 1)].symbol(), "A");
        assert_eq!(b[(0, 2)].symbol(), " ");
        assert_eq!(b[(0, 3)].symbol(), " ");
    }

    /// UI-R-128, UI-R-125 — alignment changes nothing once the natural
    /// height already fills or exceeds the area.
    #[test]
    fn ut_alignment_is_inert_without_stretching() {
        for alignment in [
            VerticalAlignment::Top,
            VerticalAlignment::Center,
            VerticalAlignment::Bottom,
        ] {
            let w = VerticalTabsBuilder::<String>::default()
                .alignment(alignment)
                .build()
                .unwrap();

            let mut st = VerticalTabsState {
                titles: titles(&["Ab", "Cd"]),
                active: 0,
                offset: 0,
            };
            let mut b = buffer(1, 4);
            StatefulWidget::render(&w, Rect::new(0, 0, 1, 4), &mut b, &mut st);
            assert_eq!(b[(0, 0)].symbol(), "A");
            assert_eq!(b[(0, 1)].symbol(), "b");
            assert_eq!(b[(0, 2)].symbol(), "C");
            assert_eq!(b[(0, 3)].symbol(), "d");

            let mut st = VerticalTabsState {
                titles: titles(&["Ab", "Cd"]),
                active: 0,
                offset: 0,
            };
            let mut b = buffer(1, 3);
            StatefulWidget::render(&w, Rect::new(0, 0, 1, 3), &mut b, &mut st);
            assert_eq!(b[(0, 0)].symbol(), "A");
            assert_eq!(b[(0, 1)].symbol(), "b");
            assert_eq!(b[(0, 2)].symbol(), "C");
        }
    }

    /// UI-E-075, UI-R-123 — a tab that gains no rows renders alike under
    /// every alignment; only the tabs that gain rows move.
    #[test]
    fn ut_unstretched_tab_renders_alike_under_every_alignment() {
        for (alignment, first, second) in [
            (VerticalAlignment::Top, 0u16, 2u16),
            (VerticalAlignment::Center, 0, 2),
            (VerticalAlignment::Bottom, 1, 3),
        ] {
            let w = VerticalTabsBuilder::<String>::default()
                .alignment(alignment)
                .build()
                .unwrap();
            let mut st = VerticalTabsState {
                titles: titles(&["A", "B", "C"]),
                active: 0,
                offset: 0,
            };
            let mut b = buffer(1, 5);
            StatefulWidget::render(&w, Rect::new(0, 0, 1, 5), &mut b, &mut st);
            assert_eq!(b[(0, first)].symbol(), "A");
            assert_eq!(b[(0, second)].symbol(), "B");
            assert_eq!(b[(0, 4)].symbol(), "C");
        }
    }

    /// UI-R-116 — moving a title inside its extent moves no styling: the
    /// active tab's whole extent stays selected under every alignment.
    #[test]
    fn ut_active_extent_is_styled_under_every_alignment() {
        let style = ScrollingTabsStyle::default();
        for alignment in [
            VerticalAlignment::Top,
            VerticalAlignment::Center,
            VerticalAlignment::Bottom,
        ] {
            let w = VerticalTabsBuilder::<String>::default()
                .padding(Margin::new(0, 1))
                .alignment(alignment)
                .build()
                .unwrap();
            let mut st = VerticalTabsState {
                titles: titles(&["A"]),
                active: 0,
                offset: 0,
            };
            let mut b = buffer(1, 5);
            StatefulWidget::render(&w, Rect::new(0, 0, 1, 5), &mut b, &mut st);
            for y in 0..5 {
                assert!(cell_has_style(&b[(0, y)], style.selected));
            }
        }
    }
}
