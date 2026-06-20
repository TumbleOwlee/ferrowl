//! OCPP 1.6 charging-station (client) view. Left: a system-state panel (editable) over an action
//! list; right: the OCPP message log over a JSON payload viewer; an ONLINE/OFFLINE status line.
//!
//! Action buttons are `V1_6::cs_actions()`. A state-driven action (Authorize, BootNotification,
//! Heartbeat, MeterValues, Start/StopTransaction, StatusNotification) is built straight from state
//! and sent. Any other action opens a JSON dialog prefilled with its `default_action` template.

use std::sync::Arc;
use std::sync::RwLock;

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::{
    Border, COLOR_SCHEME, EventResult,
    state::{
        CodeInputFieldState, CodeInputFieldStateBuilder, InputFieldState, InputFieldStateBuilder,
        SelectionState, SelectionStateBuilder, TableState, TableStateBuilder,
    },
    style::{
        InputFieldStyle, InputFieldStyleBuilder, SelectionStyle, SelectionStyleBuilder,
        TableStyleBuilder, TextStyle,
    },
    traits::HandleEvents,
    widgets::{
        CodeInputField, CodeInputFieldBuilder, GetValue, Header, InputField, InputFieldBuilder,
        Selection, SelectionBuilder, Table, TableBuilder, TableEntry, TextBuilder, Widget, Width,
    },
};
use ratatui::style::Style;
use ratatui::{
    Frame,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, Clear, StatefulWidget, Widget as UiWidget},
};
use tokio::sync::RwLock as AsyncRwLock;

use ferrowl_ocpp::{V1_6, Version};

use crate::app::LogRing;
use crate::module::ocpp::client::backend::{OcppClient, OcppMessage, rfc3339_now};
use crate::module::ocpp::client::config::ConfigEditDialog;
use crate::module::ocpp::client::v1_6::handler::CsStateHandler;
use crate::module::ocpp::client::v1_6::state::{ConfigKey, ConfigRow, CsState, NvRow};
use crate::module::ocpp::config::session::{OcppRole, OcppSpec};
use crate::module::ocpp::setup_dialog::OcppSetupDialog;
use crate::module::ocpp::view::OcppServerView;
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

// --- Config table ----------------------------------------------------------

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
        ["Key".into(), "Value".into(), "ReadOnly".into()]
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
    ConnectorId,
    Phases,
    Voltage,
    Current(usize),
    Power,
    TotalEnergy,
    SessionEnergy,
    Status,
    Rfid,
    Model,
    Vendor,
}

impl EditField {
    fn from_row(row: usize) -> Option<EditField> {
        Some(match row {
            0 => EditField::ConnectorId,
            1 => EditField::Phases,
            2 => EditField::Voltage,
            3 => EditField::Current(0),
            4 => EditField::Current(1),
            5 => EditField::Current(2),
            6 => EditField::Power,
            7 => EditField::TotalEnergy,
            8 => EditField::SessionEnergy,
            9 => EditField::Status,
            10 => EditField::Rfid,
            11 => EditField::Model,
            12 => EditField::Vendor,
            _ => return None,
        })
    }

    fn label(self) -> &'static str {
        match self {
            EditField::ConnectorId => "Connector ID",
            EditField::Phases => "Used Phases",
            EditField::Voltage => "Voltage",
            EditField::Current(0) => "Current L1",
            EditField::Current(1) => "Current L2",
            EditField::Current(_) => "Current L3",
            EditField::Power => "Power",
            EditField::TotalEnergy => "Total Energy",
            EditField::SessionEnergy => "Session Energy",
            EditField::Status => "Status",
            EditField::Rfid => "RFID",
            EditField::Model => "Model",
            EditField::Vendor => "Vendor",
        }
    }
}

const PHASE_CHOICES: [&str; 7] = ["L1", "L2", "L3", "L1,L2", "L1,L3", "L2,L3", "L1,L2,L3"];
const STATUS_CHOICES: [&str; 7] = [
    "Available",
    "Preparing",
    "Charging",
    "SuspendedEV",
    "SuspendedEVSE",
    "Finishing",
    "Faulted",
];

/// State-driven actions: their request is fully built from state, no dialog.
const STATE_DRIVEN: [&str; 7] = [
    "Authorize",
    "BootNotification",
    "Heartbeat",
    "MeterValues",
    "StartTransaction",
    "StatusNotification",
    "StopTransaction",
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
    ConfigTable,
    ConfigKey,
    ConfigValue,
    Actions,
    Messages,
}

type StateTable = Widget<TableState<NvRow, 3>, Table<NvRow, NvHeader, 3>>;
type ConfigTable = Widget<TableState<ConfigRow, 3>, Table<ConfigRow, ConfigHeader, 3>>;
type MsgTable = Widget<TableState<MsgRow, 5>, Table<MsgRow, MsgHeader, 5>>;

