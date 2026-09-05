use crate::COLOR_SCHEME;
use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters};
use ratatui::style::{Modifier, Style};

/// Styles for [`MarkdownInputField`](crate::widgets::MarkdownInputField) rendering.
#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters)]
#[getset(set = "pub")]
pub struct MarkdownTheme {
    #[builder(default = "[
        Style::default().fg(COLOR_SCHEME.hi).add_modifier(Modifier::BOLD),
        Style::default().fg(COLOR_SCHEME.info).add_modifier(Modifier::BOLD),
        Style::default().fg(COLOR_SCHEME.success).add_modifier(Modifier::BOLD),
        Style::default().fg(COLOR_SCHEME.warning).add_modifier(Modifier::BOLD),
        Style::default().fg(COLOR_SCHEME.text_hi).add_modifier(Modifier::BOLD),
        Style::default().fg(COLOR_SCHEME.text).add_modifier(Modifier::BOLD),
    ]")]
    heading: [Style; 6],
    #[builder(default = "[
        Style::default().fg(COLOR_SCHEME.border),
        Style::default().fg(COLOR_SCHEME.hi),
        Style::default().fg(COLOR_SCHEME.info),
    ]")]
    quote_bar: [Style; 3],
    #[getset(get = "pub")]
    #[builder(
        default = "Style::default().fg(COLOR_SCHEME.text).add_modifier(Modifier::DIM | Modifier::ITALIC)"
    )]
    pub quote_text: Style,
    #[getset(get = "pub")]
    #[builder(
        default = "Style::default().fg(COLOR_SCHEME.info).add_modifier(Modifier::UNDERLINED)"
    )]
    pub link: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.syntax.string)")]
    pub code: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.hi)")]
    pub bullet: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.border)")]
    pub rule: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.warning)")]
    pub image: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().bg(COLOR_SCHEME.hi_bg)")]
    pub highlighted_row: Style,
}

impl MarkdownTheme {
    /// The heading style for `level` (1–6); out-of-range clamps to the deepest level.
    pub fn heading(&self, level: u8) -> Style {
        let idx = level.saturating_sub(1).min(self.heading.len() as u8 - 1) as usize;
        self.heading[idx]
    }

    /// The quote-bar style for `depth` (1-based nesting); out-of-range clamps to the last entry.
    pub fn quote_bar(&self, depth: usize) -> Style {
        let idx = depth.saturating_sub(1).min(self.quote_bar.len() - 1);
        self.quote_bar[idx]
    }
}

impl Default for MarkdownTheme {
    fn default() -> Self {
        MarkdownThemeBuilder::default()
            .build()
            .expect("MarkdownThemeBuilder fields all default")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// UI-R-141 — the default theme builds and its level/depth lookups clamp out-of-range input.
    fn ut_markdown_theme_defaults_build_and_clamp_level_lookups() {
        let theme = MarkdownTheme::default();
        assert_eq!(theme.heading(1), theme.heading[0]);
        assert_eq!(theme.heading(6), theme.heading[5]);
        assert_eq!(theme.heading(9), theme.heading[5]);
        assert_eq!(theme.heading(0), theme.heading[0]);

        assert_eq!(theme.quote_bar(1), theme.quote_bar[0]);
        assert_eq!(theme.quote_bar(3), theme.quote_bar[2]);
        assert_eq!(theme.quote_bar(9), theme.quote_bar[2]);
        assert_eq!(theme.quote_bar(0), theme.quote_bar[0]);
    }
}
