//! Integration coverage for `ferrowl-syntax`'s public API: [`format`] (source re-indenting,
//! or `None` when the input can't be safely reformatted) and [`indent_delta`] (the block-depth
//! hint an editor uses on Enter). Driven over public paths, as the TUI editor widget does.

use ferrowl_syntax::{Language, format, indent_delta};

#[test]
/// UI-R-033 — format declines on invalid JSON so the buffer is left unchanged.
fn it_format_returns_none_for_invalid_json() {
    assert_eq!(
        format(Language::Json, "{ not valid json "),
        None,
        "unparseable JSON must be left untouched (None), never mangled"
    );
}

#[test]
/// UI-R-033 — format reformats valid JSON and is idempotent.
fn it_format_reformats_valid_json_and_is_idempotent() {
    let once =
        format(Language::Json, "{\"b\":2,\"a\":[1,2]}").expect("valid JSON reformats to Some");
    assert!(once.contains('\n'), "pretty output spans multiple lines");
    let twice = format(Language::Json, &once).expect("reformatted JSON is still valid");
    assert_eq!(once, twice, "formatting is idempotent");
}

#[test]
/// UI-R-033 — the Lua formatter always returns a formatted buffer.
fn it_format_lua_always_returns_some() {
    // Lua re-indenting never rejects: the caller always gets a buffer back.
    assert!(format(Language::Lua, "local x=1\nif x then\nprint(x)\nend").is_some());
}

#[test]
/// UI-R-032 — a Lua opener's indent delta deepens the next line.
fn it_indent_delta_lua_opener_deepens_next_line() {
    assert_eq!(indent_delta(Language::Lua, "function foo()"), 1);
    assert_eq!(indent_delta(Language::Lua, "if cond then"), 1);
}

#[test]
/// UI-R-032 — a leading closer does not shift the next line's indent.
fn it_indent_delta_leading_closer_does_not_shift_next_line() {
    // A leading closer dedents its own line, not the following one, so its net delta is zero.
    assert_eq!(indent_delta(Language::Lua, "end"), 0);
    assert_eq!(indent_delta(Language::Json, "}"), 0);
}

#[test]
/// UI-R-032 — a JSON open bracket's indent delta deepens the next line.
fn it_indent_delta_json_open_bracket_deepens() {
    assert_eq!(indent_delta(Language::Json, "{"), 1);
    assert_eq!(indent_delta(Language::Json, "["), 1);
}

#[test]
/// UI-R-032 — indent delta ignores block words appearing inside strings.
fn it_indent_delta_ignores_block_words_inside_strings() {
    // The lexers classify `if`/`then`/`end` inside a string literal as string text, so they
    // must not count as block openers/closers.
    assert_eq!(
        indent_delta(Language::Lua, "local s = \"if x then end\""),
        0
    );
}
