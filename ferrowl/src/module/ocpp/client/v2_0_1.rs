//! OCPP 2.0.1 charging-station (client) view — placeholder. The full 2.0.1 client UI (its own
//! state schema, action set, and TransactionEvent shortcuts) is a later task; for now it renders a
//! stub and supports `:edit`, mirroring the server placeholder.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::EventResult;
use ferrowl_ui::traits::HandleEvents;
use ratatui::Frame;
use ratatui::layout::Rect;
use tokio::sync::RwLock;

use crate::app::LogRing;
use crate::module::ocpp::config::session::OcppSpec;
use crate::module::ocpp::setup_dialog::OcppSetupDialog;
use crate::module::view::{
    CommandDescriptor, CommandFuture, CommandResult, ModuleView, RefreshFuture, SharedLog,
};

/// Placeholder OCPP 2.0.1 client view.
pub struct OcppClientV201View {
    spec: OcppSpec,
    log: SharedLog,
    setup_overlay: Option<OcppSetupDialog>,
}

impl OcppClientV201View {
    pub fn new(spec: OcppSpec) -> Self {
        Self {
            spec,
            log: Arc::new(RwLock::new(LogRing::init())),
            setup_overlay: None,
        }
    }
}

impl ModuleView for OcppClientV201View {
    fn name(&self) -> String {
        self.spec.name.clone()
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _focused: bool) {
        use ferrowl_ui::{COLOR_SCHEME, style::TextStyle, widgets::TextBuilder};
        use ratatui::{
            layout::{Constraint, HorizontalAlignment, Layout},
            widgets::StatefulWidget,
        };

        let [_, line_area, _] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(area);
        let widget = TextBuilder::default()
            .horizontal_alignment(HorizontalAlignment::Center)
            .style(TextStyle {
                general: ratatui::prelude::Style::default()
                    .fg(COLOR_SCHEME.hi)
                    .bg(COLOR_SCHEME.bg),
            })
            .build()
            .unwrap();
        let mut label = "OCPP 2.0.1 client — not yet implemented".to_string();
        StatefulWidget::render(&widget, line_area, frame.buffer_mut(), &mut label);

        if let Some(setup) = self.setup_overlay.as_mut() {
            setup.render(area, frame.buffer_mut());
        }
    }

    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        if let Some(setup) = self.setup_overlay.as_mut() {
            match (modifiers, code) {
                (KeyModifiers::NONE, KeyCode::Esc) => self.setup_overlay = None,
                (KeyModifiers::NONE, KeyCode::Enter) => self.setup_overlay = None,
                (KeyModifiers::NONE, KeyCode::Tab) => setup.focus_next(),
                (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => {
                    setup.focus_previous()
                }
                _ => {
                    let _ = setup.handle_events(modifiers, code);
                }
            }
            return EventResult::Consumed;
        }
        EventResult::Unhandled(modifiers, code)
    }

    fn refresh<'a>(&'a mut self) -> RefreshFuture<'a> {
        Box::pin(async {})
    }

    fn handle_command<'a>(&'a mut self, cmd: &'a str) -> CommandFuture<'a> {
        let result = match cmd.trim() {
            "start" | "stop" | "restart" => {
                CommandResult::Handled(Some("OCPP 2.0.1 client is a stub".into()))
            }
            "edit" | "e" => {
                self.setup_overlay = Some(OcppSetupDialog::edit(&self.spec));
                CommandResult::Handled(None)
            }
            _ => CommandResult::Unhandled,
        };
        Box::pin(std::future::ready(result))
    }

    fn commands(&self) -> &[CommandDescriptor] {
        &[]
    }

    fn log(&self) -> SharedLog {
        self.log.clone()
    }

    fn session_spec(&self) -> Option<serde_json::Value> {
        let mut v = serde_json::to_value(&self.spec).ok()?;
        v.as_object_mut()?.insert("type".into(), "ocpp".into());
        Some(v)
    }
}
