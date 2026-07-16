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

#[cfg(test)]
mod tests {
    use super::*;
    use ferrowl_ui::state::InputFieldState;
    use ferrowl_ui::traits::SetFocus;

    #[derive(Default)]
    struct Host {
        add: Option<AddNamedValueDialog>,
        confirm: Option<ConfirmDeleteDialog>,
        name_error: Option<String>,
        accepted: Vec<NamedValue>,
    }

    impl SubDialogs for Host {
        fn add_dialog_opt(&self) -> Option<&AddNamedValueDialog> {
            self.add.as_ref()
        }
        fn add_dialog_slot(&mut self) -> &mut Option<AddNamedValueDialog> {
            &mut self.add
        }
        fn confirm_delete_opt(&self) -> Option<&ConfirmDeleteDialog> {
            self.confirm.as_ref()
        }
        fn confirm_delete_slot(&mut self) -> &mut Option<ConfirmDeleteDialog> {
            &mut self.confirm
        }
        fn name_error_slot(&mut self) -> &mut Option<String> {
            &mut self.name_error
        }
        fn register_label(&self) -> String {
            "reg".to_string()
        }
        fn accept_named_value(&mut self, nv: NamedValue) {
            self.accepted.push(nv);
        }
    }

    fn type_into(state: &mut InputFieldState, s: &str) {
        state.set_focused(true);
        for c in s.chars() {
            state.handle_events(KeyModifiers::NONE, KeyCode::Char(c));
        }
    }

    #[test]
    fn ut_add_dialog_open_close_and_route() {
        let mut h = Host::default();
        assert!(!h.has_sub_dialog());
        h.open_add_dialog();
        assert!(h.has_sub_dialog());
        // Routing / focus helpers run against the open dialog without panicking.
        h.add_dialog_focus_next();
        h.add_dialog_focus_previous();
        h.add_dialog_handle_events(KeyModifiers::NONE, KeyCode::Char('a'));
        h.close_add_dialog();
        assert!(!h.has_sub_dialog());
    }

    #[test]
    fn ut_confirm_add_accepts_valid_and_keeps_open_on_error() {
        let mut h = Host::default();
        h.open_add_dialog();
        // Empty fields fail validation: the dialog stays open with an error.
        h.confirm_add_dialog();
        assert!(h.has_sub_dialog());
        assert!(h.accepted.is_empty());
        // Fill valid label/value, then confirm: the value is accepted and the dialog closes.
        {
            let d = h.add_dialog_slot().as_mut().unwrap();
            type_into(&mut d.label.state, "Idle");
            type_into(&mut d.value.state, "0");
        }
        h.confirm_add_dialog();
        assert_eq!(h.accepted.len(), 1);
        assert_eq!(h.accepted[0].name, "Idle");
        assert!(!h.has_sub_dialog());
    }

    #[test]
    fn ut_confirm_delete_open_focus_and_close() {
        let mut h = Host::default();
        assert!(!h.has_confirm_delete());
        h.open_confirm_delete();
        assert!(h.has_confirm_delete());
        assert!(!h.confirm_delete_is_confirmed());
        h.confirm_delete_focus_next();
        assert!(h.confirm_delete_is_confirmed());
        h.confirm_delete_focus_previous();
        assert!(!h.confirm_delete_is_confirmed());
        h.close_confirm_delete();
        assert!(!h.has_confirm_delete());
    }

    #[test]
    fn ut_name_error_set_and_clear() {
        let mut h = Host::default();
        h.set_name_error("dup".into());
        assert_eq!(h.name_error, Some("dup".to_string()));
        h.clear_name_error();
        assert!(h.name_error.is_none());
    }
}
