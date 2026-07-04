//! Shared OCPP 2.x charging-station bindings, used by both 2.0.1 and 2.1.
//!
//! 2.1 is a strict superset of 2.0.1 and the simulator answers the same core Calls the same way, so
//! the `ClientVersion` body lives here once as plain free functions and each version's
//! `impl ClientVersion for V…` (in `v2_0_1/version.rs` / `v2_1/version.rs`) delegates to them —
//! only the inbound handler type and the action-spec module actually differ per version, and those
//! two seams stay in each version's own `impl` block. Both versions share the one
//! [`crate::module::ocpp::client::v2_0_1::state::CsState`].
//!
//! The inbound (CSMS→CS) handler itself (`CsStateHandler`) is *not* shared here: it builds
//! strongly-typed responses (`GetVariablesResponse`, `ResetResponse`, …) from the version's own
//! `rust_ocpp` module, so its concrete type differs per version even though the decision logic is
//! identical. It is defined once per version in `v2_0_1/handler.rs` and `v2_1/handler.rs`; only the
//! version-independent helpers it calls (`unknown_evse`, `inbound_scope`, …) live here.

use crate::module::ocpp::client::backend::{boot_interval, rfc3339_now};
use crate::module::ocpp::client::config::ConfigKey;
use crate::module::ocpp::client::v2_0_1::state::CsState;
use crate::module::ocpp::client::view::{
    ClientState, EditField, EditKind, EditOverlay, NvRowData, PHASE_CHOICES, ResolvedEdit, choice,
    number, parse_id, text_input,
};
use crate::module::ocpp::config::device::ConnectorRef;
use crate::module::ocpp::scope::{Scope, evse_id};
use ferrowl_lua::module::ValueType;

/// Clear the per-purpose charge limit a ClearChargingProfile targets: the field matching `purpose`,
/// or every per-purpose limit when no purpose criterion is given. An unknown purpose clears nothing.
pub(crate) fn clear_limit_by_purpose(
    c: &mut crate::module::ocpp::client::v2_0_1::state::ConnectorState,
    purpose: Option<&str>,
) {
    match purpose {
        Some("TxProfile") => c.limit = None,
        Some("TxDefaultProfile") => c.default_limit = None,
        Some("ChargingStationMaxProfile") => c.max_limit = None,
        Some("ChargingStationExternalConstraints") => c.external_limit = None,
        Some(_) => {}
        None => {
            c.limit = None;
            c.default_limit = None;
            c.max_limit = None;
            c.external_limit = None;
        }
    }
}

/// The EVSE id an inbound Call targets, from a nested `evse.id` or a top-level `evseId`. A bare
/// `connectorId` is ignored: in 2.0.1 messages are addressed by EVSE only.
pub(crate) fn inbound_evse(request: &serde_json::Value) -> Option<i64> {
    evse_id(request)
}

/// Scope an inbound CSMS→CS Call belongs to, for the message log: keyed by EVSE id (connector kept
/// `None`), or CS-level when no EVSE is addressed.
pub(crate) fn inbound_scope(request: &serde_json::Value) -> Scope {
    match inbound_evse(request) {
        Some(e) => Scope::evse(e, None),
        None => Scope::CS,
    }
}

/// An addressed EVSE id this charging station does not have, if any. EVSE `0` is the charge point
/// itself and is always valid; an absent EVSE is CS-level.
pub(crate) fn unknown_evse(request: &serde_json::Value, state: &CsState) -> Option<i64> {
    let e = inbound_evse(request)?;
    if e == 0 || state.connectors.iter().any(|c| c.evse_id == e) {
        None
    } else {
        Some(e)
    }
}

// ---- Shared concrete `ClientState` over `CsState` (defined once; both versions reuse it). ----

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

// ---- Shared `ClientVersion` body (both 2.0.1 and 2.1's `impl` blocks delegate to these). ----

