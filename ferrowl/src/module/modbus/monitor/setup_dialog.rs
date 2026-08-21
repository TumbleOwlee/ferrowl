//! MB-R-140 — the monitor's setup dialog: name, an optional device-config path, and the
//! `Rtu`/`Ascii` transport field set only (no TLS, no role switch — a monitor tab is created and
//! stays a monitor). Mirrors `module/modbus/setup_dialog.rs::SetupDialog` at roughly 1/20th the
//! size, since a monitor has no TCP/UDP/RtuOverTcp/AsciiOverTcp fields and no role selector.

use crossterm::event::{KeyCode, KeyModifiers};
use derive_builder::Builder;
use ferrowl_ui::{
    Border, COLOR_SCHEME, EventResult,
    state::{
        InputFieldState, InputFieldStateBuilder, SelectionState, SelectionStateBuilder,
        SuggestInputState, SuggestInputStateBuilder,
    },
    style::{InputFieldStyle, SelectionStyle, TextStyle},
    traits::{HandleEvents, ToLabel},
    widgets::{
        GetValue, InputField, InputFieldBuilder, Selection, SelectionBuilder, SuggestInput,
        SuggestInputBuilder, Text, TextBuilder, Validate, ValidateResult, Widget,
    },
};
use ferrowl_ui_derive::{Focus, focusable};
use ferrowl_util::convert::FileType;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, Clear, Widget as UiWidget},
};

use crate::config::{Endpoint, ModuleSpec, MonitorDeviceConfig};
use crate::dialog::NonEmpty;
use crate::dialog::path_suggest::FsPathProvider;

use super::build::endpoint_to_monitor_config;

/// The validated per-instance settings.
pub struct MonitorSetupValues {
    pub name: String,
    pub endpoint: Endpoint,
    pub reconnect: bool,
}

/// The full validated dialog result. `device` is set in New mode: the config path (or `""`)
/// and the loaded (or empty) device config.
pub struct MonitorSetupOutcome {
    pub values: MonitorSetupValues,
    pub device: Option<(String, MonitorDeviceConfig)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DialogMode {
    Edit,
    New,
}

/// Transport selection value — `Rtu`/`Ascii` only (MB-R-140).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Transport {
    Rtu,
    Ascii,
}

impl ToLabel for Transport {
    fn to_label(&self) -> String {
        match self {
            Transport::Rtu => "RTU",
            Transport::Ascii => "ASCII",
        }
        .to_string()
    }
}

/// Serial parity selection value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Parity {
    None,
    Odd,
    Even,
}

impl ToLabel for Parity {
    fn to_label(&self) -> String {
        match self {
            Parity::None => "None",
            Parity::Odd => "Odd",
            Parity::Even => "Even",
        }
        .to_string()
    }
}

impl Parity {
    fn to_config(&self) -> Option<String> {
        match self {
            Parity::None => None,
            Parity::Odd => Some("odd".to_string()),
            Parity::Even => Some("even".to_string()),
        }
    }

    fn from_config(value: Option<&str>) -> Parity {
        match value.map(|s| s.to_ascii_lowercase()).as_deref() {
            Some("odd") => Parity::Odd,
            Some("even") => Parity::Even,
            _ => Parity::None,
        }
    }

    fn index(&self) -> usize {
        match self {
            Parity::None => 0,
            Parity::Odd => 1,
            Parity::Even => 2,
        }
    }
}

/// A numeric serial choice (data/stop bits) rendered as a selection label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct U8Choice(u8);

impl ToLabel for U8Choice {
    fn to_label(&self) -> String {
        self.0.to_string()
    }
}

fn select_u8(state: &mut SelectionState<U8Choice>, current: Option<u8>) {
    if let Some(value) = current
        && let Some(index) = state.values().iter().position(|c| c.0 == value)
    {
        state.set_selection(index);
    }
}

/// Client/server-only auto-reconnect toggle, reused verbatim for the monitor's serial-open
/// retry (MB-R-141).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReconnectChoice {
    On,
    Off,
}

