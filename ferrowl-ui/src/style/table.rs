use crate::COLOR_SCHEME;
use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters};
use ratatui::style::Style;

#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters)]
#[getset(set = "pub")]
pub struct TableStyle {
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.hi).bg(COLOR_SCHEME.bg)")]
    pub focused: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.border).bg(COLOR_SCHEME.bg)")]
    pub border: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.hi).bg(COLOR_SCHEME.bg)")]
    pub general: Style,
    #[getset(get = "pub")]
    #[builder(
        default = "[Style::default().fg(COLOR_SCHEME.text).bg(COLOR_SCHEME.row[0]), Style::default().fg(COLOR_SCHEME.text).bg(COLOR_SCHEME.row[1])]"
    )]
    pub rows: [Style; 2],
    #[getset(get = "pub")]
    #[builder(default = "Style::default().bg(COLOR_SCHEME.hi_bg).fg(COLOR_SCHEME.text).bold()")]
    pub header: Style,
}

impl Default for TableStyle {
    fn default() -> Self {
        TableStyleBuilder::default().build().unwrap()
    }
}
