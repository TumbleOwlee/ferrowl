//! Shared OCPP 2.x charging-station bindings, generated for both 2.0.1 and 2.1.
//!
//! 2.1 is a strict superset of 2.0.1 and the simulator answers the same core Calls the same way,
//! so the inbound handler and the `ClientVersion` body live here once and are instantiated per
//! version. Each macro takes plain idents (the `ferrowl_ocpp` marker, the `rust_ocpp` module, the
//! Action/Response enums, the client submodule, the spec submodule) and builds full paths from
//! them — `:ident` fragments, unlike `:path`, may be followed by `::`. Both versions share the one
//! [`crate::module::ocpp::client::v2_0_1::state::CsState`].

/// Emit a version's `CsStateHandler` (struct + inbound `CsActionHandler` impl). `$marker` is the
/// `ferrowl_ocpp` marker (`V2_0_1`/`V2_1`), `$ocpp` the `rust_ocpp` module the four typed responses
/// are built from (`v2_0_1`/`v2_1`), and `$Action`/`$Response` the version's wire enums.
macro_rules! define_cs_state_handler {
    ($marker:ident, $ocpp:ident, $Action:ident, $Response:ident) => {
use std::future::Future;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

use ferrowl_ocpp::cs::CsActionHandler;
use ferrowl_ocpp::$ocpp::datatypes::get_variable_result::GetVariableResultType;
use ferrowl_ocpp::$ocpp::datatypes::set_variable_result::SetVariableResultType;
use ferrowl_ocpp::$ocpp::enumerations::charging_profile_status::ChargingProfileStatusEnumType;
use ferrowl_ocpp::$ocpp::enumerations::get_variable_status::GetVariableStatusEnumType;
use ferrowl_ocpp::$ocpp::enumerations::reset_status::ResetStatusEnumType;
use ferrowl_ocpp::$ocpp::enumerations::set_variable_status::SetVariableStatusEnumType;
use ferrowl_ocpp::$ocpp::messages::get_variables::GetVariablesResponse;
use ferrowl_ocpp::$ocpp::messages::reset::ResetResponse;
use ferrowl_ocpp::$ocpp::messages::set_charging_profile::SetChargingProfileResponse;
use ferrowl_ocpp::$ocpp::messages::set_variables::SetVariablesResponse;
use ferrowl_ocpp::{CallError, CallErrorCode, Version};

use crate::module::ocpp::client::backend::{Dir, Messages, OcppMessage, push_capped};
use crate::module::ocpp::client::config::ConfigKey;
use crate::module::ocpp::client::v2_0_1::state::CsState;
use crate::module::ocpp::scope::Scope;

/// Clear the per-purpose charge limit a ClearChargingProfile targets: the field matching `purpose`,
/// or every per-purpose limit when no purpose criterion is given. An unknown purpose clears nothing.
fn clear_limit_by_purpose(c: &mut crate::module::ocpp::client::v2_0_1::state::ConnectorState, purpose: Option<&str>) {
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
fn inbound_evse(request: &serde_json::Value) -> Option<i64> {
    request["evse"]["id"]
        .as_i64()
        .or_else(|| request["evseId"].as_i64())
}

/// Scope an inbound CSMS→CS Call belongs to, for the message log: keyed by EVSE id (connector kept
/// `None`), or CS-level when no EVSE is addressed.
fn inbound_scope(request: &serde_json::Value) -> Scope {
    match inbound_evse(request) {
        Some(e) => Scope::evse(e, None),
        None => Scope::CS,
    }
}

/// An addressed EVSE id this charging station does not have, if any. EVSE `0` is the charge point
/// itself and is always valid; an absent EVSE is CS-level.
fn unknown_evse(request: &serde_json::Value, state: &CsState) -> Option<i64> {
    let e = inbound_evse(request)?;
    if e == 0 || state.connectors.iter().any(|c| c.evse_id == e) {
        None
    } else {
        Some(e)
    }
}

/// Inbound handler for an OCPP 2.0.1 charging station, backed by shared [`CsState`].
pub struct CsStateHandler {
    online: Arc<AtomicBool>,
    messages: Messages,
    state: Arc<RwLock<CsState>>,
}

impl CsStateHandler {
    pub fn new(online: Arc<AtomicBool>, messages: Messages, state: Arc<RwLock<CsState>>) -> Self {
        Self {
            online,
            messages,
            state,
        }
    }

    fn respond(&self, action: &ferrowl_ocpp::$Action) -> (Result<ferrowl_ocpp::$Response, CallError>, String) {
        match action {
            ferrowl_ocpp::$Action::GetVariables(req) => {
                let state = self.state.read().unwrap();
                let results = req
                    .get_variable_data
                    .iter()
                    .map(|d| {
                        let found = state.config.iter().find(|c| c.key == d.variable.name);
                        GetVariableResultType {
                            attribute_status: match found {
                                Some(_) => GetVariableStatusEnumType::Accepted,
                                None => GetVariableStatusEnumType::UnknownVariable,
                            },
                            attribute_type: d.attribute_type.clone(),
                            attribute_value: found.map(|c| c.value.clone()),
                            component: d.component.clone(),
                            variable: d.variable.clone(),
                            attribute_status_info: None,
                            custom_data: None,
                        }
                    })
                    .collect();
                (
                    Ok(ferrowl_ocpp::$Response::GetVariables(Box::new(GetVariablesResponse {
                        get_variable_result: results,
                        custom_data: None,
                    }))),
                    "answered from variables".to_string(),
                )
            }
            ferrowl_ocpp::$Action::SetVariables(req) => {
                let mut state = self.state.write().unwrap();
                let results = req
                    .set_variable_data
                    .iter()
                    .map(|d| {
                        let status =
                            match state.config.iter_mut().find(|c| c.key == d.variable.name) {
                                Some(c) if c.readonly => SetVariableStatusEnumType::Rejected,
                                Some(c) => {
                                    c.value = d.attribute_value.clone();
                                    SetVariableStatusEnumType::Accepted
                                }
                                None => {
                                    state.config.push(ConfigKey {
                                        key: d.variable.name.clone(),
                                        value: d.attribute_value.clone(),
                                        readonly: false,
                                    });
                                    SetVariableStatusEnumType::Accepted
                                }
                            };
                        SetVariableResultType {
                            attribute_type: d.attribute_type.clone(),
                            attribute_status: status,
                            component: d.component.clone(),
                            variable: d.variable.clone(),
                            attribute_status_info: None,
                            custom_data: None,
                        }
                    })
                    .collect();
                (
                    Ok(ferrowl_ocpp::$Response::SetVariables(Box::new(SetVariablesResponse {
                        set_variable_result: results,
                        custom_data: None,
                    }))),
                    "variables updated".to_string(),
                )
            }
            ferrowl_ocpp::$Action::Reset(_) => {
                let mut state = self.state.write().unwrap();
                for c in &mut state.connectors {
                    c.status = "Available".to_string();
                    c.transaction_id = None;
                    c.session_energy = 0.0;
                }
                (
                    Ok(ferrowl_ocpp::$Response::Reset(Box::new(ResetResponse {
                        status: ResetStatusEnumType::Accepted,
                        status_info: None,
                        custom_data: None,
                    }))),
                    "state reset".to_string(),
                )
            }
            ferrowl_ocpp::$Action::SetChargingProfile(_) => {
                let json = ferrowl_ocpp::$marker::encode_action(action).unwrap_or(serde_json::Value::Null);
                let profile = &json["chargingProfile"];
                let stack = profile["stackLevel"].as_i64().unwrap_or(0);
                let purpose = profile["chargingProfilePurpose"]
                    .as_str()
                    .unwrap_or("TxProfile")
                    .to_string();
                let schedule = &profile["chargingSchedule"][0];
                let period = &schedule["chargingSchedulePeriod"][0];
                let evse = json["evseId"].as_i64();
                let mut state = self.state.write().unwrap();
                // Reject profiles whose stack level exceeds ChargeProfileMaxStackLevel (when that
                // key is configured with a numeric value); otherwise accept (no ceiling).
                let max_stack = state
                    .config
                    .iter()
                    .find(|c| c.key == "ChargeProfileMaxStackLevel")
                    .and_then(|c| c.value.parse::<i64>().ok());
                if let Some(max) = max_stack
                    && stack > max
                {
                    drop(state);
                    let resp =
                        ferrowl_ocpp::$Response::SetChargingProfile(Box::new(SetChargingProfileResponse {
                            status: ChargingProfileStatusEnumType::Rejected,
                            status_info: None,
                            custom_data: None,
                        }));
                    (
                        Ok(resp),
                        format!("rejected: stackLevel {stack} > max {max}"),
                    )
                } else {
                    // Apply the limit to the connector on the targeted EVSE (fall back to the
                    // first), routed by charging-profile purpose into the matching field.
                    let context = if let Some(limit) = period["limit"].as_f64() {
                        let unit = schedule["chargingRateUnit"]
                            .as_str()
                            .unwrap_or("A")
                            .to_string();
                        let idx = evse
                            .and_then(|e| state.connectors.iter().position(|c| c.evse_id == e))
                            .or((!state.connectors.is_empty()).then_some(0));
                        if let Some(i) = idx {
                            let c = &mut state.connectors[i];
                            match purpose.as_str() {
                                "TxDefaultProfile" => {
                                    c.default_limit = Some(limit);
                                    c.default_limit_unit = unit.clone();
                                }
                                "ChargingStationMaxProfile" => {
                                    c.max_limit = Some(limit);
                                    c.max_limit_unit = unit.clone();
                                }
                                "ChargingStationExternalConstraints" => {
                                    c.external_limit = Some(limit);
                                    c.external_limit_unit = unit.clone();
                                }
                                _ => {
                                    c.limit = Some(limit);
                                    c.limit_unit = unit.clone();
                                }
                            }
                        }
                        format!("{purpose} limit {limit} {unit}")
                    } else {
                        "no limit in profile".to_string()
                    };
                    let resp = ferrowl_ocpp::$marker::default_response("SetChargingProfile")
                        .expect("SetChargingProfile is a known action");
                    (Ok(resp), context)
                }
            }
            ferrowl_ocpp::$Action::ReserveNow(_) => {
                let json = ferrowl_ocpp::$marker::encode_action(action).unwrap_or(serde_json::Value::Null);
                let tag = json["idToken"]["idToken"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let id = json["id"].as_i64();
                let mut state = self.state.write().unwrap();
                // An evseId-less (or evseId 0) ReserveNow reserves the station itself (CS-level);
                // otherwise it targets the connector on that EVSE (entries are addressed by EVSE id).
                let context = match json["evseId"].as_i64().filter(|&e| e != 0) {
                    Some(e) => match state.connector_mut_by_evse(e) {
                        Some(c) => {
                            c.reserved_rfid = Some(tag.clone());
                            c.reservation_id = id;
                            format!("reserved evse {e} for {tag}")
                        }
                        None => format!("unknown evse {e}"),
                    },
                    None => {
                        state.reserved_rfid = Some(tag.clone());
                        state.reservation_id = id;
                        format!("reserved CS for {tag}")
                    }
                };
                let resp =
                    ferrowl_ocpp::$marker::default_response("ReserveNow").expect("ReserveNow is a known action");
                (Ok(resp), context)
            }
            ferrowl_ocpp::$Action::CancelReservation(_) => {
                let json = ferrowl_ocpp::$marker::encode_action(action).unwrap_or(serde_json::Value::Null);
                let mut state = self.state.write().unwrap();
                // Clear whichever level holds the matching reservationId.
                let context = match json["reservationId"].as_i64() {
                    Some(rid) if state.reservation_id == Some(rid) => {
                        state.reserved_rfid = None;
                        state.reservation_id = None;
                        format!("cancelled CS reservation {rid}")
                    }
                    Some(rid) => match state
                        .connectors
                        .iter_mut()
                        .find(|c| c.reservation_id == Some(rid))
                    {
                        Some(c) => {
                            c.reserved_rfid = None;
                            c.reservation_id = None;
                            format!("cancelled evse {} reservation {rid}", c.evse_id)
                        }
                        None => "no matching reservation".to_string(),
                    },
                    None => "no matching reservation".to_string(),
                };
                let resp = ferrowl_ocpp::$marker::default_response("CancelReservation")
                    .expect("CancelReservation is a known action");
                (Ok(resp), context)
            }
            ferrowl_ocpp::$Action::ChangeAvailability(_) => {
                let json = ferrowl_ocpp::$marker::encode_action(action).unwrap_or(serde_json::Value::Null);
                let status = match json["operationalStatus"].as_str() {
                    Some("Inoperative") => "Unavailable",
                    _ => "Available",
                };
                let mut state = self.state.write().unwrap();
                // An evseId-less (or evseId 0) ChangeAvailability targets the whole station.
                let context = match json["evse"]["id"].as_i64().filter(|&e| e != 0) {
                    Some(e) => {
                        if let Some(c) = state.connector_mut_by_evse(e) {
                            c.status = status.to_string();
                        }
                        format!("evse {e} -> {status}")
                    }
                    None => {
                        for c in &mut state.connectors {
                            c.status = status.to_string();
                        }
                        format!("all -> {status}")
                    }
                };
                let resp = ferrowl_ocpp::$marker::default_response("ChangeAvailability")
                    .expect("ChangeAvailability is a known action");
                (Ok(resp), context)
            }
            ferrowl_ocpp::$Action::RequestStartTransaction(_) => {
                let json = ferrowl_ocpp::$marker::encode_action(action).unwrap_or(serde_json::Value::Null);
                let mut state = self.state.write().unwrap();
                // Optional evseId; fall back to the first connector. Mint a transaction and charge.
                let idx = json["evseId"]
                    .as_i64()
                    .filter(|&e| e != 0)
                    .and_then(|e| state.connectors.iter().position(|c| c.evse_id == e))
                    .or((!state.connectors.is_empty()).then_some(0));
                let context = match idx {
                    Some(i) => {
                        let tx = state.connectors[i].start_tx();
                        state.connectors[i].status = "Charging".to_string();
                        format!("started tx {tx} on evse {}", state.connectors[i].evse_id)
                    }
                    None => "no connector to start".to_string(),
                };
                let resp = ferrowl_ocpp::$marker::default_response("RequestStartTransaction")
                    .expect("RequestStartTransaction is a known action");
                (Ok(resp), context)
            }
            ferrowl_ocpp::$Action::RequestStopTransaction(_) => {
                let json = ferrowl_ocpp::$marker::encode_action(action).unwrap_or(serde_json::Value::Null);
                let tx = json["transactionId"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let mut state = self.state.write().unwrap();
                let context = match state
                    .connectors
                    .iter_mut()
                    .find(|c| c.transaction_id.as_deref() == Some(tx.as_str()))
                {
                    Some(c) => {
                        c.transaction_id = None;
                        c.limit = None;
                        c.status = "Available".to_string();
                        format!("stopped tx {tx} on evse {}", c.evse_id)
                    }
                    None => format!("no active tx {tx}"),
                };
                let resp = ferrowl_ocpp::$marker::default_response("RequestStopTransaction")
                    .expect("RequestStopTransaction is a known action");
                (Ok(resp), context)
            }
            ferrowl_ocpp::$Action::ClearChargingProfile(_) => {
                let json = ferrowl_ocpp::$marker::encode_action(action).unwrap_or(serde_json::Value::Null);
                let criteria = &json["chargingProfileCriteria"];
                let purpose = criteria["chargingProfilePurpose"]
                    .as_str()
                    .map(str::to_owned);
                let mut state = self.state.write().unwrap();
                // evseId lives in the criteria; absent (or 0) clears every connector. The purpose
                // criterion (when given) selects which per-purpose limit is erased; absent clears all.
                match criteria["evseId"].as_i64().filter(|&e| e != 0) {
                    Some(e) => {
                        if let Some(c) = state.connector_mut_by_evse(e) {
                            clear_limit_by_purpose(c, purpose.as_deref());
                        }
                    }
                    None => {
                        for c in &mut state.connectors {
                            clear_limit_by_purpose(c, purpose.as_deref());
                        }
                    }
                }
                let resp = ferrowl_ocpp::$marker::default_response("ClearChargingProfile")
                    .expect("ClearChargingProfile is a known action");
                (Ok(resp), "charging profile cleared".to_string())
            }
            ferrowl_ocpp::$Action::UnlockConnector(_) => {
                let json = ferrowl_ocpp::$marker::encode_action(action).unwrap_or(serde_json::Value::Null);
                let mut state = self.state.write().unwrap();
                let context = match json["evseId"].as_i64().filter(|&e| e != 0) {
                    Some(e) => {
                        if let Some(c) = state.connector_mut_by_evse(e) {
                            c.status = "Available".to_string();
                        }
                        format!("evse {e} unlocked")
                    }
                    None => "no evse to unlock".to_string(),
                };
                let resp = ferrowl_ocpp::$marker::default_response("UnlockConnector")
                    .expect("UnlockConnector is a known action");
                (Ok(resp), context)
            }
            other => {
                let name = ferrowl_ocpp::$marker::action_name(other);
                match ferrowl_ocpp::$marker::default_response(name) {
                    Some(resp) => (Ok(resp), "default-accepted".to_string()),
                    None => (
                        Err(CallError::new(
                            CallErrorCode::NotImplemented,
                            "action not handled by the charging-station simulator",
                        )),
                        "not implemented".to_string(),
                    ),
                }
            }
        }
    }
}

impl CsActionHandler<ferrowl_ocpp::$marker> for CsStateHandler {
    fn handle_call(
        &self,
        action: ferrowl_ocpp::$Action,
    ) -> impl Future<Output = Result<ferrowl_ocpp::$Response, CallError>> + Send {
        let name = ferrowl_ocpp::$marker::action_name(&action).to_string();
        let request = ferrowl_ocpp::$marker::encode_action(&action).unwrap_or(serde_json::Value::Null);
        // Reject Calls targeting an EVSE this station does not have. Scope the read guard so it is
        // dropped before `respond()` (which takes a write lock) — holding both deadlocks.
        let unknown = unknown_evse(&request, &self.state.read().unwrap());
        let (result, context) = match unknown {
            Some(e) => (
                Err(CallError::new(
                    CallErrorCode::PropertyConstraintViolation,
                    "unknown evseId",
                )),
                format!("unknown evse {e}"),
            ),
            None => self.respond(&action),
        };
        let reply_payload = match &result {
            Ok(resp) => ferrowl_ocpp::$marker::encode_response(resp).unwrap_or(serde_json::Value::Null),
            Err(_) => serde_json::Value::Null,
        };
        let ok = result.is_ok();
        let scope = inbound_scope(&request);
        let messages = self.messages.clone();
        async move {
            let mut guard = messages.write().await;
            push_capped(
                &mut guard,
                OcppMessage::new_scoped(
                    scope,
                    Dir::In,
                    name.clone(),
                    request,
                    None,
                    "inbound call",
                ),
            );
            push_capped(
                &mut guard,
                OcppMessage::new_scoped(scope, Dir::Out, name, reply_payload, Some(ok), context),
            );
            drop(guard);
            result
        }
    }

    fn on_connected(&self) -> impl Future<Output = ()> + Send {
        let online = self.online.clone();
        async move {
            online.store(true, Ordering::Relaxed);
        }
    }

    fn on_disconnected(&self) -> impl Future<Output = ()> + Send {
        let online = self.online.clone();
        async move {
            online.store(false, Ordering::Relaxed);
        }
    }
}
    };
}

pub(crate) use define_cs_state_handler;

// ---- Shared concrete `ClientState` over `CsState` (defined once; both versions reuse it). ----

use crate::module::ocpp::client::config::ConfigKey;
use crate::module::ocpp::client::v2_0_1::state::CsState;
use crate::module::ocpp::client::view::{ClientState, NvRowData};
use ferrowl_lua::module::ValueType;

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

/// Emit the `ClientVersion` impl for marker `ferrowl_ocpp::$marker`, wiring the shared `CsState`
/// view to the handler in `client::$ver::handler` and the action specs in `spec::$specver`. The
/// body is identical for 2.0.1 and 2.1.
macro_rules! define_client_version {
    ($marker:ident, $ver:ident, $specver:ident) => {
        use std::sync::Arc;
        use std::sync::RwLock;
        use std::sync::atomic::AtomicBool;
        use crate::module::ocpp::action_dialog::ActionSpec;
        use crate::module::ocpp::client::backend::{Messages, boot_interval, rfc3339_now};
        use crate::module::ocpp::client::v2_0_1::state::CsState;
        use crate::module::ocpp::client::view::{
            ClientVersion, EditField, EditKind, EditOverlay, PHASE_CHOICES, ResolvedEdit, choice,
            number, parse_id, text_input,
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

impl ClientVersion for ferrowl_ocpp::$marker {
    type Cs = CsState;
    type Handler = crate::module::ocpp::client::$ver::handler::CsStateHandler;

    fn handler(
        online: Arc<AtomicBool>,
        messages: Messages,
        state: Arc<RwLock<CsState>>,
    ) -> crate::module::ocpp::client::$ver::handler::CsStateHandler {
        crate::module::ocpp::client::$ver::handler::CsStateHandler::new(online, messages, state)
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
        crate::module::ocpp::spec::$specver::action_spec(name)
    }

    fn json_actions() -> &'static [&'static str] {
        crate::module::ocpp::spec::$specver::json_actions()
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
    };
}

pub(crate) use define_client_version;
