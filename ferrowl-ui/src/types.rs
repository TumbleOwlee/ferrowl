//! Shared plain types used across widgets.

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::Margin;

/// Outcome of offering a key event to a widget.
#[derive(Debug)]
pub enum EventResult {
    /// The widget handled the key.
    Consumed,
    /// The widget ignored the key; it is returned for the caller to handle.
    Unhandled(KeyModifiers, KeyCode),
}

/// Whether a widget draws a border, and the margin it occupies if so.
#[derive(Debug, Clone)]
pub enum Border {
    None,
    Full(Margin),
}
