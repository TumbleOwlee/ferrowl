//! Generic OCPP **server** (CSMS) view, instantiated once per OCPP version (see the
//! [`ServerVersion`] impls in `v1_6`/`v2_0_1`). Left: a table of connected charging stations and
//! their connectors, a "Lua Scripts" button, and the CSMS action list (filtered by the selected
//! entry's level); right: the selected entry's message log over a JSON payload viewer; an
//! ONLINE/OFFLINE status line for the listening socket.
//!
//! Each WebSocket connection yields a CS-level entry (no connector id) plus a connector entry for
//! every `connectorId` seen in inbound traffic. The selected entry scopes the message log and the
//! action list. A single Lua sim backs the whole module (see `server/lua.rs`): its `C_OCPP` global
//! addresses every station/connector via `ChargingStation(cs)` / `Connector(cs, id)` over a shared
//! state registry the view keeps in step with its entries.
//!
//! Split by concern: rows/types/the `ServerView` struct live here; [`mod@render`] holds frame
//! rendering + widget builders, [`mod@input`] key handling, [`mod@backend`] the sim/queue/refresh
//! glue.

mod backend;
mod input;
mod render;

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::state::{ButtonState, CodeInputFieldState, SelectionState, TableState};
use ferrowl_ui::traits::OverlayKeys;
use ferrowl_ui::widgets::{Button, CodeInputField, Selection, Table, Widget};
use ferrowl_ui::{COLOR_SCHEME, EventResult};
use ferrowl_ui_derive::{Focus, Overlay, TableEntry, focusable};
use ratatui::style::Style;

use ferrowl_ocpp::Version;
use ferrowl_ocpp::csms::{ConnectionId, CsmsActionHandler};

use crate::app::LogRing;
use crate::config::script::ScriptDef;
use crate::dialog::scripts::ScriptDialog;
use crate::module::modbus::dialog::ConfirmDeleteDialog;
use crate::module::ocpp::action_dialog::ActionDialog;
use crate::module::ocpp::client::backend::OcppMessage;
use crate::module::ocpp::client::lua_sim::OcppFields;
use crate::module::ocpp::config::device::{ConnectorRfids, OcppDeviceConfig};
use crate::module::ocpp::config::session::{OcppModuleSpec, OcppSpec};
use crate::module::ocpp::lock::{with_state, with_state_mut};
use crate::module::ocpp::server::backend::{
    EventRx, EventTx, OcppServer, RfidLists, RfidStore, Scope,
};
use crate::module::ocpp::server::detail::DetailOverlay;
use crate::module::ocpp::server::lua::{ServerActionQueue, ServerStates, SharedServerStates};
use crate::module::ocpp::setup_dialog::OcppSetupDialog;
use crate::module::view::{CommandDescriptor, ModuleView, SharedLog};
use crate::view::log::format_timestamp;

/// Build the runtime RFID store from a persisted device config (CS list + per-connector lists).
fn rfid_store_from_device(device: &OcppDeviceConfig) -> RfidStore {
    let mut store = RfidStore {
        cs: device.rfids.clone(),
        ..Default::default()
    };
    for cr in &device.connector_rfids {
        let scope = Scope {
            evse: cr.evse,
            connector: cr.connector,
        };
        store.by_scope.insert(scope, cr.rfids.clone());
    }
    store
}

/// Write the runtime RFID store back into a device config for persistence, dropping empty
/// per-connector lists.
pub(super) fn fill_device_rfids(device: &mut OcppDeviceConfig, store: &RfidStore) {
    device.rfids = store.cs.clone();
    let mut conns: Vec<ConnectorRfids> = store
        .by_scope
        .iter()
        .filter(|(_, list)| !list.is_empty())
        .map(|(scope, list)| ConnectorRfids {
            evse: scope.evse,
            connector: scope.connector,
            rfids: list.clone(),
        })
        .collect();
    // Stable order so saves are deterministic.
    conns.sort_by_key(|c| (c.evse, c.connector));
    device.connector_rfids = conns;
}

