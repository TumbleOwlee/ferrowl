use crate::COLOR_SCHEME;
use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters};
use ratatui::style::Style;

/// Styles for input fields: normal, focused, placeholder text, cursor cell,
/// and invalid-input error.
#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters)]
#[getset(set = "pub")]
pub struct InputFieldStyle {
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.text).bg(COLOR_SCHEME.bg)")]
    pub general: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.hi).bg(COLOR_SCHEME.bg)")]
    pub focused: Style,
    /// Border color when the field is not focused. Defaults to the general text color (preserving
    /// the previous look); set it to the theme border color to match table/selection borders.
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.text).bg(COLOR_SCHEME.bg)")]
    pub border: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.placeholder).bg(COLOR_SCHEME.bg)")]
    pub placeholder: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.text_hi).bg(COLOR_SCHEME.hi)")]
    pub cursor: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.error).bg(COLOR_SCHEME.bg)")]
    pub error: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.success).bg(COLOR_SCHEME.bg)")]
    pub success: Style,
}

impl Default for InputFieldStyle {
    fn default() -> Self {
        InputFieldStyleBuilder::default().build().unwrap()
    }
}
