//! The single, version-generic inbound (CSMS→CS) handler for the OCPP 2.x charging-station
//! simulator, answered from the shared [`CsState`]. GetVariables is built from the variable store,
//! SetVariables writes it, Reset mutates state. EVSE-scoped Calls are simulated against the
//! connector on the targeted EVSE; every other inbound Call is default-accepted (see `UNHANDLED.md`).
//!
//! 2.0.1 and 2.1 answer these Calls identically, so the decision logic lives once as
//! [`CsStateHandler::respond`], generic over `V: TypedInbound`. The few responses whose body
//! depends on request/state data (`GetVariables`, `SetVariables`, `Reset`, and the
//! `SetChargingProfile` reject) are built with the version's own strongly-typed `rust_ocpp` structs
//! via the [`TypedInbound`] trait (impl'd in each version's `inbound.rs`); the shared handler
//! supplies the store lookup/mutation as closures, so only the typed plumbing — not the decision
//! logic — differs per version. The version-independent helpers it calls (`unknown_evse`,
//! `inbound_scope`, …) live in [`v2_common`](crate::module::ocpp::client::v2_common). Each inbound
//! Call and our reply are recorded.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::RwLock;
use serde_json::Value;

use ferrowl_ocpp::cs::CsActionHandler;
use ferrowl_ocpp::{CallError, CallErrorCode, Version};

use crate::module::ocpp::client::backend::{Dir, Messages, OcppMessage, push_capped};
use crate::module::ocpp::client::config::ConfigKey;
use crate::module::ocpp::client::v2_0_1::state::CsState;
use crate::module::ocpp::client::v2_common::{clear_limit_by_purpose, inbound_scope, unknown_evse};
use crate::module::ocpp::lock::HasState;
use crate::module::ocpp::wire_log::{encode_action_or_log, encode_response_or_log};

/// Inbound handler for an OCPP 2.x charging station, backed by the shared [`CsState`]. One struct
/// serves both 2.0.1 and 2.1: the [`CsActionHandler`] impl is blanket over `V: TypedInbound`.
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

/// Outcome of applying one `SetVariables` entry to the store, reported by the caller's `apply`
/// closure back to the version's typed response builder.
pub enum SetOutcome {
    Accepted,
    Rejected,
}

