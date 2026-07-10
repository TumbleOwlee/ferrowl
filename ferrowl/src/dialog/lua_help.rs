//! The `?` overlay for script dialogs: lists the custom Lua bindings available in that dialog's
//! script context.

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::COLOR_SCHEME;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};

/// Which script the overlay is describing bindings for, so it can show only the modules actually
/// reachable from that script's global table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptContext {
    Modbus,
    OcppClient,
    OcppServer,
    Session,
}

/// One titled group of Lua bindings shown in the overlay: a module name plus its
/// `(signature, description)` entries.
pub struct BindingSection {
    pub title: &'static str,
    pub entries: &'static [(&'static str, &'static str)],
}

// NOTE: keep these tables in sync with the `add_method` registrations in the `ferrowl-lua` module
// crate (`ferrowl-lua/src/module/*.rs`).

static REGISTER_SECTION: BindingSection = BindingSection {
    title: "C_Register",
    entries: &[
        ("C_Register:Get(name)", "read a register's value"),
        ("C_Register:Set(name, value)", "write a register's value"),
    ],
};

static OCPP_CLIENT_SECTION: BindingSection = BindingSection {
    title: "C_OCPP",
    entries: &[
        ("C_OCPP:Get(name)", "read charging-station-level state"),
        (
            "C_OCPP:Set(name, value)",
            "write charging-station-level state",
        ),
        (
            "C_OCPP:<Action>(overrides?)",
            "one method per OCPP action, e.g. BootNotification(), StartTransaction({ idTag = \"ABC\" })",
        ),
        (
            "C_OCPP:Connector(id)",
            "accessor scoped to one connector, with its own Get/Set/<Action>",
        ),
    ],
};

static OCPP_SERVER_SECTION: BindingSection = BindingSection {
    title: "C_OCPP",
    entries: &[
        (
            "C_OCPP:GetChargingStations()",
            "list known charging station ids",
        ),
        (
            "C_OCPP:GetConnectors(cs)",
            "list connector ids for a station",
        ),
        (
            "C_OCPP:ChargingStation(cs)",
            "accessor scoped to one station, with its own Get/Set/<Action>",
        ),
        (
            "C_OCPP:Connector(cs, id)",
            "accessor scoped to one connector, with its own Get/Set/<Action>",
        ),
        (
            "<accessor>:<Action>(overrides?)",
            "one method per OCPP action, e.g. StatusNotification(), MeterValues({ energy = 100 })",
        ),
    ],
};

static MODULE_SECTION: BindingSection = BindingSection {
    title: "C_Module",
    entries: &[
        (
            "C_Module:List()",
            "sorted names of every module in the session",
        ),
        (
            "C_Module:Get(name)",
            "resolve a module by name to a handle (raises if unknown)",
        ),
        (
            "<handle>:Type()",
            "module kind, e.g. \"modbus\" or \"ocpp\"",
        ),
        (
            "<handle>:Role()",
            "module role, e.g. \"client\" or \"server\"",
        ),
        (
            "<handle>:Register()",
            "C_Register-shaped accessor (modbus modules only)",
        ),
        (
            "<handle>:OCPP()",
            "C_OCPP-shaped accessor (ocpp modules only)",
        ),
    ],
};

static TIME_SECTION: BindingSection = BindingSection {
    title: "C_Time",
    entries: &[
        ("C_Time:Get()", "seconds elapsed since module start"),
        ("C_Time:GetMs()", "milliseconds elapsed since module start"),
    ],
};

static TEST_SECTION: BindingSection = BindingSection {
    title: "C_Test",
    entries: &[
        (
            "C_Test:Assert(cond, msg)",
            "raise if cond is falsy (nil/false)",
        ),
        ("C_Test:Fail(msg)", "always raise"),
    ],
};

static LOG_SECTION: BindingSection = BindingSection {
    title: "C_Log",
    entries: &[("C_Log:Print(message)", "append a line to the module log")],
};

static PRINT_SECTION: BindingSection = BindingSection {
    title: "print",
    entries: &[(
        "print(...)",
        "redirected to the module log, like C_Log:Print",
    )],
};