impl ToLabel for ReconnectChoice {
    fn to_label(&self) -> String {
        match self {
            ReconnectChoice::On => "On",
            ReconnectChoice::Off => "Off",
        }
        .to_string()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ConfigPath;

impl Validate for ConfigPath {
    fn validate(input: &str) -> ValidateResult {
        let input = input.trim();
        let resolved = ferrowl_util::path::expand(input);

        if input.is_empty() {
            ValidateResult::None
        } else if FileType::from_path(input).is_some() {
            if resolved.exists() {
                match crate::config::load_monitor_device(input) {
                    Ok(_) => ValidateResult::Success,
                    Err(e) => ValidateResult::Error(format!("Config: {e}")),
                }
            } else {
                ValidateResult::None
            }
        } else {
            ValidateResult::Error("Invalid filetype, TOML or JSON expected.".to_string())
        }
    }
}

#[focusable]
#[derive(Builder, Focus)]
pub struct MonitorSetupDialog {
    #[focus]
    pub(crate) name: Widget<InputFieldState, InputField<NonEmpty>>,
    #[focus]
    pub(crate) config_path:
        Widget<SuggestInputState<FsPathProvider>, SuggestInput<ConfigPath, FsPathProvider>>,
    #[focus]
    pub(crate) transport: Widget<SelectionState<Transport>, Selection<Transport>>,
    #[focus]
    pub(crate) path:
        Widget<SuggestInputState<FsPathProvider>, SuggestInput<String, FsPathProvider>>,
    #[focus]
    pub(crate) baud: Widget<InputFieldState, InputField<String>>,
    #[focus]
    pub(crate) parity: Widget<SelectionState<Parity>, Selection<Parity>>,
    #[focus]
    pub(crate) data_bits: Widget<SelectionState<U8Choice>, Selection<U8Choice>>,
    #[focus]
    pub(crate) stop_bits: Widget<SelectionState<U8Choice>, Selection<U8Choice>>,
    #[focus]
    pub(crate) reconnect: Widget<SelectionState<ReconnectChoice>, Selection<ReconnectChoice>>,
    error: Widget<String, Text>,
    keybinds: Widget<String, Text>,
    mode: DialogMode,
}

impl MonitorSetupDialog {
    /// Create a new monitor module (`:n`/`:new`), with an optional device-config path.
    pub fn create() -> Self {
        Self::build("", "", DialogMode::New)
    }

    /// Edit an existing monitor instance (`:e`).
    pub fn edit(name: &str, spec: &ModuleSpec, device: &MonitorDeviceConfig) -> Self {
        let mut dialog = Self::build(name, &spec.device, DialogMode::Edit);
        match &spec.endpoint {
            Endpoint::Rtu {
                path,
                baud_rate,
                parity,
                data_bits,
                stop_bits,
            } => {
                dialog.transport.state.set_selection(0);
                set_suggest_input(&mut dialog.path, path);
                set_input(&mut dialog.baud, &baud_rate.to_string());
                dialog
                    .parity
                    .state
                    .set_selection(Parity::from_config(parity.as_deref()).index());
                select_u8(&mut dialog.data_bits.state, *data_bits);
                select_u8(&mut dialog.stop_bits.state, *stop_bits);
            }
            Endpoint::Ascii {
                path,
                baud_rate,
                parity,
                data_bits,
                stop_bits,
            } => {
                dialog.transport.state.set_selection(1);
                set_suggest_input(&mut dialog.path, path);
                set_input(&mut dialog.baud, &baud_rate.to_string());
                dialog
                    .parity
                    .state
                    .set_selection(Parity::from_config(parity.as_deref()).index());
                select_u8(&mut dialog.data_bits.state, *data_bits);
                select_u8(&mut dialog.stop_bits.state, *stop_bits);
            }
            // MB-R-140 is only enforced at `ModbusMonitorModule::start()`, never at
            // construction — a hand-edited session file can carry `role = "monitor"` with a
            // non-serial transport (e.g. `transport = "tcp"`). Such a tab loads fine and fails
            // `:start` gracefully (logged); `:edit` on it must not panic either. Degrade to the
            // dialog's own Rtu default instead (leaves path/baud/parity/data_bits/stop_bits at
            // their placeholder defaults, since a Tcp/Udp/RtuOverTcp/AsciiOverTcp endpoint
            // carries no equivalent serial fields to prefill from).
            Endpoint::Tcp { .. }
            | Endpoint::RtuOverTcp { .. }
            | Endpoint::Udp { .. }
            | Endpoint::AsciiOverTcp { .. } => {
                dialog.transport.state.set_selection(0);
            }
        }
        dialog.reconnect.state.set_selection(
            if device
                .reconnect
                .unwrap_or(crate::config::device::DEFAULT_RECONNECT)
            {
                0
            } else {
                1
            },
        );
        dialog
    }

    fn build(name: &str, config_path: &str, mode: DialogMode) -> Self {
        let selection_style = SelectionStyle::default();
        let input_style = InputFieldStyle::default();
        let error_style = TextStyle {
            general: ratatui::style::Style::default()
                .fg(COLOR_SCHEME.error)
                .bg(COLOR_SCHEME.bg),
        };

        let mut name_field = input("Name", "module name", &input_style);
        set_input(&mut name_field, name);
        let mut config_path_field = suggest_input(
            "Config Path [TOML/JSON] (optional)",
            "device.toml",
            &input_style,
            FsPathProvider::with_extensions(&["toml", "json"]),
        );
        set_suggest_input(&mut config_path_field, config_path);

        let mut dialog = MonitorSetupDialogBuilder::default()
            .name(name_field)
            .config_path(config_path_field)
            .transport(selection(
                "Transport",
                vec![Transport::Rtu, Transport::Ascii],
                &selection_style,
            ))
            .path(suggest_input(
                "Serial Path",
                "/dev/ttyUSB0",
                &input_style,
                FsPathProvider::default(),
            ))
            .baud(input("Baud", "19200", &input_style))
            .parity(selection(
                "Parity",
                vec![Parity::None, Parity::Odd, Parity::Even],
                &selection_style,
            ))
            .data_bits(selection(
                "Data Bits",
                vec![U8Choice(8), U8Choice(7), U8Choice(6), U8Choice(5)],
                &selection_style,
            ))
            .stop_bits(selection(
                "Stop Bits",
                vec![U8Choice(1), U8Choice(2)],
                &selection_style,
            ))
            .reconnect(selection(
                "Reconnect",
                vec![ReconnectChoice::On, ReconnectChoice::Off],
                &selection_style,
            ))
            .error(Widget {
                state: String::new(),
                widget: TextBuilder::default()
                    .title(Some("Error".into()))
                    .border(Border::Full(Margin::new(1, 0)))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .multiline(true)
                    .style(error_style)
                    .build()
                    .expect("all required builder fields are set"),
            })
            .keybinds(Widget {
                state: "<Tab> next   <Enter> confirm   <Esc> cancel".to_string(),
                widget: TextBuilder::default()
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .style(TextStyle::default())
                    .build()
                    .expect("all required builder fields are set"),
            })
            .mode(mode)
            .focus(MonitorSetupDialogFocus::Name)
            .build()
            .expect("all required builder fields are set");

        dialog
            .reconnect
            .state
            .set_selection(if crate::config::device::DEFAULT_RECONNECT {
                0
            } else {
                1
            });
        dialog
    }

    pub fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        if code == KeyCode::Tab && modifiers == KeyModifiers::NONE {
            self.focus_next();
            return EventResult::Consumed;
        }
        if code == KeyCode::BackTab {
            self.focus_previous();
            return EventResult::Consumed;
        }
        HandleEvents::handle_events(self, modifiers, code)
    }

    /// Resolve the dialog's current field values, validating the transport (MB-R-140,
    /// belt-and-suspenders — the picker already structurally excludes every non-Rtu/Ascii
    /// transport).
    pub fn resolve(&self) -> Result<MonitorSetupOutcome, String> {
        let values = self.values()?;
        let device = if self.mode == DialogMode::New {
            let path = self.config_path.state.input().trim().to_string();
            if path.is_empty() || !ferrowl_util::path::expand(&path).exists() {
                Some((path, MonitorDeviceConfig::default()))
            } else {
                let device = crate::config::load_monitor_device(&path)
                    .map_err(|e| format!("Config: {e}"))?;
                Some((path, device))
            }
        } else {
            None
        };
        Ok(MonitorSetupOutcome { values, device })
    }

    fn values(&self) -> Result<MonitorSetupValues, String> {
        let name = self.name.state.input().trim().to_string();
        if name.is_empty() {
            return Err("Name is required.".into());
        }
        let config_path = self.config_path.state.input().trim().to_string();
        if !config_path.is_empty() && FileType::from_path(&config_path).is_none() {
            return Err(format!(
                "Unknown format for '{config_path}' (use .toml or .json)"
            ));
        }

        let mut path = self.path.state.input().trim().to_string();
        if path.is_empty() {
            path = "/dev/ttyUSB0".to_string();
        }
        let baud_rate = self.baud.state.input();
        let baud_rate = if !baud_rate.is_empty() {
            baud_rate
                .trim()
                .parse::<u32>()
                .map_err(|_| "Baud rate must be a number.".to_string())?
        } else {
            19200
        };
        let parity = self.parity.state.get_value().to_config();
        let data_bits = Some(self.data_bits.state.get_value().0);
        let stop_bits = Some(self.stop_bits.state.get_value().0);

        let endpoint = match self.transport.state.get_value() {
            Transport::Rtu => Endpoint::Rtu {
                path,
                baud_rate,
                parity,
                data_bits,
                stop_bits,
            },
            Transport::Ascii => Endpoint::Ascii {
                path,
                baud_rate,
                parity,
                data_bits,
                stop_bits,
            },
        };
        let reconnect = self.reconnect.state.get_value() == ReconnectChoice::On;

        // Belt-and-suspenders (MB-R-140): the picker structurally offers only Rtu/Ascii, but
        // validate through the same resolution step every other call site uses anyway.
        endpoint_to_monitor_config(&endpoint, reconnect).map_err(|e| e.0.to_string())?;

        Ok(MonitorSetupValues {
            name,
            endpoint,
            reconnect,
        })
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);
        let block = Block::bordered().title(match self.mode {
            DialogMode::New => " New Modbus Monitor ",
            DialogMode::Edit => " Edit Modbus Monitor ",
        });
        let inner = block.inner(area);
        block.render(area, buf);

        let rows = Layout::vertical([
            Constraint::Length(3), // name
            Constraint::Length(3), // config path
            Constraint::Length(3), // transport
            Constraint::Length(3), // path
            Constraint::Length(3), // baud
            Constraint::Length(3), // parity
            Constraint::Length(3), // data bits
            Constraint::Length(3), // stop bits
            Constraint::Length(3), // reconnect
            Constraint::Length(3), // error
            Constraint::Length(1), // keybinds
        ])
        .split(inner);

        use ratatui::widgets::StatefulWidget;
        StatefulWidget::render(&self.name.widget, rows[0], buf, &mut self.name.state);
        StatefulWidget::render(
            &self.config_path.widget,
            rows[1],
            buf,
            &mut self.config_path.state,
        );
        StatefulWidget::render(
            &self.transport.widget,
            rows[2],
            buf,
            &mut self.transport.state,
        );
        StatefulWidget::render(&self.path.widget, rows[3], buf, &mut self.path.state);
        StatefulWidget::render(&self.baud.widget, rows[4], buf, &mut self.baud.state);
        StatefulWidget::render(&self.parity.widget, rows[5], buf, &mut self.parity.state);
        StatefulWidget::render(
            &self.data_bits.widget,
            rows[6],
            buf,
            &mut self.data_bits.state,
        );
        StatefulWidget::render(
            &self.stop_bits.widget,
            rows[7],
            buf,
            &mut self.stop_bits.state,
        );
        StatefulWidget::render(
            &self.reconnect.widget,
            rows[8],
            buf,
            &mut self.reconnect.state,
        );
        StatefulWidget::render(&self.error.widget, rows[9], buf, &mut self.error.state);
        StatefulWidget::render(
            &self.keybinds.widget,
            rows[10],
            buf,
            &mut self.keybinds.state,
        );
    }
}

