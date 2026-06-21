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
use crate::module::ocpp::client::backend::{Dir, OcppMessage};
use crate::module::ocpp::client::build_client_view;
use crate::module::ocpp::client::lua_sim::{
    ActionQueue, OcppFields, OcppSimHandle, merge_overrides, run_ocpp_sim,
};
use crate::module::ocpp::client::scripts::ScriptDialog;
use crate::module::modbus::dialog::ConfirmDeleteDialog;
use crate::module::ocpp::config::device::OcppDeviceConfig;
use crate::module::ocpp::config::session::{OcppModuleSpec, OcppRole, OcppSpec};
use crate::module::ocpp::server::backend::{EventRx, EventTx, OcppServer, ServerEvent, inbound_messages};
use crate::module::ocpp::setup_dialog::OcppSetupDialog;
use crate::module::view::{
    CommandDescriptor, CommandFuture, CommandResult, ModuleView, RefreshFuture, SharedLog,
};
use crate::view::log::format_timestamp;

/// Per-entry observed state behaviour the generic view needs from each version's state types.
pub trait EntryStateT: OcppFields + Default + Send + Sync + 'static {
    /// Update the observed state from an inbound CS→CSMS request.
    fn apply_inbound(&mut self, name: &str, request: &serde_json::Value);
    /// Derive a complete outbound payload for `name` from observed state (e.g. `idTag` from the
    /// last RFID, the connector id), or `None` to fall back to the JSON editor.
    fn derive_payload(&self, name: &str, connector_id: Option<i64>) -> Option<serde_json::Value>;
}

/// Everything version-specific the generic server view needs.
pub trait ServerVersion: Version + Sized + 'static {
    /// CS-level observed state (non-connector info: model/vendor/firmware).
    type Cs: EntryStateT;
    /// Per-connector observed state (metering/status/transaction).
    type Conn: EntryStateT;
    /// The inbound handler answering CS→CSMS Calls and emitting [`ServerEvent`]s.
    type Handler: CsmsActionHandler<Self>;

    /// Build the inbound handler, wiring it to the view's event channel.
    fn handler(tx: EventTx) -> Self::Handler;

    /// The connector id an inbound request targets (`None` = CS-level), used to bucket it.
    fn inbound_connector(name: &str, request: &serde_json::Value) -> Option<i64>;
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
        [self.name.clone(), self.connector.clone(), self.state.clone()]
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
        ["Charging Station".into(), "Connector".into(), "State".into()]
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
    /// `None` = CS-level entry; `Some(id)` = connector entry.
    connector_id: Option<i64>,
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

    fn apply_inbound(&mut self, name: &str, request: &serde_json::Value) {
        match &self.state {
            EntryState::Cs(s) => s.write().unwrap().apply_inbound(name, request),
            EntryState::Conn(s) => s.write().unwrap().apply_inbound(name, request),
        }
    }

    fn derive_payload(&self, name: &str) -> Option<serde_json::Value> {
        match &self.state {
            EntryState::Cs(s) => s.read().unwrap().derive_payload(name, self.connector_id),
            EntryState::Conn(s) => s.read().unwrap().derive_payload(name, self.connector_id),
        }
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
}

type CsTable = Widget<TableState<CsRow, 3>, Table<CsRow, CsHeader, 3>>;
type MsgTable = Widget<TableState<MsgRow, 5>, Table<MsgRow, MsgHeader, 5>>;
/// An open JSON action editor: action name, target connection + connector, and the editor widget.
type ActionDialog = (
    String,
    ConnectionId,
    Option<i64>,
    Widget<CodeInputFieldState, CodeInputField>,
);

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
    action_dialog: Option<ActionDialog>,
    setup_overlay: Option<OcppSetupDialog>,
    pending_setup: Option<(OcppSpec, String)>,
    replacement: Option<Box<dyn ModuleView>>,
    /// Delete-confirmation dialog for the focused CS-table entry.
    delete_confirm: Option<ConfirmDeleteDialog>,
    focus: Pane,
    /// Whether the listener should be running (auto-bind on open; toggled by `:start`/`:stop`).
    want_running: bool,
    /// Compact table rows (no vertical margin); toggled by `:compact`.
    compact: bool,
}

