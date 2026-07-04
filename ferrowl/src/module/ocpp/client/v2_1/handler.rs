//! OCPP 2.1 inbound (CSMS→CS) handler: the same decision logic as 2.0.1's handler
//! (`v2_0_1::handler`), but built from `rust_ocpp::v2_1` typed responses, so the two are separate
//! (non-generic) impls; the version-independent bits (`unknown_evse`, `inbound_scope`, …) are
//! shared via [`v2_common`](crate::module::ocpp::client::v2_common).

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::RwLock;

use ferrowl_ocpp::cs::CsActionHandler;
use ferrowl_ocpp::v2_1::datatypes::get_variable_result::GetVariableResultType;
use ferrowl_ocpp::v2_1::datatypes::set_variable_result::SetVariableResultType;
use ferrowl_ocpp::v2_1::enumerations::charging_profile_status::ChargingProfileStatusEnumType;
use ferrowl_ocpp::v2_1::enumerations::get_variable_status::GetVariableStatusEnumType;
use ferrowl_ocpp::v2_1::enumerations::reset_status::ResetStatusEnumType;
use ferrowl_ocpp::v2_1::enumerations::set_variable_status::SetVariableStatusEnumType;
use ferrowl_ocpp::v2_1::messages::get_variables::GetVariablesResponse;
use ferrowl_ocpp::v2_1::messages::reset::ResetResponse;
use ferrowl_ocpp::v2_1::messages::set_charging_profile::SetChargingProfileResponse;
use ferrowl_ocpp::v2_1::messages::set_variables::SetVariablesResponse;
use ferrowl_ocpp::{Action21, CallError, CallErrorCode, Response21, V2_1, Version};

use crate::module::ocpp::client::backend::{Dir, Messages, OcppMessage, push_capped};
use crate::module::ocpp::client::config::ConfigKey;
use crate::module::ocpp::client::v2_0_1::state::CsState;
use crate::module::ocpp::client::v2_common::{clear_limit_by_purpose, inbound_scope, unknown_evse};
use crate::module::ocpp::lock::HasState;

/// Inbound handler for an OCPP 2.1 charging station, backed by shared [`CsState`].
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
}

impl HasState for CsStateHandler {
    type State = CsState;

    fn state(&self) -> &Arc<RwLock<CsState>> {
        &self.state
    }
}

