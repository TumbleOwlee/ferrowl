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
//!
//! Split by concern: rows/types/the `ClientView` struct live here; [`mod@render`] holds frame
//! rendering + widget builders, [`mod@input`] key handling, [`mod@backend`] the sim/queue/refresh
//! glue.

mod backend;
mod input;
mod render;

use std::marker::PhantomData;
use std::sync::Arc;

use parking_lot::RwLock;

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::{
    COLOR_SCHEME, EventResult,
    state::{ButtonState, CodeInputFieldState, InputFieldState, SelectionState, TableState},
    traits::OverlayKeys,
    widgets::{Button, CodeInputField, InputField, Selection, Table, Widget},
};
use ferrowl_ui_derive::{Focus, Overlay, TableEntry, focusable};
use ratatui::style::Style;
use tokio::sync::RwLock as AsyncRwLock;

use ferrowl_ocpp::Version;
use ferrowl_ocpp::cs::CsActionHandler;

use crate::app::LogRing;
use crate::config::script::ScriptDef;
use crate::dialog::scripts::ScriptDialog;
use crate::module::ocpp::action_dialog::ActionDialog;
use crate::module::ocpp::client::backend::{Messages, OcppClient, OcppMessage};
use crate::module::ocpp::client::config::{ConfigEditDialog, ConfigKey};
use crate::module::ocpp::client::lua_sim::{ClientFields, OcppSimHandle, ScopedActionQueue};
use crate::module::ocpp::config::device::{ConnectorRef, OcppDeviceConfig};
use crate::module::ocpp::config::session::OcppSpec;
use crate::module::ocpp::lock::{HasState, with_state, with_state_mut};
use crate::module::ocpp::scope::Scope;
use crate::module::ocpp::setup_dialog::OcppSetupDialog;
use crate::module::view::{CommandDescriptor, ModuleView, SharedLog};
use crate::view::log::format_timestamp;

pub use render::{choice, number, text_input};

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
    /// shared behind a `parking_lot::RwLock`.
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

    /// A handcrafted example payload prefilling the raw JSON editor for a [`Self::json_actions`]
    /// entry (falls back to the serde-`Default` skeleton when `None`).
    fn json_template(name: &str) -> Option<serde_json::Value>;

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

/// The single modal overlay over the client view (mutually exclusive by construction). The derive
/// supplies `is_active`/`take`/`close` and common-key routing (`Esc` closes, `Tab`/`BackTab` cycle
/// focus on the tagged variants); each variant's `Enter`/inner dispatch stays in `handle_events`.
#[derive(Overlay)]
enum ClientOverlay {
    #[overlay(none)]
    None,
    /// State-row edit (choice/number/text).
    #[overlay(esc_close)]
    Edit(Box<EditOverlay>),
    /// Config-key editor.
    #[overlay(esc_close, focus_cycle)]
    Config(Box<ConfigEditDialog>),
    /// Action send dialog (routes all keys via its own `input()`).
    Action(Box<ActionDialog>),
    /// Module re-setup dialog.
    #[overlay(esc_close, focus_cycle)]
    Setup(Box<OcppSetupDialog>),
    /// Lua scripts editor (routes all keys via its own `handle_events()`).
    Scripts(Box<ScriptDialog>),
}

impl OverlayKeys for ConfigEditDialog {
    fn focus_cycle(&mut self, forward: bool) {
        if forward {
            self.focus_next();
        } else {
            self.focus_previous();
        }
    }
}

