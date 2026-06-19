//! Overlay (modal dialog) lifecycle: creation dialog (`:new`/`:load`), tab creation.

use crossterm::event::{KeyCode, KeyModifiers};

use crate::module::modbus::setup::ModbusSetupView;
use crate::module::view::ModuleView;

use super::{App, Focus, Overlay, Tab};

impl App {
    pub(super) async fn handle_dialog_key(
        &mut self,
        modifiers: KeyModifiers,
        code: KeyCode,
    ) -> bool {
        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Esc) => self.close_overlay(),
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
            _ => {
                if let Some(o) = self.overlay.as_mut() {
                    o.handle_events(modifiers, code);
                }
            }
        }
        false
    }

    /// Confirm the active creation overlay. Creates a new tab when the dialog validates.
    async fn confirm_overlay(&mut self) {
        let action = match &self.overlay {
            Some(Overlay::Creation(sv)) => {
                sv.confirm().map(|(name, factory)| (name, factory()))
            }
            None => None,
        };
        if let Some((name, view)) = action {
            self.create_tab(name, view).await;
        }
    }

    /// Open the creation dialog for a new module tab (`:n`/`:new`).
    pub(super) fn enter_new(&mut self) {
        self.overlay = Some(Overlay::Creation(Box::new(ModbusSetupView::new_create())));
        self.focus = Focus::Dialog;
    }

    /// Open the creation dialog pre-filled with an optional device-config path (`:l`).
    pub(super) fn enter_load(&mut self, path: Option<&str>) {
        let mut sv = ModbusSetupView::new_create();
        if let Some(path) = path {
            sv.dialog_mut().config_path.state.set_input(path.to_string());
            sv.dialog_mut()
                .config_path
                .state
                .set_cursor(path.chars().count());
        }
        self.overlay = Some(Overlay::Creation(Box::new(sv)));
        self.focus = Focus::Dialog;
    }

    /// Create and append a new tab from a `Box<dyn ModuleView>`, start its module, then
    /// close the overlay.
    async fn create_tab(&mut self, name: String, view: Box<dyn ModuleView>) {
        self.tabs.push(Tab::new_from_view(name, view));
        self.active = self.tabs.len() - 1;
        let result = self.tabs[self.active].view.handle_command("start").await;
        if let crate::module::view::CommandResult::Handled(Some(msg)) = result {
            self.log_active(msg).await;
        }
        self.close_overlay();
    }
}