fn input<T: Validate + Clone>(
    title: &str,
    placeholder: &str,
    style: &InputFieldStyle,
) -> Widget<InputFieldState, InputField<T>> {
    Widget {
        state: InputFieldStateBuilder::default()
            .focused(false)
            .disabled(false)
            .placeholder(Some(placeholder.to_string()))
            .allowed_for::<T>()
            .build()
            .expect("all required builder fields are set"),
        widget: InputFieldBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some(title.into()))
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .style(style.clone())
            .build()
            .expect("all required builder fields are set"),
    }
}

fn suggest_input<T: Validate + Clone, P: ferrowl_ui::traits::SuggestionProvider + Clone>(
    title: &str,
    placeholder: &str,
    style: &InputFieldStyle,
    provider: P,
) -> Widget<SuggestInputState<P>, SuggestInput<T, P>> {
    Widget {
        state: SuggestInputStateBuilder::default()
            .field(
                InputFieldStateBuilder::default()
                    .focused(false)
                    .disabled(false)
                    .placeholder(Some(placeholder.to_string()))
                    .allowed_for::<T>()
                    .build()
                    .expect("all required builder fields are set"),
            )
            .provider(provider)
            .build()
            .expect("all required builder fields are set"),
        widget: SuggestInputBuilder::default()
            .input_field(
                InputFieldBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some(title.into()))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(style.clone())
                    .build()
                    .expect("all required builder fields are set"),
            )
            .build()
            .expect("all required builder fields are set"),
    }
}