impl CsStateHandler {
    fn respond(&self, action: &Action21) -> (Result<Response21, CallError>, String) {
        match action {
            Action21::GetVariables(req) => self.with_state(|state| {
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
                    Ok(Response21::GetVariables(Box::new(GetVariablesResponse {
                        get_variable_result: results,
                        custom_data: None,
                    }))),
                    "answered from variables".to_string(),
                )
            }),
            Action21::SetVariables(req) => self.with_state_mut(|state| {
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
                    Ok(Response21::SetVariables(Box::new(SetVariablesResponse {
                        set_variable_result: results,
                        custom_data: None,
                    }))),
                    "variables updated".to_string(),
                )
            }),
            Action21::Reset(_) => self.with_state_mut(|state| {
                for c in &mut state.connectors {
                    c.status = "Available".to_string();
                    c.transaction_id = None;
                    c.session_energy = 0.0;
                }
                (
                    Ok(Response21::Reset(Box::new(ResetResponse {
                        status: ResetStatusEnumType::Accepted,
                        status_info: None,
                        custom_data: None,
                    }))),
                    "state reset".to_string(),
                )
            }),
            Action21::SetChargingProfile(_) => {
                let json = V2_1::encode_action(action).unwrap_or(serde_json::Value::Null);
                let profile = &json["chargingProfile"];
                let stack = profile["stackLevel"].as_i64().unwrap_or(0);
                let purpose = profile["chargingProfilePurpose"]
                    .as_str()
                    .unwrap_or("TxProfile")
                    .to_string();
                let schedule = &profile["chargingSchedule"][0];
                let period = &schedule["chargingSchedulePeriod"][0];
                let evse = json["evseId"].as_i64();
                self.with_state_mut(|state| {
                    // Reject profiles whose stack level exceeds ChargeProfileMaxStackLevel (when
                    // that key is configured with a numeric value); otherwise accept (no ceiling).
                    let max_stack = state
                        .config
                        .iter()
                        .find(|c| c.key == "ChargeProfileMaxStackLevel")
                        .and_then(|c| c.value.parse::<i64>().ok());
                    if let Some(max) = max_stack
                        && stack > max
                    {
                        let resp =
                            Response21::SetChargingProfile(Box::new(SetChargingProfileResponse {
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
                        let resp = V2_1::default_response("SetChargingProfile")
                            .expect("SetChargingProfile is a known action");
                        (Ok(resp), context)
                    }
                })
            }
            Action21::ReserveNow(_) => {
                let json = V2_1::encode_action(action).unwrap_or(serde_json::Value::Null);
                let tag = json["idToken"]["idToken"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let id = json["id"].as_i64();
                self.with_state_mut(|state| {
                    // An evseId-less (or evseId 0) ReserveNow reserves the station itself
                    // (CS-level); otherwise it targets the connector on that EVSE (entries are
                    // addressed by EVSE id).
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
                        V2_1::default_response("ReserveNow").expect("ReserveNow is a known action");
                    (Ok(resp), context)
                })
            }
            Action21::CancelReservation(_) => {
                let json = V2_1::encode_action(action).unwrap_or(serde_json::Value::Null);
                self.with_state_mut(|state| {
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
                    let resp = V2_1::default_response("CancelReservation")
                        .expect("CancelReservation is a known action");
                    (Ok(resp), context)
                })
            }
            Action21::ChangeAvailability(_) => {
                let json = V2_1::encode_action(action).unwrap_or(serde_json::Value::Null);
                let status = match json["operationalStatus"].as_str() {
                    Some("Inoperative") => "Unavailable",
                    _ => "Available",
                };
                self.with_state_mut(|state| {
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
                    let resp = V2_1::default_response("ChangeAvailability")
                        .expect("ChangeAvailability is a known action");
                    (Ok(resp), context)
                })
            }
            Action21::RequestStartTransaction(_) => {
                let json = V2_1::encode_action(action).unwrap_or(serde_json::Value::Null);
                self.with_state_mut(|state| {
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
                    let resp = V2_1::default_response("RequestStartTransaction")
                        .expect("RequestStartTransaction is a known action");
                    (Ok(resp), context)
                })
            }
            Action21::RequestStopTransaction(_) => {
                let json = V2_1::encode_action(action).unwrap_or(serde_json::Value::Null);
                let tx = json["transactionId"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                self.with_state_mut(|state| {
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
                    let resp = V2_1::default_response("RequestStopTransaction")
                        .expect("RequestStopTransaction is a known action");
                    (Ok(resp), context)
                })
            }
            Action21::ClearChargingProfile(_) => {
                let json = V2_1::encode_action(action).unwrap_or(serde_json::Value::Null);
                let criteria = &json["chargingProfileCriteria"];
                let purpose = criteria["chargingProfilePurpose"]
                    .as_str()
                    .map(str::to_owned);
                self.with_state_mut(|state| {
                    // evseId lives in the criteria; absent (or 0) clears every connector. The
                    // purpose criterion (when given) selects which per-purpose limit is erased;
                    // absent clears all.
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
                    let resp = V2_1::default_response("ClearChargingProfile")
                        .expect("ClearChargingProfile is a known action");
                    (Ok(resp), "charging profile cleared".to_string())
                })
            }
            Action21::UnlockConnector(_) => {
                let json = V2_1::encode_action(action).unwrap_or(serde_json::Value::Null);
                self.with_state_mut(|state| {
                    let context = match json["evseId"].as_i64().filter(|&e| e != 0) {
                        Some(e) => {
                            if let Some(c) = state.connector_mut_by_evse(e) {
                                c.status = "Available".to_string();
                            }
                            format!("evse {e} unlocked")
                        }
                        None => "no evse to unlock".to_string(),
                    };
                    let resp = V2_1::default_response("UnlockConnector")
                        .expect("UnlockConnector is a known action");
                    (Ok(resp), context)
                })
            }
            other => {
                let name = V2_1::action_name(other);
                match V2_1::default_response(name) {
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

impl CsActionHandler<V2_1> for CsStateHandler {
    fn handle_call(
        &self,
        action: Action21,
    ) -> impl Future<Output = Result<Response21, CallError>> + Send {
        let name = V2_1::action_name(&action).to_string();
        let request = V2_1::encode_action(&action).unwrap_or(serde_json::Value::Null);
        // Reject Calls targeting an EVSE this station does not have. `with_state` drops the read
        // guard before `respond()` runs (which takes its own write lock) — holding both deadlocks.
        let unknown = self.with_state(|s| unknown_evse(&request, s));
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
            Ok(resp) => V2_1::encode_response(resp).unwrap_or(serde_json::Value::Null),
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
