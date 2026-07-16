//! OCPP 2.0.1 inbound (CSMS→CS) handler, answered from [`CsState`]. GetVariables is built from the
//! variable store, SetVariables writes it, Reset mutates state. EVSE-scoped Calls are simulated
//! against the connector on the targeted EVSE. The decision logic is identical to 2.1's handler
//! (`v2_1::handler`) but the typed responses are built from `rust_ocpp::v2_0_1`, so the two are
//! separate (non-generic) impls; the version-independent bits (`unknown_evse`, `inbound_scope`, …)
//! are shared via [`v2_common`](crate::module::ocpp::client::v2_common). Every other inbound Call is
//! default-accepted (see `UNHANDLED.md`). Each inbound Call and our reply are recorded.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::RwLock;

use ferrowl_ocpp::cs::CsActionHandler;
use ferrowl_ocpp::v2_0_1::datatypes::get_variable_result::GetVariableResultType;
use ferrowl_ocpp::v2_0_1::datatypes::set_variable_result::SetVariableResultType;
use ferrowl_ocpp::v2_0_1::enumerations::charging_profile_status::ChargingProfileStatusEnumType;
use ferrowl_ocpp::v2_0_1::enumerations::get_variable_status::GetVariableStatusEnumType;
use ferrowl_ocpp::v2_0_1::enumerations::reset_status::ResetStatusEnumType;
use ferrowl_ocpp::v2_0_1::enumerations::set_variable_status::SetVariableStatusEnumType;
use ferrowl_ocpp::v2_0_1::messages::get_variables::GetVariablesResponse;
use ferrowl_ocpp::v2_0_1::messages::reset::ResetResponse;
use ferrowl_ocpp::v2_0_1::messages::set_charging_profile::SetChargingProfileResponse;
use ferrowl_ocpp::v2_0_1::messages::set_variables::SetVariablesResponse;
use ferrowl_ocpp::{Action201, CallError, CallErrorCode, Response201, V2_0_1, Version};

use crate::module::ocpp::client::backend::{Dir, Messages, OcppMessage, push_capped};
use crate::module::ocpp::client::config::ConfigKey;
use crate::module::ocpp::client::v2_0_1::state::CsState;
use crate::module::ocpp::client::v2_common::{
    clear_limit_by_purpose, encode_action_or_log, inbound_scope, unknown_evse,
};
use crate::module::ocpp::lock::HasState;

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
}

impl HasState for CsStateHandler {
    type State = CsState;

    fn state(&self) -> &Arc<RwLock<CsState>> {
        &self.state
    }
}

