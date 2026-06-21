//! OCPP 2.0.1 charging-station (client) view. Same layout as the 1.6 view (state panel over the
//! action list and a variable table+inputs on the left; message log over a JSON payload viewer on
//! the right) adapted to 2.0.1: EVSE id, connector-status enum, GetVariables-backed variable store,
//! and `StartTransaction`/`StopTransaction` *shortcut* buttons that emit a `TransactionEvent`.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::{Mutex, RwLock};

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::{
    Border, COLOR_SCHEME, EventResult,
    state::{
        ButtonState, ButtonStateBuilder, CodeInputFieldState, CodeInputFieldStateBuilder,
        InputFieldState, InputFieldStateBuilder, SelectionState, SelectionStateBuilder, TableState,
        TableStateBuilder,
    },
    style::{
        ButtonStyle, InputFieldStyle, InputFieldStyleBuilder, SelectionStyle,
        SelectionStyleBuilder, TableStyleBuilder, TextStyle,
    },
    traits::HandleEvents,
    widgets::{
        Button, ButtonBuilder, CodeInputField, CodeInputFieldBuilder, GetValue, Header, InputField,
        InputFieldBuilder, Selection, SelectionBuilder, Table, TableBuilder, TableEntry,
        TextBuilder, Widget, Width,
    },
};
use ratatui::style::Style;
use ratatui::{
    Frame,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, Clear, StatefulWidget, Widget as UiWidget},
};
use tokio::sync::RwLock as AsyncRwLock;

use ferrowl_ocpp::{V2_0_1, Version};

use crate::app::LogRing;
use crate::module::ocpp::client::backend::{OcppClient, OcppMessage, rfc3339_now};
use crate::module::ocpp::client::build_client_view;
use crate::module::ocpp::client::config::{ConfigEditDialog, ConfigKey};
use crate::module::ocpp::client::lua_sim::{
    ActionQueue, OcppSimHandle, merge_overrides, run_ocpp_sim,
};
use crate::module::ocpp::client::scripts::ScriptDialog;
use crate::module::ocpp::client::v2_0_1::handler::CsStateHandler;
use crate::module::ocpp::client::v2_0_1::state::{ConfigRow, CsState, NvRow};
use crate::module::ocpp::config::device::OcppDeviceConfig;
use crate::module::ocpp::config::session::{OcppModuleSpec, OcppRole, OcppSpec};
use crate::module::ocpp::server::build_server_view;
use crate::module::ocpp::setup_dialog::OcppSetupDialog;
use crate::module::view::{
    CommandDescriptor, CommandFuture, CommandResult, ModuleView, RefreshFuture, SharedLog,
};
use crate::view::log::format_timestamp;

// --- State table -----------------------------------------------------------

impl TableEntry<3> for NvRow {
    fn values(&self) -> [String; 3] {
        [self.name.clone(), self.unit.clone(), self.value.clone()]
    }
    fn height(&self) -> u16 {
        1
    }
}

#[derive(Clone, Debug)]
struct NvHeader;

impl Header<3> for NvHeader {
    fn header() -> [String; 3] {
        ["Name".into(), "Unit".into(), "Value".into()]
    }
    fn widths() -> [Width; 3] {
        [
            Width { min: 18, max: 30 },
            Width { min: 6, max: 6 },
            Width { min: 6, max: 30 },
        ]
    }
}

// --- Variable table --------------------------------------------------------

impl TableEntry<3> for ConfigRow {
    fn values(&self) -> [String; 3] {
        [self.key.clone(), self.value.clone(), self.ro.clone()]
    }
    fn height(&self) -> u16 {
        1
    }
}

#[derive(Clone, Debug)]
struct ConfigHeader;

impl Header<3> for ConfigHeader {
    fn header() -> [String; 3] {
        ["Variable".into(), "Value".into(), "ReadOnly".into()]
    }
    fn widths() -> [Width; 3] {
        [
            Width { min: 16, max: 30 },
            Width { min: 8, max: 30 },
            Width { min: 9, max: 9 },
        ]
    }
}

/// Which state row an edit overlay is changing.
#[derive(Clone, Copy)]
enum EditField {
    EvseId,
    ConnectorId,
    Phases,
    Voltage,
    Current(usize),
    Power,
    Frequency,
    TotalEnergy,
    SessionEnergy,
    Soc,
    Temperature,
    Status,
    Rfid,
    Model,
    Vendor,
    FirmwareVersion,
    SerialNumber,
}

impl EditField {
    fn from_row(row: usize) -> Option<EditField> {
        Some(match row {
            0 => EditField::EvseId,
            1 => EditField::ConnectorId,
            2 => EditField::Phases,
            3 => EditField::Voltage,
            4 => EditField::Current(0),
            5 => EditField::Current(1),
            6 => EditField::Current(2),
            7 => EditField::Power,
            8 => EditField::Frequency,
            9 => EditField::TotalEnergy,
            10 => EditField::SessionEnergy,
            11 => EditField::Soc,
            12 => EditField::Temperature,
            13 => EditField::Status,
            14 => EditField::Rfid,
            15 => EditField::Model,
            16 => EditField::Vendor,
            17 => EditField::FirmwareVersion,
            18 => EditField::SerialNumber,
            _ => return None,
        })
    }

    fn label(self) -> &'static str {
        match self {
            EditField::EvseId => "EVSE ID",
            EditField::ConnectorId => "Connector ID",
            EditField::Phases => "Used Phases",
            EditField::Voltage => "Voltage",
            EditField::Current(0) => "Current L1",
            EditField::Current(1) => "Current L2",
            EditField::Current(_) => "Current L3",
            EditField::Power => "Power",
            EditField::Frequency => "Frequency",
            EditField::TotalEnergy => "Total Energy",
            EditField::SessionEnergy => "Session Energy",
            EditField::Soc => "State of Charge",
            EditField::Temperature => "Temperature",
            EditField::Status => "Status",
            EditField::Rfid => "RFID",
            EditField::Model => "Model",
            EditField::Vendor => "Vendor",
            EditField::FirmwareVersion => "Firmware Version",
            EditField::SerialNumber => "Serial Number",
        }
    }
}

