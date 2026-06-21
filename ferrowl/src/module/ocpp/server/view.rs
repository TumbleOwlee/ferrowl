//! Generic OCPP **server** (CSMS) view, instantiated once per OCPP version (see the
//! [`ServerVersion`] impls in `v1_6`/`v2_0_1`). Left: a table of connected charging stations and
//! their connectors, a "Lua Scripts" button, and the CSMS action list (filtered by the selected
//! entry's level); right: the selected entry's message log over a JSON payload viewer; an
//! ONLINE/OFFLINE status line for the listening socket.
//!
//! Each WebSocket connection yields a CS-level entry (no connector id) plus a connector entry for
//! every `connectorId` seen in inbound traffic. The selected entry scopes the message log, the
//! action list, and — via one Lua sim per entry over its own observed state — `C_OCPP:Get`.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::traits::HandleEvents;
use ferrowl_ui::widgets::GetValue;
use ferrowl_ui::{
    Border, COLOR_SCHEME, EventResult,
    state::{
        ButtonState, ButtonStateBuilder, CodeInputFieldState, CodeInputFieldStateBuilder,
        SelectionState, SelectionStateBuilder, TableState, TableStateBuilder,
    },
    style::{
        ButtonStyle, InputFieldStyleBuilder, SelectionStyleBuilder, TableStyleBuilder, TextStyle,
    },
    widgets::{
        Button, ButtonBuilder, CodeInputField, CodeInputFieldBuilder, Header, Selection,
        SelectionBuilder, Table, TableBuilder, TableEntry, TextBuilder, Widget, Width,
    },
};
use ratatui::style::Style;
use ratatui::{
    Frame,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::StatefulWidget,
};
use tokio::sync::RwLock as AsyncRwLock;

use ferrowl_ocpp::csms::{ConnectionId, CsmsActionHandler};
use ferrowl_ocpp::{ConnectorScope, Version};

use crate::app::LogRing;
use crate::module::modbus::dialog::ConfirmDeleteDialog;
use crate::module::ocpp::action_dialog::{ActionDialog, ActionResult, gen_tx_id};
use crate::module::ocpp::client::backend::{Dir, OcppMessage, push_capped};
use crate::module::ocpp::client::build_client_view;
use crate::module::ocpp::client::lua_sim::{
    ActionQueue, OcppFields, OcppSimHandle, merge_overrides, run_ocpp_sim,
};
use crate::module::ocpp::client::scripts::ScriptDialog;
use crate::module::ocpp::config::device::OcppDeviceConfig;
use crate::module::ocpp::config::session::{OcppModuleSpec, OcppRole, OcppSpec};
use crate::module::ocpp::server::backend::{
    EventRx, EventTx, OcppServer, RfidList, Scope, ServerEvent, inbound_messages,
};
use crate::module::ocpp::server::build_server_view;
use crate::module::ocpp::server::detail::{DetailOverlay, DetailRequest};
use crate::module::ocpp::setup_dialog::OcppSetupDialog;
use crate::module::view::{
    CommandDescriptor, CommandFuture, CommandResult, ModuleView, RefreshFuture, SharedLog,
};
use crate::view::log::format_timestamp;

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
    /// Derive a complete outbound payload for `name` from observed state (e.g. `idTag` from the
    /// last RFID, the connector/EVSE id from `scope`), or `None` to fall back to the JSON editor.
    fn derive_payload(&self, name: &str, scope: Scope) -> Option<serde_json::Value>;
    /// Ordered (field, value) rows describing the observed non-metering state, for the detail
    /// overlay's "State" table.
    fn fields(&self) -> Vec<(String, String)>;
    /// Ordered (field, value) metering rows for the detail overlay's "Metering" table (default
    /// empty; connector states override).
    fn metering(&self) -> Vec<(String, String)> {
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
    fn handler(tx: EventTx, rfids: RfidList) -> Self::Handler;

    /// The scope an inbound request targets (CS-level/connector/EVSE), used to bucket it.
    fn inbound_connector(name: &str, request: &serde_json::Value) -> Scope;

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
}

// --- Connection table ------------------------------------------------------

#[derive(Clone, Debug)]
struct CsRow {
    name: String,
    connector: String,
    state: String,
}

impl TableEntry<3> for CsRow {
    fn values(&self) -> [String; 3] {
        [
            self.name.clone(),
            self.connector.clone(),
            self.state.clone(),
        ]
    }
    fn height(&self) -> u16 {
        1
    }
    fn cell_styles(&self) -> [Option<Style>; 3] {
        let style = match self.state.as_str() {
            "Connected" => Some(Style::default().fg(COLOR_SCHEME.success)),
            "Disconnected" => Some(Style::default().fg(COLOR_SCHEME.error)),
            _ => None,
        };
        [None, None, style]
    }
}

#[derive(Clone, Debug)]
struct CsHeader;

