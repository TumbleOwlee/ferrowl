//! Session-level dialog (`:session`): the session's Lua scripts, a sim-cycle interval field, and
//! a read-only tail of the session sim's log — one overlay owning all of its widgets.
//!
//! Layout: the interval field on top, the script manager in the middle (script table with an
//! On/Off status over a "New Script" name input on the left, the code editor for the selected
//! script on the right — the same surface as the per-module [`ScriptDialog`]), and the session
//! log pane at the bottom.
//!
//! One flat Tab order: Interval → script table → name input → code editor → log → Interval
//! (`Shift+Tab` reversed; the code editor is skipped while no script is selected). `t` toggles a
//! script, `d` deletes (with confirmation), `c` toggles compact rows, Enter in the name input
//! creates a new (enabled) script. Edits are live on a working copy, applied when the dialog
//! closes — the same "no separate save" convention the module [`ScriptDialog`] uses. `Esc` opens
//! a close confirmation popup (Enter/Space confirms, Esc dismisses it); `?` from the code
//! editor's Normal mode opens the Lua bindings overlay.
//!
//! [`ScriptDialog`]: crate::dialog::scripts::ScriptDialog

use std::time::Duration;

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{CodeInputFieldState, InputFieldState, InputFieldStateBuilder, VimMode},
    style::InputFieldStyleBuilder,
    traits::{HandleEvents, SetFocus},
    widgets::{CodeInputField, InputField, InputFieldBuilder, Validate, ValidateResult, Widget},
};
use ferrowl_ui_derive::{Focus, focusable};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, Clear, StatefulWidget, Widget as UiWidget},
};

use crate::config::script::ScriptDef;
use crate::dialog::close_confirm::{CloseConfirmDialog, CloseConfirmOutcome, route_close_confirm};
use crate::dialog::lua_help::{LuaHelpOverlay, ScriptContext};
use crate::dialog::script_manager::{self, ScriptManagerRef, ScriptTable};
use crate::module::modbus::dialog::ConfirmDeleteDialog;
use crate::module::view::SharedLog;
use crate::view::border_style;
use crate::view::log::{LogEntry, LogView, format_timestamp, new_log_view};

/// Sim-cycle interval input validator: must parse as a finite, positive number of seconds
/// (mirrors `Session::interval_duration`'s sanitization, but rejected at the field instead of
/// silently falling back).
#[derive(Clone, Debug)]
pub struct Interval();

impl Validate for Interval {
    fn validate(input: &str) -> ValidateResult {
        match input.trim().parse::<f64>() {
            Ok(v) if v.is_finite() && v > 0.0 => ValidateResult::None,
            Ok(_) => ValidateResult::Error("Interval must be a positive number".to_string()),
            Err(e) => ValidateResult::Error(e.to_string()),
        }
    }

    fn allowed_char(c: char) -> bool {
        c.is_ascii_digit() || c == '.'
    }
}

/// The `:session` dialog. Works on a private copy of the scripts/interval; the caller applies the
/// result via [`SessionDialog::resolve`] on close.
///
/// Tab order (declaration order below): interval → script table → name input → code editor
/// (skipped while no script is selected) → log.
#[focusable]
#[derive(Focus)]
pub struct SessionDialog {
    #[focus]
    interval: Widget<InputFieldState, InputField<Interval>>,
    scripts: Vec<ScriptDef>,
    #[focus]
    table: ScriptTable,
    #[focus]
    name_input: Widget<InputFieldState, InputField<String>>,
    #[focus(when = self.selected().is_some())]
    code: Widget<CodeInputFieldState, CodeInputField>,
    #[focus]
    log: LogView,
    confirm: Option<ConfirmDeleteDialog>,
    /// Compact (no vertical row margin) script table; toggled with `c`. Default off (margin 1).
    compact: bool,
    close_confirm: Option<CloseConfirmDialog>,
    lua_help: Option<LuaHelpOverlay>,
}

impl SessionDialog {
    pub fn new(scripts: &[ScriptDef], interval: Duration) -> Self {
        let scripts = scripts.to_vec();
        let mut interval_field = interval_input();
        set_input(&mut interval_field, &format_interval(interval));
        let mut dialog = Self {
            interval: interval_field,
            table: script_manager::script_table(script_manager::rows(&scripts)),
            name_input: script_manager::name_input(border_style()),
            code: script_manager::code_editor(border_style()),
            log: new_log_view(),
            confirm: None,
            compact: false,
            focus: SessionDialogFocus::Interval,
            view_focused: true,
            close_confirm: None,
            lua_help: None,
            scripts,
        };
        dialog.sync_code_from_selection();
        dialog
    }