const PHASE_CHOICES: [&str; 7] = ["L1", "L2", "L3", "L1,L2", "L1,L3", "L2,L3", "L1,L2,L3"];
const STATUS_CHOICES: [&str; 5] = [
    "Available",
    "Occupied",
    "Reserved",
    "Unavailable",
    "Faulted",
];

/// Friendly shortcut buttons that map to a `TransactionEvent`.
const SHORTCUTS: [&str; 2] = ["StartTransaction", "StopTransaction"];

/// State-driven real actions: built straight from state, no dialog.
const STATE_DRIVEN: [&str; 5] = [
    "Authorize",
    "BootNotification",
    "Heartbeat",
    "MeterValues",
    "StatusNotification",
];

enum EditOverlay {
    Choice {
        field: EditField,
        sel: Widget<SelectionState<String>, Selection<String>>,
    },
    Number {
        field: EditField,
        input: Widget<InputFieldState, InputField<f64>>,
    },
    Text {
        field: EditField,
        input: Widget<InputFieldState, InputField<String>>,
    },
}

impl EditOverlay {
    fn field(&self) -> EditField {
        match self {
            EditOverlay::Choice { field, .. }
            | EditOverlay::Number { field, .. }
            | EditOverlay::Text { field, .. } => *field,
        }
    }
}

// --- Message table ---------------------------------------------------------

#[derive(Clone, Debug)]
struct MsgRow {
    timestamp: String,
    direction: String,
    name: String,
    status: String,
    context: String,
}

impl TableEntry<5> for MsgRow {
    fn values(&self) -> [String; 5] {
        [
            self.timestamp.clone(),
            self.direction.clone(),
            self.name.clone(),
            self.status.clone(),
            self.context.clone(),
        ]
    }
    fn height(&self) -> u16 {
        1
    }
    fn cell_styles(&self) -> [Option<ratatui::prelude::Style>; 5] {
        let status_style = match self.status.as_str() {
            "Success" => Some(ratatui::prelude::Style::default().fg(COLOR_SCHEME.success)),
            "Error" => Some(ratatui::prelude::Style::default().fg(COLOR_SCHEME.error)),
            _ => None,
        };
        [None, None, None, status_style, None]
    }
}

#[derive(Clone, Debug)]
struct MsgHeader;

impl Header<5> for MsgHeader {
    fn header() -> [String; 5] {
        [
            "Timestamp".into(),
            "Direction".into(),
            "Message".into(),
            "Status".into(),
            "Context".into(),
        ]
    }
    fn widths() -> [Width; 5] {
        [
            Width { min: 23, max: 23 },
            Width { min: 8, max: 10 },
            Width { min: 14, max: 30 },
            Width { min: 7, max: 8 },
            Width { min: 6, max: 40 },
        ]
    }
}

fn msg_row(m: &OcppMessage) -> MsgRow {
    let status = match m.ok {
        Some(true) => "Success",
        Some(false) => "Error",
        None => "",
    };
    MsgRow {
        timestamp: format_timestamp(m.ts),
        direction: m.direction.label().to_string(),
        name: m.name.clone(),
        status: status.to_string(),
        context: m.context.clone(),
    }
}

// --- View ------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
    State,
    Scripts,
    ConfigTable,
    ConfigKey,
    ConfigValue,
    Actions,
    Messages,
}

type StateTable = Widget<TableState<NvRow, 3>, Table<NvRow, NvHeader, 3>>;
type ConfigTable = Widget<TableState<ConfigRow, 3>, Table<ConfigRow, ConfigHeader, 3>>;
type MsgTable = Widget<TableState<MsgRow, 5>, Table<MsgRow, MsgHeader, 5>>;

pub struct OcppClientV201View {
    spec: OcppSpec,
    /// Path to the OCPP device-config file backing this module (empty = none yet).
    device_path: String,
    /// Device config (role/version/timeout/scripts); source of truth for scripts. `:wd` persists.
    device: OcppDeviceConfig,
    backend: OcppClient<V2_0_1>,
    state: Arc<RwLock<CsState>>,
    log: SharedLog,
    state_table: StateTable,
    config_table: ConfigTable,
    key_input: Widget<InputFieldState, InputField<String>>,
    value_input: Widget<InputFieldState, InputField<String>>,
    actions: Widget<SelectionState<String>, Selection<String>>,
    msg_table: MsgTable,
    messages: Vec<OcppMessage>,
    code: Widget<CodeInputFieldState, CodeInputField>,
    scripts_button: Widget<ButtonState, Button>,
    script_dialog: Option<ScriptDialog>,
    focus: Pane,
    edit: Option<EditOverlay>,
    config_edit: Option<ConfigEditDialog>,
    action_dialog: Option<(String, Widget<CodeInputFieldState, CodeInputField>)>,
    pending_send: Option<(String, serde_json::Value)>,
    setup_overlay: Option<OcppSetupDialog>,
    /// A resolved `:edit` (spec + device-config path) awaiting application in `refresh`.
    pending_setup: Option<(OcppSpec, String)>,
    replacement: Option<Box<dyn ModuleView>>,
    /// Actions enqueued by Lua scripts, drained and sent each `refresh`.
    action_queue: ActionQueue,
    /// The running Lua simulation thread, if any.
    sim: Option<OcppSimHandle>,
    meter_tick: u32,
    compact: bool,
}