impl Header<3> for CsHeader {
    fn header() -> [String; 3] {
        [
            "Charging Station".into(),
            "Connector".into(),
            "State".into(),
        ]
    }
    fn widths() -> [Width; 3] {
        [
            Width { min: 18, max: 40 },
            Width { min: 9, max: 9 },
            Width { min: 12, max: 12 },
        ]
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
    fn cell_styles(&self) -> [Option<Style>; 5] {
        let status_style = match self.status.as_str() {
            "Success" => Some(Style::default().fg(COLOR_SCHEME.success)),
            "Error" => Some(Style::default().fg(COLOR_SCHEME.error)),
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
    queue: ActionQueue,
    sim: Option<OcppSimHandle>,
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
            EntryState::Cs(s) => s.write().unwrap().apply_inbound(name, request, response),
            EntryState::Conn(s) => s.write().unwrap().apply_inbound(name, request, response),
        }
    }

    fn derive_payload(&self, name: &str) -> Option<serde_json::Value> {
        match &self.state {
            EntryState::Cs(s) => s.read().unwrap().derive_payload(name, self.scope),
            EntryState::Conn(s) => s.read().unwrap().derive_payload(name, self.scope),
        }
    }

    /// Read an observed-state field as a display string, for action-dialog prefill.
    fn get_field_str(&self, name: &str) -> Option<String> {
        use crate::module::ocpp::action_dialog::value_to_string;
        let v = match &self.state {
            EntryState::Cs(s) => s.read().unwrap().get_field(name),
            EntryState::Conn(s) => s.read().unwrap().get_field(name),
        };
        v.map(value_to_string)
    }

    /// (Re)start this entry's Lua sim over the given scripts (no-op if no enabled scripts).
    fn restart_sim(&mut self, scripts: Vec<(String, String)>, log: SharedLog) {
        if let Some(mut sim) = self.sim.take() {
            sim.stop();
        }
        let queue = self.queue.clone();
        self.sim = match &self.state {
            EntryState::Cs(s) => run_ocpp_sim(s.clone(), queue, scripts, log),
            EntryState::Conn(s) => run_ocpp_sim(s.clone(), queue, scripts, log),
        };
    }
}

// --- View ------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
    CsTable,
    Scripts,
    Actions,
    Messages,
    Payload,
}

type CsTable = Widget<TableState<CsRow, 3>, Table<CsRow, CsHeader, 3>>;
type MsgTable = Widget<TableState<MsgRow, 5>, Table<MsgRow, MsgHeader, 5>>;

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
    cs_table: CsTable,
    scripts_button: Widget<ButtonState, Button>,
    actions: Widget<SelectionState<String>, Selection<String>>,
    /// Whether the action list is currently built for a connector entry (`Some(true)`), a CS-level
    /// entry (`Some(false)`), or not yet built (`None`) — to avoid rebuilding every tick.
    actions_for_connector: Option<bool>,
    msg_table: MsgTable,
    code: Widget<CodeInputFieldState, CodeInputField>,
    script_dialog: Option<ScriptDialog>,
    /// A JSON action editor: (action name, target conn, connector id, editor widget).
    /// An open per-action send dialog with its target (connection, connector).
    action_dialog: Option<(ConnectionId, Scope, ActionDialog)>,
    setup_overlay: Option<OcppSetupDialog>,
    pending_setup: Option<(OcppSpec, String)>,
    replacement: Option<Box<dyn ModuleView>>,
    /// Delete-confirmation dialog for the focused CS-table entry.
    delete_confirm: Option<ConfirmDeleteDialog>,
    focus: Pane,
    /// Whether the listener should be running (auto-bind on open; toggled by `:start`/`:stop`).
    want_running: bool,
    /// Highest message `seq` already teed into the persistent log, so each is logged once.
    logged_seq: u64,
    /// The `log_file` currently applied to the `SharedLog`, to detect `:log`/edit changes.
    applied_log_file: Option<String>,
    /// Last content pushed into the payload viewer, so periodic refreshes don't reset its scroll.
    code_content: String,
    /// Shared RFID accept-list handed to each (re)built inbound handler; edited via `:rfid`.
    rfids: RfidList,
    /// Compact table rows (no vertical margin); toggled by `:compact`.
    compact: bool,
    /// The per-entry detail overlay (Enter on a Charging Stations row), if open.
    detail: Option<DetailOverlay>,
    /// In-memory per-CS configuration rows (identity → key/value), kept across overlay open/close
    /// only while the CS is in the list; dropped when its entry is removed (delete/`:stop`/`:restart`).
    cs_configs: HashMap<String, Vec<(String, String, bool)>>,
}

