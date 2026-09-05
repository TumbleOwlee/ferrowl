use crate::COLOR_SCHEME;
use derive_builder::Builder;
use ferrowl_syntax::SyntaxKind;
use getset::{CopyGetters, Getters, Setters};
use ratatui::style::Style;

/// Styles mapping [`SyntaxKind`] to colors for [`CodeInputField`](crate::widgets::CodeInputField) syntax highlighting.
#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters)]
#[getset(set = "pub")]
pub struct SyntaxTheme {
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.syntax.keyword)")]
    pub keyword: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.syntax.ident)")]
    pub ident: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.syntax.number)")]
    pub number: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.syntax.string)")]
    pub string: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.syntax.comment)")]
    pub comment: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.syntax.punct)")]
    pub punct: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.syntax.key)")]
    pub key: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.syntax.literal)")]
    pub literal: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.syntax.object)")]
    pub object: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.syntax.function)")]
    pub function: Style,
    // Diff kinds draw from the top-level `ColorScheme` (success/error/placeholder), not
    // `COLOR_SCHEME.syntax.*`: they reuse the scheme's general-purpose meaning colors
    // rather than getting dedicated syntax-theme entries.
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.success)")]
    pub added: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.error)")]
    pub removed: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(COLOR_SCHEME.placeholder)")]
    pub meta: Style,
}

impl SyntaxTheme {
    pub fn style(&self, kind: SyntaxKind) -> Style {
        match kind {
            SyntaxKind::Keyword => self.keyword,
            SyntaxKind::Ident => self.ident,
            SyntaxKind::Number => self.number,
            SyntaxKind::String => self.string,
            SyntaxKind::Comment => self.comment,
            SyntaxKind::Punct => self.punct,
            SyntaxKind::Key => self.key,
            SyntaxKind::Literal => self.literal,
            SyntaxKind::Object => self.object,
            SyntaxKind::Function => self.function,
            SyntaxKind::Added => self.added,
            SyntaxKind::Removed => self.removed,
            SyntaxKind::Meta => self.meta,
        }
    }
}

impl Default for SyntaxTheme {
    fn default() -> Self {
        SyntaxThemeBuilder::default()
            .build()
            .expect("SyntaxThemeBuilder fields all default")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// UI-R-039, UI-R-120 — the theme maps the three diff kinds to their own styles.
    fn ut_theme_maps_diff_kinds() {
        let theme = SyntaxTheme::default();
        assert_eq!(theme.style(SyntaxKind::Added), theme.added);
        assert_eq!(theme.style(SyntaxKind::Removed), theme.removed);
        assert_eq!(theme.style(SyntaxKind::Meta), theme.meta);
    }

    #[test]
    /// UI-R-121 — default diff styles set foreground only, from success/error/placeholder.
    fn ut_default_diff_styles_are_foreground_only() {
        let theme = SyntaxTheme::default();

        assert_eq!(theme.added.fg, Some(COLOR_SCHEME.success));
        assert_eq!(theme.added.bg, None);
        assert!(theme.added.add_modifier.is_empty());

        assert_eq!(theme.removed.fg, Some(COLOR_SCHEME.error));
        assert_eq!(theme.removed.bg, None);
        assert!(theme.removed.add_modifier.is_empty());

        assert_eq!(theme.meta.fg, Some(COLOR_SCHEME.placeholder));
        assert_eq!(theme.meta.bg, None);
        assert!(theme.meta.add_modifier.is_empty());
    }
}
