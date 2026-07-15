use crate::COLOR_SCHEME;
use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters};
use ratatui::style::Style;

/// Styles for [`Button`](crate::widgets::Button): normal and focused.
#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters)]
#[getset(set = "pub")]
pub struct ButtonStyle {
    #[builder(default = "Style::default().fg(COLOR_SCHEME.text).bg(COLOR_SCHEME.bg)")]
    pub general: Style,
    #[builder(default = "Style::default().fg(COLOR_SCHEME.hi).bg(COLOR_SCHEME.bg)")]
    pub focused: Style,
}

impl Default for ButtonStyle {
    fn default() -> Self {
        ButtonStyleBuilder::default().build().expect("ButtonStyleBuilder fields all default")
    }
}
