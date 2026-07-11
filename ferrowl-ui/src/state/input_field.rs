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
    // getset's struct-level `set = "pub"` would otherwise generate an inherent
    // `set_focused` shadowing the `SetFocus` trait impl below.
    #[getset(skip)]
    #[builder(default = "true")]
    focused: bool,
    #[getset(get_copy = "pub")]
    #[builder(default = "false")]
    disabled: bool,
    #[getset(get = "pub")]
    #[builder(default = "None")]
    placeholder: Option<String>,
    #[getset(get = "pub")]
    #[builder(default = "None")]
    autofill: Option<String>,
    /// Keystroke filter; `None` allows all characters. Applies only to typed
    /// characters — autofill (Ctrl+F) and programmatic `set_input` bypass it.
    #[builder(default = "None")]
    allowed: Option<fn(char) -> bool>,
}

impl InputFieldStateBuilder {
    /// Restrict typeable characters to those allowed by validator `V`.
    pub fn allowed_for<V: crate::widgets::Validate>(&mut self) -> &mut Self {
        self.allowed(Some(V::allowed_char as fn(char) -> bool))
    }
}

impl GetValue for InputFieldState {
    type ValueType = String;

    fn get_value(&self) -> Self::ValueType {
        self.input.clone()
    }
}

impl InputFieldState {
    pub fn focused(&self) -> bool {
        self.focused
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
            (KeyModifiers::CONTROL, KeyCode::Char('f')) if !self.disabled => {
                if self.input.is_empty()
                    && let Some(autofill) = &self.autofill()
                {
                    self.input = autofill.clone();
                    self.cursor = self.input.chars().count();
                }
                EventResult::Consumed
            }
            // Clear the input line
            (KeyModifiers::CONTROL, KeyCode::Char('d')) if !self.disabled => {
                self.input.clear();
                self.cursor = 0;
                EventResult::Consumed
            }
            // Accept SHIFT so capital letters and shifted symbols are inserted; CTRL/ALT
            // combinations are left unhandled for app-level shortcuts.
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) if !self.disabled => {
                // Always consume, even if `c` is rejected below: a focused field must
                // swallow typing so disallowed chars don't leak into app-level shortcuts.
                #[allow(clippy::collapsible_if)]
                if self.allowed.is_none_or(|f| f(c)) {
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
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Backspace) if !self.disabled => {
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
            (KeyModifiers::NONE, KeyCode::Delete) if !self.disabled => {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn field() -> InputFieldState {
        InputFieldStateBuilder::default().build().unwrap()
    }

    fn type_str(s: &mut InputFieldState, text: &str) {
        for c in text.chars() {
            s.handle_events(KeyModifiers::NONE, KeyCode::Char(c));
        }
    }

    #[test]
    fn typing_appends_and_advances_cursor() {
        let mut s = field();
        type_str(&mut s, "abc");
        assert_eq!(s.input(), "abc");
        assert_eq!(s.cursor(), 3);
    }

    #[test]
    fn insert_at_cursor_mid_string() {
        let mut s = field();
        type_str(&mut s, "ac");
        s.handle_events(KeyModifiers::NONE, KeyCode::Left); // cursor between a|c
        s.handle_events(KeyModifiers::NONE, KeyCode::Char('b'));
        assert_eq!(s.input(), "abc");
        assert_eq!(s.cursor(), 2);
    }

    #[test]
    fn backspace_at_start_is_noop() {
        let mut s = field();
        type_str(&mut s, "ab");
        s.handle_events(KeyModifiers::NONE, KeyCode::Home);
        s.handle_events(KeyModifiers::NONE, KeyCode::Backspace);
        assert_eq!(s.input(), "ab");
        assert_eq!(s.cursor(), 0);
    }

    #[test]
    fn delete_removes_char_under_cursor() {
        let mut s = field();
        type_str(&mut s, "abc");
        s.handle_events(KeyModifiers::NONE, KeyCode::Home);
        s.handle_events(KeyModifiers::NONE, KeyCode::Delete);
        assert_eq!(s.input(), "bc");
        assert_eq!(s.cursor(), 0);
    }

    #[test]
    fn home_and_end_move_cursor_to_bounds() {
        let mut s = field();
        type_str(&mut s, "hello");
        s.handle_events(KeyModifiers::NONE, KeyCode::Home);
        assert_eq!(s.cursor(), 0);
        s.handle_events(KeyModifiers::NONE, KeyCode::End);
        assert_eq!(s.cursor(), 5);
    }

    #[test]
    fn left_right_clamp_at_bounds() {
        let mut s = field();
        type_str(&mut s, "ab");
        s.handle_events(KeyModifiers::NONE, KeyCode::Right); // already at end
        assert_eq!(s.cursor(), 2);
        s.handle_events(KeyModifiers::NONE, KeyCode::Left);
        s.handle_events(KeyModifiers::NONE, KeyCode::Left);
        s.handle_events(KeyModifiers::NONE, KeyCode::Left); // clamp at 0
        assert_eq!(s.cursor(), 0);
    }

    #[test]
    fn multibyte_backspace_removes_one_character() {
        let mut s = field();
        type_str(&mut s, "aé语");
        assert_eq!(s.cursor(), 3);
        s.handle_events(KeyModifiers::NONE, KeyCode::Backspace);
        assert_eq!(s.input(), "aé");
        assert_eq!(s.cursor(), 2);
    }

    #[test]
    fn ctrl_f_fills_empty_field_from_autofill() {
        let mut s = InputFieldStateBuilder::default()
            .autofill(Some("default".to_string()))
            .build()
            .unwrap();
        s.handle_events(KeyModifiers::CONTROL, KeyCode::Char('f'));
        assert_eq!(s.input(), "default");
        assert_eq!(s.cursor(), 7);
    }

    #[test]
    fn ctrl_f_does_not_overwrite_non_empty_field() {
        let mut s = InputFieldStateBuilder::default()
            .autofill(Some("default".to_string()))
            .build()
            .unwrap();
        type_str(&mut s, "x");
        s.handle_events(KeyModifiers::CONTROL, KeyCode::Char('f'));
        assert_eq!(s.input(), "x");
    }

    #[test]
    fn ctrl_d_clears_the_input() {
        let mut s = field();
        type_str(&mut s, "abc");
        s.handle_events(KeyModifiers::CONTROL, KeyCode::Char('d'));
        assert_eq!(s.input(), "");
        assert_eq!(s.cursor(), 0);
    }

    #[test]
    fn disabled_field_ignores_typing() {
        let mut s = InputFieldStateBuilder::default()
            .disabled(true)
            .build()
            .unwrap();
        type_str(&mut s, "abc");
        assert_eq!(s.input(), "");
    }

    #[test]
    fn allowed_for_filters_disallowed_chars() {
        let mut s = InputFieldStateBuilder::default()
            .allowed_for::<u32>()
            .build()
            .unwrap();
        type_str(&mut s, "a1b2");
        assert_eq!(s.input(), "12");
        assert_eq!(s.cursor(), 2);
    }

    #[test]
    fn rejected_char_still_returns_consumed() {
        let mut s = InputFieldStateBuilder::default()
            .allowed_for::<u32>()
            .build()
            .unwrap();
        let result = s.handle_events(KeyModifiers::NONE, KeyCode::Char('a'));
        assert!(matches!(result, EventResult::Consumed));
        assert_eq!(s.input(), "");
    }

    #[test]
    fn allowed_filter_does_not_affect_autofill() {
        let mut s = InputFieldStateBuilder::default()
            .allowed_for::<u32>()
            .autofill(Some("abc".to_string()))
            .build()
            .unwrap();
        s.handle_events(KeyModifiers::CONTROL, KeyCode::Char('f'));
        assert_eq!(s.input(), "abc");
    }

    #[test]
    fn allowed_filter_does_not_affect_ctrl_d_backspace_delete_navigation() {
        let mut s = InputFieldStateBuilder::default()
            .allowed_for::<u32>()
            .build()
            .unwrap();
        type_str(&mut s, "12");
        s.handle_events(KeyModifiers::NONE, KeyCode::Home);
        assert_eq!(s.cursor(), 0);
        s.handle_events(KeyModifiers::NONE, KeyCode::End);
        assert_eq!(s.cursor(), 2);
        s.handle_events(KeyModifiers::NONE, KeyCode::Left);
        s.handle_events(KeyModifiers::NONE, KeyCode::Delete);
        assert_eq!(s.input(), "1");
        s.handle_events(KeyModifiers::NONE, KeyCode::End);
        s.handle_events(KeyModifiers::NONE, KeyCode::Backspace);
        assert_eq!(s.input(), "");
        type_str(&mut s, "12");
        s.handle_events(KeyModifiers::CONTROL, KeyCode::Char('d'));
        assert_eq!(s.input(), "");
    }

    #[test]
    fn set_input_bypasses_allowed_filter() {
        let mut s = InputFieldStateBuilder::default()
            .allowed_for::<u32>()
            .build()
            .unwrap();
        s.set_input("abc".to_string());
        assert_eq!(s.input(), "abc");
    }

    #[test]
    fn shift_modified_char_respects_allowed_filter() {
        let mut s = InputFieldStateBuilder::default()
            .allowed_for::<u32>()
            .build()
            .unwrap();
        s.handle_events(KeyModifiers::SHIFT, KeyCode::Char('A'));
        assert_eq!(s.input(), "");
        s.handle_events(KeyModifiers::SHIFT, KeyCode::Char('5'));
        assert_eq!(s.input(), "5");
    }
}
