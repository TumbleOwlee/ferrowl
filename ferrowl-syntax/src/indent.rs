//! Indent hint for editors: given the text left of the cursor when Enter is pressed,
//! how many levels deeper (or shallower) should the new line start relative to that
//! line's own leading whitespace? Token-scanned with the language lexers, so block
//! keywords or brackets inside strings/comments never count — mirroring the depth
//! rules of the [`format`](crate::format) re-indenters.

use crate::format::lua::{is_closer, is_else, is_opener};
use crate::{Language, LineState, SyntaxKind, highlight_line};

/// Net block-depth change `line` contributes to the line that follows it, in levels
/// (callers multiply by their indent width). Leading closers don't count: they dedent
/// the line itself, not the next one. A leading `else` counts as one level (its body is
/// deeper than the `else` line); `elseif` opens via its own trailing `then`.
pub fn indent_delta(lang: Language, line: &str) -> i32 {
    let (spans, _) = highlight_line(lang, line, LineState::default());
    let chars: Vec<char> = line.chars().collect();
    let mut delta = 0i32;
    let mut leading = true;
    for (start, end, kind) in spans {
        let text: String = chars[start..end].iter().collect();
        let (opener, closer) = classify(lang, &text, kind);
        if leading {
            if closer {
                continue;
            }
            if lang == Language::Lua && is_else(&text) {
                leading = false;
                if text == "else" {
                    delta += 1;
                }
                continue;
            }
            leading = false;
        }
        if opener {
            delta += 1;
        } else if closer {
            delta -= 1;
        }
    }
    delta
}

fn classify(lang: Language, text: &str, kind: SyntaxKind) -> (bool, bool) {
    match lang {
        Language::Lua => (is_opener(text, kind), is_closer(text, kind)),
        Language::Json => (
            kind == SyntaxKind::Punct && matches!(text, "{" | "["),
            kind == SyntaxKind::Punct && matches!(text, "}" | "]"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_lua_openers_indent_next_line() {
        assert_eq!(indent_delta(Language::Lua, "function foo()"), 1);
        assert_eq!(indent_delta(Language::Lua, "if x then"), 1);
        assert_eq!(indent_delta(Language::Lua, "for i=1,10 do"), 1);
        assert_eq!(indent_delta(Language::Lua, "repeat"), 1);
        assert_eq!(indent_delta(Language::Lua, "local t = {"), 1);
    }

    #[test]
    fn ut_lua_balanced_and_plain_lines_are_flat() {
        assert_eq!(indent_delta(Language::Lua, "print(x)"), 0);
        assert_eq!(indent_delta(Language::Lua, "if x then y() end"), 0);
        assert_eq!(indent_delta(Language::Lua, ""), 0);
    }

    #[test]
    fn ut_lua_leading_closer_does_not_dedent_next_line() {
        assert_eq!(indent_delta(Language::Lua, "end"), 0);
        assert_eq!(indent_delta(Language::Lua, "until done"), 0);
        assert_eq!(indent_delta(Language::Lua, "}"), 0);
    }

    #[test]
    fn ut_lua_else_and_elseif_indent_their_body() {
        assert_eq!(indent_delta(Language::Lua, "else"), 1);
        assert_eq!(indent_delta(Language::Lua, "elseif y then"), 1);
    }

    #[test]
    fn ut_lua_mid_line_closer_dedents_next_line() {
        assert_eq!(indent_delta(Language::Lua, "x() end"), -1);
    }

    #[test]
    fn ut_lua_keywords_in_strings_do_not_count() {
        assert_eq!(indent_delta(Language::Lua, "local s = \"function do then\""), 0);
        assert_eq!(indent_delta(Language::Lua, "-- if x then"), 0);
    }

    #[test]
    fn ut_json_braces_and_brackets() {
        assert_eq!(indent_delta(Language::Json, "{"), 1);
        assert_eq!(indent_delta(Language::Json, "\"a\": ["), 1);
        assert_eq!(indent_delta(Language::Json, "\"a\": 1,"), 0);
        assert_eq!(indent_delta(Language::Json, "},"), 0);
        assert_eq!(indent_delta(Language::Json, "\"a\": { \"b\": ["), 2);
        assert_eq!(indent_delta(Language::Json, "\"s\": \"{[\","), 0);
    }
}