impl OverlayKeys for OcppSetupDialog {
    fn focus_cycle(&mut self, forward: bool) {
        self.focus_step(forward);
    }
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

type StateTable = Widget<TableState<NvRow, 3>, Table<NvRow, NvHeader, 3>>;
type ConnTable = Widget<TableState<ConnRow, 2>, Table<ConnRow, ConnHeader, 2>>;
type ConfigTable = Widget<TableState<ConfigRow, 3>, Table<ConfigRow, ConfigHeader, 3>>;
type MsgTable = Widget<TableState<MsgRow, 5>, Table<MsgRow, MsgHeader, 5>>;

/// Results produced mid-tick (in `handle_events`/`refresh`) and consumed by a later `refresh`:
/// a queued send, a queued module re-setup, or a built replacement view (version/role switch).
#[derive(Default)]
struct Deferred {
    /// A pending send: action name, payload, and the scope it targets.
    send: Option<(String, serde_json::Value, Scope)>,
    setup: Option<(OcppSpec, String)>,
    replacement: Option<Box<dyn ModuleView>>,
}

/// Simulation + liveness bookkeeping: the Lua sim handle, the script action queue, and the tick
/// counters / online & log-file tracking advanced each `refresh`.
#[derive(Default)]
struct SimRuntime {
    handle: Option<OcppSimHandle>,
    /// Actions enqueued by Lua scripts (with their scope), drained and sent each `refresh`.
    action_queue: ScopedActionQueue,
    meter_tick: u32,
    /// Whether any connector had an open transaction on the previous `refresh` (1.6 meter reset).
    tx_was_active: bool,
    heartbeat_tick: u32,
    was_online: bool,
    logged_seq: u64,
    applied_log_file: Option<String>,
}

#[focusable]
#[derive(Focus)]
pub struct ClientView<V: ClientVersion> {
    spec: OcppSpec,
    device_path: String,
    device: OcppDeviceConfig,
    backend: OcppClient<V>,
    state: Arc<RwLock<V::Cs>>,
    log: SharedLog,
    // Focus panes, declared in Tab-cycle order (see `#[derive(Focus)]`). The config trio is only
    // reachable for the CS-level entry, gated with `#[focus(when = self.cs_selected())]`.
    #[focus]
    conn_input: Widget<InputFieldState, InputField<String>>,
    #[focus]
    conn_table: ConnTable,
    #[focus]
    state_table: StateTable,
    #[focus]
    scripts_button: Widget<ButtonState, Button>,
    #[focus]
    actions: Widget<SelectionState<String>, Selection<String>>,
    #[focus(when = self.cs_selected())]
    config_table: ConfigTable,
    #[focus(when = self.cs_selected())]
    key_input: Widget<InputFieldState, InputField<String>>,
    #[focus(when = self.cs_selected())]
    value_input: Widget<InputFieldState, InputField<String>>,
    #[focus]
    msg_table: MsgTable,
    #[focus]
    code: Widget<CodeInputFieldState, CodeInputField>,
    /// All messages from the backend (every scope).
    messages: Vec<OcppMessage>,
    /// Messages for the currently-selected scope, indexed by the message table.
    visible_messages: Vec<OcppMessage>,
    /// The single active modal overlay (edit / config / action / setup / scripts).
    overlay: ClientOverlay,
    /// Results produced mid-tick, consumed by a later `refresh` (send / re-setup / replacement).
    deferred: Deferred,
    /// Simulation + liveness bookkeeping (sim handle, action queue, tick counters, online/log).
    runtime: SimRuntime,
    code_content: String,
    compact: bool,
    /// Action-list level cached so `sync_actions` only rebuilds on a CS↔connector change.
    actions_for_connector: Option<bool>,
    _version: PhantomData<V>,
}

impl<V: ClientVersion> HasState for ClientView<V> {
    type State = V::Cs;

    fn state(&self) -> &Arc<RwLock<Self::State>> {
        &self.state
    }
}

impl<V: ClientVersion> ClientView<V> {
    pub fn new(spec: OcppSpec, device_path: String, device: OcppDeviceConfig) -> Self {
        let state = Arc::new(RwLock::new(V::Cs::default()));
        // Seed connectors from the device config (else keep the single default connector).
        if !device.connectors.is_empty() {
            with_state_mut(&state, |s| {
                s.clear_connectors();
                for c in &device.connectors {
                    V::seed_connector(s, c);
                }
                if s.connector_count() == 0 {
                    V::add_connector(s, "1");
                }
            });
        }
        // Seed persisted config keys from the device config (else keep the built-in defaults).
        if !device.config.is_empty() {
            with_state_mut(&state, |s| {
                *s.config_mut() = device
                    .config
                    .iter()
                    .map(|c| ConfigKey {
                        key: c.key.clone(),
                        value: c.value.clone(),
                        readonly: c.readonly,
                    })
                    .collect();
            });
        }
        let cp = spec.name.clone();
        let (conn_rows, state_rows, config_rows) = with_state(&state, |s| {
            (
                conn_rows::<V>(&cp, s),
                nv_rows(s.cs_state_rows()),
                config_rows(s),
            )
        });
        let mut view = Self {
            device_path,
            device,
            backend: OcppClient::new(spec.clone()),
            state,
            log: Arc::new(AsyncRwLock::new(LogRing::init())),
            conn_table: render::conn_table(conn_rows),
            conn_input: render::panel_input(V::add_connector_placeholder()),
            state_table: render::nv_table(state_rows),
            config_table: render::config_table::<V>(config_rows),
            key_input: render::panel_input("Key"),
            value_input: render::panel_input("Value"),
            actions: render::action_list(Vec::new()),
            msg_table: render::msg_table(),
            messages: Vec::new(),
            visible_messages: Vec::new(),
            code: render::code_view(),
            scripts_button: render::scripts_button(),
            overlay: ClientOverlay::None,
            focus: ClientViewFocus::ConnTable,
            view_focused: false,
            deferred: Deferred::default(),
            runtime: SimRuntime::default(),
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
            Some(i) if i >= 1 => self.with_state(|s| {
                if i - 1 < s.connector_count() {
                    V::scope_of(s, i - 1)
                } else {
                    Scope::CS
                }
            }),
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
}

impl<V: ClientVersion> ModuleView for ClientView<V> {
    fn name(&self) -> String {
        self.spec.name.clone()
    }

    fn is_overlay_active(&self) -> bool {
        self.overlay.is_active()
    }

    fn render(&mut self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        self.render_impl(frame, area);
    }

    fn render_overlay(&mut self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        self.render_overlay_impl(frame, area);
    }

    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        self.handle_events_impl(modifiers, code)
    }

    fn refresh<'a>(&'a mut self) -> crate::module::view::RefreshFuture<'a> {
        self.refresh_impl()
    }

