use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyModifiers};
use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters};

use crate::EventResult;
use crate::traits::{HandleEvents, IsFocus, SetFocus};

/// State of a multi-line [`CodeInputField`](crate::widgets::CodeInputField)
/// editor: line buffer, cursor (line + column), and scroll offsets.
#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters)]
#[getset(set = "pub")]
pub struct CodeInputFieldState {
    #[getset(get = "pub")]
    #[builder(default = "vec![String::new()]")]
    lines: Vec<String>,
    #[getset(get_copy = "pub")]
    #[builder(default = "0")]
    active_line: usize,
    #[getset(get_copy = "pub")]
    #[builder(default = "0")]
    cursor_col: usize,
    #[getset(get_copy = "pub")]
    #[builder(default = "0")]
    scroll_offset: usize,
    #[getset(get_copy = "pub")]
    #[builder(default = "0")]
    h_scroll: usize,
    // `set_focused` needs the format-on-blur side effect (see `impl SetFocus` below), so
    // getset's auto-generated setter is skipped here to avoid it shadowing the trait
    // method on direct calls; the getter is hand-written just below.
    #[getset(skip)]
    #[builder(default = "true")]
    focused: bool,
    #[getset(get_copy = "pub")]
    #[builder(default = "false")]
    disabled: bool,
    #[getset(get = "pub")]
    #[builder(default = "None")]
    placeholder: Option<String>,
    #[getset(get_copy = "pub")]
    #[builder(default = "Some(Duration::from_millis(300))")]
    space_indent: Option<Duration>,
    #[getset(skip)]
    #[builder(setter(skip), default)]
    last_space: Option<(Instant, usize, usize)>,
    #[getset(get_copy = "pub")]
    #[builder(default = "None")]
    language: Option<ferrowl_syntax::Language>,
}

impl CodeInputFieldState {
    /// Whether the field currently holds focus.
    pub fn focused(&self) -> bool {
        self.focused
    }

    /// Returns the full text with lines joined by `\n`.
    pub fn content(&self) -> String {
        self.lines.join("\n")
    }

    /// Replaces the full text, resetting scroll and placing the cursor at
    /// the end of the last line.
    pub fn set_content(&mut self, s: &str) {
        self.lines = s.split('\n').map(|l| l.to_string()).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        // Place cursor at end of last line so Backspace works immediately.
        self.active_line = self.lines.len() - 1;
        self.cursor_col = self.lines[self.active_line].chars().count();
        self.scroll_offset = 0;
        self.h_scroll = 0;
    }

    fn clamp_cursor(&mut self) {
        let line_len = self.lines[self.active_line].chars().count();
        if self.cursor_col > line_len {
            self.cursor_col = line_len;
        }
    }
}

impl SetFocus for CodeInputFieldState {
    fn set_focused(&mut self, focus: bool) {
        if self.focused
            && !focus
            && !self.disabled
            && let Some(lang) = self.language
            && let Some(new) = ferrowl_syntax::format(lang, &self.content())
            && new != self.content()
        {
            self.set_content(&new);
        }
        self.focused = focus;
    }
}

impl IsFocus for CodeInputFieldState {
    fn is_focused(&self) -> bool {
        self.focused
    }
}

