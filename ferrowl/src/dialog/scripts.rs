//! Lua script manager dialog, shared by the OCPP views and the Modbus view. Left: a table of scripts with an
//! On/Off status over a "New Script" name input; right: a code editor for the selected script.
//! Edits are live on a working copy; the view reads it back via [`ScriptDialog::resolve`] on close
//! and reloads the simulation. `t` toggles a script, `d` deletes (with confirmation), `c` toggles
//! compact rows, Enter in the name input creates a new (enabled) script. `Esc` opens a close
//! confirmation popup (Enter/Space confirms, Esc dismisses it); `?` from the code editor's Normal
//! mode opens the Lua bindings overlay.

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::{
    COLOR_SCHEME,
    state::{CodeInputFieldState, InputFieldState, VimMode},
    traits::{HandleEvents, SetFocus},
    widgets::{CodeInputField, InputField, Widget},
};
use ferrowl_ui_derive::{Focus, focusable};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, Clear, StatefulWidget, Widget as UiWidget},
};

use crate::config::script::ScriptDef;
use crate::dialog::close_confirm::{CloseConfirmDialog, CloseConfirmOutcome, route_close_confirm};
use crate::dialog::lua_help::{LuaHelpOutcome, LuaHelpOverlay, ScriptContext, route_lua_help};
use crate::dialog::script_manager::{self, ScriptManagerRef, ScriptTable};
use crate::module::modbus::dialog::{
    ConfirmDeleteDialog, DeleteConfirmOutcome, route_delete_confirm,
};
use crate::view::border_style;

/// The script manager dialog. Works on a private copy of the script list; the view applies the
/// result on close.
#[focusable]
#[derive(Focus)]
pub struct ScriptDialog {
    scripts: Vec<ScriptDef>,
    #[focus]
    table: ScriptTable,
    #[focus]
    name_input: Widget<InputFieldState, InputField<String>>,
    #[focus]
    code: Widget<CodeInputFieldState, CodeInputField>,
    confirm: Option<ConfirmDeleteDialog>,
    /// Compact (no vertical row margin) script table; toggled with `c`. Default off (margin 1).
    compact: bool,
    close_confirm: Option<CloseConfirmDialog>,
    context: ScriptContext,
    lua_help: Option<LuaHelpOverlay>,
}

impl ScriptDialog {
    pub fn new(scripts: &[ScriptDef], context: ScriptContext) -> Self {
        let scripts = scripts.to_vec();
        let mut dialog = Self {
            table: script_manager::script_table(script_manager::rows(&scripts)),
            name_input: script_manager::name_input(border_style()),
            code: script_manager::code_editor(border_style()),
            focus: ScriptDialogFocus::Table,
            view_focused: true,
            confirm: None,
            compact: false,
            close_confirm: None,
            context,
            lua_help: None,
            scripts,
        };
        dialog.sync_code_from_selection();
        dialog
    }

