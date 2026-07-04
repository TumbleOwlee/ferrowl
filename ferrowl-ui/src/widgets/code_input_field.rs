use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters, WithSetters};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Margin, Rect},
    style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Paragraph, StatefulWidget, Widget},
};

use crate::Border;
use crate::state::CodeInputFieldState;
use crate::style::{InputFieldStyle, SyntaxTheme};
use crate::traits::Margins;
use crate::widgets::Title;

/// A multi-line text editor (e.g. for Lua snippets) rendered from a
/// [`CodeInputFieldState`](crate::state::CodeInputFieldState), with line
/// numbers and vertical/horizontal scrolling. Configure border, title,
/// margins, and [`InputFieldStyle`] via [`CodeInputFieldBuilder`].
#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters, WithSetters)]
#[getset(set = "pub")]
pub struct CodeInputField {
    #[getset(get = "pub")]
    #[builder(default = "Border::None")]
    border: Border,
    #[getset(get = "pub")]
    #[builder(default = "InputFieldStyle::default()")]
    style: InputFieldStyle,
    #[getset(get = "pub")]
    #[builder(default = "None")]
    title: Option<Title>,
    #[getset(get = "pub")]
    #[builder(default = "Margin::default()")]
    margin: Margin,
    #[getset(get = "pub")]
    #[builder(default = "SyntaxTheme::default()")]
    syntax_theme: SyntaxTheme,
}

impl Margins for CodeInputField {
    fn margins(&self) -> Margin {
        let horizontal = if let Border::Full(m) = &self.border {
            4 + m.horizontal * 2
        } else {
            0
        } + 2 * self.margin.horizontal
            + 1;
        let vertical = if let Border::Full(m) = &self.border {
            2 + m.vertical * 2
        } else if self.title.is_some() {
            1
        } else {
            0
        } + self.margin.vertical;
        Margin {
            horizontal,
            vertical,
        }
    }
}

impl StatefulWidget for &CodeInputField {
    type State = CodeInputFieldState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        buf.set_style(area, self.style.general);

        let area = Layout::vertical([
            Constraint::Length(self.margin.vertical),
            Constraint::Min(1),
            Constraint::Length(self.margin.vertical),
        ])
        .split(area)[1];

        let mut area = Layout::horizontal([
            Constraint::Length(self.margin.horizontal),
            Constraint::Min(1),
            Constraint::Length(self.margin.horizontal),
        ])
        .split(area)[1];

        if let Border::Full(m) = &self.border {
            // A focused field shows the focused border even when disabled (read-only): a disabled
            // viewer can still hold focus for scrolling, and the border must reflect that.
            let border_style = if state.focused() {
                self.style.focused
            } else {
                self.style.border
            };
            let mut block = Block::bordered().style(border_style);
            if let Some(t) = self.title.as_ref() {
                block = block.title(t.name.as_str()).title_alignment(t.alignment);
            }
            let inner = block.inner(area);
            block.render(area, buf);
            area = inner.inner(*m);
        }

        let visible_height = area.height as usize;
        if visible_height == 0 {
            return;
        }

        let line_count = state.lines().len();
        let active = state.active_line();

        // Show placeholder when empty and not focused
        let is_empty = line_count == 1 && state.lines()[0].is_empty();
        if is_empty && !state.focused() {
            if let Some(ph) = state.placeholder() {
                let para = Paragraph::new(Text::from(ph.as_str()).style(self.style.placeholder));
                para.render(area, buf);
            }
            return;
        }

        // Adjust scroll so active_line is always visible
        let scroll = state.scroll_offset();
        let scroll = if active < scroll {
            active
        } else if active >= scroll + visible_height {
            active + 1 - visible_height
        } else {
            scroll
        };
        state.set_scroll_offset(scroll);

        // Gutter width: digit count of line_count + 1 separator space
        let gutter_width = line_count.to_string().len() as u16 + 1;
        let content_x = area.x + gutter_width;
        let content_width = area.width.saturating_sub(gutter_width) as usize;

