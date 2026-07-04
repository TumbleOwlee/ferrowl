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

/// The fixed color scheme used by all widgets (dark theme with plum-tinted
/// surfaces and a wine-red highlight).
pub const COLOR_SCHEME: ColorScheme = ColorScheme {
    text: Color::Rgb(237, 232, 234),    // soft white   #EDE8EA
    text_hi: Color::Rgb(245, 237, 239), // rosy white   #F5EDEF
    hi: Color::Rgb(195, 79, 99),        // wine red   #C34F63
    hi_bg: Color::Rgb(92, 36, 48),      // deep bordeaux   #5C2430
    bg: Color::Rgb(18, 13, 15),         // plum black   #120D0F
    border: Color::Rgb(125, 106, 112),  // muted mauve   #7D6A70
    row: [Color::Rgb(35, 29, 31), Color::Rgb(47, 40, 43)], // plum elevations
    placeholder: Color::Rgb(140, 120, 126), // rose gray   #8C787E
    error: Color::Rgb(224, 108, 91),    // soft vermilion   #E06C5B
    success: Color::Rgb(129, 199, 132), // fresh green   #81C784 (green 300)
};
