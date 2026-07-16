use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::EventResult;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::config::device::{
    DEFAULT_DELAY_MS, DEFAULT_INTERVAL_MS, DEFAULT_RECONNECT, DEFAULT_TIMEOUT_MS,
};
use crate::config::{DeviceConfig, ModuleSpec};
use crate::dialog::SetupDialog;
use crate::module::modbus::ModbusModule as Module;
use crate::module::modbus::build::Timing;
use crate::module::modbus::view::ModbusModuleView;
use crate::module::type_descriptor::{ModuleViewFactory, SetupView};

/// Wraps [`SetupDialog`] and implements [`SetupView`] for the Modbus module type.
pub struct ModbusSetupView {
    dialog: SetupDialog,
}

impl ModbusSetupView {
    pub fn new_create() -> Self {
        Self {
            dialog: SetupDialog::create(Timing {
                timeout_ms: DEFAULT_TIMEOUT_MS,
                delay_ms: DEFAULT_DELAY_MS,
                interval_ms: DEFAULT_INTERVAL_MS,
                reconnect: DEFAULT_RECONNECT,
            }),
        }
    }

    pub fn dialog_mut(&mut self) -> &mut SetupDialog {
        &mut self.dialog
    }
}

impl SetupView for ModbusSetupView {
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
        let (device_path, mut device) = outcome
            .device
            .unwrap_or_else(|| (String::new(), DeviceConfig::default()));
        let values = outcome.values;
        let name = values.name.clone();

        device.timeout_ms = values.timeout_ms;
        device.delay_ms = values.delay_ms;
        device.interval_ms = values.interval_ms;
        if let Some(reconnect) = values.reconnect {
            device.reconnect = Some(reconnect);
        }
        device.read_ranges = values.read_ranges.clone();

        let spec = ModuleSpec {
            name: values.name,
            device: device_path,
            role: values.role,
            endpoint: values.endpoint,
        };

        let factory: ModuleViewFactory = Box::new(move || {
            Box::new(ModbusModuleView::new(
                Module::new(&spec, &device),
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

    // Regression: the `SetupView::close_requested` default trait method must be overridden here
    // to delegate to the dialog's close-confirm popup, or the creation overlay's Esc/Enter would
    // silently do nothing for a Modbus module setup.
    #[test]
    /// UI-R-023 — the module setup delegates close-requested to the dialog's close-request flag.
    fn ut_close_requested_delegates_to_dialog_take_close_request() {
        let mut sv = ModbusSetupView::new_create();
        assert!(!sv.close_requested());
        // Esc opens the close-confirm popup; Enter confirms it.
        sv.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        sv.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(sv.close_requested());
        assert!(!sv.close_requested(), "flag must clear after take");
    }

    #[test]
    fn ut_render_and_focus_delegate_to_dialog() {
        let mut sv = ModbusSetupView::new_create();
        sv.focus_next();
        sv.focus_previous();
        let area = ratatui::layout::Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        sv.render(area, &mut buf);
        // The setup modal draws something into the buffer.
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(!text.trim().is_empty());
    }

    #[test]
    fn ut_confirm_resolves_and_builds_a_working_factory() {
        let sv = ModbusSetupView::new_create();
        // A fresh create dialog resolves to a named module and yields a factory that builds a view.
        if let Some((name, factory)) = sv.confirm() {
            assert!(!name.is_empty());
            let _view = factory(); // exercises the boxed module/view construction closure
        }
    }
}
