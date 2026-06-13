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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_button_state_focus_toggles() {
        let mut s = ButtonStateBuilder::default().label("ok".to_string()).build().unwrap();
        assert!(s.is_focused()); // focused defaults to true
        // Call the trait method explicitly: getset also generates an inherent `set_focused`
        // that would otherwise shadow the `SetFocus` impl.
        SetFocus::set_focused(&mut s, false);
        assert!(!IsFocus::is_focused(&s));
        assert_eq!(s.label(), "ok");
        assert!(!s.disabled());
    }

    #[test]
    fn ut_button_state_never_consumes_keys() {
        let mut s = ButtonState::default();
        let r = s.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(matches!(r, EventResult::Unhandled(..)));
    }
}
