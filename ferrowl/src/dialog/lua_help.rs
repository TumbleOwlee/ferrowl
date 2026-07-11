//! The `?` overlay for script dialogs: lists the custom Lua bindings available in that dialog's
//! script context.

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::COLOR_SCHEME;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Margin, Rect},
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
            "one method per OCPP action, e.g. StartTransaction({ idTag = \"ABC\" })",
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
            "one method per OCPP action, e.g. MeterValues({ energy = 100 })",
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
            "<module>:Type()",
            "module kind, e.g. \"modbus\" or \"ocpp\"",
        ),
        (
            "<module>:Role()",
            "module role, e.g. \"client\" or \"server\"",
        ),
        (
            "<module>:Register()",
            "C_Register-shaped accessor (modbus modules only)",
        ),
        ("<register>:Get(name)", "read a register's value"),
        ("<register>:Set(name, value)", "write a register's value"),
        (
            "<module>:OCPP()",
            "C_OCPP-shaped accessor (ocpp modules only)",
        ),
        (
            "<ocpp>:GetChargingStations()",
            "list known charging station ids",
        ),
        (
            "<ocpp>:GetConnectors(cs)",
            "list connector ids for a station",
        ),
        (
            "<ocpp>:ChargingStation(cs)",
            "accessor scoped to one station, with its own Get/Set/<Action>",
        ),
        (
            "<ocpp>:Connector(cs, id)",
            "accessor scoped to one connector, with its own Get/Set/<Action>",
        ),
        (
            "<ocpp-accessor>:<Action>(json?)",
            "one method per OCPP action, e.g. MeterValues({ energy = 100 })",
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
    entries: &[
        ("C_Log:Info(message)", "append an info line to the module log"),
        (
            "C_Log:Warn(message)",
            "append a warning line to the module log",
        ),
        (
            "C_Log:Error(message)",
            "append an error line to the module log",
        ),
    ],
};

