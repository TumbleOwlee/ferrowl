use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters};
use ratatui::style::Style;

use crate::COLOR_SCHEME;

#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters)]
pub struct ScrollingTabsStyle {
    #[builder(default = "Style::default().fg(COLOR_SCHEME.hi).bg(COLOR_SCHEME.bg).bold()")]
    pub general: Style,
    #[builder(default = "Style::default().bg(COLOR_SCHEME.hi).fg(COLOR_SCHEME.text_hi).bold()")]
    pub selected: Style,
}

impl Default for ScrollingTabsStyle {
    fn default() -> Self {
        ScrollingTabsStyleBuilder::default().build().unwrap()
    }
}
