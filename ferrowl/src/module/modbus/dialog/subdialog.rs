//! Shared sub-dialog plumbing for the two edit dialogs.
//!
//! `EditInputDialog` and `EditSelectionDialog` both carry an optional add-value sub-dialog, an
//! optional delete-confirmation box and a name-conflict error. The open/close/route/confirm
//! logic around those is identical; this trait holds it once as default methods. Each dialog
//! only supplies the field accessors plus `accept_named_value` (what to do with a confirmed
//! new value, which genuinely differs between the two).

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::traits::HandleEvents;

use crate::config::device::NamedValue;

use super::{AddNamedValueDialog, ConfirmDeleteDialog};

pub trait SubDialogs {
    fn add_dialog_opt(&self) -> Option<&AddNamedValueDialog>;
    fn add_dialog_slot(&mut self) -> &mut Option<AddNamedValueDialog>;
    fn confirm_delete_opt(&self) -> Option<&ConfirmDeleteDialog>;
    fn confirm_delete_slot(&mut self) -> &mut Option<ConfirmDeleteDialog>;
    fn name_error_slot(&mut self) -> &mut Option<String>;
    /// Current register label, used in the delete-confirmation message.
    fn register_label(&self) -> String;
    /// Store a value confirmed in the add sub-dialog.
    fn accept_named_value(&mut self, nv: NamedValue);

    fn has_sub_dialog(&self) -> bool {
        self.add_dialog_opt().is_some()
    }

    fn open_add_dialog(&mut self) {
        *self.add_dialog_slot() = Some(AddNamedValueDialog::new());
    }

    fn close_add_dialog(&mut self) {
        *self.add_dialog_slot() = None;
    }

    fn confirm_add_dialog(&mut self) {
        let result = self.add_dialog_opt().map(|d| d.apply());
        match result {
            Some(Ok(nv)) => {
                self.accept_named_value(nv);
                *self.add_dialog_slot() = None;
            }
            Some(Err(e)) => {
                if let Some(d) = self.add_dialog_slot().as_mut() {
                    d.error.state = e;
                }
            }
            None => {}
        }
    }

    fn add_dialog_focus_next(&mut self) {
        if let Some(d) = self.add_dialog_slot().as_mut() {
            d.focus_next();
        }
    }

    fn add_dialog_focus_previous(&mut self) {
        if let Some(d) = self.add_dialog_slot().as_mut() {
            d.focus_previous();
        }
    }

    fn add_dialog_handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) {
        if let Some(d) = self.add_dialog_slot().as_mut() {
            let _ = d.handle_events(modifiers, code);
        }
    }

    fn has_confirm_delete(&self) -> bool {
        self.confirm_delete_opt().is_some()
    }

    fn open_confirm_delete(&mut self) {
        let name = self.register_label();
        *self.confirm_delete_slot() = Some(ConfirmDeleteDialog::new(&name));
    }

    fn close_confirm_delete(&mut self) {
        *self.confirm_delete_slot() = None;
    }

    fn confirm_delete_focus_next(&mut self) {
        if let Some(d) = self.confirm_delete_slot().as_mut() {
            d.focus_next();
        }
    }

    fn confirm_delete_focus_previous(&mut self) {
        if let Some(d) = self.confirm_delete_slot().as_mut() {
            d.focus_previous();
        }
    }

    fn confirm_delete_is_confirmed(&self) -> bool {
        self.confirm_delete_opt()
            .map(|d| d.is_confirm_focused())
            .unwrap_or(false)
    }

    fn set_name_error(&mut self, msg: String) {
        *self.name_error_slot() = Some(msg);
    }

    fn clear_name_error(&mut self) {
        *self.name_error_slot() = None;
    }
}
