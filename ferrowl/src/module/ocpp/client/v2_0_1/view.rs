//! OCPP 2.0.1 charging-station (client) view. Same multi-connector layout as the 1.6 view (a
//! connector table + add-connector input over the selected entry's state table, scripts button,
//! action list and CS-level variable block on the left; message log filtered to the selection over
//! a JSON payload viewer on the right), adapted to 2.0.1: EVSE+connector scope, a connector-status
//! enum, a GetVariables-backed variable store, and `StartTransaction`/`StopTransaction` *shortcut*
//! buttons that emit a `TransactionEvent` for the selected connector.

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
use crate::module::ocpp::action_dialog::{ActionDialog, ActionResult, gen_tx_id, value_to_string};
use crate::module::ocpp::client::backend::{
    DEFAULT_HEARTBEAT_SECS, OcppClient, OcppMessage, TICKS_PER_SEC, boot_interval, rfc3339_now,
};
use crate::module::ocpp::client::build_client_view;
use crate::module::ocpp::client::config::{ConfigEditDialog, ConfigKey};
use crate::module::ocpp::client::lua_sim::{
    ClientFields, OcppSimHandle, ScopedActionQueue, merge_overrides, run_client_sim,
};
use crate::module::ocpp::client::scripts::ScriptDialog;
use crate::module::ocpp::client::v2_0_1::handler::CsStateHandler;
use crate::module::ocpp::client::v2_0_1::state::{ConfigRow, CsState, NvRow};
use crate::module::ocpp::config::device::{ConfigKeyDef, ConnectorRef, OcppDeviceConfig};
use crate::module::ocpp::config::session::{OcppModuleSpec, OcppRole, OcppSpec};
use crate::module::ocpp::scope::Scope;
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

// --- Connector table -------------------------------------------------------

#[derive(Clone, Debug)]
struct ConnRow {
    cp: String,
    connector: String,
}

impl TableEntry<2> for ConnRow {
    fn values(&self) -> [String; 2] {
        [self.cp.clone(), self.connector.clone()]
    }
    fn height(&self) -> u16 {
        1
    }
}

#[derive(Clone, Debug)]
struct ConnHeader;