impl OcppClientV201View {
    pub fn new(spec: OcppSpec, device_path: String, device: OcppDeviceConfig) -> Self {
        let state = Arc::new(RwLock::new(CsState::default()));
        let (rows, config_rows) = {
            let s = state.read().unwrap();
            (s.rows(), s.config_rows())
        };
        // Shortcut buttons first, then the real CS-originated actions.
        let action_values: Vec<String> = SHORTCUTS
            .iter()
            .map(|s| s.to_string())
            .chain(V2_0_1::cs_actions().iter().map(|s| s.to_string()))
            .collect();
        let mut view = Self {
            device_path,
            device,
            backend: OcppClient::new(spec.clone()),
            state,
            log: Arc::new(AsyncRwLock::new(LogRing::init())),
            state_table: nv_table(rows),
            config_table: config_table(config_rows),
            key_input: panel_input("Key"),
            value_input: panel_input("Value"),
            actions: action_list(action_values),
            msg_table: msg_table(),
            messages: Vec::new(),
            code: code_view(),
            scripts_button: scripts_button(),
            script_dialog: None,
            focus: Pane::State,
            edit: None,
            config_edit: None,
            action_dialog: None,
            pending_send: None,
            setup_overlay: None,
            pending_setup: None,
            replacement: None,
            action_queue: Arc::new(Mutex::new(VecDeque::new())),
            sim: None,
            meter_tick: 0,
            compact: false,
            spec,
        };
        view.start_sim();
        view
    }

    /// Enabled scripts as `(name, code)` pairs for the simulation thread.
    fn enabled_scripts(&self) -> Vec<(String, String)> {
        self.device
            .scripts
            .iter()
            .filter(|s| s.enabled)
            .map(|s| (s.name.clone(), s.code.clone()))
            .collect()
    }

    /// (Re)start the Lua simulation thread from the currently-enabled scripts (no-op if none).
    fn start_sim(&mut self) {
        self.stop_sim();
        self.sim = run_ocpp_sim(
            self.state.clone(),
            self.action_queue.clone(),
            self.enabled_scripts(),
            self.log.clone(),
        );
    }

    /// Stop and join the simulation thread if one is running.
    fn stop_sim(&mut self) {
        if let Some(mut sim) = self.sim.take() {
            sim.stop();
        }
    }

    /// Drain and send one Lua-enqueued action. The transaction shortcuts map to a TransactionEvent
    /// (like the buttons); state-driven and other actions build their payload then merge overrides.
    fn dispatch_lua_action(&mut self, name: &str, overrides: serde_json::Value) {
        let (send_name, mut payload) = match name {
            "StartTransaction" => ("TransactionEvent".to_string(), self.start_event()),
            "StopTransaction" => match self.stop_event() {
                Some(payload) => ("TransactionEvent".to_string(), payload),
                None => return,
            },
            n if STATE_DRIVEN.contains(&n) => (name.to_string(), self.state_payload(n)),
            _ => {
                let template = V2_0_1::default_action(name)
                    .and_then(|a| V2_0_1::encode_action(&a).ok())
                    .unwrap_or_else(|| serde_json::json!({}));
                (name.to_string(), template)
            }
        };
        merge_overrides(&mut payload, overrides);
        self.send_payload(&send_name, payload);
    }

