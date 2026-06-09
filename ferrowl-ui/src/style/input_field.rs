use crate::COLOR_SCHEME;
use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters};
use ratatui::style::Style;

/// Styles for input fields: normal, focused, placeholder text, cursor cell,
/// and invalid-input error.
#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters)]
#[getset(set = "pub")]
pub struct InputFieldStyle {
    #[builder(default = "Style::default().fg(COLOR_SCHEME.text).bg(COLOR_SCHEME.bg)")]
    pub general: Style,
    #[builder(default = "Style::default().fg(COLOR_SCHEME.hi).bg(COLOR_SCHEME.bg)")]
    pub focused: Style,
    #[builder(default = "Style::default().fg(COLOR_SCHEME.placeholder)")]
    pub placeholder: Style,
    #[builder(default = "Style::default().fg(COLOR_SCHEME.text_dark).bg(COLOR_SCHEME.hi)")]
    pub cursor: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.error).bg(COLOR_SCHEME.bg)")]
    pub error: Style,
}

impl Default for InputFieldStyle {
    fn default() -> Self {
        InputFieldStyleBuilder::default().build().unwrap()
    }
}