impl<V: ServerVersion> ServerView<V>
where
    V::Action: Clone,
{
    pub fn new(spec: OcppSpec, device_path: String, device: OcppDeviceConfig) -> Self {
        let (events_tx, events_rx) = tokio::sync::mpsc::unbounded_channel();
        let rfids: RfidList = Arc::new(std::sync::RwLock::new(device.rfids.clone()));
        Self {
            backend: OcppServer::new(spec.clone()),
            spec,
            device_path,
            device,
            log: Arc::new(AsyncRwLock::new(LogRing::init())),
            events_tx,
            events_rx,
            entries: Vec::new(),
            conn_identity: HashMap::new(),
            cs_table: cs_table(),
            scripts_button: scripts_button(),
            actions: action_list(Vec::new()),
            actions_for_connector: None,
            msg_table: msg_table(),
            code: code_view(),
            script_dialog: None,
            action_dialog: None,
            setup_overlay: None,
            pending_setup: None,
            replacement: None,
            delete_confirm: None,
            focus: Pane::CsTable,
            want_running: true,
            logged_seq: 0,
            applied_log_file: None,
            code_content: String::new(),
            rfids,
            compact: false,
            detail: None,
            cs_configs: HashMap::new(),
        }
    }

    fn set_compact(&mut self, compact: bool) {
        self.compact = compact;
        let margin = Margin {
            vertical: if compact { 0 } else { 1 },
            horizontal: 0,
        };
        self.cs_table.widget.set_row_margin(margin);
        self.msg_table.widget.set_row_margin(margin);
    }

    /// Delete the selected entry. A CS-level entry takes its connector entries with it.
    fn delete_selected(&mut self) {
        let Some(idx) = self.selected() else { return };
        let entry = &self.entries[idx];
        if !entry.scope.is_connector() {
            let identity = entry.identity.clone();
            self.entries.retain(|e| e.identity != identity);
            self.cs_configs.remove(&identity);
        } else {
            self.entries.remove(idx);
        }
    }

    /// Open the detail overlay for the selected entry, seeding any persisted config rows.
    fn open_detail(&mut self) {
        let Some(idx) = self.selected() else { return };
        let entry = &self.entries[idx];
        let identity = entry.identity.clone();
        let scope = entry.scope;
        let mut overlay = DetailOverlay::new(identity.clone(), scope);
        if !scope.is_connector()
            && let Some(rows) = self.cs_configs.get(&identity)
        {
            overlay.set_config(rows.clone());
        }
        self.detail = Some(overlay);
    }

    /// The live connection for a charge-point identity, if any entry of it is online.
    fn conn_for(&self, identity: &str) -> Option<ConnectionId> {
        self.entries
            .iter()
            .find(|e| e.identity == identity && e.conn.is_some())
            .and_then(|e| e.conn)
    }

    /// Feed the open detail overlay live state/metering rows from its target entry.
    fn refresh_detail(&mut self) {
        let Some((identity, scope, is_cs)) = self
            .detail
            .as_ref()
            .map(|d| (d.identity.clone(), d.scope, d.is_cs))
        else {
            return;
        };
        let Some(entry) = self
            .entries
            .iter()
            .find(|e| e.identity == identity && e.scope == scope)
        else {
            return;
        };
        let (fields, metering) = match &entry.state {
            EntryState::Cs(s) => {
                let g = s.read().unwrap();
                (g.fields(), g.metering())
            }
            EntryState::Conn(s) => {
                let g = s.read().unwrap();
                (g.fields(), g.metering())
            }
        };
        let detail = self.detail.as_mut().unwrap();
        detail.set_state_rows(fields);
        if !is_cs {
            detail.set_metering_rows(metering);
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

    /// Resolve (and cache) a connection's charge-point identity.
    fn identity_of(&mut self, conn: ConnectionId) -> String {
        if let Some(id) = self.conn_identity.get(&conn) {
            return id.clone();
        }
        let id = self
            .backend
            .identity(conn)
            .unwrap_or_else(|| conn.to_string());
        self.conn_identity.insert(conn, id.clone());
        id
    }

    /// Find an entry by (identity, connector), creating it if missing. Returns its index.
    fn entry_index(&mut self, identity: &str, scope: Scope, conn: Option<ConnectionId>) -> usize {
        if let Some(i) = self
            .entries
            .iter()
            .position(|e| e.identity == identity && e.scope == scope)
        {
            return i;
        }
        let state = if scope.is_connector() {
            EntryState::Conn(Arc::new(RwLock::new(V::Conn::default())))
        } else {
            EntryState::Cs(Arc::new(RwLock::new(V::Cs::default())))
        };
        let mut entry = Entry {
            identity: identity.to_string(),
            scope,
            conn,
            online: conn.is_some(),
            state,
            messages: Vec::new(),
            queue: Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new())),
            sim: None,
        };
        if self.want_running {
            entry.restart_sim(self.enabled_scripts(), self.log.clone());
        }
        self.entries.push(entry);
        self.entries.len() - 1
    }

    /// Drain backend events into entries (create/update state, append to per-entry logs).
    fn drain_events(&mut self) {
        let mut events = Vec::new();
        while let Ok(ev) = self.events_rx.try_recv() {
            events.push(ev);
        }
        for ev in events {
            match ev {
                ServerEvent::Connected { conn } => {
                    let identity = self.identity_of(conn);
                    // Ensure the CS-level entry exists and bring every entry of this CS online.
                    self.entry_index(&identity, Scope::CS, Some(conn));
                    for e in self.entries.iter_mut().filter(|e| e.identity == identity) {
                        e.online = true;
                        e.conn = Some(conn);
                    }
                }
                ServerEvent::Disconnected { conn } => {
                    for e in self.entries.iter_mut().filter(|e| e.conn == Some(conn)) {
                        e.online = false;
                        e.conn = None;
                    }
                }
                ServerEvent::Inbound {
                    conn,
                    name,
                    request,
                    response,
                } => {
                    let identity = self.identity_of(conn);
                    let scope = V::inbound_connector(&name, &request);
                    // Always make sure the CS-level entry exists for this connection.
                    self.entry_index(&identity, Scope::CS, Some(conn));
                    let idx = self.entry_index(&identity, scope, Some(conn));
                    let entry = &mut self.entries[idx];
                    entry.online = true;
                    entry.conn = Some(conn);
                    entry.apply_inbound(&name, &request, &response);
                    for m in inbound_messages(&name, request, response) {
                        push_capped(&mut entry.messages, m);
                    }
                }
                ServerEvent::Outbound {
                    conn,
                    scope,
                    name,
                    request,
                    response,
                    ok,
                    context,
                } => {
                    let identity = self.identity_of(conn);
                    // Persist a config-fetch response for this CS so it is available whether or not
                    // the detail overlay is open, and live-merge it into an open matching overlay.
                    if ok && name == V::config_action() {
                        let rows = V::parse_config(&response);
                        if !rows.is_empty() {
                            let store = self.cs_configs.entry(identity.clone()).or_default();
                            for (k, v, ro) in rows {
                                match store.iter_mut().find(|(ek, _, _)| *ek == k) {
                                    Some(r) => {
                                        r.1 = v;
                                        r.2 = ro;
                                    }
                                    None => store.push((k, v, ro)),
                                }
                            }
                            if let Some(d) = self.detail.as_mut()
                                && d.is_cs
                                && d.identity == identity
                            {
                                d.set_config(self.cs_configs[&identity].clone());
                            }
                        }
                    }
                    let idx = self.entry_index(&identity, scope, Some(conn));
                    let entry = &mut self.entries[idx];
                    push_capped(
                        &mut entry.messages,
                        OcppMessage::new(Dir::Out, name.clone(), request, None, "outbound call"),
                    );
                    push_capped(
                        &mut entry.messages,
                        OcppMessage::new(Dir::In, name, response, Some(ok), context),
                    );
                }
            }
        }
    }

    /// Spawn an outbound Call to `conn` and post its result back as an [`ServerEvent::Outbound`].
    fn send_to(&self, conn: ConnectionId, scope: Scope, name: &str, payload: serde_json::Value) {
        let Some(sender) = self.backend.sender() else {
            return;
        };
        let tx = self.events_tx.clone();
        let name = name.to_string();
        let action = match V::decode_call(&name, payload.clone()) {
            Ok(a) => a,
            Err(e) => {
                let _ = tx.send(ServerEvent::Outbound {
                    conn,
                    scope,
                    name,
                    request: payload,
                    response: serde_json::Value::Null,
                    ok: false,
                    context: format!("invalid payload: {e}"),
                });
                return;
            }
        };
        let request = payload;
        tokio::spawn(async move {
            let (response, ok, context) = match sender.call(conn, action).await {
                Ok(resp) => (resp, true, String::new()),
                Err(e) => (serde_json::Value::Null, false, e.to_string()),
            };
            let _ = tx.send(ServerEvent::Outbound {
                conn,
                scope,
                name,
                request,
                response,
                ok,
                context,
            });
        });
    }

    /// Selected entry index, if any.
    fn selected(&self) -> Option<usize> {
        let i = self.cs_table.state.table_state().selected()?;
        (i < self.entries.len()).then_some(i)
    }

    /// Trigger the focused action against the selected entry.
    fn trigger_action(&mut self) {
        let name = self.actions.state.get_value();
        if name.is_empty() {
            return;
        }
        let Some(idx) = self.selected() else { return };
        let Some(conn) = self.entries[idx].conn else {
            return;
        };
        let scope = self.entries[idx].scope;
        match self.entries[idx].derive_payload(&name) {
            Some(payload) => self.send_to(conn, scope, &name, payload),
            None => {
                // Open a per-action dialog from the spec, or a raw JSON editor if none yet.
                let dialog = match V::action_spec(&name) {
                    Some(spec) => {
                        let entry = &self.entries[idx];
                        ActionDialog::new(
                            name.clone(),
                            &spec,
                            |f| entry.get_field_str(f),
                            gen_tx_id,
                        )
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
                        ActionDialog::json_only(name.clone(), &template)
                    }
                };
                self.action_dialog = Some((conn, scope, dialog));
            }
        }
    }

    /// Drain each entry's Lua action queue and send the actions to its connection.
    fn drain_lua_actions(&mut self) {
        let mut sends: Vec<(ConnectionId, Scope, String, serde_json::Value)> = Vec::new();
        for entry in &self.entries {
            let Some(conn) = entry.conn else { continue };
            let queued: Vec<(String, serde_json::Value)> =
                entry.queue.lock().unwrap().drain(..).collect();
            for (name, overrides) in queued {
                let mut payload = entry.derive_payload(&name).unwrap_or_else(|| {
                    V::default_action(&name)
                        .and_then(|a| V::encode_action(&a).ok())
                        .unwrap_or_else(|| serde_json::json!({}))
                });
                merge_overrides(&mut payload, overrides);
                sends.push((conn, entry.scope, name, payload));
            }
        }
        for (conn, scope, name, payload) in sends {
            self.send_to(conn, scope, &name, payload);
        }
    }

    /// Rebuild the action list for the selected entry's level, if it changed.
    fn sync_actions(&mut self) {
        let is_connector = self
            .selected()
            .map(|i| self.entries[i].scope.is_connector());
        let want = is_connector.unwrap_or(false);
        // Rebuild only when the level (CS vs connector) actually changes. Gating on a live
        // selection rebuilt the list every frame while the table was empty, resetting the
        // selection so it could never move.
        if self.actions_for_connector == Some(want) {
            return;
        }
        self.actions_for_connector = Some(want);
        let values: Vec<String> = V::csms_actions()
            .iter()
            .filter(|(_, scope)| {
                matches!(
                    (want, scope),
                    (true, ConnectorScope::Required | ConnectorScope::Optional)
                        | (false, ConnectorScope::None | ConnectorScope::Optional)
                )
            })
            .map(|(n, _)| n.to_string())
            .collect();
        self.actions = action_list(values);
    }

    /// Load the selected message's payload into the read-only viewer.
    fn sync_code(&mut self) {
        let content = self
            .selected()
            .and_then(|i| {
                let sel = self.msg_table.state.table_state().selected()?;
                self.entries[i].messages.get(sel).cloned()
            })
            .map(|m| serde_json::to_string_pretty(&m.payload).unwrap_or_default())
            .unwrap_or_default();
        // Only reset the viewer when the selected payload actually changes; otherwise the periodic
        // refresh would snap its scroll position back to the top every tick.
        if content != self.code_content {
            self.code.state.set_content(&content);
            self.code_content = content;
        }
    }

    fn open_scripts(&mut self) {
        self.script_dialog = Some(ScriptDialog::new(&self.device.scripts));
    }

    /// Restart every entry's sim (after a script edit).
    fn restart_all_sims(&mut self) {
        let scripts = self.enabled_scripts();
        let log = self.log.clone();
        for entry in &mut self.entries {
            entry.restart_sim(scripts.clone(), log.clone());
        }
    }

    fn focus_next(&mut self) {
        self.focus = match self.focus {
            Pane::CsTable => Pane::Scripts,
            Pane::Scripts => Pane::Actions,
            Pane::Actions => Pane::Messages,
            Pane::Messages => Pane::Payload,
            Pane::Payload => Pane::CsTable,
        };
    }

    fn focus_previous(&mut self) {
        self.focus = match self.focus {
            Pane::CsTable => Pane::Payload,
            Pane::Payload => Pane::Messages,
            Pane::Messages => Pane::Actions,
            Pane::Actions => Pane::Scripts,
            Pane::Scripts => Pane::CsTable,
        };
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
        device.rfids = self.rfids.read().unwrap().clone();
        match Converter::save(&device, path, ty) {
            Ok(()) => CommandResult::Handled(Some(format!("Saved device config to {path}"))),
            Err(e) => CommandResult::Handled(Some(format!("Save failed: {e:?}"))),
        }
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
        self.script_dialog.is_some()
            || self.action_dialog.is_some()
            || self.setup_overlay.is_some()
            || self.delete_confirm.is_some()
            || self.pending_setup.is_some()
            || self.detail.is_some()
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let buf = frame.buffer_mut();
        let [body, status_area] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
        let [left, right] =
            Layout::horizontal([Constraint::Length(54), Constraint::Min(1)]).areas(body);
        let [cs_area, scripts_btn_area, actions_area] = Layout::vertical([
            Constraint::Min(3),
            Constraint::Length(3),
            Constraint::Max(2 + self.actions.state.values().len() as u16),
        ])
        .areas(left);
        let [right_top, right_bottom] =
            Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(right);

        // Keep the right-hand panes in sync with the current selection.
        self.sync_actions();
        let rows: Vec<MsgRow> = self
            .selected()
            .map(|i| {
                self.entries[i]
                    .rows_for_state()
                    .iter()
                    .map(msg_row)
                    .collect()
            })
            .unwrap_or_default();
        self.msg_table.state.set_values(rows);
        // Autoscroll the message log to the newest row unless the user is scrolling it.
        if !(focused && self.focus == Pane::Messages) {
            self.msg_table.state.move_to_bottom();
        }
        let cs_rows: Vec<CsRow> = self
            .entries
            .iter()
            .map(|e| CsRow {
                name: e.identity.clone(),
                connector: e.scope.label(),
                state: if e.online {
                    "Connected"
                } else {
                    "Disconnected"
                }
                .to_string(),
            })
            .collect();
        self.cs_table.state.set_values(cs_rows);
        self.sync_code();

        self.cs_table
            .state
            .set_focused(focused && self.focus == Pane::CsTable);
        self.scripts_button
            .state
            .set_focused(focused && self.focus == Pane::Scripts);
        self.actions
            .state
            .set_focused(focused && self.focus == Pane::Actions);
        self.msg_table
            .state
            .set_focused(focused && self.focus == Pane::Messages);
        self.code
            .state
            .set_focused(focused && self.focus == Pane::Payload);

        StatefulWidget::render(
            &self.cs_table.widget,
            cs_area,
            buf,
            &mut self.cs_table.state,
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
        StatefulWidget::render(
            &self.msg_table.widget,
            right_top,
            buf,
            &mut self.msg_table.state,
        );
        StatefulWidget::render(&self.code.widget, right_bottom, buf, &mut self.code.state);

        // ONLINE/OFFLINE status line (with the bound address when running).
        let online = self.backend.is_online();
        let status_widget = TextBuilder::default()
            .horizontal_alignment(HorizontalAlignment::Center)
            .style(TextStyle {
                general: Style::default()
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
        let mut status = if online {
            format!("ONLINE  {}", self.backend.bound_addr().unwrap_or_default())
        } else {
            "OFFLINE".to_string()
        };
        StatefulWidget::render(&status_widget, status_area, buf, &mut status);

        if let Some(dialog) = self.script_dialog.as_mut() {
            dialog.render(area, buf);
        }
        if let Some((_, _, dlg)) = self.action_dialog.as_mut() {
            dlg.render(area, buf);
        }
        if self.detail.is_some() {
            self.refresh_detail();
            self.detail.as_mut().unwrap().render(area, buf);
        }
        if let Some(setup) = self.setup_overlay.as_mut() {
            setup.render(area, buf);
        }
        if let Some(confirm) = self.delete_confirm.as_mut() {
            confirm.render(area, buf);
        }
    }

    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        if self.detail.is_some() {
            let req = self.detail.as_mut().unwrap().input(modifiers, code);
            let identity = self.detail.as_ref().unwrap().identity.clone();
            match req {
                Some(DetailRequest::Close) => {
                    // Keep the (possibly edited) config rows in memory so reopening keeps them
                    // while the CS stays in the list.
                    if let Some(d) = self.detail.take()
                        && d.is_cs
                    {
                        self.cs_configs.insert(d.identity.clone(), d.config_rows());
                    }
                    self.detail = None;
                }
                Some(DetailRequest::Fetch(key)) => {
                    if let Some(conn) = self.conn_for(&identity) {
                        self.send_to(conn, Scope::CS, V::config_action(), V::config_request(&key));
                    }
                }
                Some(DetailRequest::Set(key, value)) => {
                    if let Some(conn) = self.conn_for(&identity) {
                        self.send_to(
                            conn,
                            Scope::CS,
                            V::set_action(),
                            V::set_request(&key, &value),
                        );
                    }
                }
                None => {}
            }
            return EventResult::Consumed;
        }
        if let Some(confirm) = self.delete_confirm.as_mut() {
            match (modifiers, code) {
                (KeyModifiers::NONE, KeyCode::Esc) => self.delete_confirm = None,
                (KeyModifiers::NONE, KeyCode::Tab) => confirm.focus_next(),
                (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => {
                    confirm.focus_previous()
                }
                (KeyModifiers::NONE, KeyCode::Enter | KeyCode::Char(' ')) => {
                    let confirmed = confirm.is_confirm_focused();
                    self.delete_confirm = None;
                    if confirmed {
                        self.delete_selected();
                    }
                }
                _ => {
                    let _ = confirm.handle_events(modifiers, code);
                }
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
        if let Some(dialog) = self.script_dialog.as_mut() {
            if dialog.handle_events(modifiers, code) {
                let scripts = self.script_dialog.take().unwrap().resolve();
                self.device.scripts = scripts;
                self.restart_all_sims();
            }
            return EventResult::Consumed;
        }
        if self.action_dialog.is_some() {
            let res = self
                .action_dialog
                .as_mut()
                .unwrap()
                .2
                .input(modifiers, code);
            match res {
                Some(ActionResult::Close) => self.action_dialog = None,
                Some(ActionResult::Send(payload)) => {
                    let (conn, scope, dlg) = self.action_dialog.as_ref().unwrap();
                    let (conn, scope, name) = (*conn, *scope, dlg.name.clone());
                    // Validate before sending; keep the dialog open on an invalid payload.
                    if V::decode_call(&name, payload.clone()).is_ok() {
                        self.action_dialog = None;
                        self.send_to(conn, scope, &name, payload);
                    }
                }
                None => {}
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
            (KeyModifiers::NONE, KeyCode::Enter) if self.focus == Pane::CsTable => {
                self.open_detail();
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Enter) if self.focus == Pane::Scripts => {
                self.open_scripts();
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Enter) if self.focus == Pane::Actions => {
                self.trigger_action();
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Char('d')) if self.focus == Pane::CsTable => {
                if let Some(idx) = self.selected() {
                    self.delete_confirm =
                        Some(ConfirmDeleteDialog::new(&self.entries[idx].identity));
                }
                EventResult::Consumed
            }
            _ => match self.focus {
                Pane::CsTable => self.cs_table.state.handle_events(modifiers, code),
                Pane::Actions => self.actions.state.handle_events(modifiers, code),
                Pane::Messages => {
                    let consumed = self.msg_table.state.handle_events(modifiers, code);
                    self.sync_code();
                    consumed
                }
                Pane::Scripts => EventResult::Unhandled(modifiers, code),
                Pane::Payload => self.code.state.handle_events(modifiers, code),
            },
        }
    }

    fn refresh<'a>(&'a mut self) -> RefreshFuture<'a> {
        Box::pin(async move {
            // Apply a resolved `:edit`.
            if let Some((spec, path)) = self.pending_setup.take() {
                let mut device = OcppDeviceConfig::from_spec(&spec, self.device.scripts.clone());
                device.log_file = self.device.log_file.clone();
                device.rfids = self.rfids.read().unwrap().clone();
                if spec.role == OcppRole::Client {
                    // Stop the listener first: dropping `Server<V>` only detaches its accept task,
                    // leaving the port bound, so the swapped-in view could never rebind.
                    let _ = self.backend.stop().await;
                    self.replacement = Some(build_client_view(spec, path, device));
                    return;
                }
                if spec.version != self.spec.version {
                    // A version change must swap the whole view: `ServerView<V>`/`OcppServer<V>` are
                    // generic over the *old* version and would rebind with the old subprotocol,
                    // rejecting the (now-different-version) client handshake with a 400.
                    let _ = self.backend.stop().await;
                    self.replacement = Some(build_server_view(spec, path, device));
                    return;
                }
                self.spec = spec;
                self.device = device;
                self.device_path = path;
                // Rebind on the (possibly changed) endpoint.
                let _ = self.backend.stop().await;
                self.entries.clear();
                self.conn_identity.clear();
                self.cs_configs.clear();
                self.log.write().await.write("Settings updated");
            }

            // Auto-bind / honour `:start`.
            if self.want_running && !self.backend.is_online() {
                let handler = V::handler(self.events_tx.clone(), self.rfids.clone());
                if let Err(e) = self.backend.start(handler).await {
                    self.log.write().await.write(&format!("listen failed: {e}"));
                    self.want_running = false;
                }
            }

            self.drain_events();
            self.drain_lua_actions();

            // Apply a pending `:log` change (or device-config log file) to the persistent sink.
            if self.applied_log_file != self.device.log_file {
                let name = self.spec.name.clone();
                self.log
                    .write()
                    .await
                    .set_log_file(self.device.log_file.as_deref(), &name);
                self.applied_log_file = self.device.log_file.clone();
            }

            // Tee new protocol messages (across all entries) into the persistent log. Each entry's
            // log is filtered separately on screen, but the persistent log is the whole CSMS.
            let mut max_seq = self.logged_seq;
            let mut new: Vec<(u64, String)> = Vec::new();
            for entry in &self.entries {
                for m in entry.messages.iter().filter(|m| m.seq > self.logged_seq) {
                    max_seq = max_seq.max(m.seq);
                    new.push((m.seq, m.log_line()));
                }
            }
            if !new.is_empty() {
                new.sort_by_key(|(seq, _)| *seq);
                let mut log = self.log.write().await;
                for (_, line) in new {
                    log.write(&line);
                }
                self.logged_seq = max_seq;
            }
        })
    }

    fn handle_command<'a>(&'a mut self, cmd: &'a str) -> CommandFuture<'a> {
        Box::pin(async move {
            match cmd.trim() {
                "start" => {
                    self.want_running = true;
                    let handler = V::handler(self.events_tx.clone(), self.rfids.clone());
                    match self.backend.start(handler).await {
                        Ok(()) => CommandResult::Handled(Some("CSMS server started".into())),
                        Err(e) => CommandResult::Handled(Some(format!("listen failed: {e}"))),
                    }
                }
                "stop" => {
                    self.want_running = false;
                    let _ = self.backend.stop().await;
                    self.entries.clear();
                    self.conn_identity.clear();
                    self.cs_configs.clear();
                    CommandResult::Handled(Some("CSMS server stopped".into()))
                }
                "restart" => {
                    let _ = self.backend.stop().await;
                    self.entries.clear();
                    self.conn_identity.clear();
                    self.cs_configs.clear();
                    self.want_running = true;
                    let handler = V::handler(self.events_tx.clone(), self.rfids.clone());
                    match self.backend.start(handler).await {
                        Ok(()) => CommandResult::Handled(Some("CSMS server restarted".into())),
                        Err(e) => CommandResult::Handled(Some(format!("listen failed: {e}"))),
                    }
                }
                "edit" | "e" => {
                    self.setup_overlay = Some(OcppSetupDialog::edit(&self.spec, &self.device_path));
                    CommandResult::Handled(None)
                }
                "wd" => {
                    if self.device_path.is_empty() {
                        CommandResult::Handled(Some(
                            "No configuration file path configured.".into(),
                        ))
                    } else {
                        self.save_device_to(&self.device_path.clone())
                    }
                }
                cmd if cmd.starts_with("wd ") => {
                    let path = cmd["wd ".len()..].trim().to_string();
                    self.save_device_to(&path)
                }
                "compact" => {
                    self.set_compact(!self.compact);
                    CommandResult::Handled(None)
                }
                "log" => {
                    self.device.log_file = None;
                    CommandResult::Handled(Some("File logging disabled".into()))
                }
                cmd if cmd.starts_with("log ") => {
                    let path = cmd["log ".len()..].trim().to_string();
                    if path.is_empty() {
                        self.device.log_file = None;
                        CommandResult::Handled(Some("File logging disabled".into()))
                    } else {
                        self.device.log_file = Some(path.clone());
                        CommandResult::Handled(Some(format!("Logging to {path}")))
                    }
                }
                "rfid" => {
                    let list = self.rfids.read().unwrap();
                    let msg = if list.is_empty() {
                        "RFID accept-list empty (all tags accepted)".to_string()
                    } else {
                        format!("Accepted RFIDs: {}", list.join(", "))
                    };
                    CommandResult::Handled(Some(msg))
                }
                "rfid clear" => {
                    self.rfids.write().unwrap().clear();
                    CommandResult::Handled(Some("RFID accept-list cleared (all accepted)".into()))
                }
                cmd if cmd.starts_with("rfid add ") => {
                    let tag = cmd["rfid add ".len()..].trim().to_string();
                    if tag.is_empty() {
                        return CommandResult::Handled(Some("Usage: :rfid add <tag>".into()));
                    }
                    let mut list = self.rfids.write().unwrap();
                    if list.contains(&tag) {
                        CommandResult::Handled(Some(format!("{tag} already in accept-list")))
                    } else {
                        list.push(tag.clone());
                        CommandResult::Handled(Some(format!("Added RFID {tag}")))
                    }
                }
                cmd if cmd.starts_with("rfid del ") => {
                    let tag = cmd["rfid del ".len()..].trim().to_string();
                    let mut list = self.rfids.write().unwrap();
                    let before = list.len();
                    list.retain(|t| t != &tag);
                    if list.len() < before {
                        CommandResult::Handled(Some(format!("Removed RFID {tag}")))
                    } else {
                        CommandResult::Handled(Some(format!("{tag} not in accept-list")))
                    }
                }
                _ => CommandResult::Unhandled,
            }
        })
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

// --- Widget builders -------------------------------------------------------

fn border_style() -> Style {
    Style::default().fg(COLOR_SCHEME.border).bg(COLOR_SCHEME.bg)
}

fn cs_table() -> CsTable {
    Widget {
        state: TableStateBuilder::default()
            .values(Vec::new())
            .build()
            .unwrap(),
        widget: TableBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some("Charging Stations".into()))
            .style(TableStyleBuilder::default().build().unwrap())
            .row_margin(Margin {
                vertical: 1,
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
    use crate::module::ocpp::config::session::{OcppProtocol, OcppVersion};
    use ferrowl_ocpp::V1_6;

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
            seen.contains(&Pane::Payload),
            "Payload pane not in Tab order"
        );
        assert!(v.focus == Pane::CsTable, "focus_next did not wrap to start");
        // BackTab from CsTable lands on Payload (reverse order).
        v.focus_previous();
        assert!(v.focus == Pane::Payload);
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
        let d = v.detail.as_ref().expect("detail overlay open");
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
        assert_eq!(
            v.detail.as_ref().unwrap().config_rows(),
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
        assert_eq!(
            v.detail.as_ref().unwrap().config_rows(),
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
        v.detail
            .as_mut()
            .unwrap()
            .merge_config("HeartbeatInterval".into(), "30".into(), false);
        // Close the overlay (Esc) — keep rows in memory, not discard.
        v.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(v.detail.is_none());
        assert_eq!(
            v.cs_configs.get("CP1").unwrap(),
            &vec![("HeartbeatInterval".into(), "30".into(), false)]
        );
        // Reopening seeds the overlay from the in-memory rows.
        v.open_detail();
        assert_eq!(
            v.detail.as_ref().unwrap().config_rows(),
            vec![("HeartbeatInterval".into(), "30".into(), false)]
        );
        v.detail = None;
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
