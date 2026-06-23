//! OCPP charging-station (client) view, generic over the OCPP version via [`ClientVersion`]. Left
//! column: a connector table (CS row + one row per connector) over an add-connector input, then the
//! selected entry's state table, the scripts button, the action list and (CS-level only) the
//! config/variable block. Right column: the message log (filtered to the selected entry) over a JSON
//! payload viewer; an ONLINE/OFFLINE status line.
//!
//! Selecting the CS row shows CS-level state (identity), the config table, and non-connector
//! actions. Selecting a connector shows that connector's metering/status, hides config, and shows
//! connector-scoped actions. The message log is partitioned by the same scope.
//!
//! Per-version behaviour (scope ctor, the `EditField` row map + labels, the connector-status choice
//! list, the state-driven action set, the config-vs-variable labels, the exact request JSON, and the
//! 2.0.1 `StartTransaction`/`StopTransaction` transaction-shortcut buttons) lives behind the
//! [`ClientVersion`] trait; the two concrete views are `ClientView<V1_6>` / `ClientView<V2_0_1>`.

use std::collections::VecDeque;
use std::marker::PhantomData;
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
        Button, ButtonBuilder, CodeInputField, CodeInputFieldBuilder, GetValue, InputField,
        InputFieldBuilder, Selection, SelectionBuilder, Table, TableBuilder, TableEntry,
        TextBuilder, Widget,
    },
};
use ferrowl_ui_derive::TableEntry;
use ratatui::style::Style;
use ratatui::{
    Frame,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, Clear, StatefulWidget, Widget as UiWidget},
};
use tokio::sync::RwLock as AsyncRwLock;

use ferrowl_ocpp::Version;
use ferrowl_ocpp::cs::CsActionHandler;

use crate::app::LogRing;
use crate::module::ocpp::action_dialog::{ActionDialog, ActionResult, gen_tx_id, value_to_string};
use crate::module::ocpp::client::backend::{
    DEFAULT_HEARTBEAT_SECS, Messages, OcppClient, OcppMessage, TICKS_PER_SEC,
};
use crate::module::ocpp::client::build_client_view;
use crate::module::ocpp::client::config::{ConfigEditDialog, ConfigKey};
use crate::module::ocpp::client::lua_sim::{
    ClientFields, OcppSimHandle, ScopedActionQueue, merge_overrides, run_client_sim,
};
use crate::module::ocpp::client::scripts::ScriptDialog;
use crate::module::ocpp::config::device::{ConfigKeyDef, ConnectorRef, OcppDeviceConfig};
use crate::module::ocpp::config::session::{OcppModuleSpec, OcppRole, OcppSpec};
use crate::module::ocpp::scope::Scope;
use crate::module::ocpp::server::build_server_view;
use crate::module::ocpp::setup_dialog::OcppSetupDialog;
use crate::module::view::{
    CommandDescriptor, CommandFuture, CommandResult, ModuleView, RefreshFuture, SharedLog,
};
use crate::view::log::format_timestamp;

// --- Version trait ---------------------------------------------------------

/// The shared charging-station state surface the generic client view needs: a list of connectors,
/// the CS-level identity/config store, and the heartbeat cadence. Each version's `CsState`
/// implements this; the version-specific operations stay on [`ClientVersion`].
pub trait ClientState: ClientFields + Default + Send + Sync + 'static {
    /// Number of connectors (always ≥ 1).
    fn connector_count(&self) -> usize;
    /// Remove all connectors (re-seeded from device config).
    fn clear_connectors(&mut self);
    /// Remove the connector at list index `idx`.
    fn remove_connector_at(&mut self, idx: usize);
    /// The list index of the connector with `connector_id`.
    fn connector_position(&self, connector_id: i64) -> Option<usize>;
    /// Read a connector field (by list index) for the action-dialog field lookup.
    fn conn_get_field(&self, idx: usize, name: &str) -> Option<ferrowl_lua::module::ValueType>;
    /// Read a CS-level field for the action-dialog field lookup.
    fn cs_get_field_named(&self, name: &str) -> Option<ferrowl_lua::module::ValueType>;
    /// The CS-level state table rows.
    fn cs_state_rows(&self) -> Vec<NvRowData>;
    /// The connector state table rows for the connector at list index `idx`.
    fn conn_state_rows(&self, idx: usize) -> Vec<NvRowData>;
    /// The config/variable store.
    fn config(&self) -> &[ConfigKey];
    /// The config/variable store (mutable).
    fn config_mut(&mut self) -> &mut Vec<ConfigKey>;
    /// The heartbeat cadence (seconds) from the last BootNotification response, if any.
    fn heartbeat_interval_secs(&self) -> Option<u64>;
}

