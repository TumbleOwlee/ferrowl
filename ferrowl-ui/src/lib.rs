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

/// Colors for syntax highlighting, keyed by [`SyntaxKind`](ferrowl_syntax::SyntaxKind) category.
pub struct SyntaxColorScheme {
    pub keyword: Color,
    pub ident: Color,
    pub number: Color,
    pub string: Color,
    pub comment: Color,
    pub punct: Color,
    pub key: Color,
    pub literal: Color,
    pub object: Color,
    pub function: Color,
}

/// The named colors making up the application's theme.
pub struct ColorScheme {
    pub text: Color,
    pub text_hi: Color,
    pub hi: Color,
    pub hi_bg: Color,
    pub hi_bg_unfocused: Color,
    pub bg: Color,
    pub border: Color,
    pub row: [Color; 2],
    pub placeholder: Color,
    pub error: Color,
    pub success: Color,
    pub info: Color,
    pub warning: Color,
    /// Text on bright status backgrounds (e.g. ONLINE/OFFLINE bar on
    /// `success`/`error`); dark for readability.
    pub text_status: Color,
    pub syntax: SyntaxColorScheme,
}

#[cfg(not(any(
    feature = "vscode_dark",
    feature = "catppuccin_mocha",
    feature = "gruvbox_dark"
)))]
compile_error!(
    "select a color scheme feature: `vscode_dark`, `catppuccin_mocha` or `gruvbox_dark`"
);

/// The fixed color scheme used by all widgets (Catppuccin Mocha accents on
/// neutral dark-gray surfaces).
#[cfg(feature = "catppuccin_mocha")]
pub const COLOR_SCHEME: ColorScheme = ColorScheme {
    text: Color::Rgb(205, 214, 244),         // mocha text   #CDD6F4
    text_hi: Color::Rgb(245, 224, 220),      // rosewater   #F5E0DC
    hi: Color::Rgb(203, 166, 247),           // mauve   #CBA6F7
    hi_bg: Color::Rgb(69, 71, 90),           // surface1   #45475A
    hi_bg_unfocused: Color::Rgb(49, 50, 68), // surface0   #313244
    bg: Color::Rgb(24, 24, 24),              // dark gray   #181818
    border: Color::Rgb(108, 112, 134),       // overlay0   #6C7086
    row: [Color::Rgb(38, 38, 38), Color::Rgb(48, 48, 48)], // gray elevations
    placeholder: Color::Rgb(127, 132, 156),  // overlay1   #7F849C
    error: Color::Rgb(243, 139, 168),        // red   #F38BA8
    success: Color::Rgb(166, 227, 161),      // green   #A6E3A1
    info: Color::Rgb(137, 180, 250),         // blue   #89B4FA
    warning: Color::Rgb(250, 179, 135),      // peach   #FAB387
    text_status: Color::Rgb(17, 17, 27),     // crust   #11111B
    syntax: SyntaxColorScheme {
        keyword: Color::Rgb(203, 166, 247),  // mauve   #CBA6F7
        ident: Color::Rgb(205, 214, 244),    // text   #CDD6F4
        number: Color::Rgb(250, 179, 135),   // peach   #FAB387
        string: Color::Rgb(166, 227, 161),   // green   #A6E3A1
        comment: Color::Rgb(108, 112, 134),  // overlay0   #6C7086
        punct: Color::Rgb(147, 153, 178),    // overlay2   #9399B2
        key: Color::Rgb(180, 190, 254),      // lavender   #B4BEFE
        literal: Color::Rgb(250, 179, 135),  // peach   #FAB387
        object: Color::Rgb(249, 226, 175),   // yellow   #F9E2AF
        function: Color::Rgb(137, 180, 250), // blue   #89B4FA
    },
};

