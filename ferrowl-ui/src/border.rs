//! Whether a widget draws a border and the margin it occupies.

use ratatui::layout::Margin;

/// Whether a widget draws a border, and the margin it occupies if so.
#[derive(Debug, Clone)]
pub enum Border {
    None,
    Full(Margin),
}