/// One row of a state table (decoupled from the per-version `NvRow` so the generic view owns its
/// own table-row type).
pub struct NvRowData {
    pub name: String,
    pub unit: String,
    pub value: String,
}

/// Everything version-specific the generic charging-station (client) view needs. Parallels the
/// server's `ServerVersion`. Each version supplies its split [`ClientState`], its inbound handler,
/// and the per-version seams: scope construction, the connector lookup/add, the `EditField` row
/// map + the connector-status choices, the state-driven action set, the exact request payloads, and
/// (2.0.1) the `StartTransaction`/`StopTransaction` transaction shortcuts.
pub trait ClientVersion: Version + Sized + 'static {
    /// The charging-station state (CS-level identity, the config/variable store, the connectors),
    /// shared behind a `std::sync::RwLock`.
    type Cs: ClientState;
    /// The inbound (CSMS→CS) handler answering Calls from observed state.
    type Handler: CsActionHandler<Self>;

    /// Build the inbound handler, wiring it to the backend's online flag + message log and the
    /// shared state.
    fn handler(
        online: Arc<std::sync::atomic::AtomicBool>,
        messages: Messages,
        state: Arc<RwLock<Self::Cs>>,
    ) -> Self::Handler;

    /// State-driven actions (their request is fully built from state, no dialog).
    fn state_driven() -> &'static [&'static str];

    /// The title of the config/variable table ("Config" for 1.6, "Variables" for 2.0.1).
    fn config_title() -> &'static str;

    /// The placeholder of the add-connector input ("Add connector id" / "Add evse/connector").
    fn add_connector_placeholder() -> &'static str;

    /// Whether this version exposes the `StartTransaction`/`StopTransaction` transaction-shortcut
    /// buttons (which emit a `TransactionEvent`). 1.6 builds those as ordinary state-driven actions.
    fn has_tx_shortcuts() -> bool {
        false
    }

    /// The per-action send-dialog spec for `name`, or `None` (raw JSON editor).
    fn action_spec(name: &str) -> Option<crate::module::ocpp::action_dialog::ActionSpec>;

    /// Dialog-reachable actions that intentionally use the raw JSON editor (no typed form yet).
    fn json_actions() -> &'static [&'static str];

    /// The scope of the connector at list index `idx` (1.6 `Scope::connector`, 2.0.1 `Scope::evse`).
    fn scope_of(s: &Self::Cs, idx: usize) -> Scope;

    /// The connectors targeted by `scope` resolved to a list index (the connector on its EVSE /
    /// connector id, falling back to the first connector). Used for the action-dialog field lookup.
    fn connector_index(s: &Self::Cs, scope: Scope) -> Option<usize>;

    /// The connector index `scope` *explicitly* targets (no fall-back to the first connector): the
    /// state table shows the CS-level rows for the CS scope or an unresolved connector. `None` =
    /// show `cs_rows`.
    fn connector_index_for_state(s: &Self::Cs, scope: Scope) -> Option<usize>;

    /// Parse `raw` and add a connector, returning the new connector's id (for selection) or `None`.
    fn add_connector(s: &mut Self::Cs, raw: &str) -> Option<i64>;

    /// Seed a connector from a device-config [`ConnectorRef`] (1.6 keys on `connector`, ignoring
    /// `evse`; 2.0.1 uses `evse` defaulting to 1).
    fn seed_connector(s: &mut Self::Cs, c: &ConnectorRef);

    /// Save-time `ConnectorRef` for the connector at `idx` (1.6 `evse: None`, 2.0.1 `evse: Some`).
    fn connector_ref(s: &Self::Cs, idx: usize) -> ConnectorRef;

    /// Map a connector state-table row to its [`EditField`] (`None` = read-only / no field).
    fn conn_edit_field(row: usize) -> Option<EditField>;

    /// Build the [`EditKind`] (overlay widget) for `field`, seeded from the connector resolved by
    /// `scope` (or the CS-level identity), or `None` to suppress the overlay.
    fn edit_kind(s: &Self::Cs, scope: Scope, cs: bool, field: EditField) -> Option<EditKind>;

    /// Apply a resolved edit value back into state.
    fn apply_edit(s: &mut Self::Cs, edit: &EditOverlay, value: ResolvedEdit);

    /// Build the request payload for a state-driven action from state and `scope`.
    fn state_payload(s: &Self::Cs, name: &str, scope: Scope) -> serde_json::Value;

    /// Build a `TransactionEvent(Started)` for `scope`, minting a tx id (2.0.1 only).
    fn start_event(_s: &mut Self::Cs, _scope: Scope) -> serde_json::Value {
        serde_json::json!({})
    }

    /// Build a `TransactionEvent(Ended)` for `scope`, or `None` if idle (2.0.1 only).
    fn stop_event(_s: &mut Self::Cs, _scope: Scope) -> Option<serde_json::Value> {
        None
    }

    /// Apply a successful response's side-effects (1.6 transaction bookkeeping + heartbeat cadence;
    /// 2.0.1 confirms the eagerly-minted tx + sets heartbeat cadence).
    fn apply_post_send(
        s: &mut Self::Cs,
        name: &str,
        scope: Scope,
        started_tx: Option<&str>,
        response: &serde_json::Value,
    );

    /// Roll back an eagerly-minted transaction whose send failed (2.0.1 only; 1.6 is a no-op).
    fn rollback_tx(_s: &mut Self::Cs, _scope: Scope, _started_tx: Option<&str>) {}

    /// Scopes with a live transaction (1.6: open; 2.0.1: confirmed), for the auto-MeterValues tick.
    fn active_meter_scopes(s: &Self::Cs) -> Vec<Scope>;

    /// Reset the meter tick when transactions transition idle→active (1.6 only; 2.0.1 resets eagerly
    /// in `start_event`). Updates the remembered "any active" flag.
    fn track_meter_reset(_s: &Self::Cs, _tx_was_active: &mut bool, _meter_tick: &mut u32) {}
}

