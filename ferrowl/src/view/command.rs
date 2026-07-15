//! Vim-style command input line at the very bottom, activated by `:`. Reuses the
//! `InputField` widget; parsing/execution of the typed command lands in a later phase.

use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{InputFieldState, InputFieldStateBuilder},
    style::InputFieldStyle,
    widgets::{InputField, InputFieldBuilder, Widget},
};
use ratatui::layout::Margin;

/// The composed command line: an `InputField` plus its input/cursor state.
pub type CommandLine = Widget<InputFieldState, InputField<String>>;

/// Build an empty, unfocused command line.
pub fn new_command_line() -> CommandLine {
    Widget {
        state: InputFieldStateBuilder::default()
            .focused(false)
            .disabled(false)
            .build()
            .expect("all required builder fields are set"),
        widget: InputFieldBuilder::default()
            .border(Border::None)
            .multiline(false)
            .margin(Margin::new(0, 0))
            .style(InputFieldStyle {
                focused: ratatui::style::Style::default()
                    .fg(COLOR_SCHEME.hi)
                    .bg(COLOR_SCHEME.bg),
                cursor: ratatui::style::Style::default()
                    .bg(COLOR_SCHEME.hi)
                    .fg(COLOR_SCHEME.text_hi),
                ..InputFieldStyle::default()
            })
            .build()
            .expect("all required builder fields are set"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrowl_ui::traits::IsFocus;

    #[test]
    fn ut_new_command_line_is_unfocused_and_empty() {
        let cl = new_command_line();
        assert!(!cl.is_focused());
        assert_eq!(cl.state.input(), "");
    }
}