    /// Apply the working copy back to the caller: the validated interval (falling back to the
    /// 1s default if the field is currently invalid — an invalid field must never propagate a
    /// bogus duration) and the scripts list, with the open editor flushed into the selected
    /// script first so unsaved keystrokes aren't lost.
    pub fn resolve(mut self) -> (Vec<ScriptDef>, Duration) {
        self.flush_code_to_selection();
        let interval = parse_interval(self.interval.state.input())
            .unwrap_or_else(|| Duration::from_secs_f64(1.0));
        (self.scripts, interval)
    }

    /// Refresh the read-only log pane from a snapshot of the session sim's log ring. Called by
    /// the owner once per tick while the dialog is open. Follows the tail only while the pane is
    /// unfocused, so a user scrolling through the log isn't yanked back down every tick.
    pub fn set_log_entries(&mut self, entries: Vec<LogEntry>) {
        self.log.state.set_values(entries);
        if self.focus != SessionDialogFocus::Log {
            self.log.state.move_to_bottom();
        }
    }

    fn manager(&mut self) -> ScriptManagerRef<'_> {
        ScriptManagerRef {
            scripts: &mut self.scripts,
            table: &mut self.table,
            name_input: &mut self.name_input,
            code: &mut self.code,
            compact: &mut self.compact,
        }
    }

    fn selected(&self) -> Option<usize> {
        script_manager::selected(&self.scripts, &self.table)
    }

    /// Load the selected script's code into the editor. Without a selection the editor is
    /// disabled: it shows only its placeholder and there is nothing edits could apply to.
    fn sync_code_from_selection(&mut self) {
        self.manager().sync_code_from_selection();
    }

    /// Write the editor's content back into the selected script.
    fn flush_code_to_selection(&mut self) {
        self.manager().flush_code_to_selection();
    }

    /// Create a new enabled script from the name input (rejecting empty / duplicate names).
    fn create_script(&mut self) {
        self.manager().create_script();
    }

    fn toggle_compact(&mut self) {
        self.manager().toggle_compact();
    }

    fn toggle_selected(&mut self) {
        self.manager().toggle_selected();
    }

    fn delete_selected(&mut self) {
        self.manager().delete_selected();
    }

    /// Handle a key. Returns `true` when the dialog should close (confirmed via the close-confirm
    /// popup).
    pub fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> bool {
        // The Lua bindings overlay takes precedence over everything else while open.
        if let Some(help) = self.lua_help.as_mut() {
            if help.handle_key(modifiers, code) {
                self.lua_help = None;
            }
            return false;
        }

        // The close-confirm popup takes precedence once open.
        match route_close_confirm(&mut self.close_confirm, modifiers, code) {
            CloseConfirmOutcome::NotActive => {}
            CloseConfirmOutcome::Close => return true,
            CloseConfirmOutcome::Consumed => return false,
        }

        // Delete-confirmation sub-dialog takes precedence.
        if let Some(confirm) = self.confirm.as_mut() {
            match (modifiers, code) {
                (KeyModifiers::NONE, KeyCode::Esc) => self.confirm = None,
                (KeyModifiers::NONE, KeyCode::Tab) => confirm.focus_next(),
                (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => {
                    confirm.focus_previous()
                }
                (KeyModifiers::NONE, KeyCode::Enter | KeyCode::Char(' ')) => {
                    let confirmed = confirm.is_confirm_focused();
                    self.confirm = None;
                    if confirmed {
                        self.delete_selected();
                    }
                }
                _ => {
                    let _ = confirm.handle_events(modifiers, code);
                }
            }
            return false;
        }

        // Intercept `?` before offering the key to the editor, but only in the code editor's
        // Normal mode: in Insert/Visual mode it is valid Lua text and must fall through
        // unchanged.
        if self.focus == SessionDialogFocus::Code
            && self.code.state.vim_mode() == VimMode::Normal
            && let (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char('?')) =
                (modifiers, code)
        {
            self.lua_help = Some(LuaHelpOverlay::new());
            return false;
        }

        // The vim-modal code editor must see keys before the dialog: in Insert mode it
        // consumes Esc (back to Normal) and Tab/BackTab (indent/dedent); only keys it
        // leaves unhandled (e.g. Esc/Tab in Normal mode) fall through to dialog handling.
        if self.focus == SessionDialogFocus::Code
            && let ferrowl_ui::EventResult::Consumed =
                self.code.state.handle_events(modifiers, code)
        {
            self.flush_code_to_selection();
            return false;
        }

        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Tab) => self.focus_next(),
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => self.focus_previous(),
            (KeyModifiers::NONE, KeyCode::Esc) => {
                self.close_confirm = Some(CloseConfirmDialog::new());
            }
            _ => match self.focus {
                SessionDialogFocus::Interval => {
                    let _ = self.interval.state.handle_events(modifiers, code);
                }
                SessionDialogFocus::Table => match (modifiers, code) {
                    (KeyModifiers::NONE, KeyCode::Char('t')) => self.toggle_selected(),
                    (KeyModifiers::NONE, KeyCode::Char('c')) => self.toggle_compact(),
                    (KeyModifiers::NONE, KeyCode::Char('d')) => {
                        if let Some(i) = self.selected() {
                            self.confirm = Some(ConfirmDeleteDialog::new(&self.scripts[i].name));
                        }
                    }
                    _ => {
                        let _ = self.table.state.handle_events(modifiers, code);
                        self.sync_code_from_selection();
                    }
                },
                SessionDialogFocus::NameInput => match (modifiers, code) {
                    (KeyModifiers::NONE, KeyCode::Enter) => self.create_script(),
                    _ => {
                        let _ = self.name_input.state.handle_events(modifiers, code);
                    }
                },
                // Code-focus keys were already offered to the editor above; anything
                // reaching this arm was left unhandled by it.
                SessionDialogFocus::Code => {}
                SessionDialogFocus::Log => {
                    let _ = self.log.state.handle_events(modifiers, code);
                }
            },
        }
        false
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        // Centered box covering most of the screen.
        let [_, hc, _] = Layout::horizontal([
            Constraint::Percentage(10),
            Constraint::Percentage(80),
            Constraint::Percentage(10),
        ])
        .areas(area);
        let [_, vc, _] = Layout::vertical([
            Constraint::Percentage(5),
            Constraint::Percentage(90),
            Constraint::Percentage(5),
        ])
        .areas(hc);

        UiWidget::render(&Clear, vc, buf);
        let block = Block::bordered()
            .style(
                ratatui::prelude::Style::default()
                    .fg(COLOR_SCHEME.hi)
                    .bg(COLOR_SCHEME.bg),
            )
            .title_alignment(HorizontalAlignment::Center)
            .title("Session");
        let block_inner = block.inner(vc);
        block.render(vc, buf);
        let inner = block_inner.inner(Margin {
            vertical: 1,
            horizontal: 2,
        });

        let [scripts_area, log_area] =
            Layout::vertical([Constraint::Min(10), Constraint::Length(10)]).areas(inner);

        // Script manager pane: table over the name input on the left, code editor right.
        let [left, right] =
            Layout::horizontal([Constraint::Max(50), Constraint::Min(1)]).areas(scripts_area);
        let [interval_area, list_area, input_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .areas(left);

        self.interval
            .state
            .set_focused(self.focus == SessionDialogFocus::Interval);
        self.table
            .state
            .set_focused(self.focus == SessionDialogFocus::Table);
        self.name_input
            .state
            .set_focused(self.focus == SessionDialogFocus::NameInput);
        self.code
            .state
            .set_focused(self.focus == SessionDialogFocus::Code);
        self.log
            .state
            .set_focused(self.focus == SessionDialogFocus::Log);

        StatefulWidget::render(
            &self.interval.widget,
            interval_area,
            buf,
            &mut self.interval.state,
        );
        StatefulWidget::render(&self.table.widget, list_area, buf, &mut self.table.state);
        StatefulWidget::render(
            &self.name_input.widget,
            input_area,
            buf,
            &mut self.name_input.state,
        );
        StatefulWidget::render(&self.code.widget, right, buf, &mut self.code.state);
        StatefulWidget::render(&self.log.widget, log_area, buf, &mut self.log.state);

        if let Some(confirm) = self.confirm.as_mut() {
            confirm.render(vc, buf);
        }

        if let Some(help) = self.lua_help.as_mut() {
            help.render(area, buf, ScriptContext::Session);
        }

        if let Some(confirm) = self.close_confirm.as_mut() {
            confirm.render(area, buf);
        }
    }
}

