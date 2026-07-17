//! The inbound (CSMS→CS) handler for the OCPP 2.x charging-station simulator: a single generic
//! [`CsStateHandler`] that owns the version-independent plumbing — recording each Call and reply,
//! tagging them with their connector/EVSE scope, and the pre-dispatch unknown-EVSE guard — and
//! delegates the actual decision logic to `V::respond`.
//!
//! The decision logic itself is **fully typed** and lives per version in each version's `inbound.rs`
//! ([`Inbound`] impl for `V2_0_1` / `V2_1`): it matches the typed action enum, reads typed request
//! fields, and builds typed `rust_ocpp` responses. 2.0.1 and 2.1 answer these Calls identically, so
//! the two impls are near-copies — that duplication is deliberate, the price of never touching an
//! untyped `serde_json::Value` in the response path. (The only JSON here is the encoded action the
//! plumbing inspects generically for the scope/guard, which cannot be typed without a per-action
//! accessor for all ~60 actions.)

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::RwLock;
use serde_json::Value;

use ferrowl_ocpp::cs::CsActionHandler;
use ferrowl_ocpp::{CallError, CallErrorCode, Version};

use crate::module::ocpp::client::backend::{Dir, Messages, OcppMessage, push_capped};
use crate::module::ocpp::client::v2_0_1::state::CsState;
use crate::module::ocpp::client::v2_common::{inbound_scope, unknown_evse};
use crate::module::ocpp::lock::HasState;
use crate::module::ocpp::wire_log::{encode_action_or_log, encode_response_or_log};

/// Per-version, fully-typed inbound decision logic. Given the shared state and the typed action,
/// build the typed response (or a [`CallError`]) plus a short human-readable log context. Impl'd in
/// each version's `inbound.rs`; the generic [`CsStateHandler`] owns everything around it.
pub trait Inbound: Version {
    fn respond(
        state: &Arc<RwLock<CsState>>,
        action: &Self::Action,
    ) -> (Result<Self::Response, CallError>, String);
}

/// Inbound handler for an OCPP 2.x charging station, backed by the shared [`CsState`]. One struct
/// serves both 2.0.1 and 2.1: the [`CsActionHandler`] impl is blanket over `V: Inbound`.
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

impl<V: Inbound> CsActionHandler<V> for CsStateHandler {
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
            None => V::respond(&self.state, &action),
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
    fn drive<V: Inbound>(h: &CsStateHandler, name: &str, payload: serde_json::Value) {
        let action = V::decode_call(name, payload).expect("action decodes");
        assert!(V::respond(&h.state, &action).0.is_ok(), "{name} rejected");
    }

    /// Drive an action through `respond` and return its encoded response JSON plus the log context.
    fn responded<V: Inbound>(
        h: &CsStateHandler,
        name: &str,
        payload: serde_json::Value,
    ) -> (serde_json::Value, String) {
        let action = V::decode_call(name, payload).expect("action decodes");
        let (resp, ctx) = V::respond(&h.state, &action);
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
        let (resp, ctx) = V2_0_1::respond(&h.state, &action);
        assert!(resp.is_ok());
        assert_eq!(ctx, "default-accepted");
    }

    // --- 2.1 parity: the same generic shell, driven with `V2_1` typed actions/responses, exercising
    // the 2.1 typed `respond` impl against the 2.1 `rust_ocpp` types. ---

    #[test]
    /// OC-R-065 — the 2.1 binding answers a configuration read from the shared store.
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
    /// OC-R-066 — the 2.1 binding writes/rejects/creates configuration keys.
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
    /// OC-R-071 — the 2.1 binding resets every connector.
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
    /// OC-R-067 — the 2.1 binding rejects a charging profile above the configured max stack level.
    fn ut_v21_set_charging_profile_rejects_excess_stack_level() {
        let h = handler_with(two_evses());
        let (json, ctx) = responded::<V2_1>(
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
    /// OC-R-070 — the 2.1 binding mints and clears a transaction.
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
    /// OC-R-064 — the 2.1 binding default-accepts an unmodeled inbound Call.
    fn ut_v21_unmodeled_action_default_accepted() {
        let h = handler_with(CsState::default());
        let action = V2_1::decode_call(
            "GetBaseReport",
            json!({ "requestId": 1, "reportBase": "FullInventory" }),
        )
        .expect("action decodes");
        let (resp, ctx) = V2_1::respond(&h.state, &action);
        assert!(resp.is_ok());
        assert_eq!(ctx, "default-accepted");
    }
}
