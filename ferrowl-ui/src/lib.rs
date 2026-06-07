mod screen;

pub mod state;
pub mod style;
pub mod traits;
pub mod types;
pub mod widgets;
pub use screen::AlternateScreen;
pub use types::EventResult;

use ratatui::style::{Color, palette::tailwind};

pub struct ColorScheme {
    pub text: Color,
    pub hi: Color,
    pub hi_bg: Color,
    pub bg: Color,
    pub border: Color,
    pub row: [Color; 2],
    pub placeholder: Color,
    pub error: Color,
    pub success: Color,
}

pub const COLOR_SCHEME: ColorScheme = ColorScheme {
    text: tailwind::WHITE,
    hi: tailwind::INDIGO.c400,
    hi_bg: tailwind::INDIGO.c950,
    bg: tailwind::STONE.c950,
    border: tailwind::WHITE,
    row: [tailwind::SLATE.c950, tailwind::SLATE.c800],
    placeholder: tailwind::NEUTRAL.c500,
    error: tailwind::RED.c500,
    success: tailwind::GREEN.c500,
};
