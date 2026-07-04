//! Language dispatch: routes `highlight_line` calls to the right lexer.

pub(crate) mod json;
pub(crate) mod lua;

use crate::{LineState, SyntaxKind};

/// Source language to highlight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Lua,
    Json,
}

pub(crate) fn highlight(
    lang: Language,
    line: &str,
    state: LineState,
) -> (Vec<(usize, usize, SyntaxKind)>, LineState) {
    match lang {
        Language::Lua => lua::highlight_line(line, state),
        Language::Json => json::highlight_line(line, state),
    }
}
