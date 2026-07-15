use crate::COLOR_SCHEME;
use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters};
use ratatui::style::Style;

/// Styles for the suggestion popup of a
/// [`SuggestInput`](crate::widgets::SuggestInput): background/border of the
/// popup and the highlighted (currently selected) row.
#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters)]
#[getset(set = "pub")]
pub struct SuggestInputStyle {
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.text).bg(COLOR_SCHEME.bg)")]
    pub general: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.border).bg(COLOR_SCHEME.bg)")]
    pub border: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.text_hi).bg(COLOR_SCHEME.hi_bg)")]
    pub selected: Style,
}

impl Default for SuggestInputStyle {
    fn default() -> Self {
        SuggestInputStyleBuilder::default().build().expect("SuggestInputStyleBuilder fields all default")
    }
}