    fn make_handler(&self) -> CsStateHandler {
        CsStateHandler::new(
            self.backend.online_handle(),
            self.backend.messages_handle(),
            self.state.clone(),
        )
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

    fn set_compact(&mut self, compact: bool) {
        self.compact = compact;
        let margin = Margin {
            vertical: if compact { 0 } else { 1 },
            horizontal: 0,
        };
        self.state_table.widget.set_row_margin(margin);
        self.config_table.widget.set_row_margin(margin);
        self.msg_table.widget.set_row_margin(margin);
    }

    /// Enqueue the focused action for sending. Shortcuts and state-driven actions build their
    /// payload from state; anything else opens a JSON dialog with the Default-derived template.
    fn trigger_action(&mut self) {
        let name = self.actions.state.get_value();
        if name.is_empty() {
            return;
        }
        match name.as_str() {
            "StartTransaction" => {
                let payload = self.start_event();
                self.pending_send = Some(("TransactionEvent".to_string(), payload));
            }
            "StopTransaction" => {
                if let Some(payload) = self.stop_event() {
                    self.pending_send = Some(("TransactionEvent".to_string(), payload));
                }
            }
            n if STATE_DRIVEN.contains(&n) => {
                let payload = self.state_payload(n);
                self.pending_send = Some((name, payload));
            }
            _ => {
                let template = V2_0_1::default_action(&name)
                    .and_then(|a| V2_0_1::encode_action(&a).ok())
                    .map(|v| serde_json::to_string_pretty(&v).unwrap_or_default())
                    .unwrap_or_else(|| "{}".to_string());
                let mut field = editable_code();
                field.state.set_content(&template);
                self.action_dialog = Some((name, field));
            }
        }
    }

    /// Build a `TransactionEvent(Started)`, minting a transaction id and starting charging.
    fn start_event(&mut self) -> serde_json::Value {
        let mut s = self.state.write().unwrap();
        let tx = s.start_tx();
        let seq = s.next_seq();
        s.status = "Occupied".to_string();
        s.session_energy = 0.0;
        self.meter_tick = 0;
        serde_json::json!({
            "eventType": "Started",
            "timestamp": rfc3339_now(),
            "triggerReason": "Authorized",
            "seqNo": seq,
            "transactionInfo": { "transactionId": tx },
            "idToken": { "idToken": s.rfid, "type": "Central" },
            "evse": { "id": s.evse_id, "connectorId": s.connector_id },
        })
    }

    /// Build a `TransactionEvent(Ended)` for the running transaction, or `None` if idle.
    fn stop_event(&mut self) -> Option<serde_json::Value> {
        let mut s = self.state.write().unwrap();
        let tx = s.transaction_id.clone()?;
        let seq = s.next_seq();
        s.status = "Available".to_string();
        s.transaction_id = None;
        s.tx_confirmed = false;
        Some(serde_json::json!({
            "eventType": "Ended",
            "timestamp": rfc3339_now(),
            "triggerReason": "StopAuthorized",
            "seqNo": seq,
            "transactionInfo": { "transactionId": tx },
            "idToken": { "idToken": s.rfid, "type": "Central" },
        }))
    }

    /// Build the request payload for a state-driven action from current state.
    fn state_payload(&self, name: &str) -> serde_json::Value {
        let s = self.state.read().unwrap();
        match name {
            "Authorize" => serde_json::json!({
                "idToken": { "idToken": s.rfid, "type": "Central" },
            }),
            "BootNotification" => serde_json::json!({
                "reason": "PowerUp",
                "chargingStation": {
                    "model": s.model,
                    "vendorName": s.vendor,
                    "serialNumber": s.serial_number,
                    "firmwareVersion": s.firmware_version,
                },
            }),
            "Heartbeat" => serde_json::json!({}),
            "MeterValues" => serde_json::json!({
                "evseId": s.evse_id,
                "meterValue": s.meter_value_json(),
            }),
            "StatusNotification" => serde_json::json!({
                "timestamp": rfc3339_now(),
                "connectorStatus": s.status,
                "evseId": s.evse_id,
                "connectorId": s.connector_id,
            }),
            _ => serde_json::json!({}),
        }
    }

    fn focus_next(&mut self) {
        self.focus = match self.focus {
            Pane::State => Pane::Scripts,
            Pane::Scripts => Pane::Actions,
            Pane::Actions => Pane::ConfigTable,
            Pane::ConfigTable => Pane::ConfigKey,
            Pane::ConfigKey => Pane::ConfigValue,
            Pane::ConfigValue => Pane::Messages,
            Pane::Messages => Pane::State,
        };
    }

    fn focus_previous(&mut self) {
        self.focus = match self.focus {
            Pane::State => Pane::Messages,
            Pane::Scripts => Pane::State,
            Pane::Actions => Pane::Scripts,
            Pane::ConfigTable => Pane::Actions,
            Pane::ConfigKey => Pane::ConfigTable,
            Pane::ConfigValue => Pane::ConfigKey,
            Pane::Messages => Pane::ConfigValue,
        };
    }

    /// Open the Lua script manager over the current device scripts.
    fn open_scripts(&mut self) {
        self.script_dialog = Some(ScriptDialog::new(&self.device.scripts));
    }

    /// Append a variable from the key/value inputs (readonly=false), then clear them.
    fn add_config_key(&mut self) {
        let key = self.key_input.state.input().trim().to_string();
        if key.is_empty() {
            return;
        }
        let value = self.value_input.state.input().trim().to_string();
        {
            let mut s = self.state.write().unwrap();
            match s.config.iter_mut().find(|c| c.key == key) {
                Some(c) => c.value = value,
                None => s.config.push(ConfigKey {
                    key,
                    value,
                    readonly: false,
                }),
            }
        }
        self.key_input.state.set_input(String::new());
        self.key_input.state.set_cursor(0);
        self.value_input.state.set_input(String::new());
        self.value_input.state.set_cursor(0);
    }

    fn open_config_edit(&mut self) {
        let Some(row) = self.config_table.state.table_state().selected() else {
            return;
        };
        let s = self.state.read().unwrap();
        if let Some(current) = s.config.get(row) {
            self.config_edit = Some(ConfigEditDialog::new(row, current));
        }
    }

    fn apply_config_edit(&mut self) {
        let Some(dialog) = self.config_edit.take() else {
            return;
        };
        let Some(edited) = dialog.resolve() else {
            return;
        };
        let mut s = self.state.write().unwrap();
        if let Some(slot) = s.config.get_mut(dialog.index()) {
            *slot = edited;
        }
    }

    fn open_edit(&mut self) {
        let Some(row) = self.state_table.state.table_state().selected() else {
            return;
        };
        let Some(field) = EditField::from_row(row) else {
            return;
        };
        let s = self.state.read().unwrap();
        self.edit = Some(match field {
            EditField::Phases => EditOverlay::Choice {
                field,
                sel: choice(&PHASE_CHOICES, &s.phases),
            },
            EditField::Status => EditOverlay::Choice {
                field,
                sel: choice(&STATUS_CHOICES, &s.status),
            },
            EditField::EvseId => EditOverlay::Number {
                field,
                input: number(s.evse_id as f64),
            },
            EditField::ConnectorId => EditOverlay::Number {
                field,
                input: number(s.connector_id as f64),
            },
            EditField::Voltage => EditOverlay::Number {
                field,
                input: number(s.voltage),
            },
            EditField::Current(i) => EditOverlay::Number {
                field,
                input: number(s.current[i]),
            },
            EditField::Power => EditOverlay::Number {
                field,
                input: number(s.power),
            },
            EditField::Frequency => EditOverlay::Number {
                field,
                input: number(s.frequency),
            },
            EditField::TotalEnergy => EditOverlay::Number {
                field,
                input: number(s.total_energy),
            },
            EditField::SessionEnergy => EditOverlay::Number {
                field,
                input: number(s.session_energy),
            },
            EditField::Soc => EditOverlay::Number {
                field,
                input: number(s.soc),
            },
            EditField::Temperature => EditOverlay::Number {
                field,
                input: number(s.temperature),
            },
            EditField::Rfid => EditOverlay::Text {
                field,
                input: text_input(&s.rfid),
            },
            EditField::Model => EditOverlay::Text {
                field,
                input: text_input(&s.model),
            },
            EditField::Vendor => EditOverlay::Text {
                field,
                input: text_input(&s.vendor),
            },
            EditField::FirmwareVersion => EditOverlay::Text {
                field,
                input: text_input(&s.firmware_version),
            },
            EditField::SerialNumber => EditOverlay::Text {
                field,
                input: text_input(&s.serial_number),
            },
        });
    }

    fn apply_edit(&mut self) {
        let Some(edit) = self.edit.take() else { return };
        let mut s = self.state.write().unwrap();
        match edit {
            EditOverlay::Choice { field, sel } => {
                let value = sel.state.get_value();
                match field {
                    EditField::Phases => s.phases = value,
                    EditField::Status => s.status = value,
                    _ => {}
                }
            }
            EditOverlay::Number { field, input } => {
                let Ok(value) = input.state.input().trim().parse::<f64>() else {
                    return;
                };
                match field {
                    EditField::EvseId => s.evse_id = value as i64,
                    EditField::ConnectorId => s.connector_id = value as i64,
                    EditField::Voltage => s.voltage = value,
                    EditField::Current(i) => s.current[i] = value,
                    EditField::Power => s.power = value,
                    EditField::Frequency => s.frequency = value,
                    EditField::TotalEnergy => s.total_energy = value,
                    EditField::SessionEnergy => s.session_energy = value,
                    EditField::Soc => s.soc = value,
                    EditField::Temperature => s.temperature = value,
                    _ => {}
                }
            }
            EditOverlay::Text { field, input } => {
                let value = input.state.input().trim().to_string();
                match field {
                    EditField::Rfid => s.rfid = value,
                    EditField::Model => s.model = value,
                    EditField::Vendor => s.vendor = value,
                    EditField::FirmwareVersion => s.firmware_version = value,
                    EditField::SerialNumber => s.serial_number = value,
                    _ => {}
                }
            }
        }
    }

    fn sync_code(&mut self) {
        let selected = self.msg_table.state.table_state().selected();
        let content = selected
            .and_then(|i| self.messages.get(i))
            .map(|m| serde_json::to_string_pretty(&m.payload).unwrap_or_default())
            .unwrap_or_default();
        self.code.state.set_content(&content);
    }

    /// Decode + send a (name, payload) without blocking: the round-trip runs in a spawned task so a
    /// slow or unresponsive CSMS never freezes the UI loop. The request/response land in the shared
    /// message log, picked up on the next `refresh`. State side-effects for transactions already
    /// applied at payload-build time (`start_event`/`stop_event`).
    fn send_payload(&mut self, name: &str, payload: serde_json::Value) {
        let sender = self.backend.sender();
        let state = self.state.clone();
        let log = self.log.clone();
        let name = name.to_string();
        // A transaction start mints its id eagerly (the payload carries it), so confirm or roll back
        // that id on the response — auto-MeterValues only fire once the start is acknowledged.
        let started_tx = (name == "TransactionEvent"
            && payload.get("eventType").and_then(|v| v.as_str()) == Some("Started"))
        .then(|| {
            payload
                .pointer("/transactionInfo/transactionId")
                .and_then(|v| v.as_str())
                .map(String::from)
        });
        tokio::spawn(async move {
            match V2_0_1::decode_call(&name, payload) {
                Ok(action) => match sender.send(action).await {
                    Ok(_) => {
                        if let Some(tx_id) = started_tx {
                            let mut s = state.write().unwrap();
                            if s.transaction_id == tx_id {
                                s.tx_confirmed = true;
                            }
                        }
                    }
                    Err(e) => {
                        if let Some(tx_id) = started_tx {
                            let mut s = state.write().unwrap();
                            if s.transaction_id == tx_id {
                                s.transaction_id = None;
                                s.tx_confirmed = false;
                                s.status = "Available".to_string();
                            }
                        }
                        log.write().await.write(&format!("{name} failed: {e}"));
                    }
                },
                Err(e) => log
                    .write()
                    .await
                    .write(&format!("{name} invalid payload: {e}")),
            }
        });
    }
}

impl ModuleView for OcppClientV201View {
    fn name(&self) -> String {
        self.spec.name.clone()
    }

