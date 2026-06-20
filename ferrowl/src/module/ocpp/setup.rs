//! OCPP creation dialog wrapper. Implements [`SetupView`] over [`OcppSetupDialog`] and, on
//! confirm, builds the matching view for the chosen role (client → full CS view, server →
//! placeholder).

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::traits::HandleEvents;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::module::ocpp::client::build_client_view;
use crate::module::ocpp::config::session::OcppRole;
use crate::module::ocpp::setup_dialog::OcppSetupDialog;
use crate::module::ocpp::view::OcppServerView;
use crate::module::type_descriptor::{ModuleViewFactory, SetupView};

/// Setup dialog for the OCPP module type.
pub struct OcppSetupView {
    dialog: OcppSetupDialog,
}

impl OcppSetupView {
    pub fn new() -> Self {
        Self {
            dialog: OcppSetupDialog::new(),
        }
    }
}

impl SetupView for OcppSetupView {
    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        self.dialog.render(area, buf);
    }

    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) {
        let _ = self.dialog.handle_events(modifiers, code);
    }

    fn focus_next(&mut self) {
        self.dialog.focus_next();
    }

    fn focus_previous(&mut self) {
        self.dialog.focus_previous();
    }

    fn confirm(&self) -> Option<(String, ModuleViewFactory)> {
        let spec = self.dialog.resolve().ok()?;
        let name = spec.name.clone();
        let factory: ModuleViewFactory = match spec.role {
            OcppRole::Client => Box::new(move || build_client_view(spec)),
            OcppRole::Server => Box::new(move || Box::new(OcppServerView::new(spec))),
        };
        Some((name, factory))
    }
}
