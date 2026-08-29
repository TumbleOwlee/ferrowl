//! MB-R-137/152/153, OC-R-123/124 — the tri-state connection status bar shared by every
//! client/server/monitor module view.
// `#[allow(dead_code)]`: fully implemented and tested here, but not every module view has been
// wired to `render_status_bar` yet.
#![allow(dead_code)]

use ferrowl_ui::{COLOR_SCHEME, style::TextStyle, widgets::TextBuilder};
use ratatui::buffer::Buffer;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::style::Style;
use ratatui::widgets::StatefulWidget;

/// MB-R-137 — the three connection states shared by every client/server/monitor status bar: not
/// running → `Disconnected`; running and currently connected/bound/open → `Connected`; running
/// and not → `Reconnecting`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnStatus {
    Connected,
    Reconnecting,
    Disconnected,
}

impl ConnStatus {
    fn label(self) -> &'static str {
        match self {
            ConnStatus::Connected => "CONNECTED",
            ConnStatus::Reconnecting => "RECONNECTING",
            ConnStatus::Disconnected => "DISCONNECTED",
        }
    }

    fn color(self) -> ratatui::style::Color {
        match self {
            ConnStatus::Connected => COLOR_SCHEME.success,
            ConnStatus::Reconnecting => COLOR_SCHEME.warning,
            ConnStatus::Disconnected => COLOR_SCHEME.error,
        }
    }
}

/// Renders the one-line status bar at `area` (expected `Constraint::Length(1)`): `status`'s
/// label, plus `addr` (space-separated) when present.
pub fn render_status_bar(status: ConnStatus, addr: Option<&str>, area: Rect, buf: &mut Buffer) {
    let widget = TextBuilder::default()
        .horizontal_alignment(HorizontalAlignment::Center)
        .style(TextStyle {
            general: Style::default()
                .bg(status.color())
                .fg(COLOR_SCHEME.text_status)
                .bold(),
        })
        .build()
        .expect("all required builder fields are set");
    let mut label = match addr {
        Some(a) => format!("{}  {a}", status.label()),
        None => status.label().to_string(),
    };
    StatefulWidget::render(&widget, area, buf, &mut label);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All buffer cell symbols joined into one string, for containment assertions.
    fn buffer_text(buf: &Buffer) -> String {
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    /// MB-R-137 — `Connected` renders its label in the success color.
    fn ut_render_status_bar_connected_uses_success_color_and_label() {
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        render_status_bar(ConnStatus::Connected, None, area, &mut buf);
        let text = buffer_text(&buf);
        assert!(text.contains("CONNECTED"), "missing label:\n{text}");
        assert_eq!(buf[(0, 0)].bg, COLOR_SCHEME.success);
    }

    #[test]
    /// MB-R-137 — `Reconnecting` renders its label in the warning color, the only status that
    /// uses it.
    fn ut_render_status_bar_reconnecting_uses_warning_color_and_label() {
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        render_status_bar(ConnStatus::Reconnecting, None, area, &mut buf);
        let text = buffer_text(&buf);
        assert!(text.contains("RECONNECTING"), "missing label:\n{text}");
        assert_eq!(buf[(0, 0)].bg, COLOR_SCHEME.warning);
    }

    #[test]
    /// MB-R-137 — `Disconnected` renders its label in the error color.
    fn ut_render_status_bar_disconnected_uses_error_color_and_label() {
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        render_status_bar(ConnStatus::Disconnected, None, area, &mut buf);
        let text = buffer_text(&buf);
        assert!(text.contains("DISCONNECTED"), "missing label:\n{text}");
        assert_eq!(buf[(0, 0)].bg, COLOR_SCHEME.error);
    }

    #[test]
    /// MB-R-153 — an `addr` is appended after the label (server-role status line, e.g. the bound
    /// TCP address) when present, and omitted (bare label) when absent (client role, or a
    /// server not currently bound).
    fn ut_render_status_bar_appends_addr_when_present() {
        let area = Rect::new(0, 0, 30, 1);
        let mut with_addr = Buffer::empty(area);
        render_status_bar(
            ConnStatus::Connected,
            Some("127.0.0.1:502"),
            area,
            &mut with_addr,
        );
        assert!(buffer_text(&with_addr).contains("CONNECTED  127.0.0.1:502"));

        let mut without_addr = Buffer::empty(area);
        render_status_bar(ConnStatus::Connected, None, area, &mut without_addr);
        let text = buffer_text(&without_addr);
        assert!(text.contains("CONNECTED"));
        assert!(!text.contains("127.0.0.1:502"));
    }
}
