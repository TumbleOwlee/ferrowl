//! Reusable [ratatui] TUI building blocks for the ferrowl application.
//!
//! Widgets follow ratatui's stateful-widget pattern: each widget in
//! [`widgets`] renders from a corresponding state type in [`state`], styled
//! by [`style`]. Keyboard input flows through
//! [`HandleEvents`](traits::HandleEvents), returning an
//! [`EventResult`] so callers know whether a key was consumed.
//! [`AlternateScreen`] manages raw mode and the terminal's alternate screen.

mod border;
mod event_result;
mod screen;

pub mod state;
pub mod style;
pub mod traits;
pub mod widgets;
pub use border::Border;
pub use event_result::EventResult;
pub use screen::AlternateScreen;

use ratatui::style::Color;

/// The named colors making up the application's theme.
pub struct ColorScheme {
    pub text: Color,
    pub text_hi: Color,
    pub hi: Color,
    pub hi_bg: Color,
    pub bg: Color,
    pub border: Color,
    pub row: [Color; 2],
    pub placeholder: Color,
    pub error: Color,
    pub success: Color,
}

/// The fixed color scheme used by all widgets (Material 3 dark with warm taupe
/// surfaces and a fresh coral highlight).
pub const COLOR_SCHEME: ColorScheme = ColorScheme {
    text: Color::Rgb(240, 235, 229),    // warm white   #F0EBE5
    text_hi: Color::Rgb(240, 235, 229), // near-black   #1A1815
    hi: Color::Rgb(255, 138, 101),      // coral accent   #FF8A65 (deep orange 300)
    hi_bg: Color::Rgb(93, 64, 55),      // warm brown   #5D4037 (brown 700)
    bg: Color::Rgb(13, 13, 13),         // dark   #0d0d0d
    border: Color::Rgb(124, 109, 95),   // warm taupe   #7C6D5F
    row: [Color::Rgb(35, 32, 28), Color::Rgb(48, 44, 39)], // warm elevations
    placeholder: Color::Rgb(141, 126, 112), // muted clay   #8D7E70
    error: Color::Rgb(229, 115, 115),   // soft red   #E57373 (red 300)
    success: Color::Rgb(129, 199, 132), // fresh green   #81C784 (green 300)
};