    fn is_overlay_active(&self) -> bool {
        self.edit.is_some()
            || self.config_edit.is_some()
            || self.action_dialog.is_some()
            || self.setup_overlay.is_some()
            || self.script_dialog.is_some()
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let buf = frame.buffer_mut();
        let [body, status_area] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
        let [left, right] =
            Layout::horizontal([Constraint::Length(66), Constraint::Min(1)]).areas(body);
        let [left_top, scripts_btn_area, left_bottom] = Layout::vertical([
            Constraint::Percentage(50),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .areas(left);
        let [actions_area, config_area] = Layout::vertical([
            Constraint::Max(2 + self.actions.state.values().len() as u16),
            Constraint::Min(9),
        ])
        .areas(left_bottom);
        let [config_table_area, config_input_area] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).areas(config_area);
        let [key_area, value_area] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .areas(config_input_area);
        let [right_top, right_bottom] =
            Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(right);

        self.state_table
            .state
            .set_focused(focused && self.focus == Pane::State);
        self.config_table
            .state
            .set_focused(focused && self.focus == Pane::ConfigTable);
        self.key_input
            .state
            .set_focused(focused && self.focus == Pane::ConfigKey);
        self.value_input
            .state
            .set_focused(focused && self.focus == Pane::ConfigValue);
        self.actions
            .state
            .set_focused(focused && self.focus == Pane::Actions);
        self.scripts_button
            .state
            .set_focused(focused && self.focus == Pane::Scripts);
        self.msg_table
            .state
            .set_focused(focused && self.focus == Pane::Messages);

        StatefulWidget::render(
            &self.state_table.widget,
            left_top,
            buf,
            &mut self.state_table.state,
        );
        StatefulWidget::render(
            &self.scripts_button.widget,
            scripts_btn_area,
            buf,
            &mut self.scripts_button.state,
        );
        StatefulWidget::render(
            &self.config_table.widget,
            config_table_area,
            buf,
            &mut self.config_table.state,
        );
        StatefulWidget::render(
            &self.key_input.widget,
            key_area,
            buf,
            &mut self.key_input.state,
        );
        StatefulWidget::render(
            &self.value_input.widget,
            value_area,
            buf,
            &mut self.value_input.state,
        );
        StatefulWidget::render(
            &self.actions.widget,
            actions_area,
            buf,
            &mut self.actions.state,
        );
        StatefulWidget::render(
            &self.msg_table.widget,
            right_top,
            buf,
            &mut self.msg_table.state,
        );
        StatefulWidget::render(&self.code.widget, right_bottom, buf, &mut self.code.state);

        // ONLINE/OFFLINE status line.
        let online = self.backend.is_online();
        let status_widget = TextBuilder::default()
            .horizontal_alignment(HorizontalAlignment::Center)
            .style(TextStyle {
                general: ratatui::prelude::Style::default()
                    .bg(if online {
                        COLOR_SCHEME.success
                    } else {
                        COLOR_SCHEME.error
                    })
                    .fg(if online {
                        COLOR_SCHEME.text_dark
                    } else {
                        COLOR_SCHEME.text
                    })
                    .bold(),
            })
            .build()
            .unwrap();
        let mut status = if online { "ONLINE" } else { "OFFLINE" }.to_string();
        StatefulWidget::render(&status_widget, status_area, buf, &mut status);

        // State-row edit overlay over the state table.
        if let Some(edit) = self.edit.as_mut() {
            let title = edit.field().label();
            let height = match edit {
                EditOverlay::Choice { sel, .. } => sel.state.values().len() as u16 + 2,
                EditOverlay::Number { .. } | EditOverlay::Text { .. } => 3,
            };
            let width = left_top.width.min(30);
            let [_, hc, _] = Layout::horizontal([
                Constraint::Min(0),
                Constraint::Length(width),
                Constraint::Min(0),
            ])
            .areas(left_top);
            let [_, vc, _] = Layout::vertical([
                Constraint::Min(0),
                Constraint::Length(height),
                Constraint::Min(0),
            ])
            .areas(hc);
            UiWidget::render(&Clear, vc, buf);
            let block = boxed(title);
            let inner = block.inner(vc);
            block.render(vc, buf);
            match edit {
                EditOverlay::Choice { sel, .. } => {
                    StatefulWidget::render(&sel.widget, inner, buf, &mut sel.state)
                }
                EditOverlay::Number { input, .. } => {
                    StatefulWidget::render(&input.widget, inner, buf, &mut input.state)
                }
                EditOverlay::Text { input, .. } => {
                    StatefulWidget::render(&input.widget, inner, buf, &mut input.state)
                }
            }
        }

        // Variable edit dialog, centered.
        if let Some(dialog) = self.config_edit.as_mut() {
            dialog.render(area, buf);
        }

        // Action JSON dialog, centered.
        if let Some((name, field)) = self.action_dialog.as_mut() {
            let [_, hc, _] = Layout::horizontal([
                Constraint::Percentage(20),
                Constraint::Percentage(60),
                Constraint::Percentage(20),
            ])
            .areas(area);
            let [_, vc, _] = Layout::vertical([
                Constraint::Percentage(20),
                Constraint::Percentage(60),
                Constraint::Percentage(20),
            ])
            .areas(hc);
            UiWidget::render(&Clear, vc, buf);
            let block = boxed(name);
            let inner = block.inner(vc);
            block.render(vc, buf);
            StatefulWidget::render(&field.widget, inner, buf, &mut field.state);
        }

        // Setup dialog (`:edit`) on top.
        if let Some(setup) = self.setup_overlay.as_mut() {
            setup.render(area, buf);
        }

        // Script manager dialog on top of everything.
        if let Some(dialog) = self.script_dialog.as_mut() {
            dialog.render(area, buf);
        }
    }

    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        if let Some(dialog) = self.script_dialog.as_mut() {
            if dialog.handle_events(modifiers, code) {
                // Closed: apply the edited scripts and reload the simulation.
                let scripts = self.script_dialog.take().unwrap().resolve();
                self.device.scripts = scripts;
                self.start_sim();
            }
            return EventResult::Consumed;
        }

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
                (KeyModifiers::NONE, KeyCode::Tab) => setup.focus_step(true),
                (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => {
                    setup.focus_step(false)
                }
                _ => {
                    let _ = setup.handle_events(modifiers, code);
                }
            }
            return EventResult::Consumed;
        }

