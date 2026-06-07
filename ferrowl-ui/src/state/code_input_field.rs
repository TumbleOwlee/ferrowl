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
        self.active_line = 0;
        self.cursor_col = 0;
        self.scroll_offset = 0;
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
