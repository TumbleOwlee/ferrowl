//! OCPP 2.0.1 inbound (CSMS→CS) handler, answered from [`CsState`]. GetVariables is built from the
//! variable store, SetVariables writes it, Reset mutates state; every other inbound Call is
//! default-accepted (see `UNHANDLED.md`). Each inbound Call and our reply are recorded.

use std::future::Future;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

use ferrowl_ocpp::cs::CsActionHandler;
use ferrowl_ocpp::v2_0_1::datatypes::get_variable_result_type::GetVariableResultType;
use ferrowl_ocpp::v2_0_1::datatypes::set_variable_result_type::SetVariableResultType;
use ferrowl_ocpp::v2_0_1::enumerations::get_variable_status_enum_type::GetVariableStatusEnumType;
use ferrowl_ocpp::v2_0_1::enumerations::reset_status_enum_type::ResetStatusEnumType;
use ferrowl_ocpp::v2_0_1::enumerations::set_variable_status_enum_type::SetVariableStatusEnumType;
use ferrowl_ocpp::v2_0_1::messages::get_variables::GetVariablesResponse;
use ferrowl_ocpp::v2_0_1::messages::reset::ResetResponse;
use ferrowl_ocpp::v2_0_1::messages::set_variables::SetVariablesResponse;
use ferrowl_ocpp::{Action201, CallError, CallErrorCode, Response201, V2_0_1, Version};

use crate::module::ocpp::client::backend::{Dir, Messages, OcppMessage, push_capped};
use crate::module::ocpp::client::config::ConfigKey;
use crate::module::ocpp::client::v2_0_1::state::CsState;
use crate::module::ocpp::scope::Scope;

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

    fn respond(&self, action: &Action201) -> (Result<Response201, CallError>, String) {
        match action {
            Action201::GetVariables(req) => {
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
                        }
                    })
                    .collect();
                (
                    Ok(Response201::GetVariables(GetVariablesResponse {
                        get_variable_result: results,
                    })),
                    "answered from variables".to_string(),
                )
            }
            Action201::SetVariables(req) => {
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
                        }
                    })
                    .collect();
                (
                    Ok(Response201::SetVariables(SetVariablesResponse {
                        set_variable_result: results,
                    })),
                    "variables updated".to_string(),
                )
            }
            Action201::Reset(_) => {
                let mut state = self.state.write().unwrap();
                for c in &mut state.connectors {
                    c.status = "Available".to_string();
                    c.transaction_id = None;
                    c.session_energy = 0.0;
                }
                (
                    Ok(Response201::Reset(ResetResponse {
                        status: ResetStatusEnumType::Accepted,
                        status_info: None,
                    })),
                    "state reset".to_string(),
                )
            }
            Action201::SetChargingProfile(_) => {
                let json = V2_0_1::encode_action(action).unwrap_or(serde_json::Value::Null);
                let schedule = &json["chargingProfile"]["chargingSchedule"][0];
                let period = &schedule["chargingSchedulePeriod"][0];
                let context = if let Some(limit) = period["limit"].as_f64() {
                    let unit = schedule["chargingRateUnit"]
                        .as_str()
                        .unwrap_or("A")
                        .to_string();
                    let evse = json["evseId"].as_i64();
                    let mut state = self.state.write().unwrap();
                    // Apply the limit to the connector on the targeted EVSE (fall back to the first).
                    let idx = evse
                        .and_then(|e| state.connectors.iter().position(|c| c.evse_id == e))
                        .or((!state.connectors.is_empty()).then_some(0));
                    if let Some(i) = idx {
                        state.connectors[i].limit = Some(limit);
                        state.connectors[i].limit_unit = unit.clone();
                    }
                    format!("limit {limit} {unit}")
                } else {
                    "no limit in profile".to_string()
                };
                let resp = V2_0_1::default_response("SetChargingProfile")
                    .expect("SetChargingProfile is a known action");
                (Ok(resp), context)
            }
            Action201::ReserveNow(_) => {
                let json = V2_0_1::encode_action(action).unwrap_or(serde_json::Value::Null);
                let tag = json["idToken"]["idToken"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                self.state.write().unwrap().reserved_rfid = Some(tag.clone());
                let resp =
                    V2_0_1::default_response("ReserveNow").expect("ReserveNow is a known action");
                (Ok(resp), format!("reserved for {tag}"))
            }
            Action201::CancelReservation(_) => {
                self.state.write().unwrap().reserved_rfid = None;
                let resp = V2_0_1::default_response("CancelReservation")
                    .expect("CancelReservation is a known action");
                (Ok(resp), "reservation cancelled".to_string())
            }
            other => {
                let name = V2_0_1::action_name(other);
                match V2_0_1::default_response(name) {
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

impl CsActionHandler<V2_0_1> for CsStateHandler {
    fn handle_call(
        &self,
        action: Action201,
    ) -> impl Future<Output = Result<Response201, CallError>> + Send {
        let name = V2_0_1::action_name(&action).to_string();
        let request = V2_0_1::encode_action(&action).unwrap_or(serde_json::Value::Null);
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
            Ok(resp) => V2_0_1::encode_response(resp).unwrap_or(serde_json::Value::Null),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ut_unknown_evse_rejected() {
        let mut s = CsState::default();
        s.connectors.clear();
        s.add_connector(1, 1);
        // A present EVSE (nested or top-level), the CS component (0) and CS-level Calls are accepted.
        assert_eq!(unknown_evse(&json!({ "evseId": 1 }), &s), None);
        assert_eq!(unknown_evse(&json!({ "evse": { "id": 1 } }), &s), None);
        assert_eq!(unknown_evse(&json!({ "evseId": 0 }), &s), None);
        assert_eq!(unknown_evse(&json!({}), &s), None);
        // An unknown EVSE id is reported for rejection.
        assert_eq!(unknown_evse(&json!({ "evseId": 9 }), &s), Some(9));
    }

    #[test]
    fn ut_inbound_scope_keyed_by_evse_only() {
        // A nested connectorId is ignored: the scope is keyed by EVSE id with connector `None`.
        assert_eq!(
            inbound_scope(&json!({ "evse": { "id": 2, "connectorId": 5 } })),
            Scope::evse(2, None)
        );
        assert_eq!(inbound_scope(&json!({ "evseId": 3 })), Scope::evse(3, None));
        assert_eq!(inbound_scope(&json!({})), Scope::CS);
    }

    #[test]
    fn ut_write_arm_action_does_not_deadlock() {
        use std::sync::atomic::AtomicBool;
        use std::sync::mpsc;
        use std::time::Duration;

        // Reset hits the `None` (accept) arm and takes a write lock in `respond()`. If the inbound
        // read guard is still held there, the std RwLock self-deadlocks. Run on a thread and bound
        // the wait so a regression fails the test instead of hanging CI.
        let handler = CsStateHandler::new(
            Arc::new(AtomicBool::new(false)),
            Arc::new(tokio::sync::RwLock::new(Vec::<OcppMessage>::new())),
            Arc::new(RwLock::new(CsState::default())),
        );
        let action = V2_0_1::default_action("Reset").expect("Reset is a known action");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = handler.handle_call(action);
            let _ = tx.send(());
        });
        assert!(
            rx.recv_timeout(Duration::from_secs(5)).is_ok(),
            "handle_call deadlocked on a write-arm inbound action"
        );
    }
}