// Context-specific section(s) first, then the shared `C_Time`/`C_Test`/`C_Log`/`print` sections
// available in every script context.
static MODBUS_SECTIONS: &[&BindingSection] = &[
    &REGISTER_SECTION,
    &TIME_SECTION,
    &TEST_SECTION,
    &LOG_SECTION,
    &PRINT_SECTION,
];
static OCPP_CLIENT_SECTIONS: &[&BindingSection] = &[
    &OCPP_CLIENT_SECTION,
    &TIME_SECTION,
    &TEST_SECTION,
    &LOG_SECTION,
    &PRINT_SECTION,
];
static OCPP_SERVER_SECTIONS: &[&BindingSection] = &[
    &OCPP_SERVER_SECTION,
    &TIME_SECTION,
    &TEST_SECTION,
    &LOG_SECTION,
    &PRINT_SECTION,
];
static SESSION_SECTIONS: &[&BindingSection] = &[
    &MODULE_SECTION,
    &TIME_SECTION,
    &TEST_SECTION,
    &LOG_SECTION,
    &PRINT_SECTION,
];

/// Context-specific section(s) first, then shared `C_Time`, `C_Test`, `C_Log`, `print`.
pub fn sections(ctx: ScriptContext) -> &'static [&'static BindingSection] {
    match ctx {
        ScriptContext::Modbus => MODBUS_SECTIONS,
        ScriptContext::OcppClient => OCPP_CLIENT_SECTIONS,
        ScriptContext::OcppServer => OCPP_SERVER_SECTIONS,
        ScriptContext::Session => SESSION_SECTIONS,
    }
}

/// Scrollable `?` overlay listing the Lua bindings available in a script dialog's context.
pub struct LuaHelpOverlay {
    scroll: u16,
}

impl LuaHelpOverlay {
    pub fn new() -> Self {
        Self { scroll: 0 }
    }

    /// Handles one key while the overlay is open. Returns `true` if the overlay should close.
    pub fn handle_key(&mut self, modifiers: KeyModifiers, code: KeyCode) -> bool {
        let _ = modifiers;
        match code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => true,
            KeyCode::Char('j') | KeyCode::Down => {
                self.scroll = self.scroll.saturating_add(1);
                false
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.scroll = self.scroll.saturating_sub(1);
                false
            }
            KeyCode::Char('g') => {
                self.scroll = 0;
                false
            }
            KeyCode::Char('G') => {
                self.scroll = u16::MAX;
                false
            }
            _ => false,
        }
    }

    /// Renders the overlay as a centered popup over `area`.
    pub fn render(&mut self, area: Rect, buf: &mut Buffer, ctx: ScriptContext) {
        let make_line = |(sig, desc): &(&str, &str)| {
            Line::from(vec![
                Span::styled(
                    format!("  {sig:<32}"),
                    Style::default()
                        .fg(COLOR_SCHEME.hi)
                        .bg(COLOR_SCHEME.bg)
                        .bold(),
                ),
                Span::styled(
                    desc.to_string(),
                    Style::default().fg(COLOR_SCHEME.text).bg(COLOR_SCHEME.bg),
                ),
            ])
        };
        let section_title = |title: &str| {
            Line::from(Span::styled(
                title.to_string(),
                Style::default()
                    .fg(COLOR_SCHEME.text)
                    .bg(COLOR_SCHEME.bg)
                    .bold(),
            ))
        };

        let mut lines: Vec<Line> = Vec::new();
        for section in sections(ctx) {
            if !lines.is_empty() {
                lines.push(Line::default());
            }
            lines.push(section_title(section.title));
            lines.extend(section.entries.iter().map(make_line));
        }

        let popup_w = 75.min(area.width);
        let popup_h = (lines.len() as u16 + 2).min(area.height.saturating_sub(2));
        let [_, mid, _] = Layout::horizontal([
            Constraint::Min(1),
            Constraint::Length(popup_w),
            Constraint::Min(1),
        ])
        .areas(area);
        let [_, popup, _] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(popup_h),
            Constraint::Min(1),
        ])
        .areas(mid);

        ratatui::prelude::Widget::render(Clear, popup, buf);
        let block = Block::bordered()
            .title(" Lua Bindings (Esc/q/? to close) ")
            .style(Style::default().fg(COLOR_SCHEME.hi).bg(COLOR_SCHEME.bg));
        let inner = block.inner(popup);
        ratatui::prelude::Widget::render(block, popup, buf);

        self.scroll = self
            .scroll
            .min((lines.len() as u16).saturating_sub(inner.height));
        ratatui::prelude::Widget::render(
            Paragraph::new(lines)
                .scroll((self.scroll, 0))
                .style(Style::default().bg(COLOR_SCHEME.bg)),
            inner,
            buf,
        );
    }
}

