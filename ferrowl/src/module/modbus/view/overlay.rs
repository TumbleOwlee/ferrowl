//! The register-edit overlay: deferred actions plus the open-dialog dispatch enum.

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::Rect;

use crate::dialog::close_confirm::CloseConfirmEvent;
use crate::module::modbus::dialog::{
    EditInputDialog, EditSelectionDialog, EditedRegister, RegisterDialog,
};
use crate::module::modbus::setup_dialog::SetupValues;

/// Deferred async work produced by a dialog confirmation.
pub(super) enum PendingAction {
    Add(EditedRegister),
    Edit {
        edited: EditedRegister,
        idx: usize,
        original_name: String,
    },
    Delete(String),
    ApplySetup(Box<SetupValues>),
}

/// Which pane kind the open register dialog currently shows: free-text inputs, or the
/// named-value selection variant (used for a register with declared aliases, or MB-R-229's
/// boolean-kind fixed pair). Orthogonal to whether the dialog is adding or editing.
// Already behind `Box<ModbusOverlay>` at every call site (`ModbusViewOverlay::Register`), so the
// ~200-byte gap between variants costs nothing extra on the stack.
#[allow(clippy::large_enum_variant)]
pub(super) enum RegisterDialogKind {
    Input(EditInputDialog),
    Selection(EditSelectionDialog<crate::config::device::NamedValue>),
}

/// Internal register-edit/add overlay state. `Add`/`Edit` track why the dialog is open —
/// confirming decides whether a register is appended (MB-R-237) or replaces the selected row in
/// place (MB-R-238) — independently of which pane kind (`RegisterDialogKind`) is currently shown
/// (MB-R-239).
pub(super) enum ModbusOverlay {
    Add(RegisterDialogKind),
    Edit(RegisterDialogKind),
}

impl ModbusOverlay {
    /// The open dialog as a shared [`RegisterDialog`] trait object. Both the typed
    /// (`Input`) and selection (`Selection`) kinds implement the trait, so the per-method
    /// forwarders below dispatch through one place instead of re-matching.
    pub(super) fn inner(&self) -> &dyn RegisterDialog {
        let (ModbusOverlay::Add(kind) | ModbusOverlay::Edit(kind)) = self;
        match kind {
            RegisterDialogKind::Input(d) => d,
            RegisterDialogKind::Selection(d) => d,
        }
    }

    /// The open dialog as a mutable [`RegisterDialog`] trait object.
    pub(super) fn inner_mut(&mut self) -> &mut dyn RegisterDialog {
        let (ModbusOverlay::Add(kind) | ModbusOverlay::Edit(kind)) = self;
        match kind {
            RegisterDialogKind::Input(d) => d,
            RegisterDialogKind::Selection(d) => d,
        }
    }

    fn kind(&self) -> &RegisterDialogKind {
        let (ModbusOverlay::Add(kind) | ModbusOverlay::Edit(kind)) = self;
        kind
    }

    /// Rebuild the overlay around a new pane kind, preserving whether it is `Add` or `Edit`.
    fn with_kind(&self, kind: RegisterDialogKind) -> ModbusOverlay {
        match self {
            ModbusOverlay::Add(_) => ModbusOverlay::Add(kind),
            ModbusOverlay::Edit(_) => ModbusOverlay::Edit(kind),
        }
    }

    pub(super) fn render(&mut self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        self.inner_mut().render(area, buf)
    }

    pub(super) fn focus_next(&mut self) {
        self.inner_mut().focus_next()
    }

    pub(super) fn focus_previous(&mut self) {
        self.inner_mut().focus_previous()
    }

    pub(super) fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) {
        self.inner_mut().handle_events(modifiers, code)
    }

    pub(super) fn clear_name_error(&mut self) {
        self.inner_mut().clear_name_error()
    }

    pub(super) fn has_confirm_delete(&self) -> bool {
        self.inner().has_confirm_delete()
    }

    pub(super) fn confirm_delete_is_confirmed(&self) -> bool {
        self.inner().confirm_delete_is_confirmed()
    }

    pub(super) fn close_confirm_delete(&mut self) {
        self.inner_mut().close_confirm_delete()
    }

    pub(super) fn open_confirm_delete(&mut self) {
        self.inner_mut().open_confirm_delete()
    }

    pub(super) fn confirm_delete_focus_next(&mut self) {
        self.inner_mut().confirm_delete_focus_next()
    }

    pub(super) fn confirm_delete_focus_previous(&mut self) {
        self.inner_mut().confirm_delete_focus_previous()
    }

    pub(super) fn has_sub_dialog(&self) -> bool {
        self.inner().has_sub_dialog()
    }

    pub(super) fn close_add_dialog(&mut self) {
        self.inner_mut().close_add_dialog()
    }

    pub(super) fn confirm_add_dialog(&mut self) {
        self.inner_mut().confirm_add_dialog()
    }

    pub(super) fn add_dialog_focus_next(&mut self) {
        self.inner_mut().add_dialog_focus_next()
    }

    pub(super) fn add_dialog_focus_previous(&mut self) {
        self.inner_mut().add_dialog_focus_previous()
    }

    pub(super) fn add_dialog_handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) {
        self.inner_mut().add_dialog_handle_events(modifiers, code)
    }

    pub(super) fn is_confirm_button_focused(&self) -> bool {
        self.inner().is_confirm_button_focused()
    }

    pub(super) fn is_delete_register_button_focused(&self) -> bool {
        self.inner().is_delete_register_button_focused()
    }

    pub(super) fn handle_space(&mut self) {
        self.inner_mut().handle_space()
    }

    pub(super) fn set_name_error(&mut self, msg: String) {
        self.inner_mut().set_name_error(msg)
    }

    pub(super) fn apply(&self) -> Option<EditedRegister> {
        self.inner().apply().ok()
    }

    pub(super) fn close_confirm_is_active(&self) -> bool {
        self.inner().close_confirm_is_active()
    }

    pub(super) fn close_confirm_open(&mut self) {
        self.inner_mut().close_confirm_open()
    }

    pub(super) fn close_confirm_handle_key(
        &mut self,
        modifiers: KeyModifiers,
        code: KeyCode,
    ) -> CloseConfirmEvent {
        self.inner_mut().close_confirm_handle_key(modifiers, code)
    }

    pub(super) fn is_add(&self) -> bool {
        matches!(self, ModbusOverlay::Add(_))
    }

    pub(super) fn maybe_switch_to_selection(&self) -> Option<ModbusOverlay> {
        match self.kind() {
            RegisterDialogKind::Input(d)
                if !d.pending_named_values.is_empty() || d.is_boolean_kind() =>
            {
                Some(self.with_kind(RegisterDialogKind::Selection(d.to_edit_selection_dialog())))
            }
            _ => None,
        }
    }

    pub(super) fn maybe_switch_to_input(&self) -> Option<ModbusOverlay> {
        match self.kind() {
            RegisterDialogKind::Selection(d)
                if d.value.state.values().is_empty()
                    || (!d.is_boolean_kind()
                        && d.value_source
                            == crate::module::modbus::dialog::NamedValueSource::BooleanFixed) =>
            {
                Some(self.with_kind(RegisterDialogKind::Input(d.to_edit_input_dialog())))
            }
            _ => None,
        }
    }
}