    /// Apply the working copy back to the caller. Flushes the open editor into the selected script
    /// first so unsaved keystrokes aren't lost.
    pub fn resolve(mut self) -> Vec<ScriptDef> {
        self.flush_code_to_selection();
        self.scripts
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
        match route_lua_help(&mut self.lua_help, modifiers, code) {
            LuaHelpOutcome::NotActive => {}
            LuaHelpOutcome::Consumed => return false,
        }

        // The close-confirm popup takes precedence once open.
        match route_close_confirm(&mut self.close_confirm, modifiers, code) {
            CloseConfirmOutcome::NotActive => {}
            CloseConfirmOutcome::Close => return true,
            CloseConfirmOutcome::Consumed => return false,
        }

        // Delete-confirmation sub-dialog takes precedence.
        match route_delete_confirm(&mut self.confirm, modifiers, code) {
            DeleteConfirmOutcome::NotActive => {}
            DeleteConfirmOutcome::Confirmed => {
                self.delete_selected();
                return false;
            }
            DeleteConfirmOutcome::Consumed => return false,
        }

        // Intercept `?` before offering the key to the editor, but only in the code editor's
        // Normal mode: in Insert/Visual mode it is valid Lua text and must fall through
        // unchanged.
        if self.focus == ScriptDialogFocus::Code
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
        if self.focus == ScriptDialogFocus::Code
            && let ferrowl_ui::EventResult::Consumed =
                self.code.state.handle_events(modifiers, code)
        {
            self.flush_code_to_selection();
            return false;
        }

        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Tab) => {
                self.focus_next();
                if self.focus == ScriptDialogFocus::Code && self.selected().is_none() {
                    self.focus_next();
                }
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => {
                self.focus_previous();
                if self.focus == ScriptDialogFocus::Code && self.selected().is_none() {
                    self.focus_previous();
                }
            }
            (KeyModifiers::NONE, KeyCode::Esc) => {
                self.close_confirm = Some(CloseConfirmDialog::new());
            }
            _ => match self.focus {
                ScriptDialogFocus::Table => match (modifiers, code) {
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
                ScriptDialogFocus::NameInput => match (modifiers, code) {
                    (KeyModifiers::NONE, KeyCode::Enter) => self.create_script(),
                    _ => {
                        let _ = self.name_input.state.handle_events(modifiers, code);
                    }
                },
                // Code-focus keys were already offered to the editor above; anything
                // reaching this arm was left unhandled by it.
                ScriptDialogFocus::Code => {}
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
            Constraint::Percentage(10),
            Constraint::Percentage(80),
            Constraint::Percentage(10),
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
            .title("Lua Scripts");
        let block_inner = block.inner(vc);
        block.render(vc, buf);
        let inner = block_inner.inner(Margin {
            vertical: 1,
            horizontal: 2,
        });

        let [left, right] =
            Layout::horizontal([Constraint::Max(50), Constraint::Min(1)]).areas(inner);
        let [list_area, input_area] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).areas(left);

        self.table
            .state
            .set_focused(self.focus == ScriptDialogFocus::Table);
        self.name_input
            .state
            .set_focused(self.focus == ScriptDialogFocus::NameInput);
        self.code
            .state
            .set_focused(self.focus == ScriptDialogFocus::Code);

        StatefulWidget::render(&self.table.widget, list_area, buf, &mut self.table.state);
        StatefulWidget::render(
            &self.name_input.widget,
            input_area,
            buf,
            &mut self.name_input.state,
        );
        StatefulWidget::render(&self.code.widget, right, buf, &mut self.code.state);

        if let Some(confirm) = self.confirm.as_mut() {
            confirm.render(area, buf);
        }

        if let Some(help) = self.lua_help.as_mut() {
            help.render(area, buf, self.context);
        }

        if let Some(confirm) = self.close_confirm.as_mut() {
            confirm.render(area, buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog() -> ScriptDialog {
        ScriptDialog::new(
            &[ScriptDef {
                name: "boot".into(),
                code: String::new(),
                enabled: true,
            }],
            ScriptContext::Modbus,
        )
    }

    #[test]
    fn code_editor_disabled_and_skipped_without_selection() {
        let mut d = ScriptDialog::new(&[], ScriptContext::Modbus);
        assert!(d.code.state.disabled());
        // Tab cycles Table -> NameInput -> Table, never landing on Code.
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        assert_eq!(d.focus, ScriptDialogFocus::NameInput);
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        assert_eq!(d.focus, ScriptDialogFocus::Table);
        // BackTab from Table skips Code in the other direction.
        d.handle_events(KeyModifiers::NONE, KeyCode::BackTab);
        assert_eq!(d.focus, ScriptDialogFocus::NameInput);
    }

    #[test]
    fn code_editor_reachable_with_selection() {
        let mut d = dialog();
        assert!(!d.code.state.disabled());
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        assert_eq!(d.focus, ScriptDialogFocus::Code);
    }

    #[test]
    fn deleting_last_script_disables_code_editor() {
        let mut d = dialog();
        assert!(!d.code.state.disabled());
        d.delete_selected();
        assert!(d.code.state.disabled());
    }

    #[test]
    fn creating_first_script_enables_code_editor() {
        let mut d = ScriptDialog::new(&[], ScriptContext::Modbus);
        assert!(d.code.state.disabled());
        d.focus = ScriptDialogFocus::NameInput;
        for c in "boot".chars() {
            d.handle_events(KeyModifiers::NONE, KeyCode::Char(c));
        }
        d.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(!d.code.state.disabled());
    }

    #[test]
    fn c_toggles_compact_in_list_not_in_name_input() {
        let mut d = dialog();
        assert!(!d.compact);
        // `c` on the focused script list toggles compact.
        d.handle_events(KeyModifiers::NONE, KeyCode::Char('c'));
        assert!(d.compact);
        d.handle_events(KeyModifiers::NONE, KeyCode::Char('c'));
        assert!(!d.compact);
        // `c` in the name input is text, not a compact toggle.
        d.focus = ScriptDialogFocus::NameInput;
        d.handle_events(KeyModifiers::NONE, KeyCode::Char('c'));
        assert!(!d.compact);
        assert_eq!(d.name_input.state.input(), "c");
    }

    #[test]
    fn layout_scripts_table_wide_screen() {
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
    fn layout_scripts_table_narrow_screen() {
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
    fn esc_then_enter_closes() {
        let mut d = dialog();
        assert!(!d.handle_events(KeyModifiers::NONE, KeyCode::Esc));
        assert!(d.close_confirm.is_some());
        assert!(d.handle_events(KeyModifiers::NONE, KeyCode::Enter));
    }

    #[test]
    fn esc_does_not_close() {
        let mut d = dialog();
        assert!(!d.handle_events(KeyModifiers::NONE, KeyCode::Esc));
        assert!(d.close_confirm.is_some());
        assert_eq!(d.focus, ScriptDialogFocus::Table);
        d.close_confirm = None;
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        assert_eq!(d.focus, ScriptDialogFocus::Code);
        assert_eq!(d.code.state.vim_mode(), VimMode::Normal);
        assert!(!d.handle_events(KeyModifiers::NONE, KeyCode::Esc));
        assert!(d.close_confirm.is_some());
    }

    #[test]
    fn esc_in_confirm_keeps_dialog() {
        let mut d = dialog();
        assert!(!d.handle_events(KeyModifiers::NONE, KeyCode::Esc));
        assert!(d.close_confirm.is_some());
        assert!(!d.handle_events(KeyModifiers::NONE, KeyCode::Esc));
        assert!(d.close_confirm.is_none());
    }

    #[test]
    fn space_in_confirm_closes() {
        let mut d = dialog();
        d.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(d.close_confirm.is_some());
        assert!(d.handle_events(KeyModifiers::NONE, KeyCode::Char(' ')));
    }

    #[test]
    fn esc_from_code_normal_opens_confirm() {
        let mut d = dialog();
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        assert_eq!(d.focus, ScriptDialogFocus::Code);
        assert_eq!(d.code.state.vim_mode(), VimMode::Normal);
        assert!(!d.handle_events(KeyModifiers::NONE, KeyCode::Esc));
        assert!(d.close_confirm.is_some());
    }

    #[test]
    fn insert_esc_goes_normal_no_confirm() {
        let mut d = dialog();
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        assert_eq!(d.focus, ScriptDialogFocus::Code);
        d.handle_events(KeyModifiers::NONE, KeyCode::Char('i'));
        assert_eq!(d.code.state.vim_mode(), VimMode::Insert);
        assert!(!d.handle_events(KeyModifiers::NONE, KeyCode::Esc));
        assert_eq!(d.code.state.vim_mode(), VimMode::Normal);
        assert!(d.close_confirm.is_none());
    }

    #[test]
    fn colon_in_name_input_types() {
        let mut d = dialog();
        d.focus = ScriptDialogFocus::NameInput;
        assert!(!d.handle_events(KeyModifiers::NONE, KeyCode::Char(':')));
        assert_eq!(d.name_input.state.input(), ":");
    }

    #[test]
    fn colon_in_code_insert_inserts() {
        let mut d = dialog();
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        assert_eq!(d.focus, ScriptDialogFocus::Code);
        d.handle_events(KeyModifiers::NONE, KeyCode::Char('i'));
        assert_eq!(d.code.state.vim_mode(), VimMode::Insert);
        d.handle_events(KeyModifiers::NONE, KeyCode::Char(':'));
        assert!(d.code.state.content().contains(':'));
    }

    #[test]
    fn colon_in_code_normal_no_overlay() {
        let mut d = dialog();
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        assert_eq!(d.focus, ScriptDialogFocus::Code);
        assert_eq!(d.code.state.vim_mode(), VimMode::Normal);
        assert!(!d.handle_events(KeyModifiers::NONE, KeyCode::Char(':')));
        assert!(d.close_confirm.is_none());
        assert!(d.lua_help.is_none());
    }

    #[test]
    fn confirm_esc_still_cancels() {
        let mut d = dialog();
        d.handle_events(KeyModifiers::NONE, KeyCode::Char('d'));
        assert!(d.confirm.is_some());
        assert!(!d.handle_events(KeyModifiers::NONE, KeyCode::Esc));
        assert!(d.confirm.is_none());
    }

    #[test]
    fn question_opens_bindings_only_code_normal() {
        let mut d = dialog();
        // From Table: no overlay opens; `?` is not bound there.
        d.handle_events(KeyModifiers::NONE, KeyCode::Char('?'));
        assert!(d.lua_help.is_none());

        // From Code Insert mode: `?` is text.
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        d.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        assert_eq!(d.focus, ScriptDialogFocus::Code);
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
    fn bindings_close_keys() {
        for close_key in [KeyCode::Esc, KeyCode::Char('q'), KeyCode::Char('?')] {
            let mut d = dialog();
            d.handle_events(KeyModifiers::NONE, KeyCode::Tab);
            d.handle_events(KeyModifiers::NONE, KeyCode::Tab);
            d.handle_events(KeyModifiers::NONE, KeyCode::Char('?'));
            assert!(d.lua_help.is_some());
            assert!(!d.handle_events(KeyModifiers::NONE, close_key));
            assert!(d.lua_help.is_none());
        }
    }
}
