use crate::COLOR_SCHEME;
use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters};
use ratatui::style::Style;

#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters)]
#[getset(set = "pub")]
pub struct SelectionStyle {
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.hi).bg(COLOR_SCHEME.bg)")]
    pub focused: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().bg(COLOR_SCHEME.bg).fg(COLOR_SCHEME.hi)")]
    pub border: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.text).bg(COLOR_SCHEME.bg)")]
    pub general: Style,
    #[getset(get = "pub")]
    #[builder(
        default = "[Style::default().fg(COLOR_SCHEME.text).bg(COLOR_SCHEME.bg),Style::default().fg(COLOR_SCHEME.text).bg(COLOR_SCHEME.bg),]"
    )]
    pub rows: [Style; 2],
}

impl Default for SelectionStyle {
    fn default() -> Self {
        SelectionStyleBuilder::default().build().unwrap()
    }
}