pub struct OcppClientV16View {
    spec: OcppSpec,
    backend: OcppClient<V1_6>,
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
    focus: Pane,
    edit: Option<EditOverlay>,
    config_edit: Option<ConfigEditDialog>,
    action_dialog: Option<(String, Widget<CodeInputFieldState, CodeInputField>)>,
    pending_send: Option<(String, serde_json::Value)>,
    setup_overlay: Option<OcppSetupDialog>,
    pending_setup: Option<OcppSpec>,
    replacement: Option<Box<dyn ModuleView>>,
    meter_tick: u32,
    compact: bool,
}

impl OcppClientV16View {
    pub fn new(spec: OcppSpec) -> Self {
        let state = Arc::new(RwLock::new(CsState::default()));
        let (rows, config_rows) = {
            let s = state.read().unwrap();
            (s.rows(), s.config_rows())
        };
        let action_values: Vec<String> = V1_6::cs_actions().iter().map(|s| s.to_string()).collect();
        Self {
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
            focus: Pane::State,
            edit: None,
            config_edit: None,
            action_dialog: None,
            pending_send: None,
            setup_overlay: None,
            pending_setup: None,
            replacement: None,
            meter_tick: 0,
            compact: false,
            spec,
        }
    }

    /// Build a fresh inbound handler sharing this view's online flag, message log, and state.
    fn make_handler(&self) -> CsStateHandler {
        CsStateHandler::new(
            self.backend.online_handle(),
            self.backend.messages_handle(),
            self.state.clone(),
        )
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

    /// Enqueue the focused action for sending, or open a JSON dialog when it needs more than state.
    fn trigger_action(&mut self) {
        let name = self.actions.state.get_value();
        if name.is_empty() {
            return;
        }
        if STATE_DRIVEN.contains(&name.as_str()) {
            let payload = self.state_payload(&name);
            self.pending_send = Some((name, payload));
        } else {
            // Prefill a JSON editor with the Default-derived template.
            let template = V1_6::default_action(&name)
                .and_then(|a| V1_6::encode_action(&a).ok())
                .map(|v| serde_json::to_string_pretty(&v).unwrap_or_default())
                .unwrap_or_else(|| "{}".to_string());
            let mut field = editable_code();
            field.state.set_content(&template);
            self.action_dialog = Some((name, field));
        }
    }

    /// Build the request payload for a state-driven action from current state.
    fn state_payload(&self, name: &str) -> serde_json::Value {
        let s = self.state.read().unwrap();
        match name {
            "Authorize" => serde_json::json!({ "idTag": s.rfid }),
            "BootNotification" => serde_json::json!({
                "chargePointModel": s.model,
                "chargePointVendor": s.vendor,
            }),
            "Heartbeat" => serde_json::json!({}),
            "MeterValues" => serde_json::json!({
                "connectorId": s.connector_id,
                "meterValue": s.meter_value_json(),
            }),
            "StartTransaction" => serde_json::json!({
                "connectorId": s.connector_id,
                "idTag": s.rfid,
                "meterStart": s.meter_wh(),
                "timestamp": rfc3339_now(),
            }),
            "StopTransaction" => serde_json::json!({
                "transactionId": s.transaction_id.unwrap_or_default(),
                "meterStop": s.meter_wh(),
                "timestamp": rfc3339_now(),
                "idTag": s.rfid,
            }),
            "StatusNotification" => serde_json::json!({
                "connectorId": s.connector_id,
                "errorCode": "NoError",
                "status": s.status,
            }),
            _ => serde_json::json!({}),
        }
    }

    /// After a successful send, apply action-specific side effects to state.
    fn post_send(&mut self, name: &str, response: &serde_json::Value) {
        let mut s = self.state.write().unwrap();
        match name {
            "StartTransaction" => {
                s.transaction_id = response["transactionId"].as_i64();
                s.status = "Charging".to_string();
                s.session_energy = 0.0;
                self.meter_tick = 0;
            }
            "StopTransaction" => {
                s.transaction_id = None;
                s.status = "Available".to_string();
            }
            _ => {}
        }
    }

    fn focus_next(&mut self) {
        self.focus = match self.focus {
            Pane::State => Pane::Actions,
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
            Pane::Actions => Pane::State,
            Pane::ConfigTable => Pane::Actions,
            Pane::ConfigKey => Pane::ConfigTable,
            Pane::ConfigValue => Pane::ConfigKey,
            Pane::Messages => Pane::ConfigValue,
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

    /// Open the config-key editor for the selected config-table row.
    fn open_config_edit(&mut self) {
        let Some(row) = self.config_table.state.table_state().selected() else {
            return;
        };
        let s = self.state.read().unwrap();
        if let Some(current) = s.config.get(row) {
            self.config_edit = Some(ConfigEditDialog::new(row, current));
        }
    }

    /// Write the open config editor back into state.
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
            EditField::TotalEnergy => EditOverlay::Number {
                field,
                input: number(s.total_energy),
            },
            EditField::SessionEnergy => EditOverlay::Number {
                field,
                input: number(s.session_energy),
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
                    EditField::ConnectorId => s.connector_id = value as i64,
                    EditField::Voltage => s.voltage = value,
                    EditField::Current(i) => s.current[i] = value,
                    EditField::Power => s.power = value,
                    EditField::TotalEnergy => s.total_energy = value,
                    EditField::SessionEnergy => s.session_energy = value,
                    _ => {}
                }
            }
            EditOverlay::Text { field, input } => {
                let value = input.state.input().trim().to_string();
                match field {
                    EditField::Rfid => s.rfid = value,
                    EditField::Model => s.model = value,
                    EditField::Vendor => s.vendor = value,
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

    /// Decode + send a (name, payload), recording it; returns the response JSON on success.
    async fn send_payload(&mut self, name: &str, payload: serde_json::Value) {
        match V1_6::decode_call(name, payload) {
            Ok(action) => match self.backend.send(action).await {
                Ok(response) => self.post_send(name, &response),
                Err(e) => self.log.write().await.write(&format!("{name} failed: {e}")),
            },
            Err(e) => self
                .log
                .write()
                .await
                .write(&format!("{name} invalid payload: {e}")),
        }
    }
}

impl ModuleView for OcppClientV16View {
    fn name(&self) -> String {
        self.spec.name.clone()
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let buf = frame.buffer_mut();
        let [body, status_area] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
        let [left, right] =
            Layout::horizontal([Constraint::Length(66), Constraint::Min(1)]).areas(body);
        let [left_top, left_bottom] =
            Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(left);
        // Lower-left: config block (upper half) over the action list (lower half).
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

        // Config-key edit dialog, centered.
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
    }

    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        if let Some(setup) = self.setup_overlay.as_mut() {
            match (modifiers, code) {
                (KeyModifiers::NONE, KeyCode::Esc) => self.setup_overlay = None,
                (KeyModifiers::NONE, KeyCode::Enter) => {
                    if let Ok(spec) = setup.resolve() {
                        self.setup_overlay = None;
                        self.pending_setup = Some(spec);
                    }
                }
                (KeyModifiers::NONE, KeyCode::Tab) => setup.focus_next(),
                (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => {
                    setup.focus_previous()
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
                    // Keep the dialog open on invalid JSON.
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
                    Pane::ConfigTable => self.open_config_edit(),
                    Pane::ConfigKey | Pane::ConfigValue => self.add_config_key(),
                    Pane::Actions => self.trigger_action(),
                    Pane::Messages => {}
                }
                EventResult::Consumed
            }
            // Space activates list/table panes, but must type into the text inputs.
            (KeyModifiers::NONE, KeyCode::Char(' '))
                if !matches!(self.focus, Pane::ConfigKey | Pane::ConfigValue) =>
            {
                match self.focus {
                    Pane::State => self.open_edit(),
                    Pane::ConfigTable => self.open_config_edit(),
                    Pane::Actions => self.trigger_action(),
                    _ => {}
                }
                EventResult::Consumed
            }
            _ => match self.focus {
                Pane::State => self.state_table.state.handle_events(modifiers, code),
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
            // Apply an `:edit` that changed the spec.
            if let Some(spec) = self.pending_setup.take() {
                if spec.role == OcppRole::Server {
                    let _ = self.backend.stop().await;
                    self.replacement = Some(Box::new(OcppServerView::new(spec)));
                    return;
                }
                if spec.version != self.spec.version {
                    // Switching version means switching view type; the v2.0.1 view is a later task.
                    self.log
                        .write()
                        .await
                        .write("Version switch not yet supported in the 1.6 view");
                } else {
                    let was_online = self.backend.is_online();
                    let _ = self.backend.stop().await;
                    self.spec = spec.clone();
                    self.backend = OcppClient::new(spec);
                    self.log.write().await.write("Settings updated");
                    if was_online {
                        let handler = self.make_handler();
                        let _ = self.backend.start(handler).await;
                    }
                }
            }

            // Send a queued action.
            if let Some((name, payload)) = self.pending_send.take() {
                self.send_payload(&name, payload).await;
            }

            // While a transaction is active, report MeterValues periodically (~every 5s).
            let tx_active = self.state.read().unwrap().transaction_id.is_some();
            if tx_active {
                self.meter_tick = self.meter_tick.wrapping_add(1);
                if self.meter_tick.is_multiple_of(50) {
                    let payload = self.state_payload("MeterValues");
                    self.send_payload("MeterValues", payload).await;
                }
            }

            // Refresh tables from backend + state.
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
                self.setup_overlay = Some(OcppSetupDialog::edit(&self.spec));
                Box::pin(std::future::ready(CommandResult::Handled(None)))
            }
            "compact" => {
                self.set_compact(!self.compact);
                Box::pin(std::future::ready(CommandResult::Handled(None)))
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
        let mut v = serde_json::to_value(&self.spec).ok()?;
        v.as_object_mut()?.insert("type".into(), "ocpp".into());
        Some(v)
    }

    fn take_replacement(&mut self) -> Option<Box<dyn ModuleView>> {
        self.replacement.take()
    }
}

static OCPP_CLIENT_COMMANDS: [CommandDescriptor; 5] = [
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
            .title(Some("Config".into()))
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