        if let Some((name, field)) = self.action_dialog.as_mut() {
            match (modifiers, code) {
                (KeyModifiers::NONE, KeyCode::Esc) => self.action_dialog = None,
                (KeyModifiers::NONE, KeyCode::Enter) => {
                    if let Ok(payload) =
                        serde_json::from_str::<serde_json::Value>(&field.state.content())
                    {
                        let name = name.clone();
                        self.action_dialog = None;
                        self.pending_send = Some((name, payload));
                    }
                }
                _ => {
                    let _ = field.state.handle_events(modifiers, code);
                }
            }
            return EventResult::Consumed;
        }

        if let Some(edit) = self.edit.as_mut() {
            match (modifiers, code) {
                (KeyModifiers::NONE, KeyCode::Esc) => self.edit = None,
                (KeyModifiers::NONE, KeyCode::Enter) => self.apply_edit(),
                _ => match edit {
                    EditOverlay::Choice { sel, .. } => {
                        let _ = sel.state.handle_events(modifiers, code);
                    }
                    EditOverlay::Number { input, .. } => {
                        let _ = input.state.handle_events(modifiers, code);
                    }
                    EditOverlay::Text { input, .. } => {
                        let _ = input.state.handle_events(modifiers, code);
                    }
                },
            }
            return EventResult::Consumed;
        }

