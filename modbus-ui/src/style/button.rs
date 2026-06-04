use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters};
use ratatui::style::palette::tailwind;
use ratatui::style::{Color, Style};

#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters)]
#[getset(set = "pub")]
pub struct ButtonStyle {
    #[builder(default = "Style::default().fg(tailwind::WHITE).bg(Color::default())")]
    pub general: Style,
    #[builder(default = "Style::default().fg(tailwind::INDIGO.c950).bg(tailwind::SLATE.c950)")]
    pub focused: Style,
}

impl Default for ButtonStyle {
    fn default() -> Self {
        ButtonStyleBuilder::default().build().unwrap()
    }
}
