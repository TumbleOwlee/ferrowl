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
    ApplySetup(SetupValues),
}

/// Internal register-edit/add overlay state.
pub(super) enum ModbusOverlay {
    Edit(EditInputDialog),
    EditSelection(EditSelectionDialog<crate::config::device::NamedValue>),
    Add(EditInputDialog),
}

impl ModbusOverlay {
    /// The open dialog as a shared [`RegisterDialog`] trait object. Both the typed
    /// (`Edit`/`Add` → `EditInputDialog`) and selection (`EditSelection`) variants implement the
    /// trait, so the per-method forwarders below dispatch through one place instead of re-matching.
    pub(super) fn inner(&self) -> &dyn RegisterDialog {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d,
            ModbusOverlay::EditSelection(d) => d,
        }
    }

    /// The open dialog as a mutable [`RegisterDialog`] trait object.
    pub(super) fn inner_mut(&mut self) -> &mut dyn RegisterDialog {
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d) => d,
            ModbusOverlay::EditSelection(d) => d,
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
        match self {
            ModbusOverlay::Edit(d) | ModbusOverlay::Add(d)
                if !d.pending_named_values.is_empty() =>
            {
                Some(ModbusOverlay::EditSelection(d.to_edit_selection_dialog()))
            }
            _ => None,
        }
    }

    pub(super) fn maybe_switch_to_input(&self) -> Option<ModbusOverlay> {
        match self {
            ModbusOverlay::EditSelection(d) if d.value.state.values().is_empty() => {
                Some(ModbusOverlay::Edit(d.to_edit_input_dialog()))
            }
            _ => None,
        }
    }
}
