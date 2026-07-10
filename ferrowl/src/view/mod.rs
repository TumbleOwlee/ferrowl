//! Top-level UI views: tab bar, register table, log pane, and command line.

pub mod command;
pub mod log;
pub mod tabs;
pub mod text;

use ferrowl_ui::COLOR_SCHEME;
use ratatui::style::Style;

/// Theme border color for unfocused input/table borders, shared by every dialog and overlay in the
/// crate that draws one (module setup dialogs, script/session dialogs, OCPP action/detail overlays).
pub(crate) fn border_style() -> Style {
    Style::default().fg(COLOR_SCHEME.border).bg(COLOR_SCHEME.bg)
}