        if let Some(dialog) = self.config_edit.as_mut() {
            match (modifiers, code) {
                (KeyModifiers::NONE, KeyCode::Esc) => self.config_edit = None,
                (KeyModifiers::NONE, KeyCode::Enter) => self.apply_config_edit(),
                (KeyModifiers::NONE, KeyCode::Tab) => dialog.focus_next(),
                (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => {
                    dialog.focus_previous()
                }
                _ => dialog.handle_events(modifiers, code),
            }
            return EventResult::Consumed;
        }

        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Tab) => {
                self.focus_next();
                EventResult::Consumed
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => {
                self.focus_previous();
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Enter) => {
                match self.focus {
                    Pane::State => self.open_edit(),
                    Pane::Scripts => self.open_scripts(),
                    Pane::ConfigTable => self.open_config_edit(),
                    Pane::ConfigKey | Pane::ConfigValue => self.add_config_key(),
                    Pane::Actions => self.trigger_action(),
                    Pane::Messages => {}
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Char('d')) if matches!(self.focus, Pane::ConfigTable) => {
                let mut s = self.state.write().unwrap();
                if let Some(i) = self.config_table.state.table_state().selected()
                    && i < s.config.len()
                {
                    s.config.remove(i);
                    self.config_table.state.move_up();
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Char(' '))
                if !matches!(self.focus, Pane::ConfigKey | Pane::ConfigValue) =>
            {
                match self.focus {
                    Pane::State => self.open_edit(),
                    Pane::Scripts => self.open_scripts(),
                    Pane::ConfigTable => self.open_config_edit(),
                    Pane::Actions => self.trigger_action(),
                    _ => {}
                }
                EventResult::Consumed
            }
            _ => match self.focus {
                Pane::State => self.state_table.state.handle_events(modifiers, code),
                Pane::Scripts => EventResult::Consumed,
                Pane::ConfigTable => self.config_table.state.handle_events(modifiers, code),
                Pane::ConfigKey => self.key_input.state.handle_events(modifiers, code),
                Pane::ConfigValue => self.value_input.state.handle_events(modifiers, code),
                Pane::Actions => self.actions.state.handle_events(modifiers, code),
                Pane::Messages => {
                    let r = self.msg_table.state.handle_events(modifiers, code);
                    self.sync_code();
                    r
                }
            },
        }
    }

    fn refresh<'a>(&'a mut self) -> RefreshFuture<'a> {
        Box::pin(async move {
            if let Some((spec, path)) = self.pending_setup.take() {
                let device = OcppDeviceConfig::from_spec(&spec, self.device.scripts.clone());
                if spec.role == OcppRole::Server {
                    let _ = self.backend.stop().await;
                    self.replacement = Some(build_server_view(spec, path, device));
                    return;
                }
                if spec.version != self.spec.version {
                    // Switching version switches the view type (1.6 ↔ 2.0.1): rebuild via
                    // build_client_view, carrying the path + scripts. Scripts are kept but are
                    // version-specific — calls to actions the new version lacks return false.
                    let _ = self.backend.stop().await;
                    if !device.scripts.is_empty() {
                        self.log.write().await.write(
                            "Version switched: scripts kept but may call actions the new version lacks",
                        );
                    }
                    self.replacement = Some(build_client_view(spec, path, device));
                    return;
                } else {
                    let was_online = self.backend.is_online();
                    let _ = self.backend.stop().await;
                    self.spec = spec.clone();
                    self.device = device;
                    self.device_path = path;
                    self.backend = OcppClient::new(spec);
                    self.log.write().await.write("Settings updated");
                    if was_online {
                        let handler = self.make_handler();
                        let _ = self.backend.start(handler).await;
                    }
                }
            }

            if let Some((name, payload)) = self.pending_send.take() {
                self.send_payload(&name, payload);
            }

            // Drain actions enqueued by Lua scripts and send them.
            let queued: Vec<(String, serde_json::Value)> =
                self.action_queue.lock().unwrap().drain(..).collect();
            for (name, overrides) in queued {
                self.dispatch_lua_action(&name, overrides);
            }

            // While a confirmed transaction is active, report MeterValues periodically (~every 5s).
            // Gate on `tx_confirmed` so a start that the CSMS never acknowledged emits no readings.
            let tx_active = {
                let s = self.state.read().unwrap();
                s.transaction_id.is_some() && s.tx_confirmed
            };
            if tx_active {
                self.meter_tick = self.meter_tick.wrapping_add(1);
                if self.meter_tick.is_multiple_of(50) {
                    let payload = self.state_payload("MeterValues");
                    self.send_payload("MeterValues", payload);
                }
            }

            self.messages = self.backend.messages_snapshot().await;
            let rows: Vec<MsgRow> = self.messages.iter().map(msg_row).collect();
            self.msg_table.state.set_values(rows);
            let (state_rows, config_rows) = {
                let s = self.state.read().unwrap();
                (s.rows(), s.config_rows())
            };
            self.state_table.state.set_values(state_rows);
            self.config_table.state.set_values(config_rows);
            self.sync_code();
        })
    }

    fn handle_command<'a>(&'a mut self, cmd: &'a str) -> CommandFuture<'a> {
        match cmd.trim() {
            "start" => Box::pin(async move {
                let handler = self.make_handler();
                match self.backend.start(handler).await {
                    Ok(()) => {
                        CommandResult::Handled(Some(format!("Connecting to {}", self.spec.url())))
                    }
                    Err(e) => CommandResult::Handled(Some(format!("Connect failed: {e}"))),
                }
            }),
            "stop" => Box::pin(async move {
                match self.backend.stop().await {
                    Ok(()) => CommandResult::Handled(Some("Disconnected".into())),
                    Err(e) => CommandResult::Handled(Some(format!("Disconnect failed: {e}"))),
                }
            }),
            "restart" => Box::pin(async move {
                let _ = self.backend.stop().await;
                let handler = self.make_handler();
                match self.backend.start(handler).await {
                    Ok(()) => CommandResult::Handled(Some("Reconnecting".into())),
                    Err(e) => CommandResult::Handled(Some(format!("Reconnect failed: {e}"))),
                }
            }),
            "edit" | "e" => {
                self.setup_overlay = Some(OcppSetupDialog::edit(&self.spec, &self.device_path));
                Box::pin(std::future::ready(CommandResult::Handled(None)))
            }
            "compact" => {
                self.set_compact(!self.compact);
                Box::pin(std::future::ready(CommandResult::Handled(None)))
            }
            "wd" => {
                let result = if self.device_path.is_empty() {
                    CommandResult::Handled(Some("No configuration file path configured.".into()))
                } else {
                    self.save_device_to(&self.device_path.clone())
                };
                Box::pin(std::future::ready(result))
            }
            cmd if cmd.starts_with("wd ") => {
                let path = cmd["wd ".len()..].trim().to_string();
                let result = self.save_device_to(&path);
                Box::pin(std::future::ready(result))
            }
            _ => Box::pin(std::future::ready(CommandResult::Unhandled)),
        }
    }