static PRINT_SECTION: BindingSection = BindingSection {
    title: "print",
    entries: &[(
        "print(...)",
        "redirected to the module log, like C_Log:Info",
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

/// Builds the popup's logical lines for `ctx`, word-wrapping each entry's description to
/// `desc_budget` columns with a 34-col indent (2-space indent + `{sig:<32}`) on continuation
/// lines.
fn build_lines(ctx: ScriptContext, desc_budget: usize) -> Vec<Line<'static>> {
    let desc_style = Style::default().fg(COLOR_SCHEME.text).bg(COLOR_SCHEME.bg);
    let make_lines = |(sig, desc): &(&str, &str)| -> Vec<Line<'static>> {
        let mut segments = crate::view::text::wrap(desc, desc_budget).into_iter();
        let first = segments.next().unwrap_or_default();
        let mut out = vec![Line::from(vec![
            Span::styled(
                format!("  {sig:<32}"),
                Style::default()
                    .fg(COLOR_SCHEME.hi)
                    .bg(COLOR_SCHEME.bg)
                    .bold(),
            ),
            Span::styled(first, desc_style),
        ])];
        out.extend(
            segments.map(|seg| Line::from(Span::styled(format!("{:34}{seg}", ""), desc_style))),
        );
        out
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
        lines.extend(section.entries.iter().flat_map(make_lines));
    }
    lines
}

/// Scrollable `?` overlay listing the Lua bindings available in a script dialog's context.
pub struct LuaHelpOverlay {
    scroll: u16,
    /// Highest valid `scroll` value, refreshed on every `render` call from the content length and
    /// viewport height. Used to clamp `handle_key`'s `j`/`G` so `scroll` never runs past the last
    /// visible line.
    max_scroll: u16,
}

impl LuaHelpOverlay {
    pub fn new() -> Self {
        Self {
            scroll: 0,
            max_scroll: 0,
        }
    }

    /// Handles one key while the overlay is open. Returns `true` if the overlay should close.
    pub fn handle_key(&mut self, modifiers: KeyModifiers, code: KeyCode) -> bool {
        let _ = modifiers;
        match code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => true,
            KeyCode::Char('j') | KeyCode::Down => {
                self.scroll = self.scroll.saturating_add(1).min(self.max_scroll);
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
                self.scroll = self.max_scroll;
                false
            }
            _ => false,
        }
    }

    /// Renders the overlay as a centered popup over `area`.
    pub fn render(&mut self, area: Rect, buf: &mut Buffer, ctx: ScriptContext) {
        let popup_w = 120.min(area.width);
        let inner_width = popup_w.saturating_sub(4) as usize;
        let desc_budget = inner_width.saturating_sub(36);

        let lines = build_lines(ctx, desc_budget);

        let popup_h = (lines.len() as u16 + 4).min(area.height.saturating_sub(4));
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
        let inner = block.inner(popup).inner(Margin::new(2, 1));
        ratatui::prelude::Widget::render(block, popup, buf);

        self.max_scroll = (lines.len() as u16).saturating_sub(inner.height);
        self.scroll = self.scroll.min(self.max_scroll);
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

/// Outcome of [`route_lua_help`]: whether the overlay was open, so the caller knows whether to
/// keep routing the key itself.
#[derive(Debug, PartialEq, Eq)]
pub enum LuaHelpOutcome {
    /// No overlay was open; the key wasn't touched, the caller should route it itself.
    NotActive,
    /// The overlay captured the key (closing itself if applicable); the caller should stop
    /// routing this key further.
    Consumed,
}

/// Feed one key through `lua_help`, if the Lua bindings overlay is currently open. Clears
/// `*lua_help` when [`LuaHelpOverlay::handle_key`] reports the overlay should close.
pub fn route_lua_help(
    lua_help: &mut Option<LuaHelpOverlay>,
    modifiers: KeyModifiers,
    code: KeyCode,
) -> LuaHelpOutcome {
    let Some(help) = lua_help.as_mut() else {
        return LuaHelpOutcome::NotActive;
    };
    if help.handle_key(modifiers, code) {
        *lua_help = None;
    }
    LuaHelpOutcome::Consumed
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
        overlay.max_scroll = 100;
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
        overlay.max_scroll = 20;
        overlay.scroll = 5;
        assert!(!overlay.handle_key(KeyModifiers::NONE, KeyCode::Char('g')));
        assert_eq!(overlay.scroll, 0);
        assert!(!overlay.handle_key(KeyModifiers::NONE, KeyCode::Char('G')));
        assert_eq!(overlay.scroll, 20);
    }

    #[test]
    fn ut_handle_key_g_then_j_stays_at_max_no_overflow() {
        // Regression: `G` used to jump to `u16::MAX`, and `j` at the bottom used to keep
        // incrementing unboundedly. Both must clamp to `max_scroll`.
        let mut overlay = LuaHelpOverlay::new();
        overlay.max_scroll = 7;
        assert!(!overlay.handle_key(KeyModifiers::NONE, KeyCode::Char('G')));
        assert_eq!(overlay.scroll, 7);
        for _ in 0..5 {
            assert!(!overlay.handle_key(KeyModifiers::NONE, KeyCode::Char('j')));
        }
        assert_eq!(overlay.scroll, 7);
    }

    #[test]
    fn ut_render_sets_max_scroll_and_handle_key_respects_it_after() {
        // A render with a tall content and small viewport should compute a finite max_scroll;
        // subsequent `G`/`j` must not exceed it.
        let mut overlay = LuaHelpOverlay::new();
        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        overlay.render(area, &mut buf, ScriptContext::Session);
        assert!(overlay.max_scroll < u16::MAX);
        let max = overlay.max_scroll;
        assert!(!overlay.handle_key(KeyModifiers::NONE, KeyCode::Char('G')));
        assert_eq!(overlay.scroll, max);
        assert!(!overlay.handle_key(KeyModifiers::NONE, KeyCode::Char('j')));
        assert_eq!(overlay.scroll, max);
    }

    #[test]
    fn ut_build_lines_fit_popup_width() {
        // popup_w = 75, inner_width = 73, desc_budget = 73 - 34 = 39.
        let lines = build_lines(ScriptContext::OcppClient, 39);
        for line in &lines {
            let total: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
            assert!(total <= 73, "line too wide ({total} chars): {line:?}");
        }
    }

    #[test]
    fn ut_build_lines_wraps_long_action_desc() {
        let secs = sections(ScriptContext::OcppClient);
        let ocpp = secs.iter().find(|s| s.title == "C_OCPP").unwrap();
        let (_, action_desc) = ocpp
            .entries
            .iter()
            .find(|(sig, _)| sig.contains("<Action>"))
            .unwrap();
        assert!(action_desc.chars().count() > 39);
        let wrapped_segments = crate::view::text::wrap(action_desc, 39).len();
        assert!(
            wrapped_segments >= 2,
            "expected the long <Action> description to wrap into >= 2 segments, got {wrapped_segments}"
        );

        // build_lines must emit exactly that many physical lines for this entry: one entry-count
        // increase per section beyond the un-wrapped baseline.
        let lines = build_lines(ScriptContext::OcppClient, 39);
        let baseline: usize = 1 // section title
            + ocpp.entries.len();
        assert!(lines.len() >= baseline + (wrapped_segments - 1));
    }

    #[test]
    fn ut_handle_key_other_keys_eaten() {
        let mut overlay = LuaHelpOverlay::new();
        assert!(!overlay.handle_key(KeyModifiers::NONE, KeyCode::Char('x')));
        assert!(!overlay.handle_key(KeyModifiers::NONE, KeyCode::Enter));
        assert!(!overlay.handle_key(KeyModifiers::NONE, KeyCode::Tab));
    }

    // --- route_lua_help -------------------------------------------------

    #[test]
    fn ut_route_not_active_when_none() {
        let mut lua_help: Option<LuaHelpOverlay> = None;
        assert_eq!(
            route_lua_help(&mut lua_help, KeyModifiers::NONE, KeyCode::Char('q')),
            LuaHelpOutcome::NotActive
        );
        assert!(lua_help.is_none());
    }

    #[test]
    fn ut_route_close_key_clears_overlay() {
        let mut lua_help = Some(LuaHelpOverlay::new());
        assert_eq!(
            route_lua_help(&mut lua_help, KeyModifiers::NONE, KeyCode::Esc),
            LuaHelpOutcome::Consumed
        );
        assert!(lua_help.is_none());
    }

    #[test]
    fn ut_route_other_key_stays_open_and_consumed() {
        let mut lua_help = Some(LuaHelpOverlay::new());
        assert_eq!(
            route_lua_help(&mut lua_help, KeyModifiers::NONE, KeyCode::Char('j')),
            LuaHelpOutcome::Consumed
        );
        assert!(lua_help.is_some());
    }
}