        // Adjust horizontal scroll so the cursor stays in view on the active line.
        let cursor_col = state.cursor_col();
        let h_scroll = state.h_scroll();
        let h_scroll = if content_width == 0 {
            0
        } else if cursor_col < h_scroll {
            cursor_col
        } else if cursor_col >= h_scroll + content_width {
            cursor_col + 1 - content_width
        } else {
            h_scroll
        };
        state.set_h_scroll(h_scroll);

        // Fold `LineState` from line 0 through the last visible line, stashing the spans
        // for the visible window. Recomputed every render; buffers are small.
        let visible_spans: Option<Vec<Vec<(usize, usize, ferrowl_syntax::SyntaxKind)>>> =
            state.language().map(|lang| {
                let last_visible = (scroll + visible_height - 1).min(line_count - 1);
                let mut carry = ferrowl_syntax::LineState::default();
                let mut spans = Vec::with_capacity(visible_height);
                for i in 0..=last_visible {
                    let (line_spans, next_carry) =
                        ferrowl_syntax::highlight_line(lang, &state.lines()[i], carry);
                    carry = next_carry;
                    if i >= scroll {
                        spans.push(line_spans);
                    }
                }
                spans
            });

        for (row, line_idx) in (scroll..scroll + visible_height).enumerate() {
            let y = area.y + row as u16;
            if line_idx >= line_count {
                break;
            }

            let gutter_style: Style = if line_idx == active && state.focused() {
                self.style.focused.reversed().bold()
            } else {
                self.style.general
            };
            let gutter_str = format!(
                "{:>width$}",
                line_idx + 1,
                width = gutter_width as usize - 1
            );
            let gutter_rect = Rect::new(area.x, y, gutter_width - 1, 1);
            Paragraph::new(Text::from(gutter_str).style(gutter_style)).render(gutter_rect, buf);

            if content_width == 0 {
                continue;
            }

            let line = &state.lines()[line_idx];
            let chars: Vec<char> = line.chars().collect();
            let content_rect = Rect::new(content_x, y, content_width as u16, 1);

            if let Some(spans) = visible_spans.as_ref() {
                let window_start = h_scroll;
                let window_end = h_scroll.saturating_add(content_width).min(chars.len());
                let mut line_spans = Vec::new();
                let mut cursor = window_start;
                for &(start, end, kind) in &spans[row] {
                    let s = start.max(window_start);
                    let e = end.min(window_end);
                    if s >= e {
                        continue;
                    }
                    if cursor < s {
                        let gap: String = chars[cursor..s].iter().collect();
                        line_spans.push(Span::styled(gap, self.style.general));
                    }
                    let text: String = chars[s..e].iter().collect();
                    line_spans.push(Span::styled(text, self.syntax_theme.style(kind)));
                    cursor = e;
                }
                if cursor < window_end {
                    let gap: String = chars[cursor..window_end].iter().collect();
                    line_spans.push(Span::styled(gap, self.style.general));
                }
                Paragraph::new(Text::from(Line::from(line_spans))).render(content_rect, buf);
            } else {
                let visible: String = chars
                    .get(h_scroll..h_scroll.saturating_add(content_width).min(chars.len()))
                    .unwrap_or(&[])
                    .iter()
                    .collect();
                Paragraph::new(Text::from(visible).style(self.style.general))
                    .render(content_rect, buf);
            }

            if state.focused() && !state.disabled() && line_idx == active {
                let cursor_in_view = cursor_col.saturating_sub(h_scroll) as u16;
                if (cursor_in_view as usize) < content_width {
                    buf[(content_x + cursor_in_view, y)].set_style(self.style.cursor);
                }
            }
        }
    }
}

impl StatefulWidget for CodeInputField {
    type State = CodeInputFieldState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        StatefulWidget::render(&self, area, buf, state);
    }
}
