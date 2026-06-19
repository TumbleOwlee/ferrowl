use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::traits::HandleEvents;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::config::{DeviceConfig, ModuleSpec};
use crate::config::device::{DEFAULT_DELAY_MS, DEFAULT_INTERVAL_MS, DEFAULT_TIMEOUT_MS};
use crate::dialog::SetupDialog;
use crate::module::modbus::Module;
use crate::module::modbus::view::ModbusModuleView;
use crate::module::type_descriptor::{ModuleViewFactory, SetupView};

/// Wraps [`SetupDialog`] and implements [`SetupView`] for the Modbus module type.
pub struct ModbusSetupView {
    dialog: SetupDialog,
}

impl ModbusSetupView {
    pub fn new_create() -> Self {
        Self {
            dialog: SetupDialog::create((DEFAULT_TIMEOUT_MS, DEFAULT_DELAY_MS, DEFAULT_INTERVAL_MS)),
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
        let outcome = self.dialog.resolve().ok()?;
        let (device_path, mut device) = outcome
            .device
            .unwrap_or_else(|| (String::new(), DeviceConfig::default()));
        let values = outcome.values;
        let name = values.name.clone();

        device.timeout_ms = values.timeout_ms;
        device.delay_ms = values.delay_ms;
        device.interval_ms = values.interval_ms;
        device.read_ranges = values.read_ranges.clone();

        let spec = ModuleSpec {
            name: values.name,
            device: device_path,
            role: values.role,
            endpoint: values.endpoint,
            timeout_ms: values.timeout_ms,
            delay_ms: values.delay_ms,
            interval_ms: values.interval_ms,
        };

        let factory: ModuleViewFactory =
            Box::new(move || Box::new(ModbusModuleView::new(Module::new(&spec, &device), spec, device)));

        Some((name, factory))
    }
}
