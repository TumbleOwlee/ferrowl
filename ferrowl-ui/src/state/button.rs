use crossterm::event::{KeyCode, KeyModifiers};
use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters};

use crate::{
    EventResult,
    traits::{HandleEvents, IsFocus, SetFocus},
};

/// State of a [`Button`](crate::widgets::Button): label, focus, and
/// disabled flag. Buttons consume no keys themselves; activation is decided
/// by the surrounding view.
#[derive(Builder, Debug, Default, Clone, Getters, Setters, CopyGetters)]
#[getset(set = "pub")]
pub struct ButtonState {
    #[getset(get = "pub")]
    label: String,
    #[getset(get_copy = "pub")]
    #[builder(default = "true")]
    focused: bool,
    #[getset(get_copy = "pub")]
    #[builder(default = "false")]
    disabled: bool,
}

impl SetFocus for ButtonState {
    fn set_focused(&mut self, focus: bool) {
        self.focused = focus;
    }
}

impl IsFocus for ButtonState {
    fn is_focused(&self) -> bool {
        self.focused
    }
}

impl HandleEvents for ButtonState {
    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        EventResult::Unhandled(modifiers, code)
    }
}
