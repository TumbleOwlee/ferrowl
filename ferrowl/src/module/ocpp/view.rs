//! Placeholder OCPP **server** (CSMS) view. The full management-system view is a later task;
//! for now it renders a stub message, owns a log channel so the tab can wire it up, and supports
//! `:edit` so the OCPP role/settings can be changed (switching to client replaces this view).

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::EventResult;
use ferrowl_ui::traits::HandleEvents;
use ratatui::Frame;
use ratatui::layout::Rect;
use tokio::sync::RwLock;

use crate::app::LogRing;
use crate::module::ocpp::client::build_client_view;
use crate::module::ocpp::config::device::OcppDeviceConfig;
use crate::module::ocpp::config::session::{OcppModuleSpec, OcppRole, OcppSpec};
use crate::module::ocpp::setup_dialog::OcppSetupDialog;
use crate::module::view::{
    CommandDescriptor, CommandFuture, CommandResult, ModuleView, RefreshFuture, SharedLog,
};

/// Placeholder OCPP server view — a named tab with a stub body and an editable setup.
pub struct OcppServerView {
    spec: OcppSpec,
    /// Path to the OCPP device-config file backing this module (empty = none yet).
    device_path: String,
    /// Device config (role/version/timeout/scripts). Scripts are unused while in server role but
    /// preserved across edits and persisted by `:wd`.
    device: OcppDeviceConfig,
    log: SharedLog,
    setup_overlay: Option<OcppSetupDialog>,
    pending_setup: Option<(OcppSpec, String)>,
    replacement: Option<Box<dyn ModuleView>>,
}

impl OcppServerView {
    pub fn new(spec: OcppSpec, device_path: String, device: OcppDeviceConfig) -> Self {
        Self {
            spec,
            device_path,
            device,
            log: Arc::new(RwLock::new(LogRing::init())),
            setup_overlay: None,
            pending_setup: None,
            replacement: None,
        }
    }

    /// Write the device config (reconciled with the live spec, scripts preserved) to `path`,
    /// stamping the ferrowl version. Mirrors the Modbus `:wd`.
    fn save_device_to(&self, path: &str) -> CommandResult {
        use ferrowl_util::convert::{Converter, FileType};
        let Some(ty) = FileType::from_path(path) else {
            return CommandResult::Handled(Some(format!(
                "unknown format for '{path}' (use .toml or .json)"
            )));
        };
        let mut device = OcppDeviceConfig::from_spec(&self.spec, self.device.scripts.clone());
        device.version = Some(crate::config::VERSION.to_string());
        match Converter::save(&device, path, ty) {
            Ok(()) => CommandResult::Handled(Some(format!("Saved device config to {path}"))),
            Err(e) => CommandResult::Handled(Some(format!("Save failed: {e:?}"))),
        }
    }
}

impl ModuleView for OcppServerView {
    fn name(&self) -> String {
        self.spec.name.clone()
    }

    fn is_overlay_active(&self) -> bool {
        self.pending_setup.is_some()
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
        let mut label = "OCPP server (CSMS) — not yet implemented".to_string();
        StatefulWidget::render(&widget, line_area, frame.buffer_mut(), &mut label);

        // Setup dialog (`:edit`) on top of everything.
        if let Some(setup) = self.setup_overlay.as_mut() {
            setup.render(area, frame.buffer_mut());
        }
    }

    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        if let Some(setup) = self.setup_overlay.as_mut() {
            match (modifiers, code) {
                (KeyModifiers::NONE, KeyCode::Esc) => self.setup_overlay = None,
                (KeyModifiers::NONE, KeyCode::Enter) => {
                    if let Ok(spec) = setup.resolve() {
                        let path = setup.config_path();
                        self.setup_overlay = None;
                        self.pending_setup = Some((spec, path));
                    }
                }
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
        Box::pin(async move {
            // Apply an `:edit` that changed the spec. Scripts are preserved; the device config
            // mirrors the new version/role/timeout.
            if let Some((spec, path)) = self.pending_setup.take() {
                let device = OcppDeviceConfig::from_spec(&spec, self.device.scripts.clone());
                if spec.role == OcppRole::Client {
                    // Role switched to client: ask the tab to swap us out for a client view,
                    // carrying the (shared) settings + scripts over.
                    self.replacement = Some(build_client_view(spec, path, device));
                } else {
                    self.spec = spec;
                    self.device = device;
                    self.device_path = path;
                    self.log.write().await.write("Settings updated");
                }
            }
        })
    }

    fn handle_command<'a>(&'a mut self, cmd: &'a str) -> CommandFuture<'a> {
        let result = match cmd.trim() {
            "start" | "stop" | "restart" => {
                CommandResult::Handled(Some("OCPP server is a stub".into()))
            }
            "edit" | "e" => {
                self.setup_overlay = Some(OcppSetupDialog::edit(&self.spec, &self.device_path));
                CommandResult::Handled(None)
            }
            "wd" => {
                if self.device_path.is_empty() {
                    CommandResult::Handled(Some("No configuration file path configured.".into()))
                } else {
                    self.save_device_to(&self.device_path.clone())
                }
            }
            cmd if cmd.starts_with("wd ") => {
                let path = cmd["wd ".len()..].trim().to_string();
                self.save_device_to(&path)
            }
            _ => CommandResult::Unhandled,
        };
        Box::pin(std::future::ready(result))
    }

    fn commands(&self) -> &[CommandDescriptor] {
        &OCPP_SERVER_COMMANDS
    }

    fn log(&self) -> SharedLog {
        self.log.clone()
    }

    fn session_spec(&self) -> Option<serde_json::Value> {
        let module = OcppModuleSpec::from_spec(&self.spec, &self.device_path);
        let mut v = serde_json::to_value(&module).ok()?;
        v.as_object_mut()?.insert("type".into(), "ocpp".into());
        Some(v)
    }

    fn take_replacement(&mut self) -> Option<Box<dyn ModuleView>> {
        self.replacement.take()
    }
}

static OCPP_SERVER_COMMANDS: [CommandDescriptor; 2] = [
    CommandDescriptor {
        name: ":e | :edit",
        description: "edit module setup",
    },
    CommandDescriptor {
        name: ":wd | :write-device [path]",
        description: "save device config",
    },
];
