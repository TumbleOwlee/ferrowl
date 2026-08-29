use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::EventResult;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::config::{ModuleSpec, MonitorDeviceConfig, Role};
use crate::module::type_descriptor::{ModuleViewFactory, SetupView};

use super::module::ModbusMonitorModule;
use super::setup_dialog::MonitorSetupDialog;
use super::view::ModbusMonitorModuleView;

/// Wraps [`MonitorSetupDialog`] and implements [`SetupView`] for the Monitor module
/// type (UI-R-060/061), same shape as `module::modbus::setup::ModbusSetupView`.
pub struct MonitorSetupView {
    dialog: MonitorSetupDialog,
}

impl MonitorSetupView {
    pub fn new_create() -> Self {
        Self {
            dialog: MonitorSetupDialog::create(),
        }
    }
}

impl SetupView for MonitorSetupView {
    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        self.dialog.render(area, buf);
    }

    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        self.dialog.handle_events(modifiers, code)
    }

    fn focus_next(&mut self) {
        self.dialog.focus_next();
    }

    fn focus_previous(&mut self) {
        self.dialog.focus_previous();
    }

    fn close_requested(&mut self) -> bool {
        self.dialog.take_close_request()
    }

    fn confirm(&self) -> Option<(String, ModuleViewFactory)> {
        let outcome = self.dialog.resolve().ok()?;
        let (device_path, device) = outcome
            .device
            .unwrap_or_else(|| (String::new(), MonitorDeviceConfig::default()));
        let name = outcome.values.name.clone();

        let mut device = device;
        device.reconnect = Some(outcome.values.reconnect);

        let spec = ModuleSpec {
            name: outcome.values.name,
            device: device_path,
            role: Role::Monitor,
            endpoint: outcome.values.endpoint,
        };

        let factory: ModuleViewFactory = Box::new(move || {
            Box::new(ModbusMonitorModuleView::new(
                ModbusMonitorModule::new(&spec, &device),
                spec,
                device,
            ))
        });

        Some((name, factory))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creation flow end-to-end — same shape as `ModbusSetupView`'s own
    /// `ut_confirm_resolves_and_builds_a_working_factory`.
    #[test]
    fn ut_monitor_setup_view_confirm_builds_working_factory() {
        let spec = ModuleSpec {
            name: "mon1".to_string(),
            device: String::new(),
            role: Role::Monitor,
            endpoint: crate::config::Endpoint::Rtu {
                path: "/dev/ttyUSB0".to_string(),
                baud_rate: 19200,
                parity: None,
                data_bits: Some(8),
                stop_bits: Some(1),
            },
        };
        let sv = MonitorSetupView {
            dialog: MonitorSetupDialog::edit("mon1", &spec, &MonitorDeviceConfig::default()),
        };
        let (name, factory) = sv.confirm().expect("a filled-in dialog resolves");
        assert_eq!(name, "mon1");
        let _view = factory();
    }

    #[test]
    fn ut_render_and_focus_delegate_to_dialog() {
        let mut sv = MonitorSetupView::new_create();
        sv.focus_next();
        sv.focus_previous();
        let area = ratatui::layout::Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        sv.render(area, &mut buf);
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(!text.trim().is_empty());
    }

    // `SetupView::close_requested`'s default trait method must be overridden here to delegate to
    // the dialog's close-confirm popup; without the override the creation overlay's Esc/Enter
    // silently do nothing for a Monitor module setup (mirrors `ModbusSetupView`'s own test).
    #[test]
    fn ut_close_requested_delegates_to_dialog_take_close_request() {
        let mut sv = MonitorSetupView::new_create();
        assert!(!sv.close_requested());
        // Esc opens the close-confirm popup; Enter confirms it.
        sv.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        sv.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(sv.close_requested());
        assert!(!sv.close_requested(), "flag must clear after take");
    }
}
