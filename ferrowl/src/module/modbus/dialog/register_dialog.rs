//! The unified register-edit surface shared by the two edit dialogs.
//!
//! `EditInputDialog` (typed free-text fields) and `EditSelectionDialog` (enum-like list pickers)
//! present the same surface to [`ModbusOverlay`](super::super::view), which holds whichever one is
//! open. [`SubDialogs`](super::SubDialogs) (a supertrait) already covers the add-value /
//! confirm-delete / name-error plumbing; this trait adds the render / focus / event / apply surface
//! so the overlay can drive either dialog through a single trait object.

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{buffer::Buffer, layout::Rect};

use super::{EditedRegister, SubDialogs};

/// Everything `ModbusOverlay` needs from the open edit dialog, on top of [`SubDialogs`].
pub trait RegisterDialog: SubDialogs {
    fn render(&mut self, area: Rect, buf: &mut Buffer);
    fn focus_next(&mut self);
    fn focus_previous(&mut self);
    /// Route a key to the focused field (the dialog's `HandleEvents` result is not surfaced here).
    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode);
    fn handle_space(&mut self);
    fn is_update_script_focused(&self) -> bool;
    fn is_confirm_button_focused(&self) -> bool;
    fn is_delete_register_button_focused(&self) -> bool;
    /// Validate the dialog and produce the edited register (or a user-facing error string).
    fn apply(&self) -> Result<EditedRegister, String>;
}
