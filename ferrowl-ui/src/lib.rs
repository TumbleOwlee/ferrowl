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

use ratatui::style::{Color, palette::tailwind};

/// The named colors making up the application's theme.
pub struct ColorScheme {
    pub text: Color,
    pub text_dark: Color,
    pub hi: Color,
    pub hi_bg: Color,
    pub bg: Color,
    pub border: Color,
    pub row: [Color; 2],
    pub placeholder: Color,
    pub error: Color,
    pub success: Color,
}

/// The fixed color scheme used by all widgets (dark background, amber
/// highlights).
pub const COLOR_SCHEME: ColorScheme = ColorScheme {
    text: Color::White,                // white   #FFFFFF
    text_dark: Color::Black,           // black   #000000
    hi: Color::Rgb(196, 154, 99),      // amber gold  #C49A63
    hi_bg: Color::Rgb(155, 121, 73),   // mid amber   #5A4618
    bg: Color::Rgb(13, 16, 20),        // deep dark   #0D1014
    border: Color::Rgb(156, 138, 114), // warm stone  #9C8A72
    row: [Color::Rgb(27, 32, 37), Color::Rgb(39, 44, 50)],
    placeholder: Color::Rgb(110, 95, 78), // dim clay   #6E5F4E
    error: tailwind::RED.c500,
    success: Color::Rgb(143, 179, 154), // sage green  #8FB39A
};