/// Per-entry observed state behaviour the generic view needs from each version's state types.
pub trait EntryStateT: OcppFields + Default + Send + Sync + 'static {
    /// Update the observed state from an inbound CS→CSMS request and the CSMS response (e.g.
    /// StartTransaction's transactionId is minted in the response).
    fn apply_inbound(
        &mut self,
        name: &str,
        request: &serde_json::Value,
        response: &serde_json::Value,
    );
    /// Update the observed state from a CSMS→CS request we *sent* and the CS response (e.g. mirror an
    /// accepted SetChargingProfile's limit). Default no-op; connector states override.
    fn apply_outbound(
        &mut self,
        _name: &str,
        _request: &serde_json::Value,
        _response: &serde_json::Value,
    ) {
    }
    /// Derive a complete outbound payload for `name` from observed state (e.g. `idTag` from the
    /// last RFID, the connector/EVSE id from `scope`), or `None` to fall back to the JSON editor.
    fn derive_payload(&self, name: &str, scope: Scope) -> Option<serde_json::Value>;
    /// Ordered (field, unit, value) rows describing the observed non-metering state, for the detail
    /// overlay's "State" table. `unit` is empty for non-dimensional fields.
    fn fields(&self) -> Vec<(String, String, String)>;
    /// Ordered (field, unit, value) metering rows for the detail overlay's "Metering" table (default
    /// empty; connector states override).
    fn metering(&self) -> Vec<(String, String, String)> {
        Vec::new()
    }
}

/// Everything version-specific the generic server view needs.
pub trait ServerVersion: Version + Sized + 'static {
    /// CS-level observed state (non-connector info: model/vendor/firmware).
    type Cs: EntryStateT;
    /// Per-connector observed state (metering/status/transaction).
    type Conn: EntryStateT;
    /// The inbound handler answering CS→CSMS Calls and emitting [`ServerEvent`]s.
    type Handler: CsmsActionHandler<Self>;

    /// Build the inbound handler, wiring it to the view's event channel and RFID accept-list.
    fn handler(tx: EventTx, rfids: RfidLists) -> Self::Handler;

    /// The scope an inbound request targets (CS-level/connector/EVSE), used to bucket it.
    fn inbound_connector(name: &str, request: &serde_json::Value) -> Scope;

    /// The transactionId a stop message clears (1.6 `StopTransaction`, 2.0.1 `TransactionEvent`
    /// with `eventType == "Ended"`), as a string, or `None` for non-stop messages. Used to route a
    /// stop that carries no connector/EVSE id to the connector holding that transaction.
    fn stop_tx_id(_name: &str, _request: &serde_json::Value) -> Option<String> {
        None
    }

    /// The CSMS action that retrieves configuration (`GetConfiguration` for 1.6, `GetVariables` for
    /// 2.0.1).
    fn config_action() -> &'static str;

    /// Build a config-fetch request payload for a free-form key (empty = "all" where supported).
    fn config_request(key: &str) -> serde_json::Value;

    /// Parse a config-fetch response into ordered (key, value, readonly) rows.
    fn parse_config(response: &serde_json::Value) -> Vec<(String, String, bool)>;

    /// The CSMS action that writes one configuration value (`ChangeConfiguration` for 1.6,
    /// `SetVariables` for 2.0.1).
    fn set_action() -> &'static str;

    /// Build a config-write request payload setting `key` to `value`.
    fn set_request(key: &str, value: &str) -> serde_json::Value;

    /// The per-action send-dialog spec for `name`, or `None` (raw JSON editor).
    fn action_spec(name: &str) -> Option<crate::module::ocpp::action_dialog::ActionSpec>;

    /// Dialog-reachable actions that intentionally use the raw JSON editor (no typed form yet).
    fn json_actions() -> &'static [&'static str];

    /// A handcrafted example payload prefilling the raw JSON editor for a [`Self::json_actions`]
    /// entry (falls back to the serde-`Default` skeleton when `None`).
    fn json_template(name: &str) -> Option<serde_json::Value>;

    /// Inject the target scope's connector/EVSE id into a (Lua-built) payload that does not already
    /// carry it, so e.g. `con:SetChargingProfile()` defaults to the selected connector. No-op for
    /// the CS-level scope or a non-object payload.
    fn inject_scope(_payload: &mut serde_json::Value, _scope: Scope) {}

    /// The id Lua addresses a connector entry by in `C_OCPP:Connector(cs, id)` / `GetConnectors`.
    /// 1.6 uses the connector id; 2.0.1 uses the EVSE id (connectors are always `None` there).
    /// `None` for the CS-level scope, which Lua does not address as a connector.
    fn lua_connector_id(scope: Scope) -> Option<i64> {
        scope.connector
    }

    /// Whether config keys carry a component dimension (2.0.1 `Component/Variable`), so the detail
    /// overlay's config table shows a separate "Component" column. 1.6 keys are flat.
    fn config_has_component() -> bool {
        false
    }
}

