//! OCPP 1.6 binding for the generic charging-station view: the [`ClientState`] surface over
//! [`CsState`] plus the [`ClientVersion`] seams (scope ctor, `EditField` row map, status choices,
//! the state-driven action set, the exact 1.6 request payloads, and the post-send transaction
//! bookkeeping). 1.6 has no transaction shortcuts — StartTransaction/StopTransaction are ordinary
//! state-driven actions whose side-effects land in `apply_post_send`.

use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::AtomicBool;

use ferrowl_lua::module::ValueType;
use ferrowl_ocpp::V1_6;

use crate::module::ocpp::action_dialog::ActionSpec;
use crate::module::ocpp::client::backend::{Messages, boot_interval, rfc3339_now};
use crate::module::ocpp::client::config::ConfigKey;
use crate::module::ocpp::client::v1_6::handler::CsStateHandler;
use crate::module::ocpp::client::v1_6::state::CsState;
use crate::module::ocpp::client::view::{
    ClientState, ClientVersion, EditField, EditKind, EditOverlay, NvRowData, PHASE_CHOICES,
    ResolvedEdit, choice, number, text_input,
};
use crate::module::ocpp::config::device::ConnectorRef;
use crate::module::ocpp::scope::Scope;

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

impl ClientState for CsState {
    fn connector_count(&self) -> usize {
        self.connectors.len()
    }
    fn clear_connectors(&mut self) {
        self.connectors.clear();
    }
    fn remove_connector_at(&mut self, idx: usize) {
        self.connectors.remove(idx);
    }
    fn connector_position(&self, connector_id: i64) -> Option<usize> {
        self.connectors
            .iter()
            .position(|c| c.connector_id == connector_id)
    }
    fn conn_get_field(&self, idx: usize, name: &str) -> Option<ValueType> {
        self.connectors.get(idx).and_then(|c| c.get_field(name))
    }
    fn cs_get_field_named(&self, name: &str) -> Option<ValueType> {
        self.cs_get_field(name)
    }
    fn cs_state_rows(&self) -> Vec<NvRowData> {
        CsState::cs_rows(self)
            .into_iter()
            .map(|r| NvRowData {
                name: r.name,
                unit: r.unit,
                value: r.value,
            })
            .collect()
    }
    fn conn_state_rows(&self, idx: usize) -> Vec<NvRowData> {
        self.connectors
            .get(idx)
            .map(|c| {
                c.rows()
                    .into_iter()
                    .map(|r| NvRowData {
                        name: r.name,
                        unit: r.unit,
                        value: r.value,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    fn config(&self) -> &[ConfigKey] {
        &self.config
    }
    fn config_mut(&mut self) -> &mut Vec<ConfigKey> {
        &mut self.config
    }
    fn heartbeat_interval_secs(&self) -> Option<u64> {
        self.heartbeat_interval_secs
    }
}

impl ClientVersion for V1_6 {
    type Cs = CsState;
    type Handler = CsStateHandler;

    fn handler(
        online: Arc<AtomicBool>,
        messages: Messages,
        state: Arc<RwLock<CsState>>,
    ) -> CsStateHandler {
        CsStateHandler::new(online, messages, state)
    }

    fn state_driven() -> &'static [&'static str] {
        &STATE_DRIVEN
    }

    fn config_title() -> &'static str {
        "Config"
    }

    fn add_connector_placeholder() -> &'static str {
        "Add connector id"
    }

    fn action_spec(name: &str) -> Option<ActionSpec> {
        crate::module::ocpp::spec::v1_6::action_spec(name)
    }

    fn json_actions() -> &'static [&'static str] {
        crate::module::ocpp::spec::v1_6::json_actions()
    }

    fn scope_of(s: &CsState, idx: usize) -> Scope {
        Scope::connector(s.connectors[idx].connector_id)
    }

    fn connector_index(s: &CsState, scope: Scope) -> Option<usize> {
        scope
            .connector
            .and_then(|id| s.connectors.iter().position(|c| c.connector_id == id))
            .or((!s.connectors.is_empty()).then_some(0))
    }

    fn connector_index_for_state(s: &CsState, scope: Scope) -> Option<usize> {
        scope
            .connector
            .and_then(|id| s.connectors.iter().position(|c| c.connector_id == id))
    }

    fn add_connector(s: &mut CsState, raw: &str) -> Option<i64> {
        let id = raw.trim().parse::<i64>().ok()?;
        s.add_connector(id).then_some(id)
    }

    fn seed_connector(s: &mut CsState, c: &ConnectorRef) {
        s.add_connector(c.connector);
    }

    fn connector_ref(s: &CsState, idx: usize) -> ConnectorRef {
        ConnectorRef {
            evse: None,
            connector: s.connectors[idx].connector_id,
        }
    }

    /// Map a connector state-table row (see [`ConnectorState::rows`]). Charge Limit (row 14) is
    /// read-only (set by the CSMS via SetChargingProfile).
    fn conn_edit_field(row: usize) -> Option<EditField> {
        Some(match row {
            0 => EditField::ConnectorId,
            1 => EditField::Phases,
            2 => EditField::Voltage,
            3 => EditField::Current(0),
            4 => EditField::Current(1),
            5 => EditField::Current(2),
            6 => EditField::Power,
            7 => EditField::Frequency,
            8 => EditField::TotalEnergy,
            9 => EditField::SessionEnergy,
            10 => EditField::Soc,
            11 => EditField::Temperature,
            12 => EditField::Status,
            13 => EditField::Rfid,
            _ => return None,
        })
    }

    fn edit_kind(s: &CsState, scope: Scope, _cs: bool, field: EditField) -> Option<EditKind> {
        let conn = scope
            .connector
            .and_then(|id| s.connector(id))
            .or_else(|| s.connectors.first());
        Some(match field {
            EditField::Phases => EditKind::Choice(choice(
                &PHASE_CHOICES,
                conn.map(|c| c.phases.as_str()).unwrap_or(""),
            )),
            EditField::Status => EditKind::Choice(choice(
                &STATUS_CHOICES,
                conn.map(|c| c.status.as_str()).unwrap_or(""),
            )),
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
            // 1.6 has no EVSE id field; the row map never produces it.
            EditField::EvseId => return None,
        })
    }

    fn apply_edit(s: &mut CsState, edit: &EditOverlay, value: ResolvedEdit) {
        // Resolve the targeted connector for connector-level fields.
        let conn_idx = edit
            .scope
            .connector
            .and_then(|id| s.connectors.iter().position(|c| c.connector_id == id))
            .or((!s.connectors.is_empty()).then_some(0));
        match value {
            ResolvedEdit::Choice(value) => {
                if let Some(i) = conn_idx {
                    let c = &mut s.connectors[i];
                    match edit.field {
                        EditField::Phases => c.phases = value,
                        EditField::Status => c.status = value,
                        _ => {}
                    }
                }
            }
            ResolvedEdit::Number(value) => {
                if let Some(i) = conn_idx {
                    let c = &mut s.connectors[i];
                    match edit.field {
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
            ResolvedEdit::Text(value) => match edit.field {
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
            },
        }
    }

    fn state_payload(s: &CsState, name: &str, scope: Scope) -> serde_json::Value {
        let conn = scope
            .connector
            .and_then(|id| s.connector(id))
            .or_else(|| s.connectors.first());
        let cid = conn.map(|c| c.connector_id).unwrap_or(1);
        let rfid = conn.map(|c| c.rfid.clone()).unwrap_or_default();
        match name {
            "Authorize" => serde_json::json!({ "idTag": rfid }),
            "BootNotification" => serde_json::json!({
                "chargePointModel": s.model,
                "chargePointVendor": s.vendor,
                "chargePointSerialNumber": s.serial_number,
                "firmwareVersion": s.firmware_version,
            }),
            "Heartbeat" => serde_json::json!({}),
            "MeterValues" => serde_json::json!({
                "connectorId": cid,
                "meterValue": conn.map(|c| c.meter_value_json()).unwrap_or(serde_json::json!([])),
            }),
            "StartTransaction" => serde_json::json!({
                "connectorId": cid,
                "idTag": rfid,
                "meterStart": conn.map(|c| c.meter_wh()).unwrap_or(0),
                "timestamp": rfc3339_now(),
            }),
            "StopTransaction" => serde_json::json!({
                "transactionId": conn.and_then(|c| c.transaction_id).unwrap_or_default(),
                "meterStop": conn.map(|c| c.meter_wh()).unwrap_or(0),
                "timestamp": rfc3339_now(),
                "idTag": rfid,
            }),
            "StatusNotification" => serde_json::json!({
                "connectorId": cid,
                "errorCode": "NoError",
                "status": conn.map(|c| c.status.clone()).unwrap_or_default(),
            }),
            _ => serde_json::json!({}),
        }
    }

    /// Apply a successful response's side-effects to the targeted connector / CS state.
    fn apply_post_send(
        s: &mut CsState,
        name: &str,
        scope: Scope,
        _started_tx: Option<&str>,
        response: &serde_json::Value,
    ) {
        if name == "BootNotification" {
            s.heartbeat_interval_secs = boot_interval(response);
            return;
        }
        let idx = scope
            .connector
            .and_then(|id| s.connectors.iter().position(|c| c.connector_id == id))
            .or((!s.connectors.is_empty()).then_some(0));
        let Some(i) = idx else { return };
        let c = &mut s.connectors[i];
        match name {
            "StartTransaction" => {
                c.transaction_id = response["transactionId"].as_i64();
                c.status = "Charging".to_string();
                c.session_energy = 0.0;
            }
            "StopTransaction" => {
                c.transaction_id = None;
                c.limit = None;
                c.status = "Available".to_string();
            }
            _ => {}
        }
    }

    fn active_meter_scopes(s: &CsState) -> Vec<Scope> {
        s.connectors
            .iter()
            .filter(|c| c.transaction_id.is_some())
            .map(|c| Scope::connector(c.connector_id))
            .collect()
    }

    fn track_meter_reset(s: &CsState, tx_was_active: &mut bool, meter_tick: &mut u32) {
        let any_active = s.connectors.iter().any(|c| c.transaction_id.is_some());
        if any_active && !*tx_was_active {
            *meter_tick = 0;
        }
        *tx_was_active = any_active;
    }
}