impl<V: ServerVersion> ServerView<V>
where
    V::Action: Clone,
{
    pub fn new(spec: OcppSpec, device_path: String, device: OcppDeviceConfig) -> Self {
        let (events_tx, events_rx) = tokio::sync::mpsc::unbounded_channel();
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
            compact: false,
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
        if entry.connector_id.is_none() {
            let identity = entry.identity.clone();
            self.entries.retain(|e| e.identity != identity);
        } else {
            self.entries.remove(idx);
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
    fn entry_index(&mut self, identity: &str, connector_id: Option<i64>, conn: Option<ConnectionId>) -> usize {
        if let Some(i) = self
            .entries
            .iter()
            .position(|e| e.identity == identity && e.connector_id == connector_id)
        {
            return i;
        }
        let state = match connector_id {
            None => EntryState::Cs(Arc::new(RwLock::new(V::Cs::default()))),
            Some(_) => EntryState::Conn(Arc::new(RwLock::new(V::Conn::default()))),
        };
        let mut entry = Entry {
            identity: identity.to_string(),
            connector_id,
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
                    self.entry_index(&identity, None, Some(conn));
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
                    let connector = V::inbound_connector(&name, &request);
                    // Always make sure the CS-level entry exists for this connection.
                    self.entry_index(&identity, None, Some(conn));
                    let idx = self.entry_index(&identity, connector, Some(conn));
                    let entry = &mut self.entries[idx];
                    entry.online = true;
                    entry.conn = Some(conn);
                    entry.apply_inbound(&name, &request);
                    for m in inbound_messages(&name, request, response) {
                        entry.messages.push(m);
                    }
                }
                ServerEvent::Outbound {
                    conn,
                    connector_id,
                    name,
                    request,
                    response,
                    ok,
                    context,
                } => {
                    let identity = self.identity_of(conn);
                    let idx = self.entry_index(&identity, connector_id, Some(conn));
                    let entry = &mut self.entries[idx];
                    entry.messages.push(OcppMessage {
                        ts: crate::module::ocpp::client::backend::now_ms(),
                        direction: Dir::Out,
                        name: name.clone(),
                        payload: request,
                        ok: None,
                        context: "outbound call".to_string(),
                    });
                    entry.messages.push(OcppMessage {
                        ts: crate::module::ocpp::client::backend::now_ms(),
                        direction: Dir::In,
                        name,
                        payload: response,
                        ok: Some(ok),
                        context,
                    });
                }
            }
        }
    }

    /// Spawn an outbound Call to `conn` and post its result back as an [`ServerEvent::Outbound`].
    fn send_to(&self, conn: ConnectionId, connector_id: Option<i64>, name: &str, payload: serde_json::Value) {
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
                    connector_id,
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
                connector_id,
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
        let connector_id = self.entries[idx].connector_id;
        match self.entries[idx].derive_payload(&name) {
            Some(payload) => self.send_to(conn, connector_id, &name, payload),
            None => {
                // Open a JSON editor prefilled with the Default-derived template.
                let template = V::default_action(&name)
                    .and_then(|a| V::encode_action(&a).ok())
                    .map(|v| serde_json::to_string_pretty(&v).unwrap_or_default())
                    .unwrap_or_else(|| "{}".to_string());
                let mut field = editable_code();
                field.state.set_content(&template);
                self.action_dialog = Some((name, conn, connector_id, field));
            }
        }
    }

    /// Drain each entry's Lua action queue and send the actions to its connection.
    fn drain_lua_actions(&mut self) {
        let mut sends: Vec<(ConnectionId, Option<i64>, String, serde_json::Value)> = Vec::new();
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
                sends.push((conn, entry.connector_id, name, payload));
            }
        }
        for (conn, connector_id, name, payload) in sends {
            self.send_to(conn, connector_id, &name, payload);
        }
    }

    /// Rebuild the action list for the selected entry's level, if it changed.
    fn sync_actions(&mut self) {
        let is_connector = self.selected().map(|i| self.entries[i].connector_id.is_some());
        let want = is_connector.unwrap_or(false);
        if self.actions_for_connector == Some(want) && self.selected().is_some() {
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
        self.code.state.set_content(&content);
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
            Pane::Messages => Pane::CsTable,
        };
    }

    fn focus_previous(&mut self) {
        self.focus = match self.focus {
            Pane::CsTable => Pane::Messages,
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
            .map(|i| self.entries[i].rows_for_state().iter().map(msg_row).collect())
            .unwrap_or_default();
        self.msg_table.state.set_values(rows);
        let cs_rows: Vec<CsRow> = self
            .entries
            .iter()
            .map(|e| CsRow {
                name: e.identity.clone(),
                connector: e.connector_id.map(|c| c.to_string()).unwrap_or_default(),
                state: if e.online { "Connected" } else { "Disconnected" }.to_string(),
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

        StatefulWidget::render(&self.cs_table.widget, cs_area, buf, &mut self.cs_table.state);
        StatefulWidget::render(
            &self.scripts_button.widget,
            scripts_btn_area,
            buf,
            &mut self.scripts_button.state,
        );
        StatefulWidget::render(&self.actions.widget, actions_area, buf, &mut self.actions.state);
        StatefulWidget::render(&self.msg_table.widget, right_top, buf, &mut self.msg_table.state);
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
            format!(
                "ONLINE  {}",
                self.backend.bound_addr().unwrap_or_default()
            )
        } else {
            "OFFLINE".to_string()
        };
        StatefulWidget::render(&status_widget, status_area, buf, &mut status);

        if let Some(dialog) = self.script_dialog.as_mut() {
            dialog.render(area, buf);
        }
        if let Some((_, _, _, field)) = self.action_dialog.as_mut() {
            let [_, mid, _] = Layout::vertical([
                Constraint::Percentage(15),
                Constraint::Percentage(70),
                Constraint::Percentage(15),
            ])
            .areas(area);
            ratatui::widgets::Widget::render(ratatui::widgets::Clear, mid, buf);
            StatefulWidget::render(&field.widget, mid, buf, &mut field.state);
        }
        if let Some(setup) = self.setup_overlay.as_mut() {
            setup.render(area, buf);
        }
        if let Some(confirm) = self.delete_confirm.as_mut() {
            confirm.render(area, buf);
        }
    }

    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
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
        if let Some((name, conn, connector_id, field)) = self.action_dialog.as_mut() {
            match (modifiers, code) {
                (KeyModifiers::NONE, KeyCode::Esc) => self.action_dialog = None,
                (KeyModifiers::NONE, KeyCode::Enter) => {
                    // Keep the dialog open on invalid JSON.
                    if let Ok(payload) =
                        serde_json::from_str::<serde_json::Value>(&field.state.content())
                    {
                        let (name, conn, connector_id) = (name.clone(), *conn, *connector_id);
                        self.action_dialog = None;
                        self.send_to(conn, connector_id, &name, payload);
                    }
                }
                _ => {
                    let _ = field.state.handle_events(modifiers, code);
                }
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
                    self.delete_confirm = Some(ConfirmDeleteDialog::new(&self.entries[idx].identity));
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
            },
        }
    }

    fn refresh<'a>(&'a mut self) -> RefreshFuture<'a> {
        Box::pin(async move {
            // Apply a resolved `:edit`.
            if let Some((spec, path)) = self.pending_setup.take() {
                let device = OcppDeviceConfig::from_spec(&spec, self.device.scripts.clone());
                if spec.role == OcppRole::Client {
                    self.replacement = Some(build_client_view(spec, path, device));
                    return;
                }
                self.spec = spec;
                self.device = device;
                self.device_path = path;
                // Rebind on the (possibly changed) endpoint.
                let _ = self.backend.stop().await;
                self.entries.clear();
                self.conn_identity.clear();
                self.log.write().await.write("Settings updated");
            }

            // Auto-bind / honour `:start`.
            if self.want_running && !self.backend.is_online() {
                let handler = V::handler(self.events_tx.clone());
                if let Err(e) = self.backend.start(handler).await {
                    self.log.write().await.write(&format!("listen failed: {e}"));
                    self.want_running = false;
                }
            }

            self.drain_events();
            self.drain_lua_actions();
        })
    }

    fn handle_command<'a>(&'a mut self, cmd: &'a str) -> CommandFuture<'a> {
        Box::pin(async move {
            match cmd.trim() {
                "start" => {
                    self.want_running = true;
                    let handler = V::handler(self.events_tx.clone());
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
                    CommandResult::Handled(Some("CSMS server stopped".into()))
                }
                "restart" => {
                    let _ = self.backend.stop().await;
                    self.entries.clear();
                    self.conn_identity.clear();
                    self.want_running = true;
                    let handler = V::handler(self.events_tx.clone());
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
                        CommandResult::Handled(Some("No configuration file path configured.".into()))
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

static OCPP_SERVER_COMMANDS: [CommandDescriptor; 6] = [
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
        name: "d",
        description: "delete the selected charging station / connector",
    },
];

// --- Widget builders -------------------------------------------------------

fn border_style() -> Style {
    Style::default().fg(COLOR_SCHEME.border).bg(COLOR_SCHEME.bg)
}

fn cs_table() -> CsTable {
    Widget {
        state: TableStateBuilder::default().values(Vec::new()).build().unwrap(),
        widget: TableBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some("Charging Stations".into()))
            .style(TableStyleBuilder::default().build().unwrap())
            .row_margin(Margin { vertical: 1, horizontal: 0 })
            .build()
            .unwrap(),
    }
}

fn msg_table() -> MsgTable {
    Widget {
        state: TableStateBuilder::default().values(Vec::new()).build().unwrap(),
        widget: TableBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some("Messages".into()))
            .style(TableStyleBuilder::default().build().unwrap())
            .row_margin(Margin { vertical: 1, horizontal: 0 })
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
                    .focused(Style::default().fg(COLOR_SCHEME.bg).bg(COLOR_SCHEME.hi).bold())
                    .build()
                    .unwrap(),
            )
            .margin(Margin { vertical: 0, horizontal: 0 })
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
            .margin(Margin { vertical: 0, horizontal: 0 })
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
            .margin(Margin { vertical: 0, horizontal: 0 })
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
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some("Action payload (Enter to send)".into()))
            .margin(Margin { vertical: 0, horizontal: 0 })
            .build()
            .unwrap(),
    }
}
