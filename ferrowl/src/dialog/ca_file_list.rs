//! Shared "Add CA file" sub-dialog for the Modbus/OCPP client-CA list widgets (MB-R-136,
//! OC-R-113). A minimal one-field sibling of `crate::module::modbus::dialog::AddNamedValueDialog`
//! (label+value, register-specific): this one just takes a non-empty path and hands it back to
//! the caller, which pushes it onto that role's `client_ca_files: Vec<String>` — the "add" half of
//! the add/remove list interaction both dialogs' Client CA row uses (delete is a plain "remove the
//! selected entry", handled by the caller directly against the `Vec` without needing a dialog).

use super::NonEmpty;
use derive_builder::Builder;
use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{InputFieldState, InputFieldStateBuilder},
    style::{InputFieldStyle, TextStyle},
    widgets::{InputField, InputFieldBuilder, Text, TextBuilder, Validate, ValidateResult, Widget},
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, StatefulWidget, Widget as UiWidget},
};
use std::fmt::Debug;

#[derive(Builder, Clone, Debug)]
pub struct AddCaFileDialog {
    pub path: Widget<InputFieldState, InputField<NonEmpty>>,
    pub error: Widget<String, Text>,
    pub keybinds: Widget<String, Text>,
}

impl AddCaFileDialog {
    pub fn new() -> Self {
        let input_style = InputFieldStyle::default();
        let error_style = TextStyle {
            general: ratatui::prelude::Style::default()
                .fg(COLOR_SCHEME.error)
                .bg(COLOR_SCHEME.bg),
        };
        let text_style = TextStyle::default();

        AddCaFileDialogBuilder::default()
            .path(Widget {
                state: InputFieldStateBuilder::default()
                    .focused(true)
                    .disabled(false)
                    .placeholder(Some("ca.pem".to_string()))
                    .build()
                    .expect("all required builder fields are set"),
                widget: InputFieldBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some("CA File Path".into()))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(input_style)
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
                    .style(error_style)
                    .build()
                    .expect("all required builder fields are set"),
            })
            .keybinds(Widget {
                state: "<Esc>: cancel | <Enter>: confirm".to_string(),
                widget: TextBuilder::default()
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .style(text_style)
                    .build()
                    .expect("all required builder fields are set"),
            })
            .build()
            .expect("all required builder fields are set")
    }

    fn validate(&self) -> Result<(), String> {
        if let ValidateResult::Error(e) = NonEmpty::validate(self.path.state.input()) {
            return Err(format!("Path: {e}"));
        }
        Ok(())
    }

    /// Validate and return the trimmed path, or the validation error.
    pub fn apply(&self) -> Result<String, String> {
        self.validate()?;
        Ok(self.path.state.input().trim().to_string())
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
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

        let error_height = if self.error.state.is_empty() { 0 } else { 3 };
        // 2 border + 2 margin-vertical + 3 path + error + 1 keybinds
        let total_height = 2 + 2 + 3 + error_height + 1;
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
            .title("Add CA File");

        let inner = block.inner(vertical_layout[1]).inner(Margin::new(2, 1));
        ratatui::prelude::Widget::render(&ratatui::widgets::Clear, vertical_layout[1], buf);
        block.render(vertical_layout[1], buf);

        let inner_layout: [Rect; 3] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(error_height),
            Constraint::Length(1),
        ])
        .areas(inner);

        StatefulWidget::render(
            &self.path.widget,
            inner_layout[0],
            buf,
            &mut self.path.state,
        );
        if !self.error.state.is_empty() {
            StatefulWidget::render(
                &self.error.widget,
                inner_layout[1],
                buf,
                &mut self.error.state,
            );
        }
        StatefulWidget::render(
            &self.keybinds.widget,
            inner_layout[2],
            buf,
            &mut self.keybinds.state,
        );
    }
}

impl Default for AddCaFileDialog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};
    use ferrowl_ui::traits::{HandleEvents, SetFocus};

    fn type_into(state: &mut InputFieldState, s: &str) {
        state.set_focused(true);
        for c in s.chars() {
            state.handle_events(KeyModifiers::NONE, KeyCode::Char(c));
        }
    }

    /// MB-R-136/OC-R-113 — an empty path fails validation; a non-empty one is trimmed and
    /// returned.
    #[test]
    fn ut_apply_requires_non_empty_path() {
        assert!(AddCaFileDialog::new().apply().is_err());
        let mut d = AddCaFileDialog::new();
        type_into(&mut d.path.state, "  ca.pem  ");
        // The input field itself doesn't trim as typed; `apply` trims on read.
        assert_eq!(d.apply().unwrap(), "ca.pem".trim());
    }

    /// MB-R-136/OC-R-113 — the add-CA sub-dialog renders its title and an inline error while
    /// empty.
    #[test]
    fn ut_render_shows_title_and_inline_error() {
        let mut d = AddCaFileDialog::new();
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        d.render(area, &mut buf);
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("Add CA File"));
        assert!(text.contains("Non-empty input required") || text.contains("Path:"));
    }
}
