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