const STATUS_CHOICES: [&str; 5] = [
    "Available",
    "Occupied",
    "Reserved",
    "Unavailable",
    "Faulted",
];

/// State-driven real actions: built straight from state, no dialog.
pub(crate) const STATE_DRIVEN: [&str; 5] = [
    "Authorize",
    "BootNotification",
    "Heartbeat",
    "MeterValues",
    "StatusNotification",
];

pub(crate) fn config_title() -> &'static str {
    "Variables"
}

pub(crate) fn add_connector_placeholder() -> &'static str {
    "Add evse/connector"
}

pub(crate) fn has_tx_shortcuts() -> bool {
    true
}

pub(crate) fn scope_of(s: &CsState, idx: usize) -> Scope {
    Scope::evse(s.connectors[idx].evse_id, None)
}

/// Resolve the connector index targeted by `scope` (the connector on its EVSE, else the first).
pub(crate) fn connector_index(s: &CsState, scope: Scope) -> Option<usize> {
    scope
        .evse
        .and_then(|e| s.connectors.iter().position(|c| c.evse_id == e))
        .or((!s.connectors.is_empty()).then_some(0))
}

pub(crate) fn connector_index_for_state(s: &CsState, scope: Scope) -> Option<usize> {
    scope
        .evse
        .and_then(|e| s.connectors.iter().position(|c| c.evse_id == e))
}

pub(crate) fn add_connector(s: &mut CsState, raw: &str) -> Option<i64> {
    let (evse, connector) = match raw.split_once('/') {
        Some((e, c)) => (parse_id(e).unwrap_or(1), parse_id(c)),
        None => (1, parse_id(raw)),
    };
    let connector = connector?;
    s.add_connector(evse, connector).then_some(connector)
}

pub(crate) fn seed_connector(s: &mut CsState, c: &ConnectorRef) {
    s.add_connector(c.evse.unwrap_or(1), c.connector);
}

pub(crate) fn connector_ref(s: &CsState, idx: usize) -> ConnectorRef {
    let c = &s.connectors[idx];
    ConnectorRef {
        evse: Some(c.evse_id),
        connector: c.connector_id,
    }
}

/// Map a connector state-table row (see `ConnectorState::rows`). Charge Limit (row 15) is
/// read-only.
pub(crate) fn conn_edit_field(row: usize) -> Option<EditField> {
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

pub(crate) fn edit_kind(s: &CsState, scope: Scope, cs: bool, field: EditField) -> Option<EditKind> {
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
        EditField::Frequency => EditKind::Number(number(conn.map(|c| c.frequency).unwrap_or(0.0))),
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
        EditField::Rfid => EditKind::Text(text_input(conn.map(|c| c.rfid.as_str()).unwrap_or(""))),
        EditField::Model => EditKind::Text(text_input(&s.model)),
        EditField::Vendor => EditKind::Text(text_input(&s.vendor)),
        EditField::FirmwareVersion => EditKind::Text(text_input(&s.firmware_version)),
        EditField::SerialNumber => EditKind::Text(text_input(&s.serial_number)),
    })
}

pub(crate) fn apply_edit(s: &mut CsState, edit: &EditOverlay, value: ResolvedEdit) {
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

pub(crate) fn state_payload(s: &CsState, name: &str, scope: Scope) -> serde_json::Value {
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
pub(crate) fn start_event(s: &mut CsState, scope: Scope) -> serde_json::Value {
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
pub(crate) fn stop_event(s: &mut CsState, scope: Scope) -> Option<serde_json::Value> {
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

pub(crate) fn apply_post_send(
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

pub(crate) fn rollback_tx(s: &mut CsState, scope: Scope, started_tx: Option<&str>) {
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

pub(crate) fn active_meter_scopes(s: &CsState) -> Vec<Scope> {
    s.connectors
        .iter()
        .filter(|c| c.transaction_id.is_some() && c.tx_confirmed)
        .map(|c| Scope::evse(c.evse_id, None))
        .collect()
}
