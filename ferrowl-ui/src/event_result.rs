//! Outcome of offering a key event to a widget.

use crossterm::event::{KeyCode, KeyModifiers};

/// Outcome of offering a key event to a widget.
#[derive(Debug)]
pub enum EventResult {
    /// The widget handled the key.
    Consumed,
    /// The widget ignored the key; it is returned for the caller to handle.
    Unhandled(KeyModifiers, KeyCode),
}
