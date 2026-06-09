use crossterm::event::{KeyCode, KeyModifiers};
use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters};

use crate::EventResult;
use crate::traits::{HandleEvents, IsFocus, SetFocus};
use crate::widgets::GetValue;

/// State of a single-line [`InputField`](crate::widgets::InputField): text,
/// cursor position (in characters), focus/disabled flags, and an optional
/// placeholder.
///
/// Handles character insertion, Backspace/Delete, Home/End, cursor
/// movement, and Ctrl+F to fill an empty field from its placeholder.
#[derive(Builder, Debug, Default, Clone, Getters, Setters, CopyGetters)]
#[getset(set = "pub")]
pub struct InputFieldState {
    #[getset(get = "pub")]
    #[builder(default = "String::new()")]
    input: String,
    #[getset(get_copy = "pub")]
    #[builder(default = "0")]
    cursor: usize,
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

impl GetValue for InputFieldState {
    type ValueType = String;

    fn get_value(&self) -> Self::ValueType {
        self.input.clone()
    }
}

impl SetFocus for InputFieldState {
    fn set_focused(&mut self, focus: bool) {
        self.focused = focus;
    }
}

impl IsFocus for InputFieldState {
    fn is_focused(&self) -> bool {
        self.focused
    }
}

impl HandleEvents for InputFieldState {
    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Home) => {
                self.cursor = 0;
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::End) => {
                self.cursor = self.input.chars().count();
                EventResult::Consumed
            }
            // Fill an empty field from its placeholder, so the current value doesn't have to be
            // cleared before entering a new one.
            (KeyModifiers::CONTROL, KeyCode::Char('f')) => {
                if !self.disabled
                    && self.input.is_empty()
                    && let Some(placeholder) = &self.placeholder
                {
                    self.input = placeholder.clone();
                    self.cursor = self.input.chars().count();
                }
                EventResult::Consumed
            }
            // Accept SHIFT so capital letters and shifted symbols are inserted; CTRL/ALT
            // combinations are left unhandled for app-level shortcuts.
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                if !self.disabled {
                    if self.input.is_empty() || self.input.chars().count() == self.cursor {
                        self.input.push(c);
                    } else {
                        self.input = self.input.chars().enumerate().fold(
                            String::with_capacity(self.input.capacity() + 1),
                            |mut s, (i, v)| {
                                if i == self.cursor {
                                    s.push(c);
                                }
                                s.push(v);
                                s
                            },
                        );
                    }
                    self.cursor += 1;
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                if !self.disabled && self.cursor > 0 {
                    if self.input.chars().count() >= self.cursor {
                        self.input = self.input.chars().enumerate().fold(
                            String::with_capacity(self.input.capacity() + 1),
                            |mut s, (i, v)| {
                                if i != self.cursor - 1 {
                                    s.push(v);
                                }
                                s
                            },
                        );
                    }
                    self.cursor -= 1;
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Delete) => {
                if !self.disabled && self.input.chars().count() > self.cursor {
                    self.input = self.input.chars().enumerate().fold(
                        String::with_capacity(self.input.capacity() + 1),
                        |mut s, (i, v)| {
                            if i != self.cursor {
                                s.push(v);
                            }
                            s
                        },
                    );
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Left) => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Right) => {
                self.cursor = std::cmp::min(self.cursor + 1, self.input.chars().count());
                EventResult::Consumed
            }
            (m, c) => EventResult::Unhandled(m, c),
        }
    }
}