impl Default for LuaHelpOverlay {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_modbus_has_register_no_ocpp() {
        let secs = sections(ScriptContext::Modbus);
        assert!(secs.iter().any(|s| s.title == "C_Register"));
        assert!(!secs.iter().any(|s| s.title == "C_OCPP"));
    }

    #[test]
    fn ut_ocpp_server_mentions_get_charging_stations() {
        let secs = sections(ScriptContext::OcppServer);
        let ocpp = secs.iter().find(|s| s.title == "C_OCPP").unwrap();
        assert!(
            ocpp.entries
                .iter()
                .any(|(sig, _)| sig.contains("GetChargingStations"))
        );
    }

    #[test]
    fn ut_ocpp_client_mentions_connector() {
        let secs = sections(ScriptContext::OcppClient);
        let ocpp = secs.iter().find(|s| s.title == "C_OCPP").unwrap();
        assert!(
            ocpp.entries
                .iter()
                .any(|(sig, _)| sig.contains("Connector"))
        );
    }

    #[test]
    fn ut_session_has_module_section() {
        let secs = sections(ScriptContext::Session);
        assert!(secs.iter().any(|s| s.title == "C_Module"));
    }

    #[test]
    fn ut_every_context_includes_shared_sections() {
        for ctx in [
            ScriptContext::Modbus,
            ScriptContext::OcppClient,
            ScriptContext::OcppServer,
            ScriptContext::Session,
        ] {
            let secs = sections(ctx);
            for shared in ["C_Time", "C_Test", "C_Log"] {
                assert!(
                    secs.iter().any(|s| s.title == shared),
                    "{ctx:?} missing {shared}"
                );
            }
        }
    }

    #[test]
    fn ut_handle_key_close_keys() {
        let mut overlay = LuaHelpOverlay::new();
        assert!(overlay.handle_key(KeyModifiers::NONE, KeyCode::Esc));
        assert!(overlay.handle_key(KeyModifiers::NONE, KeyCode::Char('q')));
        assert!(overlay.handle_key(KeyModifiers::NONE, KeyCode::Char('?')));
    }

    #[test]
    fn ut_handle_key_scroll() {
        let mut overlay = LuaHelpOverlay::new();
        assert!(!overlay.handle_key(KeyModifiers::NONE, KeyCode::Char('j')));
        assert_eq!(overlay.scroll, 1);
        assert!(!overlay.handle_key(KeyModifiers::NONE, KeyCode::Down));
        assert_eq!(overlay.scroll, 2);
        assert!(!overlay.handle_key(KeyModifiers::NONE, KeyCode::Char('k')));
        assert_eq!(overlay.scroll, 1);
        assert!(!overlay.handle_key(KeyModifiers::NONE, KeyCode::Up));
        assert_eq!(overlay.scroll, 0);
        // Saturating: k at 0 stays at 0.
        assert!(!overlay.handle_key(KeyModifiers::NONE, KeyCode::Char('k')));
        assert_eq!(overlay.scroll, 0);
    }

    #[test]
    fn ut_handle_key_top_bottom() {
        let mut overlay = LuaHelpOverlay::new();
        overlay.scroll = 5;
        assert!(!overlay.handle_key(KeyModifiers::NONE, KeyCode::Char('g')));
        assert_eq!(overlay.scroll, 0);
        assert!(!overlay.handle_key(KeyModifiers::NONE, KeyCode::Char('G')));
        assert_eq!(overlay.scroll, u16::MAX);
    }

    #[test]
    fn ut_handle_key_other_keys_eaten() {
        let mut overlay = LuaHelpOverlay::new();
        assert!(!overlay.handle_key(KeyModifiers::NONE, KeyCode::Char('x')));
        assert!(!overlay.handle_key(KeyModifiers::NONE, KeyCode::Enter));
        assert!(!overlay.handle_key(KeyModifiers::NONE, KeyCode::Tab));
    }
}
