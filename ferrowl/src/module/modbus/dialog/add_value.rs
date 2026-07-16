//! Sub-dialog for adding a named value (label + scalar) to a register.

use crate::config::device::{NamedValue, Scalar};
use derive_builder::Builder;
use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{InputFieldState, InputFieldStateBuilder},
    style::{InputFieldStyle, TextStyle},
    widgets::{InputField, InputFieldBuilder, Text, TextBuilder, Validate, ValidateResult, Widget},
};
use ferrowl_ui_derive::{Focus, focusable};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, StatefulWidget, Widget as UiWidget},
};
use std::fmt::Debug;

// ---------------------------------------------------------------------------
// AddNamedValueDialog — small inline sub-dialog for creating a new NamedValue
// ---------------------------------------------------------------------------

#[focusable]
#[derive(Builder, Clone, Debug, Focus)]
pub struct AddNamedValueDialog {
    #[focus]
    pub label: Widget<InputFieldState, InputField<String>>,
    #[focus]
    pub value: Widget<InputFieldState, InputField<String>>,
    // Error display field
    pub error: Widget<String, Text>,
    pub keybinds: [Widget<String, Text>; 2],
}

impl AddNamedValueDialog {
    pub fn new() -> Self {
        let input_style = InputFieldStyle::default();
        let error_style = TextStyle {
            general: ratatui::prelude::Style::default()
                .fg(COLOR_SCHEME.error)
                .bg(COLOR_SCHEME.bg),
        };
        let text_style = TextStyle::default();

        AddNamedValueDialogBuilder::default()
            .label(Widget {
                state: InputFieldStateBuilder::default()
                    .focused(true)
                    .disabled(false)
                    .placeholder(Some("Name...".to_string()))
                    .build()
                    .expect("all required builder fields are set"),
                widget: InputFieldBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some("Label".into()))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(input_style.clone())
                    .build()
                    .expect("all required builder fields are set"),
            })
            .value(Widget {
                state: InputFieldStateBuilder::default()
                    .focused(false)
                    .disabled(false)
                    .placeholder(Some("0".to_string()))
                    .build()
                    .expect("all required builder fields are set"),
                widget: InputFieldBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some("Value".into()))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(input_style.clone())
                    .build()
                    .expect("all required builder fields are set"),
            })
            .error(Widget {
                state: "".to_string(),
                widget: TextBuilder::default()
                    .title(Some("Error".into()))
                    .border(Border::Full(Margin::new(1, 0)))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(error_style.clone())
                    .build()
                    .expect("all required builder fields are set"),
            })
            .keybinds([
                Widget {
                    state: "<Tab>: next".to_string(),
                    widget: TextBuilder::default()
                        .margin(Margin {
                            vertical: 0,
                            horizontal: 1,
                        })
                        .horizontal_alignment(HorizontalAlignment::Center)
                        .style(text_style.clone())
                        .build()
                        .expect("all required builder fields are set"),
                },
                Widget {
                    state: "<Esc>: cancel | <Enter>: confirm / newline".to_string(),
                    widget: TextBuilder::default()
                        .margin(Margin {
                            vertical: 0,
                            horizontal: 1,
                        })
                        .horizontal_alignment(HorizontalAlignment::Center)
                        .style(text_style.clone())
                        .build()
                        .expect("all required builder fields are set"),
                },
            ])
            .focus(AddNamedValueDialogFocus::Label)
            .build()
            .expect("all required builder fields are set")
    }

    fn validate(&self) -> Result<(), String> {
        if let ValidateResult::Error(e) = String::validate(self.label.state.input()) {
            return Err(format!("Label: {e}"));
        }
        if self.label.state.input().trim().is_empty() {
            return Err("Label must not be empty.".to_string());
        }
        if self.value.state.input().trim().is_empty() {
            return Err("Value must not be empty.".to_string());
        }
        Ok(())
    }

    pub fn apply(&self) -> Result<NamedValue, String> {
        self.validate()?;
        let name = self.label.state.input().trim().to_string();
        // Accept int, float or text; the type is inferred from the input.
        let value = Scalar::from_input(self.value.state.input());
        Ok(NamedValue { name, value })
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        // Show validation error inline.
        match self.validate() {
            Ok(_) => self.error.state.clear(),
            Err(e) => self.error.state = e,
        }

        let horizontal_layout: [Rect; 3] = Layout::horizontal([
            Constraint::Min(1),
            Constraint::Length(60),
            Constraint::Min(1),
        ])
        .areas(area);

        // 2 border + 2 margin-vertical + 3 label + 3 value + 1 error + 1 keybinds = 12
        let error_height = if self.error.state.is_empty() { 0 } else { 3 };
        let total_height = 2 + 2 + 3 + 3 + error_height + 1 + 1 + 1;
        let vertical_layout: [Rect; 3] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(total_height),
            Constraint::Min(1),
        ])
        .areas(horizontal_layout[1]);

        let block = Block::bordered()
            .style(
                ratatui::prelude::Style::default()
                    .fg(COLOR_SCHEME.hi)
                    .bg(COLOR_SCHEME.bg),
            )
            .title_alignment(HorizontalAlignment::Center)
            .title("Add Value");

        let inner = block.inner(vertical_layout[1]).inner(Margin::new(2, 1));
        ratatui::prelude::Widget::render(&ratatui::widgets::Clear, vertical_layout[1], buf);
        block.render(vertical_layout[1], buf);

        let inner_layout: [Rect; 6] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(error_height),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);

        StatefulWidget::render(
            &self.label.widget,
            inner_layout[0],
            buf,
            &mut self.label.state,
        );
        StatefulWidget::render(
            &self.value.widget,
            inner_layout[1],
            buf,
            &mut self.value.state,
        );
        if !self.error.state.is_empty() {
            StatefulWidget::render(
                &self.error.widget,
                inner_layout[2],
                buf,
                &mut self.error.state,
            );
        }
        StatefulWidget::render(
            &self.keybinds[0].widget,
            inner_layout[4],
            buf,
            &mut self.keybinds[0].state,
        );
        StatefulWidget::render(
            &self.keybinds[1].widget,
            inner_layout[5],
            buf,
            &mut self.keybinds[1].state,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};
    use ferrowl_ui::state::InputFieldState;
    use ferrowl_ui::traits::{HandleEvents, SetFocus};

    fn type_into(state: &mut InputFieldState, s: &str) {
        state.set_focused(true);
        for c in s.chars() {
            state.handle_events(KeyModifiers::NONE, KeyCode::Char(c));
        }
    }

    fn dialog_with(label: &str, value: &str) -> AddNamedValueDialog {
        let mut d = AddNamedValueDialog::new();
        type_into(&mut d.label.state, label);
        type_into(&mut d.value.state, value);
        d
    }

    #[test]
    fn ut_apply_requires_label_and_value() {
        assert!(AddNamedValueDialog::new().apply().is_err()); // both empty
        assert!(dialog_with("only-label", "").apply().is_err());
        assert!(dialog_with("", "5").apply().is_err());
    }

    #[test]
    fn ut_apply_infers_scalar_type_from_input() {
        assert!(matches!(
            dialog_with("i", "42").apply().unwrap().value,
            Scalar::Int(42)
        ));
        assert!(matches!(
            dialog_with("f", "3.5").apply().unwrap().value,
            Scalar::Float(v) if v == 3.5
        ));
        let text = dialog_with("t", "abc").apply().unwrap();
        assert_eq!(text.name, "t");
        assert!(matches!(text.value, Scalar::Text(ref s) if s == "abc"));
    }

    #[test]
    fn ut_render_shows_title_and_inline_error() {
        let mut d = AddNamedValueDialog::new(); // empty → validation error shown
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        d.render(area, &mut buf);
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("Add Value"));
        assert!(text.contains("must not be empty"));
    }
}