/// The fixed color scheme used by all widgets (Gruvbox Dark accents on
/// neutral dark-gray surfaces).
#[cfg(all(feature = "gruvbox_dark", not(feature = "catppuccin_mocha")))]
pub const COLOR_SCHEME: ColorScheme = ColorScheme {
    text: Color::Rgb(235, 219, 178),         // fg1   #EBDBB2
    text_hi: Color::Rgb(251, 241, 199),      // fg0   #FBF1C7
    hi: Color::Rgb(254, 128, 25),            // orange   #FE8019
    hi_bg: Color::Rgb(110, 65, 30),          // muted orange   #6E411E
    hi_bg_unfocused: Color::Rgb(70, 48, 30), // faint orange tint   #46301E
    bg: Color::Rgb(28, 28, 28),              // dark gray   #1C1C1C
    border: Color::Rgb(124, 111, 100),       // bg4   #7C6F64
    row: [Color::Rgb(38, 38, 38), Color::Rgb(48, 48, 48)], // gray elevations
    placeholder: Color::Rgb(146, 131, 116),  // gray   #928374
    error: Color::Rgb(251, 73, 52),          // bright red   #FB4934
    success: Color::Rgb(142, 192, 124),      // aqua green   #8EC07C
    info: Color::Rgb(131, 165, 152),         // blue   #83A598
    warning: Color::Rgb(254, 128, 25),       // orange   #FE8019
    text_status: Color::Rgb(29, 32, 33),     // bg0_h   #1D2021
    syntax: SyntaxColorScheme {
        keyword: Color::Rgb(251, 73, 52),    // red   #FB4934
        ident: Color::Rgb(235, 219, 178),    // fg1   #EBDBB2
        number: Color::Rgb(211, 134, 155),   // purple   #D3869B
        string: Color::Rgb(184, 187, 38),    // green   #B8BB26
        comment: Color::Rgb(146, 131, 116),  // gray   #928374
        punct: Color::Rgb(235, 219, 178),    // fg1   #EBDBB2
        key: Color::Rgb(131, 165, 152),      // blue   #83A598
        literal: Color::Rgb(211, 134, 155),  // purple   #D3869B
        object: Color::Rgb(250, 189, 47),    // yellow   #FABD2F
        function: Color::Rgb(142, 192, 124), // aqua   #8EC07C
    },
};

/// The fixed color scheme used by all widgets (VS Code Dark+ palette).
#[cfg(all(
    feature = "vscode_dark",
    not(feature = "catppuccin_mocha"),
    not(feature = "gruvbox_dark")
))]
pub const COLOR_SCHEME: ColorScheme = ColorScheme {
    text: Color::Rgb(212, 212, 212),         // light gray   #D4D4D4
    text_hi: Color::Rgb(255, 255, 255),      // white   #FFFFFF
    hi: Color::Rgb(86, 156, 214),            // vs blue   #569CD6
    hi_bg: Color::Rgb(4, 57, 94),            // selection blue   #04395E
    hi_bg_unfocused: Color::Rgb(55, 55, 61), // muted slate   #37373D
    bg: Color::Rgb(30, 30, 30),              // editor gray   #1E1E1E
    border: Color::Rgb(110, 118, 129),       // gray   #6E7681
    row: [Color::Rgb(37, 37, 38), Color::Rgb(45, 45, 48)], // gray elevations
    placeholder: Color::Rgb(128, 128, 128),  // dim gray   #808080
    error: Color::Rgb(241, 76, 76),          // vs red   #F14C4C
    success: Color::Rgb(137, 209, 133),      // vs green   #89D185
    info: Color::Rgb(86, 156, 214),          // vs blue   #569CD6
    warning: Color::Rgb(209, 154, 102),      // vs orange   #D19A66
    text_status: Color::Rgb(30, 30, 30),     // editor gray   #1E1E1E
    syntax: SyntaxColorScheme {
        keyword: Color::Rgb(86, 156, 214),   // blue   #569CD6
        ident: Color::Rgb(212, 212, 212),    // light gray   #D4D4D4
        number: Color::Rgb(181, 206, 168),   // pale green   #B5CEA8
        string: Color::Rgb(206, 145, 120),   // terracotta   #CE9178
        comment: Color::Rgb(106, 153, 85),   // green   #6A9955
        punct: Color::Rgb(212, 212, 212),    // light gray   #D4D4D4
        key: Color::Rgb(156, 220, 254),      // sky blue   #9CDCFE
        literal: Color::Rgb(86, 156, 214),   // blue   #569CD6
        object: Color::Rgb(78, 201, 176),    // teal   #4EC9B0
        function: Color::Rgb(220, 220, 170), // pale yellow   #DCDCAA
    },
};