/// Parse a connector/evse id, tolerating a leading `e`/`c` label (e.g. `e1`, `c2`).
pub fn parse_id(raw: &str) -> Option<i64> {
    raw.trim()
        .trim_start_matches(['e', 'c', 'E', 'C'])
        .trim()
        .parse()
        .ok()
}

// --- State table -----------------------------------------------------------

#[derive(Clone, Debug, TableEntry)]
#[table_entry(header = NvHeader)]
struct NvRow {
    #[column(name = "Name", min = 18, max = 30)]
    name: String,
    #[column(name = "Unit", min = 6, max = 6)]
    unit: String,
    #[column(name = "Value", min = 6, max = 30)]
    value: String,
}

impl From<NvRowData> for NvRow {
    fn from(d: NvRowData) -> Self {
        NvRow {
            name: d.name,
            unit: d.unit,
            value: d.value,
        }
    }
}

// --- Connector table -------------------------------------------------------

/// A row in the connector table: charge-point id + connector label (empty for the CS row).
#[derive(Clone, Debug, TableEntry)]
#[table_entry(header = ConnHeader)]
struct ConnRow {
    #[column(name = "Charge Point", min = 12, max = 40)]
    cp: String,
    #[column(name = "Connector", min = 9, max = 16)]
    connector: String,
}

// --- Config / variable table -----------------------------------------------

#[derive(Clone, Debug, TableEntry)]
#[table_entry(header = ConfigHeader)]
struct ConfigRow {
    #[column(name = "Key", min = 16, max = 30)]
    key: String,
    #[column(name = "Value", min = 8, max = 30)]
    value: String,
    #[column(name = "ReadOnly", min = 9, max = 9)]
    ro: String,
}