    fn commands(&self) -> &[CommandDescriptor] {
        &OCPP_CLIENT_COMMANDS
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

static OCPP_CLIENT_COMMANDS: [CommandDescriptor; 6] = [
    CommandDescriptor {
        name: ":e | :edit",
        description: "edit module setup",
    },
    CommandDescriptor {
        name: ":start",
        description: "connect to the CSMS",
    },
    CommandDescriptor {
        name: ":stop",
        description: "disconnect",
    },
    CommandDescriptor {
        name: ":restart",
        description: "reconnect",
    },
    CommandDescriptor {
        name: ":compact",
        description: "toggle compact rows",
    },
    CommandDescriptor {
        name: ":wd | :write-device [path]",
        description: "save device config",
    },
];

// --- Widget builders -------------------------------------------------------

/// Theme border color, matching a table/selection border when unfocused.
fn border_style() -> ratatui::prelude::Style {
    ratatui::prelude::Style::default()
        .fg(COLOR_SCHEME.border)
        .bg(COLOR_SCHEME.bg)
}

fn boxed(title: &str) -> Block<'_> {
    Block::bordered()
        .style(
            ratatui::prelude::Style::default()
                .fg(COLOR_SCHEME.hi)
                .bg(COLOR_SCHEME.bg),
        )
        .title_alignment(HorizontalAlignment::Center)
        .title(title.to_string())
}

fn nv_table(rows: Vec<NvRow>) -> StateTable {
    Widget {
        state: TableStateBuilder::default().values(rows).build().unwrap(),
        widget: TableBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some("State".into()))
            .style(TableStyleBuilder::default().build().unwrap())
            .row_margin(Margin {
                vertical: 1,
                horizontal: 0,
            })
            .build()
            .unwrap(),
    }
}

fn config_table(rows: Vec<ConfigRow>) -> ConfigTable {
    Widget {
        state: TableStateBuilder::default().values(rows).build().unwrap(),
        widget: TableBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some("Variables".into()))
            .style(TableStyleBuilder::default().build().unwrap())
            .row_margin(Margin {
                vertical: 1,
                horizontal: 0,
            })
            .build()
            .unwrap(),
    }
}

fn panel_input(title: &str) -> Widget<InputFieldState, InputField<String>> {
    Widget {
        state: InputFieldStateBuilder::default()
            .focused(false)
            .disabled(false)
            .placeholder(Some(title.to_string()))
            .build()
            .unwrap(),
        widget: InputFieldBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some((title, HorizontalAlignment::Left).into()))
            .style(
                InputFieldStyleBuilder::default()
                    .border(border_style())
                    .build()
                    .unwrap(),
            )
            .margin(Margin {
                vertical: 0,
                horizontal: 0,
            })
            .build()
            .unwrap(),
    }
}

fn msg_table() -> MsgTable {
    Widget {
        state: TableStateBuilder::default()
            .values(Vec::new())
            .build()
            .unwrap(),
        widget: TableBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some("Messages".into()))
            .style(TableStyleBuilder::default().build().unwrap())
            .row_margin(Margin {
                vertical: 1,
                horizontal: 0,
            })
            .build()
            .unwrap(),
    }
}

fn action_list(values: Vec<String>) -> Widget<SelectionState<String>, Selection<String>> {
    Widget {
        state: SelectionStateBuilder::default()
            .focused(false)
            .values(values)
            .build()
            .unwrap(),
        widget: SelectionBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some(("Actions", HorizontalAlignment::Left).into()))
            .style(
                SelectionStyleBuilder::default()
                    .general(border_style())
                    .focused(
                        Style::default()
                            .fg(COLOR_SCHEME.bg)
                            .bg(COLOR_SCHEME.hi)
                            .bold(),
                    )
                    .build()
                    .unwrap(),
            )
            .margin(Margin {
                vertical: 0,
                horizontal: 0,
            })
            .build()
            .unwrap(),
    }
}

fn choice(options: &[&str], current: &str) -> Widget<SelectionState<String>, Selection<String>> {
    let values: Vec<String> = options.iter().map(|s| s.to_string()).collect();
    let mut state = SelectionStateBuilder::default()
        .focused(true)
        .values(values)
        .build()
        .unwrap();
    if let Some(idx) = options.iter().position(|o| *o == current) {
        state.set_selection(idx);
    }
    Widget {
        state,
        widget: SelectionBuilder::default()
            .style(SelectionStyle::default())
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .build()
            .unwrap(),
    }
}

fn number(current: f64) -> Widget<InputFieldState, InputField<f64>> {
    let mut state = InputFieldStateBuilder::default()
        .focused(true)
        .disabled(false)
        .build()
        .unwrap();
    let text = format!("{current}");
    state.set_input(text.clone());
    state.set_cursor(text.chars().count());
    Widget {
        state,
        widget: InputFieldBuilder::default()
            .style(InputFieldStyle::default())
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .build()
            .unwrap(),
    }
}

fn text_input(current: &str) -> Widget<InputFieldState, InputField<String>> {
    let mut state = InputFieldStateBuilder::default()
        .focused(true)
        .disabled(false)
        .build()
        .unwrap();
    state.set_input(current.to_string());
    state.set_cursor(current.chars().count());
    Widget {
        state,
        widget: InputFieldBuilder::default()
            .style(InputFieldStyle::default())
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .build()
            .unwrap(),
    }
}

fn editable_code() -> Widget<CodeInputFieldState, CodeInputField> {
    Widget {
        state: CodeInputFieldStateBuilder::default()
            .focused(true)
            .disabled(false)
            .build()
            .unwrap(),
        widget: CodeInputFieldBuilder::default()
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .build()
            .unwrap(),
    }
}

fn scripts_button() -> Widget<ButtonState, Button> {
    Widget {
        state: ButtonStateBuilder::default()
            .focused(false)
            .label("Lua Scripts".to_string())
            .disabled(false)
            .build()
            .unwrap(),
        widget: ButtonBuilder::default()
            .border_margin(Margin::new(1, 0))
            .margin(Margin {
                vertical: 0,
                horizontal: 0,
            })
            .style(ButtonStyle {
                general: border_style(),
                ..ButtonStyle::default()
            })
            .horizontal_alignment(HorizontalAlignment::Center)
            .build()
            .unwrap(),
    }
}

fn code_view() -> Widget<CodeInputFieldState, CodeInputField> {
    Widget {
        state: CodeInputFieldStateBuilder::default()
            .focused(false)
            .disabled(true)
            .placeholder(Some("select a message".to_string()))
            .build()
            .unwrap(),
        widget: CodeInputFieldBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some("Payload".into()))
            .style(
                InputFieldStyleBuilder::default()
                    .border(border_style())
                    .build()
                    .unwrap(),
            )
            .margin(Margin {
                vertical: 0,
                horizontal: 0,
            })
            .build()
            .unwrap(),
    }
}
