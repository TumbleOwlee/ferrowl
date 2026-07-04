//! Pure text-to-spans syntax highlighting for Lua and JSON.
//!
//! No rendering, no dependencies — lexers here just walk a line of source and emit
//! `(start_char, end_char, SyntaxKind)` spans plus a carry-over [`LineState`] for
//! constructs that span multiple lines (Lua long strings/comments). Consumers (e.g. a
//! TUI editor widget) own the mapping from [`SyntaxKind`] to actual colors/styles.

mod format;
mod lang;

pub use format::format;
pub use lang::Language;

/// Classification of a highlighted span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyntaxKind {
    Keyword,
    Ident,
    Number,
    String,
    Comment,
    Punct,
    /// JSON object key (a string immediately followed by `:`).
    Key,
    /// `true` / `false` / `nil` / `null`.
    Literal,
    /// Identifier accessed via `.`/`:` (e.g. `C_Register` in `C_Register:Set`).
    Object,
    /// Identifier in call or method position (e.g. `Set` in `C_Register:Set(1)`).
    Function,
}

/// Whether a line ends mid-way through a Lua long bracket (`[[...]]` / `[=[...=]` etc.)
/// opened as a string or a comment, and at what `=` level.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum LuaCarry {
    #[default]
    None,
    LongString(u8),
    LongComment(u8),
}

/// Cross-line lexer carry state. Only Lua long strings/comments need this; JSON always
/// starts fresh, so callers highlighting JSON can pass `LineState::default()` every line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LineState(pub(crate) LuaCarry);

/// Highlights a single line of source, given the state carried over from the previous
/// line. Returns the spans (sorted by start, non-overlapping, char-index based) and the
/// state to carry into the next line.
pub fn highlight_line(
    lang: Language,
    line: &str,
    state: LineState,
) -> (Vec<(usize, usize, SyntaxKind)>, LineState) {
    lang::highlight(lang, line, state)
}
