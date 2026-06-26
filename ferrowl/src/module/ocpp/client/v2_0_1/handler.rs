//! OCPP 2.0.1 inbound (CSMS→CS) handler, answered from [`CsState`]. GetVariables is built from the
//! variable store, SetVariables writes it, Reset mutates state. EVSE-scoped Calls are simulated
//! against the connector on the targeted EVSE. The handler body is shared with 2.1 via
//! [`define_cs_state_handler!`](crate::module::ocpp::client::v2_common::define_cs_state_handler);
//! this module instantiates it for `V2_0_1`. Every other inbound Call is default-accepted (see
//! `UNHANDLED.md`). Each inbound Call and our reply are recorded.

use crate::module::ocpp::client::v2_common::define_cs_state_handler;

define_cs_state_handler!(V2_0_1, v2_0_1, Action201, Response201);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::ocpp::client::backend::OcppMessage;
    use crate::module::ocpp::client::v2_0_1::state::CsState;
    use crate::module::ocpp::scope::Scope;
    use ferrowl_ocpp::{V2_0_1, Version};
    use serde_json::json;
    use std::sync::Arc;
    use std::sync::RwLock;

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
    fn ut_reserve_now_targets_evse_not_cs() {
        let h = handler_with(two_evses());
        drive(
            &h,
            "ReserveNow",
            json!({ "id": 42, "expiryDateTime": "2030-01-01T00:00:00Z",
                    "idToken": id_token("TAG1"), "evseId": 2 }),
        );
        let st = h.state.read().unwrap();
        let c = st.connector_by_evse(2).unwrap();
        assert_eq!(c.reserved_rfid.as_deref(), Some("TAG1"));
        assert_eq!(c.reservation_id, Some(42));
        assert!(st.reserved_rfid.is_none());
        assert!(st.connector_by_evse(1).unwrap().reserved_rfid.is_none());
    }

    #[test]
    fn ut_reserve_now_without_evse_is_cs_level() {
        let h = handler_with(two_evses());
        drive(
            &h,
            "ReserveNow",
            json!({ "id": 1, "expiryDateTime": "2030-01-01T00:00:00Z",
                    "idToken": id_token("STN") }),
        );
        let st = h.state.read().unwrap();
        assert_eq!(st.reserved_rfid.as_deref(), Some("STN"));
        assert_eq!(st.reservation_id, Some(1));
        assert!(st.connector_by_evse(1).unwrap().reserved_rfid.is_none());
    }

    #[test]
    fn ut_cancel_reservation_clears_matching_evse() {
        let h = handler_with(two_evses());
        drive(
            &h,
            "ReserveNow",
            json!({ "id": 9, "expiryDateTime": "2030-01-01T00:00:00Z",
                    "idToken": id_token("T"), "evseId": 2 }),
        );
        drive(&h, "CancelReservation", json!({ "reservationId": 9 }));
        let st = h.state.read().unwrap();
        let c = st.connector_by_evse(2).unwrap();
        assert!(c.reserved_rfid.is_none());
        assert!(c.reservation_id.is_none());
    }

    #[test]
    fn ut_change_availability_status_and_absent_evse_targets_all() {
        let h = handler_with(two_evses());
        drive(
            &h,
            "ChangeAvailability",
            json!({ "operationalStatus": "Inoperative", "evse": { "id": 2 } }),
        );
        assert_eq!(
            h.state.read().unwrap().connector_by_evse(2).unwrap().status,
            "Unavailable"
        );
        drive(
            &h,
            "ChangeAvailability",
            json!({ "operationalStatus": "Operative" }),
        );
        let st = h.state.read().unwrap();
        assert!(st.connectors.iter().all(|c| c.status == "Available"));
    }

    #[test]
    fn ut_request_start_then_stop_transaction() {
        let h = handler_with(two_evses());
        drive(
            &h,
            "RequestStartTransaction",
            json!({ "remoteStartId": 5, "idToken": id_token("T"), "evseId": 1 }),
        );
        let tx = {
            let st = h.state.read().unwrap();
            let c = st.connector_by_evse(1).unwrap();
            assert_eq!(c.status, "Charging");
            c.transaction_id.clone().expect("transaction assigned")
        };
        drive(&h, "RequestStopTransaction", json!({ "transactionId": tx }));
        let st = h.state.read().unwrap();
        let c = st.connector_by_evse(1).unwrap();
        assert!(c.transaction_id.is_none());
        assert_eq!(c.status, "Available");
    }

    #[test]
    fn ut_clear_profile_and_unlock_by_evse() {
        let mut s = two_evses();
        s.connector_mut_by_evse(1).unwrap().limit = Some(16.0);
        s.connector_mut_by_evse(1).unwrap().status = "Unavailable".to_string();
        let h = handler_with(s);
        drive(&h, "ClearChargingProfile", json!({}));
        assert!(
            h.state
                .read()
                .unwrap()
                .connector_by_evse(1)
                .unwrap()
                .limit
                .is_none()
        );
        drive(
            &h,
            "UnlockConnector",
            json!({ "evseId": 1, "connectorId": 1 }),
        );
        assert_eq!(
            h.state.read().unwrap().connector_by_evse(1).unwrap().status,
            "Available"
        );
    }
}
