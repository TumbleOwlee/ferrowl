//! Placeholder OCPP content view. Renders a stub message and owns a log channel so the
//! owning [`Tab`](crate::app::Tab) can wire it up; no protocol behaviour yet.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::EventResult;
use ratatui::Frame;
use ratatui::layout::Rect;
use tokio::sync::RwLock;

use crate::app::LogRing;
use crate::module::view::{
    CommandDescriptor, CommandFuture, CommandResult, ModuleView, RefreshFuture, SharedLog,
};

/// Empty OCPP module view — a named tab with a placeholder body.
pub struct OcppModuleView {
    name: String,
    log: SharedLog,
}

impl OcppModuleView {
    pub fn new(name: String) -> Self {
        Self {
            name,
            log: Arc::new(RwLock::new(LogRing::init())),
        }
    }
}

impl ModuleView for OcppModuleView {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _focused: bool) {
        use ferrowl_ui::{COLOR_SCHEME, style::TextStyle, widgets::TextBuilder};
        use ratatui::{
            layout::{Constraint, HorizontalAlignment, Layout},
            widgets::StatefulWidget,
        };

        // Vertically center a single line of placeholder text.
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
        let mut label = "OCPP module — not yet implemented".to_string();
        StatefulWidget::render(&widget, line_area, frame.buffer_mut(), &mut label);
    }

    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        EventResult::Unhandled(modifiers, code)
    }

    fn refresh<'a>(&'a mut self) -> RefreshFuture<'a> {
        Box::pin(async {})
    }

    fn handle_command<'a>(&'a mut self, cmd: &'a str) -> CommandFuture<'a> {
        let result = match cmd.trim() {
            "start" | "stop" | "restart" => {
                CommandResult::Handled(Some("OCPP module is a stub".into()))
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
}
