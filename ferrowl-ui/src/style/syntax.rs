use derive_builder::Builder;
use ferrowl_syntax::SyntaxKind;
use getset::{CopyGetters, Getters, Setters};
use ratatui::style::{Color, Style};

/// Styles mapping [`SyntaxKind`] to colors for [`CodeInputField`](crate::widgets::CodeInputField) syntax highlighting.
#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters)]
#[getset(set = "pub")]
pub struct SyntaxTheme {
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(Color::Magenta)")]
    pub keyword: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(Color::White)")]
    pub ident: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(Color::Cyan)")]
    pub number: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(Color::Green)")]
    pub string: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(Color::DarkGray)")]
    pub comment: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(Color::White)")]
    pub punct: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(Color::Blue)")]
    pub key: Style,
    #[getset(get = "pub")]
    #[builder(default = "Style::default().fg(Color::Yellow)")]
    pub literal: Style,
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
        }
    }
}

impl Default for SyntaxTheme {
    fn default() -> Self {
        SyntaxThemeBuilder::default().build().unwrap()
    }
}
