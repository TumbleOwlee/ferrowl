//! Vim-style command input line at the very bottom, activated by `:`. Reuses the
//! `InputField` widget; parsing/execution of the typed command lands in a later phase.

use ferrowl_ui::{
    COLOR_SCHEME,
    state::{InputFieldState, InputFieldStateBuilder},
    style::InputFieldStyle,
    types::Border,
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
            .unwrap(),
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
                    .fg(COLOR_SCHEME.text),
                ..InputFieldStyle::default()
            })
            .build()
            .unwrap(),
    }
}
