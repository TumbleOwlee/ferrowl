//! Overlay (modal dialog) lifecycle: creation dialog (`:new`/`:load`), tab creation.

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::EventResult;

use crate::module::MODULE_TYPES;
use crate::module::modbus::setup::ModbusSetupView;
use crate::module::type_select::TypeSelectDialog;
use crate::module::view::ModuleView;

use super::{App, Focus, Overlay, Tab};

impl App {
    pub(super) async fn handle_dialog_key(
        &mut self,
        modifiers: KeyModifiers,
        code: KeyCode,
    ) -> bool {
        // Offer the key to the active dialog first; only fall back to the default Esc/Enter/
        // Tab/BackTab handling when the dialog leaves it unhandled. This is behavior-preserving
        // today (no widget state consumes those keys) and lets a future popup widget intercept
        // them while open.
        let result = match self.overlay.as_mut() {
            Some(o) => o.handle_events(modifiers, code),
            None => EventResult::Unhandled(modifiers, code),
        };
        if let EventResult::Unhandled(modifiers, code) = result {
            match (modifiers, code) {
                (KeyModifiers::NONE, KeyCode::Enter) => self.confirm_overlay().await,
                (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => {
                    if let Some(o) = self.overlay.as_mut() {
                        o.focus_previous();
                    }
                }
                (KeyModifiers::NONE, KeyCode::Tab) => {
                    if let Some(o) = self.overlay.as_mut() {
                        o.focus_next();
                    }
                }
                _ => {}
            }
        }
        // A confirmed close popup in the type-selector or the active creation dialog requests a close.
        let close_requested = match self.overlay.as_mut() {
            Some(Overlay::TypeSelect(d)) => d.take_close_request(),
            Some(Overlay::Creation(sv)) => sv.close_requested(),
            None => false,
        };
        if close_requested {
            self.close_overlay();
        }
        false
    }

    /// Confirm the active creation overlay.
    ///
    /// From the type selector this swaps in the chosen module type's setup dialog; from a
    /// setup dialog it creates a new tab when the dialog validates.
    async fn confirm_overlay(&mut self) {
        // Stage 1: type chosen -> open that module type's setup dialog.
        if let Some(Overlay::TypeSelect(d)) = &self.overlay {
            let setup = (MODULE_TYPES[d.selected_index()].new_setup_view)();
            self.overlay = Some(Overlay::Creation(setup));
            return;
        }

        // Stage 2: setup dialog confirmed -> create the tab.
        let action = match &self.overlay {
            Some(Overlay::Creation(sv)) => sv.confirm().map(|(name, factory)| (name, factory())),
            _ => None,
        };
        if let Some((name, view)) = action {
            if self.tabs.iter().any(|t| t.name == name) {
                // No error slot on `SetupView` to surface this in-dialog (the field-level
                // red-border validation is purely static, see `dialog::NonEmpty`); leave the
                // dialog open and nudge via the active tab's log instead.
                self.log_active(format!("Name '{name}' already in use by another tab"))
                    .await;
                return;
            }
            self.create_tab(name, view).await;
        }
    }

    /// Open the module-type selector for a new module tab (`:n`/`:new`).
    pub(super) fn enter_new(&mut self) {
        self.set_content_focus(false);
        self.overlay = Some(Overlay::TypeSelect(Box::new(TypeSelectDialog::new())));
        self.focus = Focus::Dialog;
    }

    /// Open the session-level scripts/interval dialog (`:session`).
    pub(super) fn enter_session(&mut self) {
        self.set_content_focus(false);
        self.session_dialog = Some(Box::new(crate::dialog::session::SessionDialog::new(
            &self.session_scripts,
            self.session_interval,
        )));
        self.focus = Focus::Dialog;
    }

    /// Open the creation dialog pre-filled with an optional device-config path (`:l`).
    pub(super) fn enter_load(&mut self, path: Option<&str>) {
        let mut sv = ModbusSetupView::new_create();
        if let Some(path) = path {
            sv.dialog_mut()
                .config_path
                .state
                .set_input(path.to_string());
            sv.dialog_mut()
                .config_path
                .state
                .set_cursor(path.chars().count());
        }
        self.set_content_focus(false);
        self.overlay = Some(Overlay::Creation(Box::new(sv)));
        self.focus = Focus::Dialog;
    }

    /// Create and append a new tab from a `Box<dyn ModuleView>`, start its module, then
    /// close the overlay.
    async fn create_tab(&mut self, name: String, view: Box<dyn ModuleView>) {
        self.tabs.push(Tab::new_from_view(name, view));
        self.active = self.tabs.len() - 1;
        self.rebuild_registry();
        let result = self.tabs[self.active].view.handle_command("start").await;
        if let crate::module::view::CommandResult::Handled(Some(msg)) = result {
            self.log_active(msg).await;
        }
        self.close_overlay();
    }
}