impl Header<2> for ConnHeader {
    fn header() -> [String; 2] {
        ["Charge Point".into(), "Connector".into()]
    }
    fn widths() -> [Width; 2] {
        [Width { min: 12, max: 40 }, Width { min: 9, max: 16 }]
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

/// Which state row an edit overlay is changing (CS-level identity or connector metering/status).
#[derive(Clone, Copy)]
enum EditField {
    // Connector-level
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
    // CS-level
    Model,
    Vendor,
    FirmwareVersion,
    SerialNumber,
}

impl EditField {
    /// Map a CS-level state-table row (see [`CsState::cs_rows`]). Reserved RFID (row 4) is read-only.
    fn from_cs_row(row: usize) -> Option<EditField> {
        Some(match row {
            0 => EditField::Model,
            1 => EditField::Vendor,
            2 => EditField::FirmwareVersion,
            3 => EditField::SerialNumber,
            _ => return None,
        })
    }

    /// Map a connector state-table row (see [`ConnectorState::rows`]). Charge Limit (row 15) is
    /// read-only.
    fn from_conn_row(row: usize) -> Option<EditField> {
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

/// State-driven real actions: built straight from state, no dialog.
const STATE_DRIVEN: [&str; 5] = [
    "Authorize",
    "BootNotification",
    "Heartbeat",
    "MeterValues",
    "StatusNotification",
];

enum EditKind {
    Choice(Widget<SelectionState<String>, Selection<String>>),
    Number(Widget<InputFieldState, InputField<f64>>),
    Text(Widget<InputFieldState, InputField<String>>),
}

struct EditOverlay {
    field: EditField,
    /// The EVSE id of the connector being edited (`None` = CS-level field).
    evse: Option<i64>,
    kind: EditKind,
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
    Connectors,
    ConnectorInput,
    State,
    Scripts,
    ConfigTable,
    ConfigKey,
    ConfigValue,
    Actions,
    Messages,
    Payload,
}

type StateTable = Widget<TableState<NvRow, 3>, Table<NvRow, NvHeader, 3>>;
type ConnTable = Widget<TableState<ConnRow, 2>, Table<ConnRow, ConnHeader, 2>>;
type ConfigTable = Widget<TableState<ConfigRow, 3>, Table<ConfigRow, ConfigHeader, 3>>;
type MsgTable = Widget<TableState<MsgRow, 5>, Table<MsgRow, MsgHeader, 5>>;

pub struct OcppClientV201View {
    spec: OcppSpec,
    device_path: String,
    device: OcppDeviceConfig,
    backend: OcppClient<V2_0_1>,
    state: Arc<RwLock<CsState>>,
    log: SharedLog,
    conn_table: ConnTable,
    conn_input: Widget<InputFieldState, InputField<String>>,
    state_table: StateTable,
    config_table: ConfigTable,
    key_input: Widget<InputFieldState, InputField<String>>,
    value_input: Widget<InputFieldState, InputField<String>>,
    actions: Widget<SelectionState<String>, Selection<String>>,
    msg_table: MsgTable,
    messages: Vec<OcppMessage>,
    visible_messages: Vec<OcppMessage>,
    code: Widget<CodeInputFieldState, CodeInputField>,
    scripts_button: Widget<ButtonState, Button>,
    script_dialog: Option<ScriptDialog>,
    focus: Pane,
    edit: Option<EditOverlay>,
    config_edit: Option<ConfigEditDialog>,
    action_dialog: Option<ActionDialog>,
    pending_send: Option<(String, serde_json::Value, Scope)>,
    setup_overlay: Option<OcppSetupDialog>,
    pending_setup: Option<(OcppSpec, String)>,
    replacement: Option<Box<dyn ModuleView>>,
    action_queue: ScopedActionQueue,
    sim: Option<OcppSimHandle>,
    meter_tick: u32,
    heartbeat_tick: u32,
    was_online: bool,
    logged_seq: u64,
    applied_log_file: Option<String>,
    code_content: String,
    compact: bool,
    actions_for_connector: Option<bool>,
}

impl OcppClientV201View {
    pub fn new(spec: OcppSpec, device_path: String, device: OcppDeviceConfig) -> Self {
        let state = Arc::new(RwLock::new(CsState::default()));
        if !device.connectors.is_empty() {
            let mut s = state.write().unwrap();
            s.connectors.clear();
            for c in &device.connectors {
                s.add_connector(c.evse.unwrap_or(1), c.connector);
            }
            if s.connectors.is_empty() {
                s.add_connector(1, 1);
            }
        }
        // Seed persisted config keys from the device config (else keep the built-in defaults).
        if !device.config.is_empty() {
            let mut s = state.write().unwrap();
            s.config = device
                .config
                .iter()
                .map(|c| ConfigKey {
                    key: c.key.clone(),
                    value: c.value.clone(),
                    readonly: c.readonly,
                })
                .collect();
        }
        let cp = spec.name.clone();
        let (conn_rows, state_rows, config_rows) = {
            let s = state.read().unwrap();
            (conn_rows(&cp, &s), s.cs_rows(), s.config_rows())
        };
        let mut view = Self {
            device_path,
            device,
            backend: OcppClient::new(spec.clone()),
            state,
            log: Arc::new(AsyncRwLock::new(LogRing::init())),
            conn_table: conn_table(conn_rows),
            conn_input: panel_input("Add evse/connector"),
            state_table: nv_table(state_rows),
            config_table: config_table(config_rows),
            key_input: panel_input("Key"),
            value_input: panel_input("Value"),
            actions: action_list(Vec::new()),
            msg_table: msg_table(),
            messages: Vec::new(),
            visible_messages: Vec::new(),
            code: code_view(),
            scripts_button: scripts_button(),
            script_dialog: None,
            focus: Pane::Connectors,
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
            heartbeat_tick: 0,
            was_online: false,
            logged_seq: 0,
            applied_log_file: None,
            code_content: String::new(),
            compact: false,
            actions_for_connector: None,
            spec,
        };
        view.sync_actions();
        view.start_sim();
        view
    }

    fn cs_selected(&self) -> bool {
        !matches!(self.conn_table.state.table_state().selected(), Some(i) if i >= 1)
    }

    /// The scope of the selected connector-table row (CS row → `Scope::CS`).
    fn selected_scope(&self) -> Scope {
        match self.conn_table.state.table_state().selected() {
            Some(i) if i >= 1 => {
                let s = self.state.read().unwrap();
                s.connectors
                    .get(i - 1)
                    .map(|c| Scope::evse(c.evse_id, None))
                    .unwrap_or(Scope::CS)
            }
            _ => Scope::CS,
        }
    }

    fn enabled_scripts(&self) -> Vec<(String, String)> {
        self.device
            .scripts
            .iter()
            .filter(|s| s.enabled)
            .map(|s| (s.name.clone(), s.code.clone()))
            .collect()
    }

    fn start_sim(&mut self) {
        self.stop_sim();
        self.sim = run_client_sim(
            self.state.clone(),
            self.action_queue.clone(),
            self.enabled_scripts(),
            self.log.clone(),
        );
    }

    fn stop_sim(&mut self) {
        if let Some(mut sim) = self.sim.take() {
            sim.stop();
        }
    }

    fn open_scripts(&mut self) {
        self.script_dialog = Some(ScriptDialog::new(&self.device.scripts));
    }

    fn sync_actions(&mut self) {
        let want = !self.cs_selected();
        if self.actions_for_connector == Some(want) {
            return;
        }
        let names = if want {
            <CsState as ClientFields>::conn_actions()
        } else {
            <CsState as ClientFields>::cs_actions()
        };
        let values: Vec<String> = names.into_iter().map(|s| s.to_string()).collect();
        self.actions.state.set_values(values);
        self.actions_for_connector = Some(want);
    }

    /// Drain and send one Lua-enqueued action. The transaction shortcuts map to a TransactionEvent
    /// for the action's connector; state-driven and other actions build their payload then merge.
    fn dispatch_lua_action(&mut self, scope: Scope, name: &str, overrides: serde_json::Value) {
        let (send_name, mut payload) = match name {
            "StartTransaction" => ("TransactionEvent".to_string(), self.start_event(scope)),
            "StopTransaction" => match self.stop_event(scope) {
                Some(payload) => ("TransactionEvent".to_string(), payload),
                None => return,
            },
            n if STATE_DRIVEN.contains(&n) => (name.to_string(), self.state_payload(n, scope)),
            _ => {
                let template = V2_0_1::default_action(name)
                    .and_then(|a| V2_0_1::encode_action(&a).ok())
                    .unwrap_or_else(|| serde_json::json!({}));
                (name.to_string(), template)
            }
        };
        merge_overrides(&mut payload, overrides);
        self.send_payload(&send_name, payload, scope);
    }

    fn make_handler(&self) -> CsStateHandler {
        CsStateHandler::new(
            self.backend.online_handle(),
            self.backend.messages_handle(),
            self.state.clone(),
        )
    }

    fn save_device_to(&self, path: &str) -> CommandResult {
        use ferrowl_util::convert::{Converter, FileType};
        let Some(ty) = FileType::from_path(path) else {
            return CommandResult::Handled(Some(format!(
                "unknown format for '{path}' (use .toml or .json)"
            )));
        };
        let mut device = OcppDeviceConfig::from_spec(&self.spec, self.device.scripts.clone());
        device.version = Some(crate::config::VERSION.to_string());
        device.log_file = self.device.log_file.clone();
        device.connectors = self
            .state
            .read()
            .unwrap()
            .connectors
            .iter()
            .map(|c| ConnectorRef {
                evse: Some(c.evse_id),
                connector: c.connector_id,
            })
            .collect();
        // Persist the client's config keys (server config is transient, never written).
        device.config = self
            .state
            .read()
            .unwrap()
            .config
            .iter()
            .map(|c| ConfigKeyDef {
                key: c.key.clone(),
                value: c.value.clone(),
                readonly: c.readonly,
            })
            .collect();
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
        // The connector table stays compact (no vertical margin) to save space.
        self.state_table.widget.set_row_margin(margin);
        self.config_table.widget.set_row_margin(margin);
        self.msg_table.widget.set_row_margin(margin);
    }

    /// Add a connector from the input field (`evse/connector` or bare `connector`, evse default 1).
    fn add_connector(&mut self) {
        let raw = self.conn_input.state.input().trim().to_string();
        let (evse, connector) = match raw.split_once('/') {
            Some((e, c)) => (parse_id(e).unwrap_or(1), parse_id(c)),
            None => (1, parse_id(&raw)),
        };
        let Some(connector) = connector else {
            return;
        };
        let added = self.state.write().unwrap().add_connector(evse, connector);
        self.conn_input.state.set_input(String::new());
        self.conn_input.state.set_cursor(0);
        if added {
            // Rebuild the (now-sorted) table and select the new connector's row (CS row = 0).
            let cp = self.spec.name.clone();
            let (rows, row) = {
                let s = self.state.read().unwrap();
                let row = s
                    .connectors
                    .iter()
                    .position(|c| c.connector_id == connector)
                    .map(|p| p + 1)
                    .unwrap_or(0);
                (conn_rows(&cp, &s), row)
            };
            self.conn_table.state.set_values(rows);
            select_index(&mut self.conn_table.state, row);
            self.sync_actions();
        }
    }

    fn remove_connector(&mut self) {
        let Some(i) = self.conn_table.state.table_state().selected() else {
            return;
        };
        if i == 0 {
            return;
        }
        let mut s = self.state.write().unwrap();
        if s.connectors.len() <= 1 || i > s.connectors.len() {
            return;
        }
        s.connectors.remove(i - 1);
        drop(s);
        self.conn_table.state.move_up();
        self.sync_actions();
    }

    fn trigger_action(&mut self) {
        let name = self.actions.state.get_value();
        if name.is_empty() {
            return;
        }
        let scope = self.selected_scope();
        match name.as_str() {
            "StartTransaction" => {
                let payload = self.start_event(scope);
                self.pending_send = Some(("TransactionEvent".to_string(), payload, scope));
            }
            "StopTransaction" => {
                if let Some(payload) = self.stop_event(scope) {
                    self.pending_send = Some(("TransactionEvent".to_string(), payload, scope));
                }
            }
            n if STATE_DRIVEN.contains(&n) => {
                let payload = self.state_payload(n, scope);
                self.pending_send = Some((name, payload, scope));
            }
            _ => {
                self.action_dialog = Some(
                    match crate::module::ocpp::spec::v2_0_1::action_spec(&name) {
                        Some(spec) => {
                            let state = self.state.clone();
                            let eid = scope.evse;
                            let lookup = move |f: &str| {
                                let s = state.read().unwrap();
                                eid.and_then(|e| s.connector_by_evse(e))
                                    .or_else(|| s.connectors.first())
                                    .and_then(|c| c.get_field(f))
                                    .or_else(|| s.cs_get_field(f))
                                    .map(value_to_string)
                            };
                            ActionDialog::new(name, &spec, lookup, gen_tx_id)
                        }
                        None => {
                            debug_assert!(
                                crate::module::ocpp::spec::v2_0_1::json_actions()
                                    .contains(&name.as_str()),
                                "{name} has no spec and is not a registered JSON action"
                            );
                            let template = V2_0_1::default_action(&name)
                                .and_then(|a| V2_0_1::encode_action(&a).ok())
                                .map(|v| serde_json::to_string_pretty(&v).unwrap_or_default())
                                .unwrap_or_else(|| "{}".to_string());
                            ActionDialog::json_only(name, &template)
                        }
                    },
                );
            }
        }
    }

    /// Build a `TransactionEvent(Started)` for the connector resolved from `scope`, minting a tx id.
    fn start_event(&mut self, scope: Scope) -> serde_json::Value {
        let mut s = self.state.write().unwrap();
        let idx = connector_index(&s, scope);
        let Some(i) = idx else {
            return serde_json::json!({});
        };
        let tx = s.connectors[i].start_tx();
        let seq = s.connectors[i].next_seq();
        s.connectors[i].status = "Occupied".to_string();
        s.connectors[i].session_energy = 0.0;
        let c = &s.connectors[i];
        let payload = serde_json::json!({
            "eventType": "Started",
            "timestamp": rfc3339_now(),
            "triggerReason": "Authorized",
            "seqNo": seq,
            "transactionInfo": { "transactionId": tx },
            "idToken": { "idToken": c.rfid, "type": "Central" },
            "evse": { "id": c.evse_id, "connectorId": c.connector_id },
        });
        drop(s);
        self.meter_tick = 0;
        payload
    }

    /// Build a `TransactionEvent(Ended)` for the connector resolved from `scope`, or `None` if idle.
    fn stop_event(&mut self, scope: Scope) -> Option<serde_json::Value> {
        let mut s = self.state.write().unwrap();
        let i = connector_index(&s, scope)?;
        let tx = s.connectors[i].transaction_id.clone()?;
        let seq = s.connectors[i].next_seq();
        s.connectors[i].status = "Available".to_string();
        s.connectors[i].transaction_id = None;
        s.connectors[i].tx_confirmed = false;
        let c = &s.connectors[i];
        Some(serde_json::json!({
            "eventType": "Ended",
            "timestamp": rfc3339_now(),
            "triggerReason": "StopAuthorized",
            "seqNo": seq,
            "transactionInfo": { "transactionId": tx },
            "idToken": { "idToken": c.rfid, "type": "Central" },
        }))
    }

    /// Build the request payload for a state-driven action, using the connector from `scope`.
    fn state_payload(&self, name: &str, scope: Scope) -> serde_json::Value {
        let s = self.state.read().unwrap();
        let conn = scope
            .evse
            .and_then(|e| s.connector_by_evse(e))
            .or_else(|| s.connectors.first());
        let evse = conn.map(|c| c.evse_id).unwrap_or(1);
        let cid = conn.map(|c| c.connector_id).unwrap_or(1);
        let rfid = conn.map(|c| c.rfid.clone()).unwrap_or_default();
        match name {
            "Authorize" => serde_json::json!({
                "idToken": { "idToken": rfid, "type": "Central" },
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
                "evseId": evse,
                "meterValue": conn.map(|c| c.meter_value_json()).unwrap_or(serde_json::json!([])),
            }),
            "StatusNotification" => serde_json::json!({
                "timestamp": rfc3339_now(),
                "connectorStatus": conn.map(|c| c.status.clone()).unwrap_or_default(),
                "evseId": evse,
                "connectorId": cid,
            }),
            _ => serde_json::json!({}),
        }
    }

    fn focus_next(&mut self) {
        self.focus = match self.focus {
            Pane::ConnectorInput => Pane::Connectors,
            Pane::Connectors => Pane::State,
            Pane::State => Pane::Scripts,
            Pane::Scripts => Pane::Actions,
            Pane::Actions if self.cs_selected() => Pane::ConfigTable,
            Pane::Actions => Pane::Messages,
            Pane::ConfigTable => Pane::ConfigKey,
            Pane::ConfigKey => Pane::ConfigValue,
            Pane::ConfigValue => Pane::Messages,
            Pane::Messages => Pane::Payload,
            Pane::Payload => Pane::ConnectorInput,
        };
    }

    fn focus_previous(&mut self) {
        self.focus = match self.focus {
            Pane::ConnectorInput => Pane::Payload,
            Pane::Connectors => Pane::ConnectorInput,
            Pane::State => Pane::Connectors,
            Pane::Scripts => Pane::State,
            Pane::Actions => Pane::Scripts,
            Pane::ConfigTable => Pane::Actions,
            Pane::ConfigKey => Pane::ConfigTable,
            Pane::ConfigValue => Pane::ConfigKey,
            Pane::Messages if self.cs_selected() => Pane::ConfigValue,
            Pane::Messages => Pane::Actions,
            Pane::Payload => Pane::Messages,
        };
    }

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
        let cs = self.cs_selected();
        let field = if cs {
            EditField::from_cs_row(row)
        } else {
            EditField::from_conn_row(row)
        };
        let Some(field) = field else { return };
        let evse = if cs { None } else { self.selected_scope().evse };
        let s = self.state.read().unwrap();
        let conn = evse
            .and_then(|e| s.connector_by_evse(e))
            .or_else(|| s.connectors.first());
        let kind = match field {
            EditField::Phases => EditKind::Choice(choice(
                &PHASE_CHOICES,
                conn.map(|c| c.phases.as_str()).unwrap_or(""),
            )),
            EditField::Status => EditKind::Choice(choice(
                &STATUS_CHOICES,
                conn.map(|c| c.status.as_str()).unwrap_or(""),
            )),
            EditField::EvseId => {
                EditKind::Number(number(conn.map(|c| c.evse_id as f64).unwrap_or(1.0)))
            }
            EditField::ConnectorId => {
                EditKind::Number(number(conn.map(|c| c.connector_id as f64).unwrap_or(0.0)))
            }
            EditField::Voltage => EditKind::Number(number(conn.map(|c| c.voltage).unwrap_or(0.0))),
            EditField::Current(i) => {
                EditKind::Number(number(conn.map(|c| c.current[i]).unwrap_or(0.0)))
            }
            EditField::Power => EditKind::Number(number(conn.map(|c| c.power).unwrap_or(0.0))),
            EditField::Frequency => {
                EditKind::Number(number(conn.map(|c| c.frequency).unwrap_or(0.0)))
            }
            EditField::TotalEnergy => {
                EditKind::Number(number(conn.map(|c| c.total_energy).unwrap_or(0.0)))
            }
            EditField::SessionEnergy => {
                EditKind::Number(number(conn.map(|c| c.session_energy).unwrap_or(0.0)))
            }
            EditField::Soc => EditKind::Number(number(conn.map(|c| c.soc).unwrap_or(0.0))),
            EditField::Temperature => {
                EditKind::Number(number(conn.map(|c| c.temperature).unwrap_or(0.0)))
            }
            EditField::Rfid => {
                EditKind::Text(text_input(conn.map(|c| c.rfid.as_str()).unwrap_or("")))
            }
            EditField::Model => EditKind::Text(text_input(&s.model)),
            EditField::Vendor => EditKind::Text(text_input(&s.vendor)),
            EditField::FirmwareVersion => EditKind::Text(text_input(&s.firmware_version)),
            EditField::SerialNumber => EditKind::Text(text_input(&s.serial_number)),
        };
        drop(s);
        self.edit = Some(EditOverlay { field, evse, kind });
    }

    fn apply_edit(&mut self) {
        let Some(edit) = self.edit.take() else { return };
        let mut s = self.state.write().unwrap();
        let conn_idx = edit
            .evse
            .and_then(|e| s.connectors.iter().position(|c| c.evse_id == e))
            .or((!s.connectors.is_empty()).then_some(0));
        match edit.kind {
            EditKind::Choice(sel) => {
                let value = sel.state.get_value();
                if let Some(i) = conn_idx {
                    let c = &mut s.connectors[i];
                    match edit.field {
                        EditField::Phases => c.phases = value,
                        EditField::Status => c.status = value,
                        _ => {}
                    }
                }
            }
            EditKind::Number(input) => {
                let Ok(value) = input.state.input().trim().parse::<f64>() else {
                    return;
                };
                if let Some(i) = conn_idx {
                    let c = &mut s.connectors[i];
                    match edit.field {
                        EditField::EvseId => c.evse_id = value as i64,
                        EditField::ConnectorId => c.connector_id = value as i64,
                        EditField::Voltage => c.voltage = value,
                        EditField::Current(j) => c.current[j] = value,
                        EditField::Power => c.power = value,
                        EditField::Frequency => c.frequency = value,
                        EditField::TotalEnergy => c.total_energy = value,
                        EditField::SessionEnergy => c.session_energy = value,
                        EditField::Soc => c.soc = value,
                        EditField::Temperature => c.temperature = value,
                        _ => {}
                    }
                }
            }
            EditKind::Text(input) => {
                let value = input.state.input().trim().to_string();
                match edit.field {
                    EditField::Rfid => {
                        if let Some(i) = conn_idx {
                            s.connectors[i].rfid = value;
                        }
                    }
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
            .and_then(|i| self.visible_messages.get(i))
            .map(|m| serde_json::to_string_pretty(&m.payload).unwrap_or_default())
            .unwrap_or_default();
        if content == self.code_content {
            return;
        }
        self.code_content = content.clone();
        self.code.state.set_content(&content);
    }

    /// Decode + send a (name, payload) at `scope` without blocking the UI loop. A transaction start
    /// mints its id eagerly (carried in the payload); confirm or roll it back on the response so
    /// auto-MeterValues only fire once the start is acknowledged.
    fn send_payload(&mut self, name: &str, payload: serde_json::Value, scope: Scope) {
        let sender = self.backend.sender();
        let state = self.state.clone();
        let log = self.log.clone();
        let name = name.to_string();
        let started_tx = (name == "TransactionEvent"
            && payload.get("eventType").and_then(|v| v.as_str()) == Some("Started"))
        .then(|| {
            payload
                .pointer("/transactionInfo/transactionId")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .flatten();
        let evse_id = scope.evse;
        tokio::spawn(async move {
            match V2_0_1::decode_call(&name, payload) {
                Ok(action) => match sender.send_scoped(action, scope).await {
                    Ok(response) => {
                        if name == "BootNotification" {
                            state.write().unwrap().heartbeat_interval_secs =
                                boot_interval(&response);
                        }
                        if let Some(tx_id) = started_tx {
                            let mut s = state.write().unwrap();
                            if let Some(c) = evse_id.and_then(|e| s.connector_mut_by_evse(e))
                                && c.transaction_id.as_deref() == Some(tx_id.as_str())
                            {
                                c.tx_confirmed = true;
                            }
                        }
                    }
                    Err(e) => {
                        if let Some(tx_id) = started_tx {
                            let mut s = state.write().unwrap();
                            if let Some(c) = evse_id.and_then(|e| s.connector_mut_by_evse(e))
                                && c.transaction_id.as_deref() == Some(tx_id.as_str())
                            {
                                c.transaction_id = None;
                                c.tx_confirmed = false;
                                c.status = "Available".to_string();
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

/// Resolve the connector index targeted by `scope` (the connector on its EVSE, else the first).
fn connector_index(s: &CsState, scope: Scope) -> Option<usize> {
    scope
        .evse
        .and_then(|e| s.connectors.iter().position(|c| c.evse_id == e))
        .or((!s.connectors.is_empty()).then_some(0))
}

/// Parse a connector/evse id, tolerating a leading `e`/`c` label (e.g. `e1`, `c2`).
fn parse_id(raw: &str) -> Option<i64> {
    raw.trim()
        .trim_start_matches(['e', 'c', 'E', 'C'])
        .trim()
        .parse()
        .ok()
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
        let cs = self.cs_selected();
        let [body, status_area] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
        let [left, right] =
            Layout::horizontal([Constraint::Length(66), Constraint::Min(1)]).areas(body);

        let n_conn = self.conn_table.state.values().len() as u16;
        let n_actions = self.actions.state.values().len() as u16;
        // Config block only when the CS row is selected (13 table + 3 input).
        let config_len = if cs { 16 } else { 0 };
        let [
            conn_input_area,
            conn_area,
            state_area,
            scripts_btn_area,
            actions_area,
            config_area,
        ] = Layout::vertical([
            Constraint::Length(3),                         // Add-connector input (top)
            Constraint::Length((n_conn + 3).clamp(6, 12)), // Connectors (compact, ≥3 entries)
            Constraint::Min(16),                           // State (≥5 entries + header)
            Constraint::Length(3),                         // Scripts button
            Constraint::Max(2 + n_actions),                // Actions
            Constraint::Length(config_len),                // Config block (CS only)
        ])
        .areas(left);
        let [config_table_area, config_input_area] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).areas(config_area);
        let [key_area, value_area] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .areas(config_input_area);
        let [right_top, right_bottom] =
            Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(right);

        self.conn_table
            .state
            .set_focused(focused && self.focus == Pane::Connectors);
        self.conn_input
            .state
            .set_focused(focused && self.focus == Pane::ConnectorInput);
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
        self.code
            .state
            .set_focused(focused && self.focus == Pane::Payload);

        StatefulWidget::render(
            &self.conn_table.widget,
            conn_area,
            buf,
            &mut self.conn_table.state,
        );
        StatefulWidget::render(
            &self.conn_input.widget,
            conn_input_area,
            buf,
            &mut self.conn_input.state,
        );
        StatefulWidget::render(
            &self.state_table.widget,
            state_area,
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
            &self.actions.widget,
            actions_area,
            buf,
            &mut self.actions.state,
        );
        if cs {
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
        }
        StatefulWidget::render(
            &self.msg_table.widget,
            right_top,
            buf,
            &mut self.msg_table.state,
        );
        StatefulWidget::render(&self.code.widget, right_bottom, buf, &mut self.code.state);

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

        if let Some(edit) = self.edit.as_mut() {
            let title = edit.field.label();
            let height = match &edit.kind {
                EditKind::Choice(sel) => sel.state.values().len() as u16 + 2,
                EditKind::Number(_) | EditKind::Text(_) => 3,
            };
            let width = state_area.width.min(30);
            let [_, hc, _] = Layout::horizontal([
                Constraint::Min(0),
                Constraint::Length(width),
                Constraint::Min(0),
            ])
            .areas(state_area);
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
            match &mut edit.kind {
                EditKind::Choice(sel) => {
                    StatefulWidget::render(&sel.widget, inner, buf, &mut sel.state)
                }
                EditKind::Number(input) => {
                    StatefulWidget::render(&input.widget, inner, buf, &mut input.state)
                }
                EditKind::Text(input) => {
                    StatefulWidget::render(&input.widget, inner, buf, &mut input.state)
                }
            }
        }

        if let Some(dialog) = self.config_edit.as_mut() {
            dialog.render(area, buf);
        }
        if let Some(dlg) = self.action_dialog.as_mut() {
            dlg.render(area, buf);
        }
        if let Some(setup) = self.setup_overlay.as_mut() {
            setup.render(area, buf);
        }
        if let Some(dialog) = self.script_dialog.as_mut() {
            dialog.render(area, buf);
        }
    }

    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        if let Some(dialog) = self.script_dialog.as_mut() {
            if dialog.handle_events(modifiers, code) {
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

        if self.action_dialog.is_some() {
            let res = self.action_dialog.as_mut().unwrap().input(modifiers, code);
            match res {
                Some(ActionResult::Close) => self.action_dialog = None,
                Some(ActionResult::Send(payload)) => {
                    let name = self.action_dialog.as_ref().unwrap().name.clone();
                    if V2_0_1::decode_call(&name, payload.clone()).is_ok() {
                        let scope = self.selected_scope();
                        self.action_dialog = None;
                        self.pending_send = Some((name, payload, scope));
                    }
                }
                None => {}
            }
            return EventResult::Consumed;
        }

        if let Some(edit) = self.edit.as_mut() {
            match (modifiers, code) {
                (KeyModifiers::NONE, KeyCode::Esc) => self.edit = None,
                (KeyModifiers::NONE, KeyCode::Enter) => self.apply_edit(),
                _ => match &mut edit.kind {
                    EditKind::Choice(sel) => {
                        let _ = sel.state.handle_events(modifiers, code);
                    }
                    EditKind::Number(input) => {
                        let _ = input.state.handle_events(modifiers, code);
                    }
                    EditKind::Text(input) => {
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
                    Pane::Connectors => self.sync_actions(),
                    Pane::ConnectorInput => self.add_connector(),
                    Pane::State => self.open_edit(),
                    Pane::Scripts => self.open_scripts(),
                    Pane::ConfigTable => self.open_config_edit(),
                    Pane::ConfigKey | Pane::ConfigValue => self.add_config_key(),
                    Pane::Actions => self.trigger_action(),
                    Pane::Messages | Pane::Payload => {}
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
            (KeyModifiers::NONE, KeyCode::Char('d')) if matches!(self.focus, Pane::Connectors) => {
                self.remove_connector();
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Char(' '))
                if !matches!(
                    self.focus,
                    Pane::ConfigKey | Pane::ConfigValue | Pane::ConnectorInput
                ) =>
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
                Pane::Connectors => {
                    let r = self.conn_table.state.handle_events(modifiers, code);
                    self.sync_actions();
                    r
                }
                Pane::ConnectorInput => self.conn_input.state.handle_events(modifiers, code),
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
                Pane::Payload => self.code.state.handle_events(modifiers, code),
            },
        }
    }

    fn refresh<'a>(&'a mut self) -> RefreshFuture<'a> {
        Box::pin(async move {
            if let Some((spec, path)) = self.pending_setup.take() {
                let mut device = OcppDeviceConfig::from_spec(&spec, self.device.scripts.clone());
                device.log_file = self.device.log_file.clone();
                device.connectors = self.device.connectors.clone();
                device.config = self.device.config.clone();
                if spec.role == OcppRole::Server {
                    let _ = self.backend.stop().await;
                    self.replacement = Some(build_server_view(spec, path, device));
                    return;
                }
                if spec.version != self.spec.version {
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

            if let Some((name, payload, scope)) = self.pending_send.take() {
                self.send_payload(&name, payload, scope);
            }

            let queued: Vec<(Scope, String, serde_json::Value)> =
                self.action_queue.lock().unwrap().drain(..).collect();
            for (scope, name, overrides) in queued {
                self.dispatch_lua_action(scope, &name, overrides);
            }

            let online = self.backend.is_online();
            if self.was_online && !online {
                self.log
                    .write()
                    .await
                    .write("Connection lost — auto-transmit halted");
                self.heartbeat_tick = 0;
            }
            self.was_online = online;

            if online {
                let interval_secs = self
                    .state
                    .read()
                    .unwrap()
                    .heartbeat_interval_secs
                    .unwrap_or(DEFAULT_HEARTBEAT_SECS)
                    .max(1);
                self.heartbeat_tick = self.heartbeat_tick.wrapping_add(1);
                if self.heartbeat_tick >= interval_secs as u32 * TICKS_PER_SEC {
                    self.heartbeat_tick = 0;
                    self.send_payload("Heartbeat", serde_json::json!({}), Scope::CS);
                }
            }

            // Auto-MeterValues per connector with a confirmed transaction (~every 5s), gated online.
            let active: Vec<Scope> = {
                let s = self.state.read().unwrap();
                s.connectors
                    .iter()
                    .filter(|c| c.transaction_id.is_some() && c.tx_confirmed)
                    .map(|c| Scope::evse(c.evse_id, None))
                    .collect()
            };
            if !active.is_empty() && online {
                self.meter_tick = self.meter_tick.wrapping_add(1);
                if self.meter_tick.is_multiple_of(50) {
                    for scope in active {
                        let payload = self.state_payload("MeterValues", scope);
                        self.send_payload("MeterValues", payload, scope);
                    }
                }
            }

            if self.applied_log_file != self.device.log_file {
                let name = self.spec.name.clone();
                self.log
                    .write()
                    .await
                    .set_log_file(self.device.log_file.as_deref(), &name);
                self.applied_log_file = self.device.log_file.clone();
            }

            self.messages = self.backend.messages_snapshot().await;
            let mut max_seq = self.logged_seq;
            let new_lines: Vec<String> = self
                .messages
                .iter()
                .filter(|m| m.seq > self.logged_seq)
                .map(|m| {
                    max_seq = max_seq.max(m.seq);
                    m.log_line()
                })
                .collect();
            if !new_lines.is_empty() {
                let mut log = self.log.write().await;
                for line in new_lines {
                    log.write(&line);
                }
                self.logged_seq = max_seq;
            }

            let scope = self.selected_scope();
            self.visible_messages = self
                .messages
                .iter()
                .filter(|m| m.scope == scope)
                .cloned()
                .collect();
            let rows: Vec<MsgRow> = self.visible_messages.iter().map(msg_row).collect();
            let at_bottom = msg_log_at_bottom(&self.msg_table.state);
            self.msg_table.state.set_values(rows);
            // Tail the log to the newest message so incoming traffic shows instantly, but never
            // while the user is reading it (Messages scrolled up) or scrolling the payload pane
            // (whose content is driven by the selected message row).
            let follow = match self.focus {
                Pane::Payload => false,
                Pane::Messages => at_bottom,
                _ => true,
            };
            if follow {
                self.msg_table.state.move_to_bottom();
            }

            let cp = self.spec.name.clone();
            let (conn_rows, state_rows, config_rows) = {
                let s = self.state.read().unwrap();
                let state_rows = match scope.evse {
                    Some(e) => s
                        .connector_by_evse(e)
                        .map(|c| c.rows())
                        .unwrap_or_else(|| s.cs_rows()),
                    None => s.cs_rows(),
                };
                (conn_rows(&cp, &s), state_rows, s.config_rows())
            };
            self.conn_table.state.set_values(conn_rows);
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
            "log" => {
                self.device.log_file = None;
                Box::pin(std::future::ready(CommandResult::Handled(Some(
                    "File logging disabled".into(),
                ))))
            }
            cmd if cmd.starts_with("log ") => {
                let path = cmd["log ".len()..].trim().to_string();
                let msg = if path.is_empty() {
                    self.device.log_file = None;
                    "File logging disabled".to_string()
                } else {
                    self.device.log_file = Some(path.clone());
                    format!("Logging to {path}")
                };
                Box::pin(std::future::ready(CommandResult::Handled(Some(msg))))
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

static OCPP_CLIENT_COMMANDS: [CommandDescriptor; 7] = [
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
    CommandDescriptor {
        name: ":log [file]",
        description: "set/clear log file",
    },
];

// --- Widget builders -------------------------------------------------------

/// Whether a message table's selection is on (or past) the last row — i.e. the user is tailing it.
/// An empty table or no selection counts as tailing.
fn msg_log_at_bottom<V: TableEntry<N>, const N: usize>(state: &TableState<V, N>) -> bool {
    let len = state.values().len();
    len == 0
        || state
            .table_state()
            .selected()
            .map(|s| s + 1 >= len)
            .unwrap_or(true)
}

/// Select row `idx` in a table (no direct setter on `TableState`): jump to the top, then step down.
fn select_index<V: TableEntry<N>, const N: usize>(state: &mut TableState<V, N>, idx: usize) {
    state.move_to_top();
    for _ in 0..idx {
        state.move_down();
    }
}

fn conn_rows(cp: &str, s: &CsState) -> Vec<ConnRow> {
    let mut rows = vec![ConnRow {
        cp: cp.to_string(),
        connector: String::new(),
    }];
    for c in &s.connectors {
        rows.push(ConnRow {
            cp: cp.to_string(),
            connector: Scope::evse(c.evse_id, None).label(),
        });
    }
    rows
}

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

fn conn_table(rows: Vec<ConnRow>) -> ConnTable {
    Widget {
        state: TableStateBuilder::default().values(rows).build().unwrap(),
        widget: TableBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some("Connectors".into()))
            .style(TableStyleBuilder::default().build().unwrap())
            // Always compact (no vertical margin), independent of `:compact`, to save space.
            .row_margin(Margin {
                vertical: 0,
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

#[cfg(test)]
mod tests {
    use super::EditField;
    use crate::module::ocpp::client::v2_0_1::state::{ConnectorState, CsState};

    #[test]
    fn ut_edit_field_conn_rows_align() {
        let rows = ConnectorState::new(1, 1).rows();
        for (i, row) in rows.iter().enumerate() {
            match EditField::from_conn_row(i) {
                Some(f) => assert_eq!(f.label(), row.name, "row {i} label mismatch"),
                None => assert!(
                    row.name == "Charge Limit",
                    "row {i} ({}) maps to no field",
                    row.name
                ),
            }
        }
        assert!(EditField::from_conn_row(rows.len()).is_none());
    }

    #[test]
    fn ut_edit_field_cs_rows_align() {
        let rows = CsState::default().cs_rows();
        for (i, row) in rows.iter().enumerate() {
            match EditField::from_cs_row(i) {
                Some(f) => assert_eq!(f.label(), row.name, "row {i} label mismatch"),
                None => assert!(
                    row.name == "Reserved RFID",
                    "row {i} ({}) maps to no field",
                    row.name
                ),
            }
        }
    }
}