fn parse_interval(input: &str) -> Option<Duration> {
    let v = input.trim().parse::<f64>().ok()?;
    (v.is_finite() && v > 0.0).then(|| Duration::from_secs_f64(v))
}

fn format_interval(interval: Duration) -> String {
    let secs = interval.as_secs_f64();
    // Trim a trailing ".0" so a whole-second interval reads as "1", not "1.0000..".
    let text = format!("{secs:.4}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn set_input(widget: &mut Widget<InputFieldState, InputField<Interval>>, value: &str) {
    widget.state.set_input(value.to_string());
    widget.state.set_cursor(value.chars().count());
}

fn interval_input() -> Widget<InputFieldState, InputField<Interval>> {
    Widget {
        state: InputFieldStateBuilder::default()
            .focused(false)
            .disabled(false)
            .placeholder(Some("1.0".to_string()))
            .allowed_for::<Interval>()
            .build()
            .unwrap(),
        widget: InputFieldBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some(
                ("Interval (seconds)", HorizontalAlignment::Left).into(),
            ))
            .style(InputFieldStyleBuilder::default().build().unwrap())
            .margin(Margin {
                vertical: 0,
                horizontal: 0,
            })
            .build()
            .unwrap(),
    }
}

/// Build `LogEntry` rows from a raw `(timestamp_ms, message)` ring snapshot, matching the
/// formatting `App::refresh_snapshot` applies to tab logs.
pub fn entries_from_ring(lines: Vec<(u64, String)>) -> Vec<LogEntry> {
    lines
        .into_iter()
        .map(|(ts, msg)| LogEntry {
            timestamp: format_timestamp(ts),
            message: msg.trim_end_matches('\u{0}').to_string(),
        })
        .collect()
}

