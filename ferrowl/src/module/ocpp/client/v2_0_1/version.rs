//! OCPP 2.0.1 binding for the generic charging-station view: the [`ClientState`] surface over
//! [`CsState`] plus the [`ClientVersion`] seams. 2.0.1 keys connectors by EVSE id, adds an EVSE-id
//! state row, a 5-state connector-status enum, and the `StartTransaction`/`StopTransaction`
//! *shortcut* buttons that emit a `TransactionEvent` (minting a local string tx id eagerly, carried
//! in the payload, confirmed or rolled back on the response).

use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::AtomicBool;

use ferrowl_lua::module::ValueType;
use ferrowl_ocpp::V2_0_1;

use crate::module::ocpp::action_dialog::ActionSpec;
use crate::module::ocpp::client::backend::{Messages, boot_interval, rfc3339_now};
use crate::module::ocpp::client::config::ConfigKey;
use crate::module::ocpp::client::v2_0_1::handler::CsStateHandler;
use crate::module::ocpp::client::v2_0_1::state::CsState;
use crate::module::ocpp::client::view::{
    ClientState, ClientVersion, EditField, EditKind, EditOverlay, NvRowData, PHASE_CHOICES,
    ResolvedEdit, choice, number, parse_id, text_input,
};
use crate::module::ocpp::config::device::ConnectorRef;
use crate::module::ocpp::scope::Scope;

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

/// Resolve the connector index targeted by `scope` (the connector on its EVSE, else the first).
fn connector_index(s: &CsState, scope: Scope) -> Option<usize> {
    scope
        .evse
        .and_then(|e| s.connectors.iter().position(|c| c.evse_id == e))
        .or((!s.connectors.is_empty()).then_some(0))
}

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

impl ClientVersion for V2_0_1 {
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
        "Variables"
    }

    fn add_connector_placeholder() -> &'static str {
        "Add evse/connector"
    }

    fn has_tx_shortcuts() -> bool {
        true
    }

    fn action_spec(name: &str) -> Option<ActionSpec> {
        crate::module::ocpp::spec::v2_0_1::action_spec(name)
    }

    fn json_actions() -> &'static [&'static str] {
        crate::module::ocpp::spec::v2_0_1::json_actions()
    }

    fn scope_of(s: &CsState, idx: usize) -> Scope {
        Scope::evse(s.connectors[idx].evse_id, None)
    }

    fn connector_index(s: &CsState, scope: Scope) -> Option<usize> {
        connector_index(s, scope)
    }

    fn connector_index_for_state(s: &CsState, scope: Scope) -> Option<usize> {
        scope
            .evse
            .and_then(|e| s.connectors.iter().position(|c| c.evse_id == e))
    }

    fn add_connector(s: &mut CsState, raw: &str) -> Option<i64> {
        let (evse, connector) = match raw.split_once('/') {
            Some((e, c)) => (parse_id(e).unwrap_or(1), parse_id(c)),
            None => (1, parse_id(raw)),
        };
        let connector = connector?;
        s.add_connector(evse, connector).then_some(connector)
    }

    fn seed_connector(s: &mut CsState, c: &ConnectorRef) {
        s.add_connector(c.evse.unwrap_or(1), c.connector);
    }

    fn connector_ref(s: &CsState, idx: usize) -> ConnectorRef {
        let c = &s.connectors[idx];
        ConnectorRef {
            evse: Some(c.evse_id),
            connector: c.connector_id,
        }
    }

    /// Map a connector state-table row (see `ConnectorState::rows`). Charge Limit (row 15) is
    /// read-only.
    fn conn_edit_field(row: usize) -> Option<EditField> {
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

    fn edit_kind(s: &CsState, scope: Scope, cs: bool, field: EditField) -> Option<EditKind> {
        let evse = if cs { None } else { scope.evse };
        let conn = evse
            .and_then(|e| s.connector_by_evse(e))
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
        })
    }

    fn apply_edit(s: &mut CsState, edit: &EditOverlay, value: ResolvedEdit) {
        // Resolve the targeted connector for connector-level fields.
        let conn_idx = edit
            .scope
            .evse
            .and_then(|e| s.connectors.iter().position(|c| c.evse_id == e))
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

    /// Build a `TransactionEvent(Started)` for the connector resolved from `scope`, minting a tx id.
    fn start_event(s: &mut CsState, scope: Scope) -> serde_json::Value {
        let idx = connector_index(s, scope);
        let Some(i) = idx else {
            return serde_json::json!({});
        };
        let tx = s.connectors[i].start_tx();
        let seq = s.connectors[i].next_seq();
        s.connectors[i].status = "Occupied".to_string();
        s.connectors[i].session_energy = 0.0;
        let c = &s.connectors[i];
        serde_json::json!({
            "eventType": "Started",
            "timestamp": rfc3339_now(),
            "triggerReason": "Authorized",
            "seqNo": seq,
            "transactionInfo": { "transactionId": tx },
            "idToken": { "idToken": c.rfid, "type": "Central" },
            "evse": { "id": c.evse_id, "connectorId": c.connector_id },
        })
    }

    /// Build a `TransactionEvent(Ended)` for the connector resolved from `scope`, or `None` if idle.
    fn stop_event(s: &mut CsState, scope: Scope) -> Option<serde_json::Value> {
        let i = connector_index(s, scope)?;
        let tx = s.connectors[i].transaction_id.clone()?;
        let seq = s.connectors[i].next_seq();
        s.connectors[i].status = "Available".to_string();
        s.connectors[i].transaction_id = None;
        s.connectors[i].limit = None;
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

    fn apply_post_send(
        s: &mut CsState,
        name: &str,
        scope: Scope,
        started_tx: Option<&str>,
        response: &serde_json::Value,
    ) {
        if name == "BootNotification" {
            s.heartbeat_interval_secs = boot_interval(response);
        }
        if let Some(tx_id) = started_tx
            && let Some(c) = scope.evse.and_then(|e| s.connector_mut_by_evse(e))
            && c.transaction_id.as_deref() == Some(tx_id)
        {
            c.tx_confirmed = true;
        }
    }

    fn rollback_tx(s: &mut CsState, scope: Scope, started_tx: Option<&str>) {
        if let Some(tx_id) = started_tx
            && let Some(c) = scope.evse.and_then(|e| s.connector_mut_by_evse(e))
            && c.transaction_id.as_deref() == Some(tx_id)
        {
            c.transaction_id = None;
            c.limit = None;
            c.tx_confirmed = false;
            c.status = "Available".to_string();
        }
    }

    fn active_meter_scopes(s: &CsState) -> Vec<Scope> {
        s.connectors
            .iter()
            .filter(|c| c.transaction_id.is_some() && c.tx_confirmed)
            .map(|c| Scope::evse(c.evse_id, None))
            .collect()
    }
}
