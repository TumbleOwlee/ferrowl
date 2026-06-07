use crossterm::event::{KeyCode, KeyModifiers};
use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters};

use crate::EventResult;
use crate::traits::{HandleEvents, IsFocus, SetFocus};

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
    #[getset(get_copy = "pub")]
    #[builder(default = "true")]
    focused: bool,
    #[getset(get_copy = "pub")]
    #[builder(default = "false")]
    disabled: bool,
    #[getset(get = "pub")]
    #[builder(default = "None")]
    placeholder: Option<String>,
}

impl CodeInputFieldState {
    pub fn content(&self) -> String {
        self.lines.join("\n")
    }

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
        if self.disabled {
            return EventResult::Unhandled(modifiers, code);
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
            (KeyModifiers::NONE, KeyCode::Enter) => {
                let line = self.lines[self.active_line].clone();
                let chars: Vec<char> = line.chars().collect();
                let before: String = chars[..self.cursor_col].iter().collect();
                let after: String = chars[self.cursor_col..].iter().collect();
                self.lines[self.active_line] = before;
                self.active_line += 1;
                self.lines.insert(self.active_line, after);
                self.cursor_col = 0;
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Backspace) => {
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
            (KeyModifiers::NONE, KeyCode::Delete) => {
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
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
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
}
