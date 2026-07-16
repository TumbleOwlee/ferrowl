use crossterm::event::{KeyCode, KeyModifiers};
use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters};

use crate::EventResult;
use crate::traits::HandleEvents;
use crate::traits::IsFocus;
use crate::traits::SetFocus;
use crate::traits::ToLabel;
use crate::widgets::GetValue;

/// State of a [`Selection`](crate::widgets::Selection) list: the candidate
/// `values`, the selected index, and a horizontal scroll offset.
///
/// Handles vim-style (`hjkl`) and arrow-key navigation; vertical movement
/// wraps around.
#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters)]
#[getset(set = "pub")]
pub struct SelectionState<ValueType>
where
    ValueType: ToLabel + Clone,
{
    // getset's struct-level `set = "pub"` would otherwise generate an inherent
    // `set_focused` shadowing the `SetFocus` trait impl below.
    #[getset(skip)]
    #[builder(default = "true")]
    focused: bool,
    #[getset(get_copy = "pub")]
    #[builder(setter(skip))]
    selection: usize,
    #[getset(get_copy = "pub")]
    #[builder(setter(skip))]
    horizontal_offset: usize,
    #[getset(get = "pub")]
    values: Vec<ValueType>,
}

impl<ValueType> SelectionState<ValueType>
where
    ValueType: ToLabel + Clone,
{
    pub fn focused(&self) -> bool {
        self.focused
    }
}

impl<ValueType> SetFocus for SelectionState<ValueType>
where
    ValueType: ToLabel + Clone,
{
    fn set_focused(&mut self, focus: bool) {
        self.focused = focus;
    }
}

impl<ValueType> GetValue for SelectionState<ValueType>
where
    ValueType: ToLabel + Clone,
{
    type ValueType = ValueType;

    fn get_value(&self) -> Self::ValueType {
        self.values[self.selection].clone()
    }
}

impl<ValueType> IsFocus for SelectionState<ValueType>
where
    ValueType: ToLabel + Clone,
{
    fn is_focused(&self) -> bool {
        self.focused
    }
}

impl<ValueType> SelectionState<ValueType>
where
    ValueType: ToLabel + Clone,
{
    pub fn move_down(&mut self) {
        if self.values.is_empty() {
            return;
        }
        self.selection = if self.selection >= self.values.len() - 1 {
            0
        } else {
            self.selection + 1
        };
    }

    pub fn move_up(&mut self) {
        if self.values.is_empty() {
            return;
        }
        self.selection = if self.selection == 0 {
            self.values.len() - 1
        } else {
            self.selection - 1
        };
    }

    pub fn move_right(&mut self) {
        self.horizontal_offset += 1;
    }

    pub fn move_left(&mut self) {
        self.horizontal_offset -= if self.horizontal_offset > 0 { 1 } else { 0 };
    }

    pub fn values_mut(&mut self) -> &mut Vec<ValueType> {
        &mut self.values
    }
}

impl<ValueType> HandleEvents for SelectionState<ValueType>
where
    ValueType: ToLabel + Clone,
{
    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => {
                self.move_down();
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
                self.move_up();
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Char('h')) | (KeyModifiers::NONE, KeyCode::Left) => {
                self.move_left();
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Char('l')) | (KeyModifiers::NONE, KeyCode::Right) => {
                self.move_right();
                EventResult::Consumed
            }
            _ => EventResult::Unhandled(modifiers, code),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(n: usize) -> SelectionState<String> {
        let values = (0..n).map(|i| i.to_string()).collect::<Vec<_>>();
        SelectionStateBuilder::default()
            .values(values)
            .build()
            .unwrap()
    }

    #[test]
    /// UI-R-013 — j/Down moves the selection down; a selection list wraps at the end.
    fn move_down_advances_then_wraps() {
        let mut s = sel(3);
        assert_eq!(s.selection(), 0);
        s.move_down();
        assert_eq!(s.selection(), 1);
        s.move_down();
        assert_eq!(s.selection(), 2);
        s.move_down(); // wraps to top
        assert_eq!(s.selection(), 0);
    }

    #[test]
    /// UI-R-013 — k/Up moves the selection up; a selection list wraps to the bottom.
    fn move_up_wraps_to_bottom() {
        let mut s = sel(3);
        s.move_up(); // from 0 wraps to last
        assert_eq!(s.selection(), 2);
        s.move_up();
        assert_eq!(s.selection(), 1);
    }

    #[test]
    /// UI-R-013 — selection navigation is a safe no-op on an empty list.
    fn empty_values_navigation_is_noop_without_panic() {
        let mut s = sel(0);
        s.move_down();
        assert_eq!(s.selection(), 0);
        s.move_up();
        assert_eq!(s.selection(), 0);
    }

    #[test]
    /// UI-R-013 — h/l move the horizontal item; leftward clamps at zero.
    fn move_left_does_not_underflow_at_zero() {
        let mut s = sel(3);
        s.move_left();
        assert_eq!(s.horizontal_offset(), 0);
        s.move_right();
        assert_eq!(s.horizontal_offset(), 1);
        s.move_left();
        assert_eq!(s.horizontal_offset(), 0);
    }

    #[test]
    /// UI-R-013 — j/k/arrow keys dispatch selection navigation; an unrelated key is left unhandled.
    fn handle_events_dispatches_navigation() {
        let mut s = sel(3);
        s.handle_events(KeyModifiers::NONE, KeyCode::Char('j'));
        assert_eq!(s.selection(), 1);
        s.handle_events(KeyModifiers::NONE, KeyCode::Up);
        assert_eq!(s.selection(), 0);
        let r = s.handle_events(KeyModifiers::NONE, KeyCode::Char('z'));
        assert!(matches!(r, EventResult::Unhandled(..)));
    }
}