/// Per-version construction of the inbound responses whose body depends on request/state data
/// (`GetVariables`, `SetVariables`, `Reset`, and the `SetChargingProfile` reject). Each impl (in the
/// version's `inbound.rs`) does only the typed request-extraction and response-building for its
/// `rust_ocpp` version; the shared [`CsStateHandler`] supplies the store lookup/mutation as closures
/// so the decision logic is not duplicated. Every method is dispatched by action name from
/// [`CsStateHandler::respond`], so each impl may assume its matching action variant.
pub trait TypedInbound: Version {
    /// Build the typed `GetVariables` response. `lookup(name)` returns the stored value of a known
    /// variable, `None` if the store has no such key.
    fn get_variables_response(
        action: &Self::Action,
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Self::Response;

    /// Build the typed `SetVariables` response. `apply(name, value)` writes the store and reports
    /// whether the write was accepted or rejected (read-only key).
    fn set_variables_response(
        action: &Self::Action,
        apply: impl FnMut(&str, &str) -> SetOutcome,
    ) -> Self::Response;

    /// The typed `Reset` response (always accepted).
    fn reset_response() -> Self::Response;

    /// The typed `SetChargingProfile` response for a rejected profile.
    fn set_charging_profile_rejected() -> Self::Response;
}

impl CsStateHandler {
    fn respond<V: TypedInbound>(
        &self,
        action: &V::Action,
    ) -> (Result<V::Response, CallError>, String) {
        let name = V::action_name(action);
        let request = encode_action_or_log::<V>(action);
        match name {
            "GetVariables" => self.with_state(|state| {
                let resp = V::get_variables_response(action, |name| {
                    state
                        .config
                        .iter()
                        .find(|c| c.key == name)
                        .map(|c| c.value.clone())
                });
                (Ok(resp), "answered from variables".to_string())
            }),
            "SetVariables" => self.with_state_mut(|state| {
                let resp = V::set_variables_response(action, |name, value| {
                    match state.config.iter_mut().find(|c| c.key == name) {
                        Some(c) if c.readonly => SetOutcome::Rejected,
                        Some(c) => {
                            c.value = value.to_string();
                            SetOutcome::Accepted
                        }
                        None => {
                            state.config.push(ConfigKey {
                                key: name.to_string(),
                                value: value.to_string(),
                                readonly: false,
                            });
                            SetOutcome::Accepted
                        }
                    }
                });
                (Ok(resp), "variables updated".to_string())
            }),
            "Reset" => self.with_state_mut(|state| {
                for c in &mut state.connectors {
                    c.status = "Available".to_string();
                    c.transaction_id = None;
                    c.session_energy = 0.0;
                }
                (Ok(V::reset_response()), "state reset".to_string())
            }),
            "SetChargingProfile" => {
                let profile = &request["chargingProfile"];
                let stack = profile["stackLevel"].as_i64().unwrap_or(0);
                let purpose = profile["chargingProfilePurpose"]
                    .as_str()
                    .unwrap_or("TxProfile")
                    .to_string();
                let schedule = &profile["chargingSchedule"][0];
                let period = &schedule["chargingSchedulePeriod"][0];
                let evse = request["evseId"].as_i64();
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
                        (
                            Ok(V::set_charging_profile_rejected()),
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
                        let resp = V::default_response("SetChargingProfile")
                            .map(Ok)
                            .expect("SetChargingProfile is a known action");
                        (resp, context)
                    }
                })
            }
            "ReserveNow" => {
                let tag = request["idToken"]["idToken"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let id = request["id"].as_i64();
                self.with_state_mut(|state| {
                    // An evseId-less (or evseId 0) ReserveNow reserves the station itself
                    // (CS-level); otherwise it targets the connector on that EVSE (entries are
                    // addressed by EVSE id).
                    let context = match request["evseId"].as_i64().filter(|&e| e != 0) {
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
                    let resp = V::default_response("ReserveNow")
                        .map(Ok)
                        .expect("ReserveNow is a known action");
                    (resp, context)
                })
            }
            "CancelReservation" => self.with_state_mut(|state| {
                // Clear whichever level holds the matching reservationId.
                let context = match request["reservationId"].as_i64() {
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
                let resp = V::default_response("CancelReservation")
                    .map(Ok)
                    .expect("CancelReservation is a known action");
                (resp, context)
            }),
            "ChangeAvailability" => {
                let status = match request["operationalStatus"].as_str() {
                    Some("Inoperative") => "Unavailable",
                    _ => "Available",
                };
                self.with_state_mut(|state| {
                    // An evseId-less (or evseId 0) ChangeAvailability targets the whole station.
                    let context = match request["evse"]["id"].as_i64().filter(|&e| e != 0) {
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
                    let resp = V::default_response("ChangeAvailability")
                        .map(Ok)
                        .expect("ChangeAvailability is a known action");
                    (resp, context)
                })
            }
            "RequestStartTransaction" => self.with_state_mut(|state| {
                // Optional evseId; fall back to the first connector. Mint a transaction and charge.
                let idx = request["evseId"]
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
                let resp = V::default_response("RequestStartTransaction")
                    .map(Ok)
                    .expect("RequestStartTransaction is a known action");
                (resp, context)
            }),
            "RequestStopTransaction" => {
                let tx = request["transactionId"]
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
                    let resp = V::default_response("RequestStopTransaction")
                        .map(Ok)
                        .expect("RequestStopTransaction is a known action");
                    (resp, context)
                })
            }
            "ClearChargingProfile" => {
                let criteria = &request["chargingProfileCriteria"];
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
                    let resp = V::default_response("ClearChargingProfile")
                        .map(Ok)
                        .expect("ClearChargingProfile is a known action");
                    (resp, "charging profile cleared".to_string())
                })
            }
            "UnlockConnector" => self.with_state_mut(|state| {
                let context = match request["evseId"].as_i64().filter(|&e| e != 0) {
                    Some(e) => {
                        if let Some(c) = state.connector_mut_by_evse(e) {
                            c.status = "Available".to_string();
                        }
                        format!("evse {e} unlocked")
                    }
                    None => "no evse to unlock".to_string(),
                };
                let resp = V::default_response("UnlockConnector")
                    .map(Ok)
                    .expect("UnlockConnector is a known action");
                (resp, context)
            }),
            other => match V::default_response(other) {
                Some(resp) => (Ok(resp), "default-accepted".to_string()),
                None => (
                    Err(CallError::new(
                        CallErrorCode::NotImplemented,
                        "action not handled by the charging-station simulator",
                    )),
                    "not implemented".to_string(),
                ),
            },
        }
    }
}