impl CsStateHandler {
    fn respond(&self, action: &Action201) -> (Result<Response201, CallError>, String) {
        match action {
            Action201::GetVariables(req) => self.with_state(|state| {
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
                    Ok(Response201::GetVariables(Box::new(GetVariablesResponse {
                        get_variable_result: results,
                        custom_data: None,
                    }))),
                    "answered from variables".to_string(),
                )
            }),
            Action201::SetVariables(req) => self.with_state_mut(|state| {
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
                    Ok(Response201::SetVariables(Box::new(SetVariablesResponse {
                        set_variable_result: results,
                        custom_data: None,
                    }))),
                    "variables updated".to_string(),
                )
            }),
            Action201::Reset(_) => self.with_state_mut(|state| {
                for c in &mut state.connectors {
                    c.status = "Available".to_string();
                    c.transaction_id = None;
                    c.session_energy = 0.0;
                }
                (
                    Ok(Response201::Reset(Box::new(ResetResponse {
                        status: ResetStatusEnumType::Accepted,
                        status_info: None,
                        custom_data: None,
                    }))),
                    "state reset".to_string(),
                )
            }),
            Action201::SetChargingProfile(_) => {
                let json = encode_action_or_log::<V2_0_1>(action);
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
                            Response201::SetChargingProfile(Box::new(SetChargingProfileResponse {
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
                        let resp = V2_0_1::default_response("SetChargingProfile")
                            .expect("SetChargingProfile is a known action");
                        (Ok(resp), context)
                    }
                })
            }
            Action201::ReserveNow(_) => {
                let json = encode_action_or_log::<V2_0_1>(action);
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
                    let resp = V2_0_1::default_response("ReserveNow")
                        .expect("ReserveNow is a known action");
                    (Ok(resp), context)
                })
            }
            Action201::CancelReservation(_) => {
                let json = encode_action_or_log::<V2_0_1>(action);
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
                    let resp = V2_0_1::default_response("CancelReservation")
                        .expect("CancelReservation is a known action");
                    (Ok(resp), context)
                })
            }
            Action201::ChangeAvailability(_) => {
                let json = encode_action_or_log::<V2_0_1>(action);
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
                    let resp = V2_0_1::default_response("ChangeAvailability")
                        .expect("ChangeAvailability is a known action");
                    (Ok(resp), context)
                })
            }
            Action201::RequestStartTransaction(_) => {
                let json = encode_action_or_log::<V2_0_1>(action);
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
                    let resp = V2_0_1::default_response("RequestStartTransaction")
                        .expect("RequestStartTransaction is a known action");
                    (Ok(resp), context)
                })
            }
            Action201::RequestStopTransaction(_) => {
                let json = encode_action_or_log::<V2_0_1>(action);
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
                    let resp = V2_0_1::default_response("RequestStopTransaction")
                        .expect("RequestStopTransaction is a known action");
                    (Ok(resp), context)
                })
            }
            Action201::ClearChargingProfile(_) => {
                let json = encode_action_or_log::<V2_0_1>(action);
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
                    let resp = V2_0_1::default_response("ClearChargingProfile")
                        .expect("ClearChargingProfile is a known action");
                    (Ok(resp), "charging profile cleared".to_string())
                })
            }
            Action201::UnlockConnector(_) => {
                let json = encode_action_or_log::<V2_0_1>(action);
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
                    let resp = V2_0_1::default_response("UnlockConnector")
                        .expect("UnlockConnector is a known action");
                    (Ok(resp), context)
                })
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
    use crate::module::ocpp::client::backend::OcppMessage;
    use crate::module::ocpp::client::v2_0_1::state::CsState;
    use crate::module::ocpp::scope::Scope;
    use ferrowl_ocpp::{V2_0_1, Version};
    use serde_json::json;
    use std::sync::Arc;

    use parking_lot::RwLock;

    #[test]
    /// OC-R-063 — an inbound Call naming an EVSE this CS does not have is rejected with PropertyConstraintViolation.
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
    /// OC-R-078 — an inbound Call is tagged with the connector/EVSE scope it belongs to for recording.
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
            // Building the response is the synchronous part that deadlocked; dropping the future is fine.
            drop(handler.handle_call(action));
            let _ = tx.send(());
        });
        assert!(
            rx.recv_timeout(Duration::from_secs(5)).is_ok(),
            "handle_call deadlocked on a write-arm inbound action"
        );
    }

    fn handler_with(state: CsState) -> CsStateHandler {
        use std::sync::atomic::AtomicBool;
        CsStateHandler::new(
            Arc::new(AtomicBool::new(false)),
            Arc::new(tokio::sync::RwLock::new(Vec::<OcppMessage>::new())),
            Arc::new(RwLock::new(state)),
        )
    }

    /// Build an action, drive it through `respond`, and assert it was accepted.
    fn drive(h: &CsStateHandler, name: &str, payload: serde_json::Value) {
        let action = V2_0_1::decode_call(name, payload).expect("action decodes");
        assert!(h.respond(&action).0.is_ok(), "{name} rejected");
    }

    fn two_evses() -> CsState {
        let mut s = CsState::default();
        s.connectors.clear();
        s.add_connector(1, 1);
        s.add_connector(2, 2);
        s
    }

    fn id_token(tag: &str) -> serde_json::Value {
        json!({ "idToken": tag, "type": "ISO14443" })
    }

    #[test]
    /// OC-R-069 — a reservation is recorded at the EVSE level the request targets.
    fn ut_reserve_now_targets_evse_not_cs() {
        let h = handler_with(two_evses());
        drive(
            &h,
            "ReserveNow",
            json!({ "id": 42, "expiryDateTime": "2030-01-01T00:00:00Z",
                    "idToken": id_token("TAG1"), "evseId": 2 }),
        );
        let st = h.state.read();
        let c = st.connector_by_evse(2).unwrap();
        assert_eq!(c.reserved_rfid.as_deref(), Some("TAG1"));
        assert_eq!(c.reservation_id, Some(42));
        assert!(st.reserved_rfid.is_none());
        assert!(st.connector_by_evse(1).unwrap().reserved_rfid.is_none());
    }

    #[test]
    /// OC-R-069 — a reservation with no EVSE is recorded at the charge-point level.
    fn ut_reserve_now_without_evse_is_cs_level() {
        let h = handler_with(two_evses());
        drive(
            &h,
            "ReserveNow",
            json!({ "id": 1, "expiryDateTime": "2030-01-01T00:00:00Z",
                    "idToken": id_token("STN") }),
        );
        let st = h.state.read();
        assert_eq!(st.reserved_rfid.as_deref(), Some("STN"));
        assert_eq!(st.reservation_id, Some(1));
        assert!(st.connector_by_evse(1).unwrap().reserved_rfid.is_none());
    }

    #[test]
    /// OC-R-069 — a cancellation carrying the same reservation id clears the reservation at whichever level holds it.
    fn ut_cancel_reservation_clears_matching_evse() {
        let h = handler_with(two_evses());
        drive(
            &h,
            "ReserveNow",
            json!({ "id": 9, "expiryDateTime": "2030-01-01T00:00:00Z",
                    "idToken": id_token("T"), "evseId": 2 }),
        );
        drive(&h, "CancelReservation", json!({ "reservationId": 9 }));
        let st = h.state.read();
        let c = st.connector_by_evse(2).unwrap();
        assert!(c.reserved_rfid.is_none());
        assert!(c.reservation_id.is_none());
    }

    #[test]
    /// OC-R-063 — an absent EVSE id means the charge point itself, so ChangeAvailability targets every connector.
    fn ut_change_availability_status_and_absent_evse_targets_all() {
        let h = handler_with(two_evses());
        drive(
            &h,
            "ChangeAvailability",
            json!({ "operationalStatus": "Inoperative", "evse": { "id": 2 } }),
        );
        assert_eq!(
            h.state.read().connector_by_evse(2).unwrap().status,
            "Unavailable"
        );
        drive(
            &h,
            "ChangeAvailability",
            json!({ "operationalStatus": "Operative" }),
        );
        let st = h.state.read();
        assert!(st.connectors.iter().all(|c| c.status == "Available"));
    }

    #[test]
    /// OC-R-070 — a remote start mints a transaction id and sets the EVSE charging; a remote stop clears it and returns to available.
    fn ut_request_start_then_stop_transaction() {
        let h = handler_with(two_evses());
        drive(
            &h,
            "RequestStartTransaction",
            json!({ "remoteStartId": 5, "idToken": id_token("T"), "evseId": 1 }),
        );
        let tx = {
            let st = h.state.read();
            let c = st.connector_by_evse(1).unwrap();
            assert_eq!(c.status, "Charging");
            c.transaction_id.clone().expect("transaction assigned")
        };
        drive(&h, "RequestStopTransaction", json!({ "transactionId": tx }));
        let st = h.state.read();
        let c = st.connector_by_evse(1).unwrap();
        assert!(c.transaction_id.is_none());
        assert_eq!(c.status, "Available");
    }

    #[test]
    /// OC-R-068 — clearing charging profiles erases the per-purpose limit(s) on the targeted EVSE.
    fn ut_clear_profile_and_unlock_by_evse() {
        let mut s = two_evses();
        s.connector_mut_by_evse(1).unwrap().limit = Some(16.0);
        s.connector_mut_by_evse(1).unwrap().status = "Unavailable".to_string();
        let h = handler_with(s);
        drive(&h, "ClearChargingProfile", json!({}));
        assert!(h.state.read().connector_by_evse(1).unwrap().limit.is_none());
        drive(
            &h,
            "UnlockConnector",
            json!({ "evseId": 1, "connectorId": 1 }),
        );
        assert_eq!(
            h.state.read().connector_by_evse(1).unwrap().status,
            "Available"
        );
    }

    /// Drive an action through `respond` and return its encoded response JSON plus the log context.
    fn responded(
        h: &CsStateHandler,
        name: &str,
        payload: serde_json::Value,
    ) -> (serde_json::Value, String) {
        let action = V2_0_1::decode_call(name, payload).expect("action decodes");
        let (resp, ctx) = h.respond(&action);
        (
            V2_0_1::encode_response(&resp.expect("accepted")).expect("encodes"),
            ctx,
        )
    }

    #[test]
    /// OC-R-065 — a configuration read answers known keys and flags unknown ones from the key store.
    fn ut_get_variables_reports_known_and_unknown() {
        let h = handler_with(CsState::default());
        let (json, _) = responded(
            &h,
            "GetVariables",
            json!({
                "getVariableData": [
                    { "component": { "name": "OCPPCommCtrlr" }, "variable": { "name": "OCPPCommCtrlr.HeartbeatInterval" } },
                    { "component": { "name": "X" }, "variable": { "name": "NoSuchKey" } },
                ]
            }),
        );
        let results = json["getVariableResult"].as_array().unwrap();
        assert_eq!(results[0]["attributeStatus"], "Accepted");
        assert_eq!(results[1]["attributeStatus"], "UnknownVariable");
    }

    #[test]
    /// OC-R-066 — a configuration write updates a writable key, rejects a read-only key, and creates
    /// an unknown key.
    fn ut_set_variables_update_reject_and_create() {
        let h = handler_with(CsState::default());
        let (json, _) = responded(
            &h,
            "SetVariables",
            json!({
                "setVariableData": [
                    { "attributeValue": "77", "component": { "name": "c" }, "variable": { "name": "OCPPCommCtrlr.HeartbeatInterval" } },
                    { "attributeValue": "x", "component": { "name": "c" }, "variable": { "name": "EVSE.AvailabilityState" } },
                    { "attributeValue": "v", "component": { "name": "c" }, "variable": { "name": "BrandNewKey" } },
                ]
            }),
        );
        let r = json["setVariableResult"].as_array().unwrap();
        assert_eq!(r[0]["attributeStatus"], "Accepted");
        assert_eq!(r[1]["attributeStatus"], "Rejected"); // read-only
        assert_eq!(r[2]["attributeStatus"], "Accepted"); // created
        assert!(h.state.read().config.iter().any(|c| c.key == "BrandNewKey"));
    }

    #[test]
    /// OC-R-071 — a reset returns every connector to available, clears its transaction, and zeros
    /// session energy.
    fn ut_reset_returns_all_connectors_available() {
        let mut s = two_evses();
        s.connectors[0].status = "Charging".to_string();
        s.connectors[0].transaction_id = Some("tx".into());
        s.connectors[0].session_energy = 5.0;
        let h = handler_with(s);
        responded(&h, "Reset", json!({ "type": "Immediate" }));
        let st = h.state.read();
        assert!(st.connectors.iter().all(|c| c.status == "Available"));
        assert!(st.connectors.iter().all(|c| c.transaction_id.is_none()));
        assert_eq!(st.connectors[0].session_energy, 0.0);
    }

    #[test]
    /// OC-R-067 — a charging profile whose stack level exceeds the configured max is rejected.
    fn ut_set_charging_profile_rejects_excess_stack_level() {
        let h = handler_with(two_evses()); // default ChargeProfileMaxStackLevel = 10
        let (json, ctx) = responded(
            &h,
            "SetChargingProfile",
            json!({
                "evseId": 1,
                "chargingProfile": {
                    "id": 1, "stackLevel": 99, "chargingProfilePurpose": "TxProfile",
                    "chargingProfileKind": "Absolute",
                    "chargingSchedule": [{ "id": 1, "chargingRateUnit": "A",
                        "chargingSchedulePeriod": [{ "startPeriod": 0, "limit": 16.0 }] }]
                }
            }),
        );
        assert_eq!(json["status"], "Rejected");
        assert!(ctx.contains("stackLevel"));
    }

    #[test]
    /// OC-R-067 — an accepted charging profile applies its limit to the field matching its purpose.
    fn ut_set_charging_profile_applies_by_purpose() {
        let h = handler_with(two_evses());
        responded(
            &h,
            "SetChargingProfile",
            json!({
                "evseId": 1,
                "chargingProfile": {
                    "id": 1, "stackLevel": 1, "chargingProfilePurpose": "TxDefaultProfile",
                    "chargingProfileKind": "Absolute",
                    "chargingSchedule": [{ "id": 1, "chargingRateUnit": "A",
                        "chargingSchedulePeriod": [{ "startPeriod": 0, "limit": 12.0 }] }]
                }
            }),
        );
        assert_eq!(
            h.state.read().connector_by_evse(1).unwrap().default_limit,
            Some(12.0)
        );
    }

    #[test]
    /// OC-R-064 — an inbound Call the simulator does not model is default-accepted, not rejected.
    fn ut_unmodeled_action_default_accepted() {
        let h = handler_with(CsState::default());
        let action = V2_0_1::decode_call(
            "GetBaseReport",
            json!({ "requestId": 1, "reportBase": "FullInventory" }),
        )
        .expect("action decodes");
        let (resp, ctx) = h.respond(&action);
        assert!(resp.is_ok());
        assert_eq!(ctx, "default-accepted");
    }
}