/// Which state row an edit overlay is changing (CS-level identity or connector metering/status).
/// `open_edit` picks the right mapping from the selection.
#[derive(Clone, Copy)]
pub enum EditField {
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
    /// Map a CS-level state-table row (see `CsState::cs_rows`). Reserved RFID (row 4) is read-only.
    pub fn from_cs_row(row: usize) -> Option<EditField> {
        Some(match row {
            0 => EditField::Model,
            1 => EditField::Vendor,
            2 => EditField::FirmwareVersion,
            3 => EditField::SerialNumber,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
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

pub const PHASE_CHOICES: [&str; 7] = ["L1", "L2", "L3", "L1,L2", "L1,L3", "L2,L3", "L1,L2,L3"];

/// A widget for an edit overlay's value (a choice list / number / text input).
pub enum EditKind {
    Choice(Widget<SelectionState<String>, Selection<String>>),
    Number(Widget<InputFieldState, InputField<f64>>),
    Text(Widget<InputFieldState, InputField<String>>),
}

/// A resolved edit value, handed to [`ClientVersion::apply_edit`].
pub enum ResolvedEdit {
    Choice(String),
    Number(f64),
    Text(String),
}

/// A state-row edit overlay: which field, the scope it targets (`Scope::CS` = CS-level), and the
/// input widget.
pub struct EditOverlay {
    pub field: EditField,
    pub scope: Scope,
    kind: EditKind,
}

// --- Message table ---------------------------------------------------------

#[derive(Clone, Debug, TableEntry)]
#[table_entry(header = MsgHeader, styles = msg_cell_styles)]
struct MsgRow {
    #[column(name = "Timestamp", min = 23, max = 23)]
    timestamp: String,
    #[column(name = "Direction", min = 8, max = 10)]
    direction: String,
    #[column(name = "Message", min = 14, max = 30)]
    name: String,
    #[column(name = "Status", min = 7, max = 8)]
    status: String,
    #[column(name = "Context", min = 6, max = 40)]
    context: String,
}

fn msg_cell_styles(row: &MsgRow) -> [Option<Style>; 5] {
    let status_style = match row.status.as_str() {
        "Success" => Some(Style::default().fg(COLOR_SCHEME.success)),
        "Error" => Some(Style::default().fg(COLOR_SCHEME.error)),
        _ => None,
    };
    [None, None, None, status_style, None]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

pub struct ClientView<V: ClientVersion> {
    spec: OcppSpec,
    device_path: String,
    device: OcppDeviceConfig,
    backend: OcppClient<V>,
    state: Arc<RwLock<V::Cs>>,
    log: SharedLog,
    conn_table: ConnTable,
    conn_input: Widget<InputFieldState, InputField<String>>,
    state_table: StateTable,
    config_table: ConfigTable,
    key_input: Widget<InputFieldState, InputField<String>>,
    value_input: Widget<InputFieldState, InputField<String>>,
    actions: Widget<SelectionState<String>, Selection<String>>,
    msg_table: MsgTable,
    /// All messages from the backend (every scope).
    messages: Vec<OcppMessage>,
    /// Messages for the currently-selected scope, indexed by the message table.
    visible_messages: Vec<OcppMessage>,
    code: Widget<CodeInputFieldState, CodeInputField>,
    scripts_button: Widget<ButtonState, Button>,
    script_dialog: Option<ScriptDialog>,
    focus: Pane,
    edit: Option<EditOverlay>,
    config_edit: Option<ConfigEditDialog>,
    action_dialog: Option<ActionDialog>,
    /// A pending send: action name, payload, and the scope it targets.
    pending_send: Option<(String, serde_json::Value, Scope)>,
    setup_overlay: Option<OcppSetupDialog>,
    pending_setup: Option<(OcppSpec, String)>,
    replacement: Option<Box<dyn ModuleView>>,
    /// Actions enqueued by Lua scripts (with their scope), drained and sent each `refresh`.
    action_queue: ScopedActionQueue,
    sim: Option<OcppSimHandle>,
    meter_tick: u32,
    /// Whether any connector had an open transaction on the previous `refresh` (1.6 meter reset).
    tx_was_active: bool,
    heartbeat_tick: u32,
    was_online: bool,
    logged_seq: u64,
    applied_log_file: Option<String>,
    code_content: String,
    compact: bool,
    /// Action-list level cached so `sync_actions` only rebuilds on a CS↔connector change.
    actions_for_connector: Option<bool>,
    _version: PhantomData<V>,
}

impl<V: ClientVersion> ClientView<V> {
    pub fn new(spec: OcppSpec, device_path: String, device: OcppDeviceConfig) -> Self {
        let state = Arc::new(RwLock::new(V::Cs::default()));
        // Seed connectors from the device config (else keep the single default connector).
        if !device.connectors.is_empty() {
            let mut s = state.write().unwrap();
            s.clear_connectors();
            for c in &device.connectors {
                V::seed_connector(&mut s, c);
            }
            if s.connector_count() == 0 {
                V::add_connector(&mut s, "1");
            }
        }
        // Seed persisted config keys from the device config (else keep the built-in defaults).
        if !device.config.is_empty() {
            let mut s = state.write().unwrap();
            *s.config_mut() = device
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
            (
                conn_rows::<V>(&cp, &s),
                nv_rows(s.cs_state_rows()),
                config_rows(&*s),
            )
        };
        let mut view = Self {
            device_path,
            device,
            backend: OcppClient::new(spec.clone()),
            state,
            log: Arc::new(AsyncRwLock::new(LogRing::init())),
            conn_table: conn_table(conn_rows),
            conn_input: panel_input(V::add_connector_placeholder()),
            state_table: nv_table(state_rows),
            config_table: config_table::<V>(config_rows),
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
            tx_was_active: false,
            heartbeat_tick: 0,
            was_online: false,
            logged_seq: 0,
            applied_log_file: None,
            code_content: String::new(),
            compact: false,
            actions_for_connector: None,
            _version: PhantomData,
            spec,
        };
        // The connector table defaults to row 0 (the CS row) selected.
        view.sync_actions();
        view.start_sim();
        view
    }

    /// Whether the connector table's selection is the CS-level row (row 0 / none).
    fn cs_selected(&self) -> bool {
        !matches!(self.conn_table.state.table_state().selected(), Some(i) if i >= 1)
    }

    /// The scope of the selected connector-table row (CS row → `Scope::CS`).
    fn selected_scope(&self) -> Scope {
        match self.conn_table.state.table_state().selected() {
            Some(i) if i >= 1 => {
                let s = self.state.read().unwrap();
                if i - 1 < s.connector_count() {
                    V::scope_of(&s, i - 1)
                } else {
                    Scope::CS
                }
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

    /// Rebuild the action list for the selected level (CS vs connector), preserving the selection
    /// while the level is unchanged.
    fn sync_actions(&mut self) {
        let want = !self.cs_selected();
        if self.actions_for_connector == Some(want) {
            return;
        }
        let names = if want {
            <V::Cs as ClientFields>::conn_actions()
        } else {
            <V::Cs as ClientFields>::cs_actions()
        };
        let values: Vec<String> = names.into_iter().map(|s| s.to_string()).collect();
        self.actions.state.set_values(values);
        self.actions_for_connector = Some(want);
    }

    /// Drain and send one Lua-enqueued action. The transaction shortcuts map to a TransactionEvent
    /// for the action's connector; state-driven and other actions build their payload then merge.
    fn dispatch_lua_action(&mut self, scope: Scope, name: &str, overrides: serde_json::Value) {
        let (send_name, mut payload) = match name {
            "StartTransaction" if V::has_tx_shortcuts() => {
                ("TransactionEvent".to_string(), self.start_event(scope))
            }
            "StopTransaction" if V::has_tx_shortcuts() => match self.stop_event(scope) {
                Some(payload) => ("TransactionEvent".to_string(), payload),
                None => return,
            },
            n if V::state_driven().contains(&n) => (name.to_string(), self.state_payload(n, scope)),
            _ => {
                let template = V::default_action(name)
                    .and_then(|a| V::encode_action(&a).ok())
                    .unwrap_or_else(|| serde_json::json!({}));
                (name.to_string(), template)
            }
        };
        merge_overrides(&mut payload, overrides);
        self.send_payload(&send_name, payload, scope);
    }

    fn make_handler(&self) -> V::Handler {
        V::handler(
            self.backend.online_handle(),
            self.backend.messages_handle(),
            self.state.clone(),
        )
    }

    /// Write the device config (reconciled with the live spec, scripts + connectors preserved).
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
        device.connectors = {
            let s = self.state.read().unwrap();
            (0..s.connector_count())
                .map(|i| V::connector_ref(&s, i))
                .collect()
        };
        // Persist the client's config keys (server config is transient, never written).
        device.config = self
            .state
            .read()
            .unwrap()
            .config()
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

    /// Add a connector from the input field, then clear it and select the new row.
    fn add_connector(&mut self) {
        let raw = self.conn_input.state.input().trim().to_string();
        let id = V::add_connector(&mut self.state.write().unwrap(), &raw);
        self.conn_input.state.set_input(String::new());
        self.conn_input.state.set_cursor(0);
        if let Some(id) = id {
            // Rebuild the (now-sorted) table and select the new connector's row (CS row = 0).
            let cp = self.spec.name.clone();
            let (rows, row) = {
                let s = self.state.read().unwrap();
                let row = s.connector_position(id).map(|p| p + 1).unwrap_or(0);
                (conn_rows::<V>(&cp, &s), row)
            };
            self.conn_table.state.set_values(rows);
            select_index(&mut self.conn_table.state, row);
            self.sync_actions();
        }
    }

    /// Remove the selected connector (never the CS row, never the last connector).
    fn remove_connector(&mut self) {
        let Some(i) = self.conn_table.state.table_state().selected() else {
            return;
        };
        if i == 0 {
            return;
        }
        let mut s = self.state.write().unwrap();
        if s.connector_count() <= 1 || i > s.connector_count() {
            return;
        }
        s.remove_connector_at(i - 1);
        drop(s);
        self.conn_table.state.move_up();
        self.sync_actions();
    }

    /// Enqueue the focused action for sending, or open a dialog when it needs more than state.
    fn trigger_action(&mut self) {
        let name = self.actions.state.get_value();
        if name.is_empty() {
            return;
        }
        let scope = self.selected_scope();
        match name.as_str() {
            "StartTransaction" if V::has_tx_shortcuts() => {
                let payload = self.start_event(scope);
                self.pending_send = Some(("TransactionEvent".to_string(), payload, scope));
            }
            "StopTransaction" if V::has_tx_shortcuts() => {
                if let Some(payload) = self.stop_event(scope) {
                    self.pending_send = Some(("TransactionEvent".to_string(), payload, scope));
                }
            }
            n if V::state_driven().contains(&n) => {
                let payload = self.state_payload(n, scope);
                self.pending_send = Some((name, payload, scope));
            }
            _ => {
                self.action_dialog = Some(match V::action_spec(&name) {
                    Some(spec) => {
                        let state = self.state.clone();
                        let lookup = move |f: &str| {
                            let s = state.read().unwrap();
                            // Resolve from the targeted connector first, then CS-level.
                            V::connector_index(&s, scope)
                                .and_then(|i| s.conn_get_field(i, f))
                                .or_else(|| s.cs_get_field_named(f))
                                .map(value_to_string)
                        };
                        ActionDialog::new(name, &spec, lookup, gen_tx_id)
                    }
                    None => {
                        debug_assert!(
                            V::json_actions().contains(&name.as_str()),
                            "{name} has no spec and is not a registered JSON action"
                        );
                        let template = V::default_action(&name)
                            .and_then(|a| V::encode_action(&a).ok())
                            .map(|v| serde_json::to_string_pretty(&v).unwrap_or_default())
                            .unwrap_or_else(|| "{}".to_string());
                        ActionDialog::json_only(name, &template)
                    }
                });
            }
        }
    }

    fn start_event(&mut self, scope: Scope) -> serde_json::Value {
        let payload = V::start_event(&mut self.state.write().unwrap(), scope);
        // 2.0.1 resets the meter tick eagerly on a started transaction.
        if V::has_tx_shortcuts() {
            self.meter_tick = 0;
        }
        payload
    }

    fn stop_event(&mut self, scope: Scope) -> Option<serde_json::Value> {
        V::stop_event(&mut self.state.write().unwrap(), scope)
    }

    fn state_payload(&self, name: &str, scope: Scope) -> serde_json::Value {
        V::state_payload(&self.state.read().unwrap(), name, scope)
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

    /// Append a config key from the key/value inputs (readonly=false), then clear them.
    fn add_config_key(&mut self) {
        let key = self.key_input.state.input().trim().to_string();
        if key.is_empty() {
            return;
        }
        let value = self.value_input.state.input().trim().to_string();
        {
            let mut s = self.state.write().unwrap();
            match s.config_mut().iter_mut().find(|c| c.key == key) {
                Some(c) => c.value = value,
                None => s.config_mut().push(ConfigKey {
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
        if let Some(current) = s.config().get(row) {
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
        if let Some(slot) = s.config_mut().get_mut(dialog.index()) {
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
            V::conn_edit_field(row)
        };
        let Some(field) = field else { return };
        let scope = if cs { Scope::CS } else { self.selected_scope() };
        let s = self.state.read().unwrap();
        let Some(kind) = V::edit_kind(&s, scope, cs, field) else {
            return;
        };
        drop(s);
        self.edit = Some(EditOverlay { field, scope, kind });
    }

    fn apply_edit(&mut self) {
        let Some(edit) = self.edit.take() else { return };
        let resolved = match &edit.kind {
            EditKind::Choice(sel) => ResolvedEdit::Choice(sel.state.get_value()),
            EditKind::Number(input) => {
                let Ok(value) = input.state.input().trim().parse::<f64>() else {
                    return;
                };
                ResolvedEdit::Number(value)
            }
            EditKind::Text(input) => ResolvedEdit::Text(input.state.input().trim().to_string()),
        };
        let mut s = self.state.write().unwrap();
        V::apply_edit(&mut s, &edit, resolved);
    }

    fn sync_code(&mut self) {
        let selected = self.msg_table.state.table_state().selected();
        let content = selected
            .and_then(|i| self.visible_messages.get(i))
            .map(|m| serde_json::to_string_pretty(&m.payload).unwrap_or_default())
            .unwrap_or_default();
        if content != self.code_content {
            self.code.state.set_content(&content);
            self.code_content = content;
        }
    }

    /// Decode + send a (name, payload) at `scope` without blocking the UI loop. A transaction start
    /// mints its id eagerly (carried in the payload, 2.0.1); confirm or roll it back on the response
    /// so auto-MeterValues only fire once the start is acknowledged.
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
        tokio::spawn(async move {
            match V::decode_call(&name, payload) {
                Ok(action) => match sender.send_scoped(action, scope).await {
                    Ok(response) => {
                        let mut s = state.write().unwrap();
                        V::apply_post_send(&mut s, &name, scope, started_tx.as_deref(), &response);
                    }
                    Err(e) => {
                        {
                            let mut s = state.write().unwrap();
                            V::rollback_tx(&mut s, scope, started_tx.as_deref());
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

impl<V: ClientVersion> ModuleView for ClientView<V> {
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
                    if V::decode_call(&name, payload.clone()).is_ok() {
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
                    && i < s.config().len()
                {
                    s.config_mut().remove(i);
                    self.config_table.state.move_up();
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Char('d')) if matches!(self.focus, Pane::Connectors) => {
                self.remove_connector();
                EventResult::Consumed
            }
            // Space activates list/table panes, but must type into the text inputs.
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

            // Drain Lua-enqueued actions (each with its scope) and send them.
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

            // Auto-Heartbeat (CS-level) at the BootNotification-supplied cadence while connected.
            if online {
                let interval_secs = self
                    .state
                    .read()
                    .unwrap()
                    .heartbeat_interval_secs()
                    .unwrap_or(DEFAULT_HEARTBEAT_SECS)
                    .max(1);
                self.heartbeat_tick = self.heartbeat_tick.wrapping_add(1);
                if self.heartbeat_tick >= interval_secs as u32 * TICKS_PER_SEC {
                    self.heartbeat_tick = 0;
                    self.send_payload("Heartbeat", serde_json::json!({}), Scope::CS);
                }
            }

            // Auto-MeterValues per connector with a live transaction (~every 5s), gated online.
            let active: Vec<Scope> = V::active_meter_scopes(&self.state.read().unwrap());
            V::track_meter_reset(
                &self.state.read().unwrap(),
                &mut self.tx_was_active,
                &mut self.meter_tick,
            );
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

            // Refresh tables. Messages are teed to the persistent log (all scopes) then filtered to
            // the selected entry for display.
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
                let state_rows = match V::connector_index_for_state(&s, scope) {
                    Some(i) => nv_rows(s.conn_state_rows(i)),
                    None => nv_rows(s.cs_state_rows()),
                };
                (conn_rows::<V>(&cp, &s), state_rows, config_rows(&*s))
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
fn msg_log_at_bottom<E: TableEntry<N>, const N: usize>(state: &TableState<E, N>) -> bool {
    let len = state.values().len();
    len == 0
        || state
            .table_state()
            .selected()
            .map(|s| s + 1 >= len)
            .unwrap_or(true)
}

/// Select row `idx` in a table (no direct setter on `TableState`): jump to the top, then step down.
fn select_index<E: TableEntry<N>, const N: usize>(state: &mut TableState<E, N>, idx: usize) {
    state.move_to_top();
    for _ in 0..idx {
        state.move_down();
    }
}

/// Convert version-agnostic state rows into the view's table rows.
fn nv_rows(rows: Vec<NvRowData>) -> Vec<NvRow> {
    rows.into_iter().map(NvRow::from).collect()
}

/// Connector-table rows: a CS row (empty connector) followed by one row per connector.
fn conn_rows<V: ClientVersion>(cp: &str, s: &V::Cs) -> Vec<ConnRow> {
    let mut rows = vec![ConnRow {
        cp: cp.to_string(),
        connector: String::new(),
    }];
    for i in 0..s.connector_count() {
        rows.push(ConnRow {
            cp: cp.to_string(),
            connector: V::scope_of(s, i).label(),
        });
    }
    rows
}

/// Config/variable-table rows from the store.
fn config_rows<S: ClientState>(s: &S) -> Vec<ConfigRow> {
    s.config()
        .iter()
        .map(|c| ConfigRow {
            key: c.key.clone(),
            value: c.value.clone(),
            ro: if c.readonly { "yes" } else { "no" }.to_string(),
        })
        .collect()
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

fn config_table<V: ClientVersion>(rows: Vec<ConfigRow>) -> ConfigTable {
    Widget {
        state: TableStateBuilder::default().values(rows).build().unwrap(),
        widget: TableBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some(V::config_title().into()))
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

/// A choice-list overlay widget, preselecting `current` if present (for the Status/Phases editors).
pub fn choice(
    options: &[&str],
    current: &str,
) -> Widget<SelectionState<String>, Selection<String>> {
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

/// A numeric input overlay widget seeded with `current` (for metering editors).
pub fn number(current: f64) -> Widget<InputFieldState, InputField<f64>> {
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

/// A text input overlay widget seeded with `current` (for the RFID / identity editors).
pub fn text_input(current: &str) -> Widget<InputFieldState, InputField<String>> {
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
    use super::*;
    use crate::module::ocpp::config::session::{OcppProtocol, OcppRole, OcppVersion};
    use ferrowl_ocpp::{V1_6, V2_0_1};

    fn client_view<V: ClientVersion>(version: OcppVersion) -> ClientView<V> {
        let spec = OcppSpec {
            name: "cs".into(),
            version,
            role: OcppRole::Client,
            protocol: OcppProtocol::Ws,
            ip: "127.0.0.1".into(),
            port: 0,
            path: String::new(),
            timeout_ms: None,
        };
        ClientView::<V>::new(spec, String::new(), OcppDeviceConfig::default())
    }

    /// The Tab focus order visits the Payload pane and wraps; BackTab reverses it. One per version.
    fn assert_focus_cycle<V: ClientVersion>(version: OcppVersion) {
        let mut v = client_view::<V>(version);
        // Default selection is the CS row, so the config panes are in the cycle.
        v.focus = Pane::Connectors;
        let mut seen = vec![v.focus];
        for _ in 0..10 {
            v.focus_next();
            seen.push(v.focus);
        }
        assert!(
            seen.contains(&Pane::Payload),
            "Payload pane not in Tab order"
        );
        // 10 steps from Connectors wraps the full CS-level cycle back to Connectors.
        assert_eq!(
            v.focus,
            Pane::Connectors,
            "focus_next did not wrap to start"
        );
        // BackTab from Connectors lands on the add-connector input (reverse order).
        v.focus_previous();
        assert_eq!(v.focus, Pane::ConnectorInput);
    }

    #[test]
    fn focus_cycle_includes_payload_pane_v1_6() {
        assert_focus_cycle::<V1_6>(OcppVersion::V1_6);
    }

    #[test]
    fn focus_cycle_includes_payload_pane_v2_0_1() {
        assert_focus_cycle::<V2_0_1>(OcppVersion::V2_0_1);
    }

    /// The connector state-table row order and `V::conn_edit_field` must stay in lockstep, with the
    /// only unmapped (read-only) connector row being Charge Limit.
    #[test]
    fn edit_field_conn_rows_align_v1_6() {
        use crate::module::ocpp::client::v1_6::state::ConnectorState;
        let rows = ConnectorState::new(1).rows();
        for (i, row) in rows.iter().enumerate() {
            match V1_6::conn_edit_field(i) {
                Some(f) => assert_eq!(f.label(), row.name, "row {i} label mismatch"),
                None => assert!(
                    row.name == "Charge Limit",
                    "row {i} ({}) unmapped",
                    row.name
                ),
            }
        }
        assert!(V1_6::conn_edit_field(rows.len()).is_none());
    }

    #[test]
    fn edit_field_conn_rows_align_v2_0_1() {
        use crate::module::ocpp::client::v2_0_1::state::ConnectorState;
        let rows = ConnectorState::new(1, 1).rows();
        for (i, row) in rows.iter().enumerate() {
            match V2_0_1::conn_edit_field(i) {
                Some(f) => assert_eq!(f.label(), row.name, "row {i} label mismatch"),
                None => assert!(
                    row.name == "Charge Limit",
                    "row {i} ({}) unmapped",
                    row.name
                ),
            }
        }
        assert!(V2_0_1::conn_edit_field(rows.len()).is_none());
    }

    /// The CS state-table row order and `EditField::from_cs_row` must stay in lockstep (shared
    /// across versions; the only unmapped row is the read-only Reserved RFID).
    fn assert_cs_rows_align(rows: &[NvRowData]) {
        for (i, row) in rows.iter().enumerate() {
            match EditField::from_cs_row(i) {
                Some(f) => assert_eq!(f.label(), row.name, "row {i} label mismatch"),
                None => assert!(
                    row.name == "Reserved RFID",
                    "row {i} ({}) unmapped",
                    row.name
                ),
            }
        }
    }

    #[test]
    fn edit_field_cs_rows_align_v1_6() {
        let s = crate::module::ocpp::client::v1_6::state::CsState::default();
        assert_cs_rows_align(&s.cs_state_rows());
    }

    #[test]
    fn edit_field_cs_rows_align_v2_0_1() {
        let s = crate::module::ocpp::client::v2_0_1::state::CsState::default();
        assert_cs_rows_align(&s.cs_state_rows());
    }
}
