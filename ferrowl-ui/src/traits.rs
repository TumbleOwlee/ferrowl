//! Common traits implemented by widget states and views.

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::Margin;
use std::io::{Stderr, Stdout, stderr, stdout};

use crate::EventResult;

/// Receives a key event and reports whether it was consumed.
pub trait HandleEvents {
    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult;
}

/// Constructs the output stream an
/// [`AlternateScreen`](crate::AlternateScreen) writes to (stdout or stderr).
pub trait Init {
    fn init() -> Self;
}

impl Init for Stdout {
    fn init() -> Self {
        stdout()
    }
}

impl Init for Stderr {
    fn init() -> Self {
        stderr()
    }
}

/// Converts a value into the label text a widget displays for it.
pub trait ToLabel {
    fn to_label(&self) -> String;
}

impl ToLabel for String {
    fn to_label(&self) -> String {
        self.clone()
    }
}

impl ToLabel for &str {
    fn to_label(&self) -> String {
        self.to_string()
    }
}

/// Sets whether a widget has keyboard focus.
pub trait SetFocus {
    fn set_focused(&mut self, focus: bool);
}

/// Queries whether a widget has keyboard focus.
pub trait IsFocus {
    fn is_focused(&self) -> bool;
}

/// The margin a widget reserves around its content (e.g. for borders).
pub trait Margins {
    fn margins(&self) -> Margin;
}