// --- Connection table ------------------------------------------------------

#[derive(Clone, Debug, TableEntry)]
#[table_entry(header = CsHeader, styles = cs_cell_styles)]
struct CsRow {
    #[column(name = "Charging Station", min = 18, max = 40)]
    name: String,
    #[column(name = "Connector", min = 9, max = 9)]
    connector: String,
    #[column(name = "State", min = 12, max = 12)]
    state: String,
}

fn cs_cell_styles(row: &CsRow) -> [Option<Style>; 3] {
    let style = match row.state.as_str() {
        "Connected" => Some(Style::default().fg(COLOR_SCHEME.success)),
        "Disconnected" => Some(Style::default().fg(COLOR_SCHEME.error)),
        _ => None,
    };
    [None, None, style]
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

// --- Entries ---------------------------------------------------------------

/// Shared observed state of one entry — a CS-level entry or a connector entry.
enum EntryState<V: ServerVersion> {
    Cs(Arc<RwLock<V::Cs>>),
    Conn(Arc<RwLock<V::Conn>>),
}

/// One row in the connection table: a charge point (CS-level) or one of its connectors.
struct Entry<V: ServerVersion> {
    /// Charge-point identity (URL-path segment, or a peer fallback).
    identity: String,
    /// The entry scope: CS-level, a 1.6 connector, or a 2.0.1 EVSE/connector.
    scope: Scope,
    /// The live connection while online.
    conn: Option<ConnectionId>,
    online: bool,
    state: EntryState<V>,
    messages: Vec<OcppMessage>,
}

impl<V: ServerVersion> Entry<V> {
    fn rows_for_state(&self) -> Vec<OcppMessage> {
        self.messages.clone()
    }

    fn apply_inbound(
        &mut self,
        name: &str,
        request: &serde_json::Value,
        response: &serde_json::Value,
    ) {
        match &self.state {
            EntryState::Cs(s) => with_state_mut(s, |s| s.apply_inbound(name, request, response)),
            EntryState::Conn(s) => with_state_mut(s, |s| s.apply_inbound(name, request, response)),
        }
    }

    fn apply_outbound(
        &mut self,
        name: &str,
        request: &serde_json::Value,
        response: &serde_json::Value,
    ) {
        match &self.state {
            EntryState::Cs(s) => with_state_mut(s, |s| s.apply_outbound(name, request, response)),
            EntryState::Conn(s) => with_state_mut(s, |s| s.apply_outbound(name, request, response)),
        }
    }

    fn derive_payload(&self, name: &str) -> Option<serde_json::Value> {
        match &self.state {
            EntryState::Cs(s) => with_state(s, |s| s.derive_payload(name, self.scope)),
            EntryState::Conn(s) => with_state(s, |s| s.derive_payload(name, self.scope)),
        }
    }

    /// Read an observed-state field as a display string, for action-dialog prefill.
    fn get_field_str(&self, name: &str) -> Option<String> {
        use crate::module::ocpp::action_dialog::value_to_string;
        let v = match &self.state {
            EntryState::Cs(s) => with_state(s, |s| s.get_field(name)),
            EntryState::Conn(s) => with_state(s, |s| s.get_field(name)),
        };
        v.map(value_to_string)
    }
}

// --- View ------------------------------------------------------------------

type CsTable = Widget<TableState<CsRow, 3>, Table<CsRow, CsHeader, 3>>;
type MsgTable = Widget<TableState<MsgRow, 5>, Table<MsgRow, MsgHeader, 5>>;

/// Results produced mid-tick and consumed by a later `refresh`: a queued module re-setup or a
/// built replacement view (version/role switch).
#[derive(Default)]
struct Deferred {
    setup: Option<(OcppSpec, String)>,
    replacement: Option<Box<dyn ModuleView>>,
}

/// Simulation + liveness bookkeeping: the single Lua sim handle, its action queue, and the
/// log tee/`:log` tracking advanced each `refresh`.
#[derive(Default)]
struct SimRuntime {
    handle: Option<crate::module::ocpp::client::lua_sim::OcppSimHandle>,
    /// Actions enqueued by the Lua sim (identity + scope), drained and routed each refresh.
    lua_queue: ServerActionQueue,
    /// Highest message `seq` already teed into the persistent log, so each is logged once.
    logged_seq: u64,
    /// The `log_file` currently applied to the `SharedLog`, to detect `:log`/edit changes.
    applied_log_file: Option<String>,
}

/// The single modal overlay over the server view (mutually exclusive by construction: entering `:`
/// command mode — the only other way to open one, `setup` — is itself gated on
/// `is_overlay_active()`, see `app/keys.rs`). The derive supplies `is_active`/`take`/`close` and
/// common-key routing (`Esc` closes, `Tab`/`BackTab` cycle focus on the tagged variants); each
/// variant's `Enter`/inner dispatch stays in `handle_events`.
#[derive(Overlay)]
enum ServerOverlay {
    #[overlay(none)]
    None,
    /// Per-entry detail overlay (routes every key through its own `input()`).
    Detail(Box<DetailOverlay>),
    /// Delete-confirmation dialog for the focused CS-table entry.
    #[overlay(esc_close, focus_cycle)]
    Confirm(Box<ConfirmDeleteDialog>),
    /// Module re-setup dialog.
    #[overlay(esc_close, focus_cycle)]
    Setup(Box<OcppSetupDialog>),
    /// Lua scripts editor (routes every key through its own `handle_events()`).
    Scripts(Box<ScriptDialog>),
    /// Action send dialog: target connection/scope + the dialog (routes every key via `input()`).
    Action(Box<(ConnectionId, Scope, ActionDialog)>),
}

impl OverlayKeys for ConfirmDeleteDialog {
    fn focus_cycle(&mut self, forward: bool) {
        if forward {
            self.focus_next();
        } else {
            self.focus_previous();
        }
    }
}

#[focusable]
#[derive(Focus)]
pub struct ServerView<V: ServerVersion> {
    spec: OcppSpec,
    device_path: String,
    device: OcppDeviceConfig,
    log: SharedLog,
    backend: OcppServer<V>,
    events_tx: EventTx,
    events_rx: EventRx,
    entries: Vec<Entry<V>>,
    /// conn → resolved charge-point identity, cached as events arrive.
    conn_identity: HashMap<ConnectionId, String>,
    #[focus]
    cs_table: CsTable,
    #[focus]
    scripts_button: Widget<ButtonState, Button>,
    #[focus]
    actions: Widget<SelectionState<String>, Selection<String>>,
    /// Whether the action list is currently built for a connector entry (`Some(true)`), a CS-level
    /// entry (`Some(false)`), or not yet built (`None`) — to avoid rebuilding every tick.
    actions_for_connector: Option<bool>,
    #[focus]
    msg_table: MsgTable,
    #[focus]
    code: Widget<CodeInputFieldState, CodeInputField>,
    /// The single active modal overlay (detail / delete-confirm / setup / scripts / action).
    overlay: ServerOverlay,
    /// Results produced mid-tick, consumed by a later `refresh` (re-setup / replacement).
    deferred: Deferred,
    /// Whether the listener should be running (auto-bind on open; toggled by `:start`/`:stop`).
    want_running: bool,
    /// Last content pushed into the payload viewer, so periodic refreshes don't reset its scroll.
    code_content: String,
    /// Shared RFID accept-lists (CS + per-connector) handed to each (re)built inbound handler;
    /// edited via the detail dialogs and `:rfid`.
    rfids: RfidLists,
    /// Compact table rows (no vertical margin); toggled by `:compact`.
    compact: bool,
    /// In-memory per-CS configuration rows (identity → key/value), kept across overlay open/close
    /// only while the CS is in the list; dropped when its entry is removed (delete/`:stop`/`:restart`).
    cs_configs: HashMap<String, Vec<(String, String, bool)>>,
    /// Shared registry of every entry's observed state, read by the single Lua sim.
    lua_states: SharedServerStates<V>,
    /// Simulation + liveness bookkeeping (sim handle, action queue, log tee/`:log` tracking).
    runtime: SimRuntime,
}

impl<V: ServerVersion> ServerView<V>
where
    V::Action: Clone,
{
    pub fn new(spec: OcppSpec, device_path: String, device: OcppDeviceConfig) -> Self {
        let (events_tx, events_rx) = tokio::sync::mpsc::unbounded_channel();
        let rfids: RfidLists = Arc::new(parking_lot::RwLock::new(rfid_store_from_device(&device)));
        let mut view = Self {
            backend: OcppServer::new(spec.clone()),
            spec,
            device_path,
            device,
            log: Arc::new(tokio::sync::RwLock::new(LogRing::init())),
            events_tx,
            events_rx,
            entries: Vec::new(),
            conn_identity: HashMap::new(),
            cs_table: render::cs_table(),
            scripts_button: render::scripts_button(),
            actions: render::action_list(Vec::new()),
            actions_for_connector: None,
            msg_table: render::msg_table(),
            code: render::code_view(),
            overlay: ServerOverlay::None,
            deferred: Deferred::default(),
            focus: ServerViewFocus::CsTable,
            view_focused: false,
            want_running: true,
            code_content: String::new(),
            rfids,
            compact: false,
            cs_configs: HashMap::new(),
            lua_states: Arc::new(RwLock::new(ServerStates::default())),
            runtime: SimRuntime::default(),
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
}

impl<V: ServerVersion> ModuleView for ServerView<V>
where
    V::Action: Clone,
{
    fn name(&self) -> String {
        self.spec.name.clone()
    }

    fn is_overlay_active(&self) -> bool {
        self.overlay.is_active() || self.deferred.setup.is_some()
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
        &OCPP_SERVER_COMMANDS
    }

    fn keybinds(&self) -> &[CommandDescriptor] {
        &OCPP_SERVER_KEYBINDS
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

static OCPP_SERVER_KEYBINDS: [CommandDescriptor; 3] = [
    CommandDescriptor {
        name: "Tab / Shift+Tab",
        description: "next / previous pane",
    },
    CommandDescriptor {
        name: "Enter",
        description: "open detail / scripts / trigger action",
    },
    CommandDescriptor {
        name: "d",
        description: "delete selected charging station",
    },
];

static OCPP_SERVER_COMMANDS: [CommandDescriptor; 9] = [
    CommandDescriptor {
        name: ":start | :stop",
        description: "bind / unbind the CSMS listener",
    },
    CommandDescriptor {
        name: ":restart",
        description: "rebind the listener (clears entries)",
    },
    CommandDescriptor {
        name: ":e | :edit",
        description: "edit module setup",
    },
    CommandDescriptor {
        name: ":wd | :write-device [path]",
        description: "save device config",
    },
    CommandDescriptor {
        name: ":compact",
        description: "toggle compact rows",
    },
    CommandDescriptor {
        name: ":log [file]",
        description: "set/clear log file",
    },
    CommandDescriptor {
        name: ":rfid [add|del <tag> | clear]",
        description: "CSMS RFID accept-list",
    },
    CommandDescriptor {
        name: "d",
        description: "delete the selected charging station / connector",
    },
    CommandDescriptor {
        name: "Enter",
        description: "open the selected entry's detail overlay",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::ocpp::config::session::{OcppProtocol, OcppRole, OcppVersion};
    use crate::module::ocpp::server::backend::ServerEvent;
    use ferrowl_ocpp::V1_6;

    #[test]
    fn ut_rfid_store_device_roundtrip() {
        // Device config -> runtime store -> device config preserves CS + per-connector lists.
        let device = OcppDeviceConfig {
            rfids: vec!["CS1".into()],
            connector_rfids: vec![ConnectorRfids {
                evse: Some(2),
                connector: None,
                rfids: vec!["EVSE2".into()],
            }],
            ..Default::default()
        };
        let store = rfid_store_from_device(&device);
        assert_eq!(store.cs, ["CS1"]);
        assert_eq!(store.scope_list(Scope::evse(2, None)), ["EVSE2"]);

        let mut back = OcppDeviceConfig::default();
        fill_device_rfids(&mut back, &store);
        assert_eq!(back.rfids, device.rfids);
        assert_eq!(back.connector_rfids, device.connector_rfids);
    }

    #[test]
    fn ut_empty_connector_lists_not_persisted() {
        // A connector whose list was emptied is dropped from the persisted config.
        let mut store = RfidStore::default();
        store.add(Scope::connector(1), "X".into());
        store.remove(Scope::connector(1), "X");
        let mut device = OcppDeviceConfig::default();
        fill_device_rfids(&mut device, &store);
        assert!(device.connector_rfids.is_empty());
    }

    fn server_view() -> ServerView<V1_6> {
        let spec = OcppSpec {
            name: "csms".into(),
            version: OcppVersion::V1_6,
            role: OcppRole::Server,
            protocol: OcppProtocol::Ws,
            ip: "127.0.0.1".into(),
            port: 0,
            path: String::new(),
            timeout_ms: None,
            security: Default::default(),
        };
        ServerView::<V1_6>::new(spec, String::new(), OcppDeviceConfig::default())
    }

    #[test]
    fn focus_cycle_includes_payload_pane() {
        let mut v = server_view();
        // CsTable -> Scripts -> Actions -> Messages -> Payload -> CsTable.
        let mut seen = Vec::new();
        for _ in 0..5 {
            v.focus_next();
            seen.push(v.focus);
        }
        assert!(
            seen.contains(&ServerViewFocus::Code),
            "Payload pane not in Tab order"
        );
        assert!(
            v.focus == ServerViewFocus::CsTable,
            "focus_next did not wrap to start"
        );
        // BackTab from CsTable lands on Payload (reverse order).
        v.focus_previous();
        assert!(v.focus == ServerViewFocus::Code);
    }

    #[test]
    fn entries_keyed_by_scope() {
        let mut v = server_view();
        // Two 2.0.1 connectors sharing EVSE 1 are distinct entries; re-querying is stable.
        let c1 = v.entry_index("CP1", Scope::evse(1, Some(1)), None);
        let c2 = v.entry_index("CP1", Scope::evse(1, Some(2)), None);
        assert_ne!(
            c1, c2,
            "connectors sharing an EVSE must be distinct entries"
        );
        assert_eq!(c1, v.entry_index("CP1", Scope::evse(1, Some(1)), None));
        // 1.6-style keying (CS-level vs connector) is unchanged.
        let cs = v.entry_index("CP1", Scope::CS, None);
        let conn = v.entry_index("CP1", Scope::connector(1), None);
        assert_ne!(cs, conn);
        assert_eq!(conn, v.entry_index("CP1", Scope::connector(1), None));
    }

    #[test]
    fn open_detail_builds_overlay_for_selected_entry() {
        let mut v = server_view();
        // Add a CS-level entry and select its row.
        v.entry_index("CP1", Scope::CS, None);
        v.cs_table.state.set_values(vec![CsRow {
            name: "CP1".into(),
            connector: String::new(),
            state: "Disconnected".into(),
        }]);
        v.cs_table.state.move_down();
        assert!(!v.is_overlay_active());
        v.open_detail();
        assert!(v.is_overlay_active(), "detail overlay should be active");
        let ServerOverlay::Detail(d) = &v.overlay else {
            panic!("detail overlay open")
        };
        assert_eq!(d.identity, "CP1");
        assert!(d.is_cs, "CS-level entry should yield a CS detail overlay");
    }

    fn get_config_event(conn: ConnectionId) -> ServerEvent {
        ServerEvent::Outbound {
            conn,
            scope: Scope::CS,
            name: "GetConfiguration".into(),
            request: serde_json::json!({}),
            response: serde_json::json!({ "configurationKey": [
                { "key": "HeartbeatInterval", "value": "30", "readonly": true }
            ]}),
            ok: true,
            context: String::new(),
        }
    }

    #[test]
    fn stop_transaction_clears_connector_tx_via_tx_id_routing() {
        let mut v = server_view();
        let conn = ConnectionId(1);
        v.conn_identity.insert(conn, "CP1".into());
        // Start a transaction on connector 1; the CSMS response mints transactionId 77.
        v.events_tx
            .send(ServerEvent::Inbound {
                conn,
                name: "StartTransaction".into(),
                request: serde_json::json!({ "connectorId": 1, "idTag": "T" }),
                response: serde_json::json!({ "transactionId": 77 }),
            })
            .unwrap();
        v.drain_events();
        let idx = v.entry_index("CP1", Scope::connector(1), Some(conn));
        assert_eq!(
            v.entries[idx].get_field_str("TransactionId").as_deref(),
            Some("77"),
            "connector 1 should hold the started transaction"
        );
        // StopTransaction carries no connectorId, only the transactionId. It buckets to CS scope but
        // must be re-routed to connector 1 (which holds tx 77) so its transaction id clears.
        v.events_tx
            .send(ServerEvent::Inbound {
                conn,
                name: "StopTransaction".into(),
                request: serde_json::json!({ "transactionId": 77 }),
                response: serde_json::Value::Null,
            })
            .unwrap();
        v.drain_events();
        let idx = v.entry_index("CP1", Scope::connector(1), Some(conn));
        assert_eq!(
            v.entries[idx].get_field_str("TransactionId").as_deref(),
            Some(""),
            "connector 1's transaction id should be cleared on stop"
        );
    }

    #[test]
    fn get_configuration_response_populates_config_table() {
        let mut v = server_view();
        let conn = ConnectionId(1);
        v.conn_identity.insert(conn, "CP1".into());
        v.entry_index("CP1", Scope::CS, Some(conn));
        v.cs_table.state.set_values(vec![CsRow {
            name: "CP1".into(),
            connector: String::new(),
            state: "Connected".into(),
        }]);
        v.cs_table.state.move_down();
        v.open_detail();
        v.events_tx.send(get_config_event(conn)).unwrap();
        v.drain_events();
        let ServerOverlay::Detail(d) = &v.overlay else {
            panic!("detail overlay open")
        };
        assert_eq!(
            d.config_rows(),
            vec![("HeartbeatInterval".into(), "30".into(), true)]
        );
    }

    #[test]
    fn get_configuration_response_persists_with_overlay_closed() {
        let mut v = server_view();
        let conn = ConnectionId(1);
        v.conn_identity.insert(conn, "CP1".into());
        v.entry_index("CP1", Scope::CS, Some(conn));
        // No overlay open when the response arrives (e.g. triggered via the action button).
        v.events_tx.send(get_config_event(conn)).unwrap();
        v.drain_events();
        assert_eq!(
            v.cs_configs.get("CP1").unwrap(),
            &vec![("HeartbeatInterval".into(), "30".into(), true)]
        );
        // Opening the detail later seeds the table from the persisted rows.
        v.cs_table.state.set_values(vec![CsRow {
            name: "CP1".into(),
            connector: String::new(),
            state: "Connected".into(),
        }]);
        v.cs_table.state.move_down();
        v.open_detail();
        let ServerOverlay::Detail(d) = &v.overlay else {
            panic!("detail overlay open")
        };
        assert_eq!(
            d.config_rows(),
            vec![("HeartbeatInterval".into(), "30".into(), true)]
        );
    }

    #[test]
    fn config_keys_persist_across_overlay_close() {
        let mut v = server_view();
        v.entry_index("CP1", Scope::CS, None);
        v.cs_table.state.set_values(vec![CsRow {
            name: "CP1".into(),
            connector: String::new(),
            state: "Disconnected".into(),
        }]);
        v.open_detail();
        // Simulate a fetched config row merged into the overlay.
        let ServerOverlay::Detail(d) = &mut v.overlay else {
            panic!("detail overlay open")
        };
        d.merge_config("HeartbeatInterval".into(), "30".into(), false);
        // Close the overlay (Esc) — keep rows in memory, not discard.
        // Qualified: ServerView now also derives `HandleEvents` via `#[derive(Focus)]`.
        ModuleView::handle_events(&mut v, KeyModifiers::NONE, KeyCode::Esc);
        assert!(!v.overlay.is_active());
        assert_eq!(
            v.cs_configs.get("CP1").unwrap(),
            &vec![("HeartbeatInterval".into(), "30".into(), false)]
        );
        // Reopening seeds the overlay from the in-memory rows.
        v.open_detail();
        let ServerOverlay::Detail(d) = &v.overlay else {
            panic!("detail overlay open")
        };
        assert_eq!(
            d.config_rows(),
            vec![("HeartbeatInterval".into(), "30".into(), false)]
        );
        v.overlay = ServerOverlay::None;
        // Deleting the CS drops its stored config.
        v.delete_selected();
        assert!(!v.cs_configs.contains_key("CP1"));
    }

    #[test]
    fn sync_actions_preserves_selection_when_no_entry_selected() {
        let mut v = server_view();
        // First sync builds the CS-level action list.
        v.sync_actions();
        assert!(
            v.actions.state.values().len() > 1,
            "need >1 action to exercise selection movement"
        );
        // Move the selection off the top.
        v.actions.state.move_down();
        let chosen = v.actions.state.selection();
        assert_ne!(chosen, 0);
        // A later sync with no CS entry selected must not rebuild/reset the list (the bug:
        // it rebuilt every frame, snapping the selection back to the top).
        v.sync_actions();
        assert_eq!(
            v.actions.state.selection(),
            chosen,
            "selection reset — sync_actions rebuilt the list with no selection present"
        );
    }
}