    fn handle_command<'a>(&'a mut self, cmd: &'a str) -> crate::module::view::CommandFuture<'a> {
        self.handle_command_impl(cmd)
    }

    fn commands(&self) -> &[CommandDescriptor] {
        &OCPP_CLIENT_COMMANDS
    }

    fn keybinds(&self) -> &[CommandDescriptor] {
        &OCPP_CLIENT_KEYBINDS
    }

    fn log(&self) -> SharedLog {
        self.log.clone()
    }

    fn session_spec(&self) -> Option<serde_json::Value> {
        let module = crate::module::ocpp::config::session::OcppModuleSpec::from_spec(
            &self.spec,
            &self.device_path,
        );
        let mut v = serde_json::to_value(&module).ok()?;
        v.as_object_mut()?.insert("type".into(), "ocpp".into());
        Some(v)
    }

    fn take_replacement(&mut self) -> Option<Box<dyn ModuleView>> {
        self.deferred.replacement.take()
    }

    fn scripts(&self) -> Option<&[ScriptDef]> {
        Some(&self.device.scripts)
    }

    fn set_scripts(&mut self, scripts: Vec<ScriptDef>) -> bool {
        self.device.scripts = scripts;
        self.start_sim();
        true
    }
}

static OCPP_CLIENT_KEYBINDS: [CommandDescriptor; 4] = [
    CommandDescriptor {
        name: "Tab / Shift+Tab",
        description: "next / previous pane",
    },
    CommandDescriptor {
        name: "Enter",
        description: "activate focused pane (edit/add/trigger)",
    },
    CommandDescriptor {
        name: "Space",
        description: "activate focused table/button",
    },
    CommandDescriptor {
        name: "d",
        description: "delete selected connector / config key",
    },
];

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
        v.focus = ClientViewFocus::ConnTable;
        let mut seen = vec![v.focus];
        for _ in 0..10 {
            v.focus_next();
            seen.push(v.focus);
        }
        assert!(
            seen.contains(&ClientViewFocus::Code),
            "Payload pane not in Tab order"
        );
        // 10 steps from Connectors wraps the full CS-level cycle back to Connectors.
        assert_eq!(
            v.focus,
            ClientViewFocus::ConnTable,
            "focus_next did not wrap to start"
        );
        // BackTab from Connectors lands on the add-connector input (reverse order).
        v.focus_previous();
        assert_eq!(v.focus, ClientViewFocus::ConnInput);
    }

    #[test]
    fn focus_cycle_includes_payload_pane_v1_6() {
        assert_focus_cycle::<V1_6>(OcppVersion::V1_6);
    }

    #[test]
    fn focus_cycle_includes_payload_pane_v2_0_1() {
        assert_focus_cycle::<V2_0_1>(OcppVersion::V2_0_1);
    }

    /// Connector rows that are display-only (no editable field): the CSMS-driven charge limits and
    /// reservation readouts.
    const READONLY_CONN_ROWS: &[&str] = &[
        "Charge Limit",
        "Default Charge Limit",
        "Max Charge Limit",
        "External Charge Limit",
        "Reserved RFID",
        "Reservation ID",
    ];

    /// The connector state-table row order and `V::conn_edit_field` must stay in lockstep, with the
    /// only unmapped rows being the read-only ones in [`READONLY_CONN_ROWS`].
    #[test]
    fn edit_field_conn_rows_align_v1_6() {
        use crate::module::ocpp::client::v1_6::state::ConnectorState;
        let rows = ConnectorState::new(1).rows();
        for (i, row) in rows.iter().enumerate() {
            match V1_6::conn_edit_field(i) {
                Some(f) => assert_eq!(f.label(), row.name, "row {i} label mismatch"),
                None => assert!(
                    READONLY_CONN_ROWS.contains(&row.name.as_str()),
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
                    READONLY_CONN_ROWS.contains(&row.name.as_str()),
                    "row {i} ({}) unmapped",
                    row.name
                ),
            }
        }
        assert!(V2_0_1::conn_edit_field(rows.len()).is_none());
    }

    /// The CS state-table row order and `EditField::from_cs_row` must stay in lockstep (shared
    /// across versions; the only unmapped rows are the read-only reservation readouts).
    fn assert_cs_rows_align(rows: &[NvRowData]) {
        const READONLY_CS_ROWS: &[&str] = &["Reserved RFID", "Reservation ID"];
        for (i, row) in rows.iter().enumerate() {
            match EditField::from_cs_row(i) {
                Some(f) => assert_eq!(f.label(), row.name, "row {i} label mismatch"),
                None => assert!(
                    READONLY_CS_ROWS.contains(&row.name.as_str()),
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