impl HandleEvents for CodeInputFieldState {
    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        // The space-chord path manages `last_space` itself; every other
        // handled key clears it so a stray keypress between two spaces
        // cancels the pending double-space-indent expansion.
        if !matches!(
            (modifiers, code),
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(' '))
        ) {
            self.last_space = None;
        }
        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Up) => {
                if self.active_line > 0 {
                    self.active_line -= 1;
                    self.clamp_cursor();
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Down) => {
                if self.active_line + 1 < self.lines.len() {
                    self.active_line += 1;
                    self.clamp_cursor();
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Left) => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                } else if self.active_line > 0 {
                    self.active_line -= 1;
                    self.cursor_col = self.lines[self.active_line].chars().count();
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Right) => {
                let line_len = self.lines[self.active_line].chars().count();
                if self.cursor_col < line_len {
                    self.cursor_col += 1;
                } else if self.active_line + 1 < self.lines.len() {
                    self.active_line += 1;
                    self.cursor_col = 0;
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Enter) if !self.disabled => {
                let line = self.lines[self.active_line].clone();
                let chars: Vec<char> = line.chars().collect();
                let before: String = chars[..self.cursor_col].iter().collect();
                let after: String = chars[self.cursor_col..].iter().collect();
                // Auto-indent: inherit the split line's leading whitespace, one level
                // deeper/shallower per its net block balance (format-on-blur trues it up).
                let indent = match self.language {
                    Some(lang) => {
                        let lead = before.chars().take_while(|c| *c == ' ').count() as i32;
                        let delta = ferrowl_syntax::indent_delta(lang, &before);
                        (lead + 4 * delta).max(0) as usize
                    }
                    None => 0,
                };
                self.lines[self.active_line] = before;
                self.active_line += 1;
                self.lines
                    .insert(self.active_line, format!("{}{}", " ".repeat(indent), after));
                self.cursor_col = indent;
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Backspace) if !self.disabled => {
                if self.cursor_col > 0 {
                    let chars: Vec<char> = self.lines[self.active_line].chars().collect();
                    let new_line: String = chars
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| *i != self.cursor_col - 1)
                        .map(|(_, c)| *c)
                        .collect();
                    self.lines[self.active_line] = new_line;
                    self.cursor_col -= 1;
                } else if self.active_line > 0 {
                    let current = self.lines.remove(self.active_line);
                    self.active_line -= 1;
                    self.cursor_col = self.lines[self.active_line].chars().count();
                    self.lines[self.active_line].push_str(&current);
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Delete) if !self.disabled => {
                let line_len = self.lines[self.active_line].chars().count();
                if self.cursor_col < line_len {
                    let chars: Vec<char> = self.lines[self.active_line].chars().collect();
                    let new_line: String = chars
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| *i != self.cursor_col)
                        .map(|(_, c)| *c)
                        .collect();
                    self.lines[self.active_line] = new_line;
                } else if self.active_line + 1 < self.lines.len() {
                    let next = self.lines.remove(self.active_line + 1);
                    self.lines[self.active_line].push_str(&next);
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(' ')) if !self.disabled => {
                if let (Some(threshold), Some((at, line, col))) =
                    (self.space_indent, self.last_space)
                    && line == self.active_line
                    && col == self.cursor_col
                    && at.elapsed() <= threshold
                {
                    let chars: Vec<char> = self.lines[self.active_line].chars().collect();
                    let mut new_chars = Vec::with_capacity(chars.len() + 3);
                    new_chars.extend_from_slice(&chars[..self.cursor_col]);
                    new_chars.extend([' ', ' ', ' ']);
                    new_chars.extend_from_slice(&chars[self.cursor_col..]);
                    self.lines[self.active_line] = new_chars.into_iter().collect();
                    self.cursor_col += 3;
                    self.last_space = None;
                    return EventResult::Consumed;
                }
                let chars: Vec<char> = self.lines[self.active_line].chars().collect();
                let mut new_chars = Vec::with_capacity(chars.len() + 1);
                new_chars.extend_from_slice(&chars[..self.cursor_col]);
                new_chars.push(' ');
                new_chars.extend_from_slice(&chars[self.cursor_col..]);
                self.lines[self.active_line] = new_chars.into_iter().collect();
                self.cursor_col += 1;
                if self.space_indent.is_some() {
                    self.last_space = Some((Instant::now(), self.active_line, self.cursor_col));
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) if !self.disabled => {
                let chars: Vec<char> = self.lines[self.active_line].chars().collect();
                let mut new_chars = Vec::with_capacity(chars.len() + 1);
                new_chars.extend_from_slice(&chars[..self.cursor_col]);
                new_chars.push(c);
                new_chars.extend_from_slice(&chars[self.cursor_col..]);
                self.lines[self.active_line] = new_chars.into_iter().collect();
                self.cursor_col += 1;
                EventResult::Consumed
            }
            (m, c) => EventResult::Unhandled(m, c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> CodeInputFieldState {
        CodeInputFieldStateBuilder::default().build().unwrap()
    }

    fn press(s: &mut CodeInputFieldState, modifiers: KeyModifiers, code: KeyCode) {
        s.handle_events(modifiers, code);
    }

    fn type_char(s: &mut CodeInputFieldState, c: char) {
        press(s, KeyModifiers::NONE, KeyCode::Char(c));
    }

    fn backspace(s: &mut CodeInputFieldState) {
        press(s, KeyModifiers::NONE, KeyCode::Backspace);
    }

    #[test]
    fn type_and_delete() {
        let mut s = state();
        type_char(&mut s, 'a');
        type_char(&mut s, 'b');
        type_char(&mut s, 'c');
        assert_eq!(s.content(), "abc");
        assert_eq!(s.cursor_col(), 3);
        backspace(&mut s);
        assert_eq!(s.content(), "ab");
        assert_eq!(s.cursor_col(), 2);
        backspace(&mut s);
        assert_eq!(s.content(), "a");
        assert_eq!(s.cursor_col(), 1);
        backspace(&mut s);
        assert_eq!(s.content(), "");
        assert_eq!(s.cursor_col(), 0);
        backspace(&mut s);
        assert_eq!(s.content(), "");
        assert_eq!(s.cursor_col(), 0);
    }

    #[test]
    fn backspace_mid_line() {
        let mut s = state();
        type_char(&mut s, 'a');
        type_char(&mut s, 'b');
        type_char(&mut s, 'c');
        press(&mut s, KeyModifiers::NONE, KeyCode::Left);
        backspace(&mut s);
        assert_eq!(s.content(), "ac");
        assert_eq!(s.cursor_col(), 1);
    }

    #[test]
    fn backspace_merges_lines() {
        let mut s = state();
        type_char(&mut s, 'a');
        press(&mut s, KeyModifiers::NONE, KeyCode::Enter);
        type_char(&mut s, 'b');
        assert_eq!(s.content(), "a\nb");
        assert_eq!(s.active_line(), 1);
        press(&mut s, KeyModifiers::NONE, KeyCode::Left);
        backspace(&mut s);
        assert_eq!(s.content(), "ab");
        assert_eq!(s.active_line(), 0);
        assert_eq!(s.cursor_col(), 1);
    }

    #[test]
    fn delete_forward() {
        let mut s = state();
        type_char(&mut s, 'a');
        type_char(&mut s, 'b');
        type_char(&mut s, 'c');
        press(&mut s, KeyModifiers::NONE, KeyCode::Left);
        press(&mut s, KeyModifiers::NONE, KeyCode::Left);
        press(&mut s, KeyModifiers::NONE, KeyCode::Delete);
        assert_eq!(s.content(), "ac");
        assert_eq!(s.cursor_col(), 1);
        press(&mut s, KeyModifiers::NONE, KeyCode::Delete);
        assert_eq!(s.content(), "a");
        press(&mut s, KeyModifiers::NONE, KeyCode::Delete);
        assert_eq!(s.content(), "a");
    }

    #[test]
    fn delete_merges_next_line() {
        let mut s = state();
        type_char(&mut s, 'a');
        press(&mut s, KeyModifiers::NONE, KeyCode::Enter);
        type_char(&mut s, 'b');
        press(&mut s, KeyModifiers::NONE, KeyCode::Left);
        press(&mut s, KeyModifiers::NONE, KeyCode::Left);
        assert_eq!(s.active_line(), 0);
        assert_eq!(s.cursor_col(), 1);
        press(&mut s, KeyModifiers::NONE, KeyCode::Delete);
        assert_eq!(s.content(), "ab");
        assert_eq!(s.active_line(), 0);
        assert_eq!(s.cursor_col(), 1);
    }

    #[test]
    fn set_content_cursor_at_end() {
        let mut s = state();
        s.set_content("hello\nworld");
        assert_eq!(s.active_line(), 1);
        assert_eq!(s.cursor_col(), 5);
        backspace(&mut s);
        assert_eq!(s.content(), "hello\nworl");
        assert_eq!(s.cursor_col(), 4);
    }

    #[test]
    fn up_down_navigate_and_clamp_to_shorter_line() {
        let mut s = state();
        s.set_content("longline\nx");
        // Cursor sits at end of "x" (line 1, col 1). Move up onto "longline".
        press(&mut s, KeyModifiers::NONE, KeyCode::Up);
        assert_eq!(s.active_line(), 0);
        assert_eq!(s.cursor_col(), 1);
        // Move back down; cursor clamps to the shorter line's length.
        press(&mut s, KeyModifiers::NONE, KeyCode::Down);
        assert_eq!(s.active_line(), 1);
        assert_eq!(s.cursor_col(), 1);
    }

    #[test]
    fn up_at_first_line_and_down_at_last_are_noops() {
        let mut s = state();
        s.set_content("a\nb");
        press(&mut s, KeyModifiers::NONE, KeyCode::Down); // already last line
        assert_eq!(s.active_line(), 1);
        press(&mut s, KeyModifiers::NONE, KeyCode::Up);
        assert_eq!(s.active_line(), 0);
        press(&mut s, KeyModifiers::NONE, KeyCode::Up); // already first line
        assert_eq!(s.active_line(), 0);
    }

    #[test]
    fn left_wraps_to_previous_line_end() {
        let mut s = state();
        s.set_content("ab\ncd");
        // At line 1, col 0 (move there from end).
        press(&mut s, KeyModifiers::NONE, KeyCode::Left); // col 2 -> 1
        press(&mut s, KeyModifiers::NONE, KeyCode::Left); // col 1 -> 0
        press(&mut s, KeyModifiers::NONE, KeyCode::Left); // wrap to prev line end
        assert_eq!(s.active_line(), 0);
        assert_eq!(s.cursor_col(), 2);
    }

    #[test]
    fn right_wraps_to_next_line_start() {
        let mut s = state();
        s.set_content("ab\ncd");
        press(&mut s, KeyModifiers::NONE, KeyCode::Up); // line 0, clamp col to 2
        // cursor is at col 2 (end of "ab"); Right wraps to next line start.
        press(&mut s, KeyModifiers::NONE, KeyCode::Right);
        assert_eq!(s.active_line(), 1);
        assert_eq!(s.cursor_col(), 0);
    }

    #[test]
    fn enter_splits_line_at_cursor() {
        let mut s = state();
        s.set_content("abcd");
        press(&mut s, KeyModifiers::NONE, KeyCode::Left);
        press(&mut s, KeyModifiers::NONE, KeyCode::Left); // cursor at col 2
        press(&mut s, KeyModifiers::NONE, KeyCode::Enter);
        assert_eq!(s.content(), "ab\ncd");
        assert_eq!(s.active_line(), 1);
        assert_eq!(s.cursor_col(), 0);
    }

    #[test]
    fn multibyte_chars_count_by_character() {
        let mut s = state();
        type_char(&mut s, 'é');
        type_char(&mut s, '语');
        type_char(&mut s, 'x');
        assert_eq!(s.content(), "é语x");
        assert_eq!(s.cursor_col(), 3);
        backspace(&mut s);
        assert_eq!(s.content(), "é语");
        assert_eq!(s.cursor_col(), 2);
        // Insert between the two multi-byte chars.
        press(&mut s, KeyModifiers::NONE, KeyCode::Left);
        type_char(&mut s, 'Z');
        assert_eq!(s.content(), "éZ语");
        assert_eq!(s.cursor_col(), 2);
    }

    #[test]
    fn disabled_state_ignores_keys() {
        let mut s = CodeInputFieldStateBuilder::default()
            .disabled(true)
            .build()
            .unwrap();
        let r = s.handle_events(KeyModifiers::NONE, KeyCode::Char('a'));
        assert!(matches!(r, EventResult::Unhandled(..)));
        assert_eq!(s.content(), "");
    }

    #[test]
    fn double_space_expands_to_four_within_threshold() {
        let mut s = CodeInputFieldStateBuilder::default()
            .space_indent(Some(Duration::from_secs(600)))
            .build()
            .unwrap();
        type_char(&mut s, ' ');
        type_char(&mut s, ' ');
        assert_eq!(s.content(), "    ");
        assert_eq!(s.cursor_col(), 4);
    }

    #[test]
    fn intervening_key_between_spaces_cancels_expansion() {
        let mut s = CodeInputFieldStateBuilder::default()
            .space_indent(Some(Duration::from_secs(600)))
            .build()
            .unwrap();
        type_char(&mut s, ' ');
        press(&mut s, KeyModifiers::NONE, KeyCode::Left);
        press(&mut s, KeyModifiers::NONE, KeyCode::Right);
        type_char(&mut s, ' ');
        assert_eq!(s.content(), "  ");
        assert_eq!(s.cursor_col(), 2);
    }

    #[test]
    fn space_indent_disabled_inserts_plain_spaces() {
        let mut s = CodeInputFieldStateBuilder::default()
            .space_indent(None)
            .build()
            .unwrap();
        type_char(&mut s, ' ');
        type_char(&mut s, ' ');
        assert_eq!(s.content(), "  ");
        assert_eq!(s.cursor_col(), 2);
    }

    #[test]
    fn double_space_expansion_mid_line() {
        let mut s = CodeInputFieldStateBuilder::default()
            .space_indent(Some(Duration::from_secs(600)))
            .build()
            .unwrap();
        s.set_content("ab");
        s.set_cursor_col(1);
        type_char(&mut s, ' ');
        type_char(&mut s, ' ');
        assert_eq!(s.content(), "a    b");
        assert_eq!(s.cursor_col(), 5);
    }

    #[test]
    fn four_rapid_space_presses_yield_eight_spaces() {
        let mut s = CodeInputFieldStateBuilder::default()
            .space_indent(Some(Duration::from_secs(600)))
            .build()
            .unwrap();
        type_char(&mut s, ' ');
        type_char(&mut s, ' ');
        type_char(&mut s, ' ');
        type_char(&mut s, ' ');
        assert_eq!(s.content(), "        ");
        assert_eq!(s.cursor_col(), 8);
    }

    #[test]
    fn blur_formats_messy_json() {
        let mut s = CodeInputFieldStateBuilder::default()
            .language(Some(ferrowl_syntax::Language::Json))
            .build()
            .unwrap();
        s.set_content(r#"{"b":1,"a":[1,2]}"#);
        s.set_focused(false);
        assert_eq!(
            s.content(),
            "{\n  \"b\": 1,\n  \"a\": [\n    1,\n    2\n  ]\n}"
        );
    }

    #[test]
    fn blur_leaves_invalid_json_unchanged() {
        let mut s = CodeInputFieldStateBuilder::default()
            .language(Some(ferrowl_syntax::Language::Json))
            .build()
            .unwrap();
        s.set_content("{\"a\": ");
        s.set_focused(false);
        assert_eq!(s.content(), "{\"a\": ");
    }

    #[test]
    fn disabled_field_never_formats_on_blur() {
        let mut s = CodeInputFieldStateBuilder::default()
            .language(Some(ferrowl_syntax::Language::Json))
            .disabled(true)
            .build()
            .unwrap();
        s.set_content(r#"{"b":1,"a":2}"#);
        s.set_focused(false);
        assert_eq!(s.content(), r#"{"b":1,"a":2}"#);
    }

    #[test]
    fn gaining_focus_never_formats() {
        let mut s = CodeInputFieldStateBuilder::default()
            .language(Some(ferrowl_syntax::Language::Json))
            .focused(false)
            .build()
            .unwrap();
        s.set_content(r#"{"b":1,"a":2}"#);
        s.set_focused(true);
        assert_eq!(s.content(), r#"{"b":1,"a":2}"#);
    }

    #[test]
    fn enter_auto_indents_after_lua_opener() {
        let mut s = CodeInputFieldStateBuilder::default()
            .language(Some(ferrowl_syntax::Language::Lua))
            .build()
            .unwrap();
        s.set_content("function foo()");
        press(&mut s, KeyModifiers::NONE, KeyCode::Enter);
        assert_eq!(s.content(), "function foo()\n    ");
        assert_eq!(s.cursor_col(), 4);
    }

    #[test]
    fn enter_inherits_indent_on_plain_line() {
        let mut s = CodeInputFieldStateBuilder::default()
            .language(Some(ferrowl_syntax::Language::Lua))
            .build()
            .unwrap();
        s.set_content("function foo()\n    print(1)");
        press(&mut s, KeyModifiers::NONE, KeyCode::Enter);
        assert_eq!(s.content(), "function foo()\n    print(1)\n    ");
        assert_eq!(s.cursor_col(), 4);
    }

    #[test]
    fn enter_does_not_indent_after_closing_line() {
        let mut s = CodeInputFieldStateBuilder::default()
            .language(Some(ferrowl_syntax::Language::Lua))
            .build()
            .unwrap();
        s.set_content("end");
        press(&mut s, KeyModifiers::NONE, KeyCode::Enter);
        assert_eq!(s.content(), "end\n");
        assert_eq!(s.cursor_col(), 0);
    }

    #[test]
    fn enter_auto_indents_json_and_carries_tail() {
        let mut s = CodeInputFieldStateBuilder::default()
            .language(Some(ferrowl_syntax::Language::Json))
            .build()
            .unwrap();
        // Cursor between `{` and `}`: the tail moves to the new, indented line.
        s.set_content("{}");
        s.set_cursor_col(1);
        press(&mut s, KeyModifiers::NONE, KeyCode::Enter);
        assert_eq!(s.content(), "{\n    }");
        assert_eq!(s.cursor_col(), 4);
    }

    #[test]
    fn enter_without_language_does_not_indent() {
        let mut s = state();
        s.set_content("    x {");
        press(&mut s, KeyModifiers::NONE, KeyCode::Enter);
        assert_eq!(s.content(), "    x {\n");
        assert_eq!(s.cursor_col(), 0);
    }

    #[test]
    fn blur_reindents_lua() {
        let mut s = CodeInputFieldStateBuilder::default()
            .language(Some(ferrowl_syntax::Language::Lua))
            .build()
            .unwrap();
        s.set_content("function foo()\nprint(1)\nend");
        s.set_focused(false);
        assert_eq!(s.content(), "function foo()\n    print(1)\nend");
    }
}
