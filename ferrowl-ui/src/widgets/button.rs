use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters, WithSetters};

use crate::state::ButtonState;
use crate::style::ButtonStyle;
use crate::traits::Margins;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect};
use ratatui::text::Text as UiText;
use ratatui::widgets::Widget;
use ratatui::widgets::{Block, Paragraph, StatefulWidget};

/// A bordered push button rendered from a
/// [`ButtonState`](crate::state::ButtonState) (label, focus, disabled).
/// Layout (margins, alignment, multiline) and [`ButtonStyle`] are
/// configured on the widget via [`ButtonBuilder`].
#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters, WithSetters)]
#[getset(set = "pub")]
pub struct Button {
    #[getset(get = "pub")]
    #[builder(default = "Margin::default()")]
    border_margin: Margin,
    #[getset(get = "pub")]
    #[builder(default = "ButtonStyle::default()")]
    style: ButtonStyle,
    #[getset(get = "pub")]
    #[builder(default = "Margin::default()")]
    margin: Margin,
    #[getset(get = "pub")]
    #[builder(default = "false")]
    multiline: bool,
    #[getset(get = "pub")]
    #[builder(default = "HorizontalAlignment::Left")]
    horizontal_alignment: HorizontalAlignment,
}

impl StatefulWidget for Button {
    type State = ButtonState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        StatefulWidget::render(&self, area, buf, state);
    }
}

impl StatefulWidget for &Button {
    type State = ButtonState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        buf.set_style(area, self.style.general);

        let mut height = 2 + self.border_margin.vertical * 2;
        if self.multiline {
            height += std::cmp::max(1, area.height);
        } else {
            height += 1;
        }

        let area = Layout::vertical([
            Constraint::Length(self.margin.vertical),
            Constraint::Length(height),
            Constraint::Length(self.margin.vertical),
        ])
        .split(area)[1];

        let mut area = Layout::horizontal([
            Constraint::Length(self.margin.horizontal),
            Constraint::Min(1),
            Constraint::Length(self.margin.horizontal),
        ])
        .split(area)[1];

        let style = if state.focused() && !state.disabled() {
            self.style.focused
        } else {
            self.style.general
        };
        let block = Block::bordered()
            .border_type(ratatui::widgets::BorderType::Double)
            .style(style);
        let inner = block.inner(area);
        block.render(area, buf);
        area = inner.inner(self.border_margin);

        let mut text_area = area;
        let (len, remain) = if (area.width as usize) < state.label().len() {
            state
                .label()
                .chars()
                .fold((0, String::new()), |(mut len, mut line), c| {
                    line.push(c);
                    len += 1;
                    if len >= area.width as usize {
                        let input = Paragraph::new(UiText::from(line).style(style.bold()));
                        input.render(text_area, buf);
                        text_area.y += 1;
                        (0, String::new())
                    } else {
                        (len, line)
                    }
                })
        } else {
            (state.label().len(), state.label().to_owned())
        };
        if len > 0 {
            let input = Paragraph::new(UiText::from(remain).style(style.bold()))
                .alignment(self.horizontal_alignment);
            input.render(text_area, buf);
        }
    }
}

impl Margins for Button {
    fn margins(&self) -> Margin {
        let horizontal = 4 + self.border_margin.horizontal * 2 + 2 * self.margin.horizontal + 1;
        let vertical = 2 + self.border_margin.vertical * 2 + self.margin.vertical;
        Margin {
            horizontal,
            vertical,
        }
    }
}
