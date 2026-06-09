use crate::COLOR_SCHEME;
use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters};
use ratatui::style::{Color, Style};

/// Styles for [`Table`](crate::widgets::Table): selected row (focused and
/// unfocused), border, header, and alternating rows.
#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters)]
#[getset(set = "pub")]
pub struct TableStyle {
    #[getset(get = "pub")]
    #[builder(
        default = "Style::default().fg(COLOR_SCHEME.text_dark).bg(COLOR_SCHEME.hi_bg).bold()"
    )]
    pub focused: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.hi).bg(COLOR_SCHEME.bg).bold()")]
    pub border: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.border).bg(COLOR_SCHEME.bg)")]
    pub general: Style,
    #[getset(get = "pub")]
    #[builder(
        default = "[Style::default().fg(COLOR_SCHEME.text).bg(COLOR_SCHEME.row[0]), Style::default().fg(COLOR_SCHEME.text).bg(COLOR_SCHEME.row[1])]"
    )]
    pub rows: [Style; 2],
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.hi).bg(COLOR_SCHEME.row[1]).bold()")]
    pub header: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.text).bg(Color::Rgb(34, 28, 18))")]
    pub unfocused_selected: Style,
}

impl Default for TableStyle {
    fn default() -> Self {
        TableStyleBuilder::default().build().unwrap()
    }
}