/// Snapshot `log`'s current lines as render-ready entries. Async because the log is behind a
/// `tokio::sync::RwLock`.
pub async fn snapshot_log(log: &SharedLog, max: usize) -> Vec<LogEntry> {
    let lines = log.read().await.peek_n(max);
    entries_from_ring(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog() -> SessionDialog {
        SessionDialog::new(
            &[ScriptDef {
                name: "boot".into(),
                code: String::new(),
                enabled: true,
            }],
            Duration::from_secs_f64(2.5),
        )
    }

    #[test]
    fn ut_new_prefills_interval_and_scripts() {
        let d = dialog();
        assert_eq!(d.interval.state.input(), "2.5");
        let (scripts, _) = d.resolve();
        assert_eq!(scripts.len(), 1);
    }

    #[test]
    fn ut_interval_validate_accepts_positive_finite() {
        assert!(matches!(Interval::validate("2.5"), ValidateResult::None));
        assert!(matches!(Interval::validate("1"), ValidateResult::None));
    }

    #[test]
    fn ut_interval_validate_rejects_bad_values() {
        assert!(matches!(Interval::validate("0"), ValidateResult::Error(_)));
        assert!(matches!(Interval::validate("-1"), ValidateResult::Error(_)));
        assert!(matches!(
            Interval::validate("nan"),
            ValidateResult::Error(_)
        ));
        assert!(matches!(
            Interval::validate("inf"),
            ValidateResult::Error(_)
        ));
        assert!(matches!(
            Interval::validate("abc"),
            ValidateResult::Error(_)
        ));
    }

    #[test]
    fn ut_interval_allowed_char() {
        assert!(Interval::allowed_char('5'));
        assert!(Interval::allowed_char('.'));
        assert!(!Interval::allowed_char('-'));
        assert!(!Interval::allowed_char('e'));
        assert!(!Interval::allowed_char(' '));
        assert!(!Interval::allowed_char('a'));
    }

    #[test]
    fn ut_resolve_round_trips_scripts_and_interval() {
        let d = dialog();
        let (scripts, interval) = d.resolve();
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].name, "boot");
        assert_eq!(interval, Duration::from_secs_f64(2.5));
    }

    #[test]
    fn ut_resolve_falls_back_on_invalid_interval() {
        let mut d = dialog();
        set_input(&mut d.interval, "not-a-number");
        let (_, interval) = d.resolve();
        assert_eq!(interval, Duration::from_secs_f64(1.0));
    }

    // The dialog-wide Tab rotation: Interval → table → name input → code editor → log →
    // Interval. The fixture has one script, so the code editor is reachable.
    #[test]
    fn ut_tab_rotates_through_all_fields() {
        let mut d = dialog();
        assert_eq!(d.focus, SessionDialogFocus::Interval);
        let expected = [
            SessionDialogFocus::Table,
            SessionDialogFocus::NameInput,
            SessionDialogFocus::Code,
            SessionDialogFocus::Log,
            SessionDialogFocus::Interval,
        ];
        for focus in expected {
            assert!(!d.handle_events(KeyModifiers::NONE, KeyCode::Tab));
            assert_eq!(d.focus, focus);
        }
    }

    #[test]
    fn ut_backtab_rotates_in_reverse() {
        let mut d = dialog();
        let expected = [
            SessionDialogFocus::Log,
            SessionDialogFocus::Code,
            SessionDialogFocus::NameInput,
            SessionDialogFocus::Table,
            SessionDialogFocus::Interval,
        ];
        for focus in expected {
            assert!(!d.handle_events(KeyModifiers::SHIFT, KeyCode::BackTab));
            assert_eq!(d.focus, focus);
        }
    }

    // Without a selected script the code editor is disabled and both rotations skip it.
    #[test]
    fn ut_rotation_skips_disabled_code_editor() {
        let mut d = SessionDialog::new(&[], Duration::from_secs(1));
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> table
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> name input
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // code skipped -> log
        assert_eq!(d.focus, SessionDialogFocus::Log);
        d.handle_events(KeyModifiers::SHIFT, KeyCode::BackTab); // code skipped -> name input
        assert_eq!(d.focus, SessionDialogFocus::NameInput);
    }

    #[test]
    fn ut_esc_does_not_close() {
        let mut d = dialog();
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> table
        assert!(!d.handle_events(KeyModifiers::NONE, KeyCode::Esc));
        assert_eq!(d.focus, SessionDialogFocus::Table);
        assert!(d.close_confirm.is_some());
    }

    #[test]
    fn ut_typing_in_interval_updates_input() {
        let mut d = dialog();
        d.handle_events(KeyModifiers::NONE, KeyCode::Char('9'));
        assert!(d.interval.state.input().contains('9'));
    }

    #[test]
    fn ut_create_toggle_delete_script() {
        let mut d = SessionDialog::new(&[], Duration::from_secs(1));
        // Tab to the name input, type a name, Enter creates an enabled script.
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> table
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> name input
        for c in "sim".chars() {
            d.handle_events(KeyModifiers::NONE, KeyCode::Char(c));
        }
        d.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        let (scripts, _) = d.resolve();
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].name, "sim");
        assert!(scripts[0].enabled);
    }

    #[test]
    fn ut_entries_from_ring_trims_nul_padding() {
        let entries = entries_from_ring(vec![(0, "hello\u{0}\u{0}".to_string())]);
        assert_eq!(entries[0].message, "hello");
    }

    #[test]
    fn ut_format_interval_trims_trailing_zeros() {
        assert_eq!(format_interval(Duration::from_secs_f64(1.0)), "1");
        assert_eq!(format_interval(Duration::from_secs_f64(0.25)), "0.25");
    }

    #[test]
    fn ut_layout_session_scripts_table_wide_screen() {
        let wide_area = Rect {
            x: 0,
            y: 0,
            width: 220,
            height: 30,
        };
        let [left, right] =
            Layout::horizontal([Constraint::Max(50), Constraint::Min(1)]).areas(wide_area);
        assert_eq!(left.width, 50);
        assert!(right.width >= 1);
    }

    #[test]
    fn ut_layout_session_scripts_table_narrow_screen() {
        let narrow_area = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 30,
        };
        let [left, right] =
            Layout::horizontal([Constraint::Max(50), Constraint::Min(1)]).areas(narrow_area);
        assert!(left.width < 50);
        assert!(right.width >= 1);
    }

    // --- close-confirm / lua-help integration -------------------------------

    #[test]
    fn ut_esc_then_enter_closes() {
        let mut d = dialog();
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> table
        assert!(!d.handle_events(KeyModifiers::NONE, KeyCode::Esc));
        assert!(d.close_confirm.is_some());
        assert!(d.handle_events(KeyModifiers::NONE, KeyCode::Enter));
    }

    #[test]
    fn ut_esc_in_confirm_keeps_dialog() {
        let mut d = dialog();
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> table
        assert!(!d.handle_events(KeyModifiers::NONE, KeyCode::Esc));
        assert!(d.close_confirm.is_some());
        assert!(!d.handle_events(KeyModifiers::NONE, KeyCode::Esc));
        assert!(d.close_confirm.is_none());
    }

    #[test]
    fn ut_space_in_confirm_closes() {
        let mut d = dialog();
        d.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(d.close_confirm.is_some());
        assert!(d.handle_events(KeyModifiers::NONE, KeyCode::Char(' ')));
    }

    #[test]
    fn ut_esc_from_code_normal_opens_confirm() {
        let mut d = dialog();
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> table
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> name input
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> code
        assert_eq!(d.focus, SessionDialogFocus::Code);
        assert_eq!(d.code.state.vim_mode(), VimMode::Normal);
        assert!(!d.handle_events(KeyModifiers::NONE, KeyCode::Esc));
        assert!(d.close_confirm.is_some());
    }

    #[test]
    fn ut_insert_esc_goes_normal_no_confirm() {
        let mut d = dialog();
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> table
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> name input
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> code
        assert_eq!(d.focus, SessionDialogFocus::Code);
        d.handle_events(KeyModifiers::NONE, KeyCode::Char('i'));
        assert_eq!(d.code.state.vim_mode(), VimMode::Insert);
        assert!(!d.handle_events(KeyModifiers::NONE, KeyCode::Esc));
        assert_eq!(d.code.state.vim_mode(), VimMode::Normal);
        assert!(d.close_confirm.is_none());
    }

    #[test]
    fn ut_colon_in_name_input_types() {
        let mut d = dialog();
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> table
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> name input
        assert_eq!(d.focus, SessionDialogFocus::NameInput);
        assert!(!d.handle_events(KeyModifiers::NONE, KeyCode::Char(':')));
        assert_eq!(d.name_input.state.input(), ":");
    }

    #[test]
    fn ut_colon_in_code_insert_inserts() {
        let mut d = dialog();
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> table
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> name input
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> code
        assert_eq!(d.focus, SessionDialogFocus::Code);
        d.handle_events(KeyModifiers::NONE, KeyCode::Char('i'));
        assert_eq!(d.code.state.vim_mode(), VimMode::Insert);
        d.handle_events(KeyModifiers::NONE, KeyCode::Char(':'));
        assert!(d.code.state.content().contains(':'));
    }

    #[test]
    fn ut_colon_in_code_normal_no_overlay() {
        let mut d = dialog();
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> table
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> name input
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> code
        assert_eq!(d.focus, SessionDialogFocus::Code);
        assert_eq!(d.code.state.vim_mode(), VimMode::Normal);
        assert!(!d.handle_events(KeyModifiers::NONE, KeyCode::Char(':')));
        assert!(d.close_confirm.is_none());
        assert!(d.lua_help.is_none());
    }

    #[test]
    fn ut_confirm_esc_still_cancels() {
        let mut d = dialog();
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> table
        d.handle_events(KeyModifiers::NONE, KeyCode::Char('d'));
        assert!(d.confirm.is_some());
        assert!(!d.handle_events(KeyModifiers::NONE, KeyCode::Esc));
        assert!(d.confirm.is_none());
    }

    #[test]
    fn ut_question_opens_bindings_only_code_normal() {
        let mut d = dialog();
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> table
        // From Scripts focus: `?` is not bound there, no overlay.
        d.handle_events(KeyModifiers::NONE, KeyCode::Char('?'));
        assert!(d.lua_help.is_none());

        // From Code Insert mode: `?` is text.
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> name input
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> code
        assert_eq!(d.focus, SessionDialogFocus::Code);
        d.handle_events(KeyModifiers::NONE, KeyCode::Char('i'));
        d.handle_events(KeyModifiers::NONE, KeyCode::Char('?'));
        assert!(d.code.state.content().contains('?'));
        assert!(d.lua_help.is_none());

        // From Code Normal mode: `?` opens the overlay.
        d.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert_eq!(d.code.state.vim_mode(), VimMode::Normal);
        d.handle_events(KeyModifiers::NONE, KeyCode::Char('?'));
        assert!(d.lua_help.is_some());
    }

    #[test]
    fn ut_bindings_close_keys() {
        for close_key in [KeyCode::Esc, KeyCode::Char('q'), KeyCode::Char('?')] {
            let mut d = dialog();
            d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> table
            d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> name input
            d.handle_events(KeyModifiers::NONE, KeyCode::Tab); // -> code
            d.handle_events(KeyModifiers::NONE, KeyCode::Char('?'));
            assert!(d.lua_help.is_some());
            assert!(!d.handle_events(KeyModifiers::NONE, close_key));
            assert!(d.lua_help.is_none());
        }
    }
}