impl<V: TypedInbound> CsActionHandler<V> for CsStateHandler {
    fn handle_call(
        &self,
        action: V::Action,
    ) -> impl Future<Output = Result<V::Response, CallError>> + Send {
        let name = V::action_name(&action).to_string();
        let request = encode_action_or_log::<V>(&action);
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
            None => self.respond::<V>(&action),
        };
        let reply_payload = match &result {
            Ok(resp) => encode_response_or_log::<V>(resp),
            Err(_) => Value::Null,
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
    use ferrowl_ocpp::{V2_0_1, V2_1, Version};
    use serde_json::json;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use parking_lot::RwLock;

    fn handler_with(state: CsState) -> CsStateHandler {
        CsStateHandler::new(
            Arc::new(AtomicBool::new(false)),
            Arc::new(tokio::sync::RwLock::new(Vec::<OcppMessage>::new())),
            Arc::new(RwLock::new(state)),
        )
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

    /// Build an action for version `V`, drive it through `respond`, and assert it was accepted.
    fn drive<V: TypedInbound>(h: &CsStateHandler, name: &str, payload: serde_json::Value) {
        let action = V::decode_call(name, payload).expect("action decodes");
        assert!(h.respond::<V>(&action).0.is_ok(), "{name} rejected");
    }

    /// Drive an action through `respond` and return its encoded response JSON plus the log context.
    fn responded<V: TypedInbound>(
        h: &CsStateHandler,
        name: &str,
        payload: serde_json::Value,
    ) -> (serde_json::Value, String) {
        let action = V::decode_call(name, payload).expect("action decodes");
        let (resp, ctx) = h.respond::<V>(&action);
        (
            V::encode_response(&resp.expect("accepted")).expect("encodes"),
            ctx,
        )
    }

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
        use std::sync::mpsc;
        use std::time::Duration;

        // Reset hits the `None` (accept) arm and takes a write lock in `respond()`. If the inbound
        // read guard is still held there, the std RwLock self-deadlocks. Run on a thread and bound
        // the wait so a regression fails the test instead of hanging CI.
        let handler = handler_with(CsState::default());
        let action = V2_0_1::default_action("Reset").expect("Reset is a known action");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            // Building the response is the synchronous part that deadlocked; dropping the future is fine.
            drop(CsActionHandler::<V2_0_1>::handle_call(&handler, action));
            let _ = tx.send(());
        });
        assert!(
            rx.recv_timeout(Duration::from_secs(5)).is_ok(),
            "handle_call deadlocked on a write-arm inbound action"
        );
    }

    #[test]
    /// OC-R-069 — a reservation is recorded at the EVSE level the request targets.
    fn ut_reserve_now_targets_evse_not_cs() {
        let h = handler_with(two_evses());
        drive::<V2_0_1>(
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
        drive::<V2_0_1>(
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
        drive::<V2_0_1>(
            &h,
            "ReserveNow",
            json!({ "id": 9, "expiryDateTime": "2030-01-01T00:00:00Z",
                    "idToken": id_token("T"), "evseId": 2 }),
        );
        drive::<V2_0_1>(&h, "CancelReservation", json!({ "reservationId": 9 }));
        let st = h.state.read();
        let c = st.connector_by_evse(2).unwrap();
        assert!(c.reserved_rfid.is_none());
        assert!(c.reservation_id.is_none());
    }

    #[test]
    /// OC-R-063 — an absent EVSE id means the charge point itself, so ChangeAvailability targets every connector.
    fn ut_change_availability_status_and_absent_evse_targets_all() {
        let h = handler_with(two_evses());
        drive::<V2_0_1>(
            &h,
            "ChangeAvailability",
            json!({ "operationalStatus": "Inoperative", "evse": { "id": 2 } }),
        );
        assert_eq!(
            h.state.read().connector_by_evse(2).unwrap().status,
            "Unavailable"
        );
        drive::<V2_0_1>(
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
        drive::<V2_0_1>(
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
        drive::<V2_0_1>(&h, "RequestStopTransaction", json!({ "transactionId": tx }));
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
        drive::<V2_0_1>(&h, "ClearChargingProfile", json!({}));
        assert!(h.state.read().connector_by_evse(1).unwrap().limit.is_none());
        drive::<V2_0_1>(
            &h,
            "UnlockConnector",
            json!({ "evseId": 1, "connectorId": 1 }),
        );
        assert_eq!(
            h.state.read().connector_by_evse(1).unwrap().status,
            "Available"
        );
    }

    #[test]
    /// OC-R-065 — a configuration read answers known keys and flags unknown ones from the key store.
    fn ut_get_variables_reports_known_and_unknown() {
        let h = handler_with(CsState::default());
        let (json, _) = responded::<V2_0_1>(
            &h,
            "GetVariables",
            json!({
                "getVariableData": [
                    { "attributeType": "Actual", "component": { "name": "OCPPCommCtrlr" }, "variable": { "name": "OCPPCommCtrlr.HeartbeatInterval" } },
                    { "component": { "name": "X" }, "variable": { "name": "NoSuchKey" } },
                ]
            }),
        );
        let results = json["getVariableResult"].as_array().unwrap();
        assert_eq!(results[0]["attributeStatus"], "Accepted");
        // A known key returns its stored value with the requested attributeType echoed back.
        assert_eq!(results[0]["attributeValue"], "300");
        assert_eq!(results[0]["attributeType"], "Actual");
        assert_eq!(results[1]["attributeStatus"], "UnknownVariable");
        // An unknown key carries no value.
        assert!(results[1].get("attributeValue").is_none());
    }

    #[test]
    /// OC-R-066 — a configuration write updates a writable key, rejects a read-only key, and creates
    /// an unknown key.
    fn ut_set_variables_update_reject_and_create() {
        let h = handler_with(CsState::default());
        let (json, _) = responded::<V2_0_1>(
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
        responded::<V2_0_1>(&h, "Reset", json!({ "type": "Immediate" }));
        let st = h.state.read();
        assert!(st.connectors.iter().all(|c| c.status == "Available"));
        assert!(st.connectors.iter().all(|c| c.transaction_id.is_none()));
        assert_eq!(st.connectors[0].session_energy, 0.0);
    }

    #[test]
    /// OC-R-067 — a charging profile whose stack level exceeds the configured max is rejected.
    fn ut_set_charging_profile_rejects_excess_stack_level() {
        let h = handler_with(two_evses()); // default ChargeProfileMaxStackLevel = 10
        let (json, ctx) = responded::<V2_0_1>(
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
        responded::<V2_0_1>(
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
        let (resp, ctx) = h.respond::<V2_0_1>(&action);
        assert!(resp.is_ok());
        assert_eq!(ctx, "default-accepted");
    }

    // --- 2.1 parity: the same generic handler, driven with `V2_1` typed actions/responses, proving
    // the JSON→typed bridge (`decode_result`) round-trips against the 2.1 `rust_ocpp` types too. ---

    #[test]
    /// OC-R-065 — the 2.1 binding answers a configuration read from the shared store via the generic handler.
    fn ut_v21_get_variables_reports_known_and_unknown() {
        let h = handler_with(CsState::default());
        let (json, _) = responded::<V2_1>(
            &h,
            "GetVariables",
            json!({
                "getVariableData": [
                    { "attributeType": "Actual", "component": { "name": "OCPPCommCtrlr" }, "variable": { "name": "OCPPCommCtrlr.HeartbeatInterval" } },
                    { "component": { "name": "X" }, "variable": { "name": "NoSuchKey" } },
                ]
            }),
        );
        let results = json["getVariableResult"].as_array().unwrap();
        assert_eq!(results[0]["attributeStatus"], "Accepted");
        assert_eq!(results[0]["attributeValue"], "300");
        assert_eq!(results[0]["attributeType"], "Actual");
        assert_eq!(results[1]["attributeStatus"], "UnknownVariable");
        assert!(results[1].get("attributeValue").is_none());
    }

    #[test]
    /// OC-R-066 — the 2.1 binding writes/rejects/creates configuration keys via the generic handler.
    fn ut_v21_set_variables_update_reject_and_create() {
        let h = handler_with(CsState::default());
        let (json, _) = responded::<V2_1>(
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
        assert_eq!(r[1]["attributeStatus"], "Rejected");
        assert_eq!(r[2]["attributeStatus"], "Accepted");
        assert!(h.state.read().config.iter().any(|c| c.key == "BrandNewKey"));
    }

    #[test]
    /// OC-R-071 — the 2.1 binding resets every connector via the generic handler.
    fn ut_v21_reset_returns_all_connectors_available() {
        let mut s = two_evses();
        s.connectors[0].status = "Charging".to_string();
        s.connectors[0].transaction_id = Some("tx".into());
        s.connectors[0].session_energy = 5.0;
        let h = handler_with(s);
        let (json, _) = responded::<V2_1>(&h, "Reset", json!({ "type": "Immediate" }));
        assert_eq!(json["status"], "Accepted");
        let st = h.state.read();
        assert!(st.connectors.iter().all(|c| c.status == "Available"));
        assert!(st.connectors.iter().all(|c| c.transaction_id.is_none()));
        assert_eq!(st.connectors[0].session_energy, 0.0);
    }

    #[test]
    /// OC-R-070 — the 2.1 binding mints and clears a transaction via the generic handler.
    fn ut_v21_request_start_then_stop_transaction() {
        let h = handler_with(two_evses());
        drive::<V2_1>(
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
        drive::<V2_1>(&h, "RequestStopTransaction", json!({ "transactionId": tx }));
        let st = h.state.read();
        assert!(st.connector_by_evse(1).unwrap().transaction_id.is_none());
    }

    #[test]
    /// OC-R-064 — the 2.1 binding default-accepts an unmodeled inbound Call via the generic handler.
    fn ut_v21_unmodeled_action_default_accepted() {
        let h = handler_with(CsState::default());
        let action = V2_1::decode_call(
            "GetBaseReport",
            json!({ "requestId": 1, "reportBase": "FullInventory" }),
        )
        .expect("action decodes");
        let (resp, ctx) = h.respond::<V2_1>(&action);
        assert!(resp.is_ok());
        assert_eq!(ctx, "default-accepted");
    }
}