fn selection<T: ToLabel + Clone>(
    title: &str,
    values: Vec<T>,
    style: &SelectionStyle,
) -> Widget<SelectionState<T>, Selection<T>> {
    Widget {
        state: SelectionStateBuilder::default()
            .focused(false)
            .values(values)
            .build()
            .expect("all required builder fields are set"),
        widget: SelectionBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some(title.into()))
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .style(style.clone())
            .build()
            .expect("all required builder fields are set"),
    }
}

pub(crate) fn set_input<T: Validate + Clone>(
    widget: &mut Widget<InputFieldState, InputField<T>>,
    value: &str,
) {
    widget.state.set_input(value.to_string());
    widget.state.set_cursor(value.chars().count());
}

pub(crate) fn set_suggest_input<
    T: Validate + Clone,
    P: ferrowl_ui::traits::SuggestionProvider + Clone,
>(
    widget: &mut Widget<SuggestInputState<P>, SuggestInput<T, P>>,
    value: &str,
) {
    widget.state.set_input(value.to_string());
    widget.state.set_cursor(value.chars().count());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MB-R-140 — the monitor setup dialog offers exactly the two serial transports.
    #[test]
    fn ut_monitor_setup_dialog_transport_selector_has_exactly_two_entries() {
        let dialog = MonitorSetupDialog::create();
        assert_eq!(dialog.transport.state.values().len(), 2);
        assert_eq!(
            dialog.transport.state.values(),
            &[Transport::Rtu, Transport::Ascii]
        );
    }

    /// MB-R-140 — `resolve()` still rejects a non-serial endpoint even when the picker itself
    /// is bypassed by constructing the dialog's internal state directly, proving the check isn't
    /// solely enforced by the picker's restriction.
    #[test]
    fn ut_monitor_setup_dialog_resolve_rejects_non_serial_endpoint_if_reachable() {
        // Directly exercise the belt-and-suspenders validation path used by `values()`, since
        // the dialog's `Transport` enum has no Tcp/Udp variant to select through the picker.
        let err = endpoint_to_monitor_config(
            &Endpoint::Tcp {
                ip: "127.0.0.1".into(),
                port: 502,
            },
            true,
        )
        .unwrap_err();
        assert!(err.to_string().contains("rtu or ascii"));
    }

    #[test]
    fn ut_resolve_default_dialog_builds_rtu_endpoint() {
        let mut dialog = MonitorSetupDialog::create();
        set_input(&mut dialog.name, "mon1");
        let outcome = dialog.resolve().unwrap();
        assert_eq!(outcome.values.name, "mon1");
        assert!(matches!(outcome.values.endpoint, Endpoint::Rtu { .. }));
    }

    #[test]
    fn ut_resolve_without_name_errors() {
        let dialog = MonitorSetupDialog::create();
        assert!(dialog.resolve().is_err());
    }

    #[test]
    fn ut_edit_prefills_transport_and_path() {
        let spec = ModuleSpec {
            name: "mon1".to_string(),
            device: String::new(),
            role: crate::config::Role::Monitor,
            endpoint: Endpoint::Ascii {
                path: "/dev/ttyS0".to_string(),
                baud_rate: 9600,
                parity: None,
                data_bits: Some(8),
                stop_bits: Some(1),
            },
        };
        let device = MonitorDeviceConfig::default();
        let dialog = MonitorSetupDialog::edit("mon1", &spec, &device);
        assert_eq!(dialog.transport.state.get_value(), Transport::Ascii);
        assert_eq!(dialog.path.state.input(), "/dev/ttyS0");
    }

    /// Regression (gate 3 blocker) — MB-R-140 is only enforced at
    /// `ModbusMonitorModule::start()`, never at construction, so a hand-edited session file can
    /// carry `role = "monitor"` with a non-serial transport. `:edit`ing such a tab must not
    /// panic; it degrades to the dialog's Rtu default instead.
    #[test]
    fn ut_edit_does_not_panic_on_non_serial_endpoint() {
        let spec = ModuleSpec {
            name: "mon1".to_string(),
            device: String::new(),
            role: crate::config::Role::Monitor,
            endpoint: Endpoint::Tcp {
                ip: "127.0.0.1".to_string(),
                port: 502,
            },
        };
        let device = MonitorDeviceConfig::default();
        let dialog = MonitorSetupDialog::edit("mon1", &spec, &device);
        assert_eq!(dialog.transport.state.get_value(), Transport::Rtu);
    }
}
