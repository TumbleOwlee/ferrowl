//! OCPP 1.6 inbound (CSMS→CS) handler, answered from [`CsState`]. GetConfiguration is built from
//! the config store, ChangeConfiguration writes it, Reset mutates state. Connector-scoped Calls are
//! simulated against the targeted connector (or charge-point-wide for connectorId 0): ReserveNow /
//! CancelReservation (matched by reservationId), ChangeAvailability (status), Remote Start/Stop
//! (transaction), SetChargingProfile / ClearChargingProfile (limit), UnlockConnector. Every other
//! inbound Call is default-accepted (see `UNHANDLED.md`). Each inbound Call and our reply are recorded.

use std::future::Future;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

use ferrowl_ocpp::cs::CsActionHandler;
use ferrowl_ocpp::v1_6::messages::change_configuration::ChangeConfigurationResponse;
use ferrowl_ocpp::v1_6::messages::get_configuration::GetConfigurationResponse;
use ferrowl_ocpp::v1_6::messages::reset::ResetResponse;
use ferrowl_ocpp::v1_6::messages::set_charging_profile::SetChargingProfileResponse;
use ferrowl_ocpp::v1_6::types::{
    AvailabilityType, ChargingProfileStatus, ConfigurationStatus, KeyValue, ResetResponseStatus,
};
use ferrowl_ocpp::{Action16, CallError, CallErrorCode, Response16, V1_6, Version};

use crate::module::ocpp::client::backend::{Dir, Messages, OcppMessage, push_capped};
use crate::module::ocpp::client::v1_6::state::CsState;
use crate::module::ocpp::scope::Scope;

/// Scope an inbound CSMS→CS Call belongs to, for the message log: a top-level `connectorId` targets
/// that connector, otherwise it is CS-level.
fn inbound_scope(request: &serde_json::Value) -> Scope {
    match request["connectorId"].as_i64() {
        Some(c) => Scope::connector(c),
        None => Scope::CS,
    }
}

/// Clear the per-purpose charge limit a ClearChargingProfile targets: the field matching `purpose`,
/// or every per-purpose limit when no purpose criterion is given. An unknown purpose clears nothing.
fn clear_limit_by_purpose(c: &mut super::state::ConnectorState, purpose: Option<&str>) {
    match purpose {
        Some("TxProfile") => c.limit = None,
        Some("TxDefaultProfile") => c.default_limit = None,
        Some("ChargePointMaxProfile") => c.max_limit = None,
        Some(_) => {}
        None => {
            c.limit = None;
            c.default_limit = None;
            c.max_limit = None;
        }
    }
}

/// A top-level `connectorId` that this charging station does not have, if any. Connector `0` is the
/// charge point itself in OCPP 1.6 and is always valid; an absent `connectorId` is CS-level.
fn unknown_connector(request: &serde_json::Value, state: &CsState) -> Option<i64> {
    let id = request["connectorId"].as_i64()?;
    if id == 0 || state.connector(id).is_some() {
        None
    } else {
        Some(id)
    }
}

/// Inbound handler for an OCPP 1.6 charging station, backed by shared [`CsState`].
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

    /// Build the response for an inbound action from state (or default-accept), and a log context.
    fn respond(&self, action: &Action16) -> (Result<Response16, CallError>, String) {
        match action {
            Action16::GetConfiguration(req) => {
                let state = self.state.read().unwrap();
                let wanted = req.key.as_deref();
                let mut keys = Vec::new();
                let mut unknown = Vec::new();
                match wanted {
                    Some(list) => {
                        for k in list {
                            match state.config.iter().find(|c| &c.key == k) {
                                Some(c) => keys.push(key_value(c)),
                                None => unknown.push(k.clone()),
                            }
                        }
                    }
                    None => keys = state.config.iter().map(key_value).collect(),
                }
                let resp = GetConfigurationResponse {
                    configuration_key: (!keys.is_empty()).then_some(keys),
                    unknown_key: (!unknown.is_empty()).then_some(unknown),
                };
                (
                    Ok(Response16::GetConfiguration(resp)),
                    "answered from config".to_string(),
                )
            }
            Action16::ChangeConfiguration(req) => {
                let mut state = self.state.write().unwrap();
                let status = match state.config.iter_mut().find(|c| c.key == req.key) {
                    Some(c) if c.readonly => ConfigurationStatus::Rejected,
                    Some(c) => {
                        c.value = req.value.clone();
                        ConfigurationStatus::Accepted
                    }
                    None => {
                        state.config.push(super::state::ConfigKey {
                            key: req.key.clone(),
                            value: req.value.clone(),
                            readonly: false,
                        });
                        ConfigurationStatus::Accepted
                    }
                };
                (
                    Ok(Response16::ChangeConfiguration(
                        ChangeConfigurationResponse { status },
                    )),
                    format!("{} = {}", req.key, req.value),
                )
            }
            Action16::Reset(_) => {
                let mut state = self.state.write().unwrap();
                for c in &mut state.connectors {
                    c.status = "Available".to_string();
                    c.transaction_id = None;
                    c.session_energy = 0.0;
                }
                (
                    Ok(Response16::Reset(ResetResponse {
                        status: ResetResponseStatus::Accepted,
                    })),
                    "state reset".to_string(),
                )
            }
            Action16::SetChargingProfile(req) => {
                let json = V1_6::encode_action(action).unwrap_or(serde_json::Value::Null);
                let profile = &json["csChargingProfiles"];
                let stack = profile["stackLevel"].as_i64().unwrap_or(0);
                let purpose = profile["chargingProfilePurpose"]
                    .as_str()
                    .unwrap_or("TxProfile")
                    .to_string();
                let schedule = &profile["chargingSchedule"];
                let period = &schedule["chargingSchedulePeriod"][0];
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
                    let resp = Response16::SetChargingProfile(SetChargingProfileResponse {
                        status: ChargingProfileStatus::Rejected,
                    });
                    (
                        Ok(resp),
                        format!("rejected: stackLevel {stack} > max {max}"),
                    )
                } else {
                    // Apply the limit to the targeted connector (fall back to the first), routed by
                    // charging-profile purpose into the matching per-purpose field.
                    let context = if let Some(limit) = period["limit"].as_f64() {
                        let unit = schedule["chargingRateUnit"]
                            .as_str()
                            .unwrap_or("A")
                            .to_string();
                        let target = req.connector_id as i64;
                        let idx = state
                            .connectors
                            .iter()
                            .position(|c| c.connector_id == target)
                            .or((!state.connectors.is_empty()).then_some(0));
                        if let Some(i) = idx {
                            let c = &mut state.connectors[i];
                            match purpose.as_str() {
                                "TxDefaultProfile" => {
                                    c.default_limit = Some(limit);
                                    c.default_limit_unit = unit.clone();
                                }
                                "ChargePointMaxProfile" => {
                                    c.max_limit = Some(limit);
                                    c.max_limit_unit = unit.clone();
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
                    let resp = V1_6::default_response("SetChargingProfile")
                        .expect("SetChargingProfile is a known action");
                    (Ok(resp), context)
                }
            }
            Action16::ReserveNow(req) => {
                let id = req.reservation_id as i64;
                let mut state = self.state.write().unwrap();
                // connectorId 0 reserves the charge point itself (CS-level); any other id targets
                // that connector.
                let context = if req.connector_id == 0 {
                    state.reserved_rfid = Some(req.id_tag.clone());
                    state.reservation_id = Some(id);
                    format!("reserved CS for {}", req.id_tag)
                } else if let Some(c) = state.connector_mut(req.connector_id as i64) {
                    c.reserved_rfid = Some(req.id_tag.clone());
                    c.reservation_id = Some(id);
                    format!("reserved connector {} for {}", req.connector_id, req.id_tag)
                } else {
                    format!("unknown connector {}", req.connector_id)
                };
                let resp =
                    V1_6::default_response("ReserveNow").expect("ReserveNow is a known action");
                (Ok(resp), context)
            }
            Action16::CancelReservation(req) => {
                let id = req.reservation_id as i64;
                let mut state = self.state.write().unwrap();
                // Clear whichever level holds the matching reservationId.
                let context = if state.reservation_id == Some(id) {
                    state.reserved_rfid = None;
                    state.reservation_id = None;
                    format!("cancelled CS reservation {id}")
                } else if let Some(c) = state
                    .connectors
                    .iter_mut()
                    .find(|c| c.reservation_id == Some(id))
                {
                    c.reserved_rfid = None;
                    c.reservation_id = None;
                    format!("cancelled connector {} reservation {id}", c.connector_id)
                } else {
                    format!("no reservation {id}")
                };
                let resp = V1_6::default_response("CancelReservation")
                    .expect("CancelReservation is a known action");
                (Ok(resp), context)
            }
            Action16::ChangeAvailability(req) => {
                let status = match req.kind {
                    AvailabilityType::Operative => "Available",
                    AvailabilityType::Inoperative => "Unavailable",
                };
                let mut state = self.state.write().unwrap();
                // connectorId 0 targets the whole charge point (every connector).
                if req.connector_id == 0 {
                    for c in &mut state.connectors {
                        c.status = status.to_string();
                    }
                } else if let Some(c) = state.connector_mut(req.connector_id as i64) {
                    c.status = status.to_string();
                }
                let resp = V1_6::default_response("ChangeAvailability")
                    .expect("ChangeAvailability is a known action");
                (
                    Ok(resp),
                    format!("connector {} -> {status}", req.connector_id),
                )
            }
            Action16::RemoteStartTransaction(req) => {
                let mut state = self.state.write().unwrap();
                // Optional connectorId; fall back to the first connector. Mint a local transaction
                // id (one greater than any existing) and put the connector into Charging.
                let next = state
                    .connectors
                    .iter()
                    .filter_map(|c| c.transaction_id)
                    .max()
                    .unwrap_or(0)
                    + 1;
                let idx = req
                    .connector_id
                    .and_then(|t| {
                        state
                            .connectors
                            .iter()
                            .position(|c| c.connector_id == t as i64)
                    })
                    .or((!state.connectors.is_empty()).then_some(0));
                let context = match idx {
                    Some(i) => {
                        state.connectors[i].transaction_id = Some(next);
                        state.connectors[i].status = "Charging".to_string();
                        format!(
                            "started tx {next} on connector {}",
                            state.connectors[i].connector_id
                        )
                    }
                    None => "no connector to start".to_string(),
                };
                let resp = V1_6::default_response("RemoteStartTransaction")
                    .expect("RemoteStartTransaction is a known action");
                (Ok(resp), context)
            }
            Action16::RemoteStopTransaction(req) => {
                let tx = req.transaction_id as i64;
                let mut state = self.state.write().unwrap();
                let context = match state
                    .connectors
                    .iter_mut()
                    .find(|c| c.transaction_id == Some(tx))
                {
                    Some(c) => {
                        c.transaction_id = None;
                        c.limit = None;
                        c.status = "Available".to_string();
                        format!("stopped tx {tx} on connector {}", c.connector_id)
                    }
                    None => format!("no active tx {tx}"),
                };
                let resp = V1_6::default_response("RemoteStopTransaction")
                    .expect("RemoteStopTransaction is a known action");
                (Ok(resp), context)
            }
            Action16::ClearChargingProfile(req) => {
                let json = V1_6::encode_action(action).unwrap_or(serde_json::Value::Null);
                let purpose = json["chargingProfilePurpose"].as_str().map(str::to_owned);
                let mut state = self.state.write().unwrap();
                // Optional connectorId; absent clears every connector. The purpose criterion (when
                // given) selects which per-purpose limit is erased; absent clears all of them.
                match req.connector_id {
                    Some(id) => {
                        if let Some(c) = state.connector_mut(id as i64) {
                            clear_limit_by_purpose(c, purpose.as_deref());
                        }
                    }
                    None => {
                        for c in &mut state.connectors {
                            clear_limit_by_purpose(c, purpose.as_deref());
                        }
                    }
                }
                let resp = V1_6::default_response("ClearChargingProfile")
                    .expect("ClearChargingProfile is a known action");
                (Ok(resp), "charging profile cleared".to_string())
            }
            Action16::UnlockConnector(req) => {
                let mut state = self.state.write().unwrap();
                if let Some(c) = state.connector_mut(req.connector_id as i64) {
                    c.status = "Available".to_string();
                }
                let resp = V1_6::default_response("UnlockConnector")
                    .expect("UnlockConnector is a known action");
                (Ok(resp), format!("connector {} unlocked", req.connector_id))
            }
            other => {
                let name = V1_6::action_name(other);
                match V1_6::default_response(name) {
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

impl CsActionHandler<V1_6> for CsStateHandler {
    fn handle_call(
        &self,
        action: Action16,
    ) -> impl Future<Output = Result<Response16, CallError>> + Send {
        let name = V1_6::action_name(&action).to_string();
        let request = V1_6::encode_action(&action).unwrap_or(serde_json::Value::Null);
        // Reject Calls targeting a connector this station does not have. Scope the read guard so it
        // is dropped before `respond()` (which takes a write lock) — holding both deadlocks.
        let unknown = unknown_connector(&request, &self.state.read().unwrap());
        let (result, context) = match unknown {
            Some(id) => (
                Err(CallError::new(
                    CallErrorCode::PropertyConstraintViolation,
                    "unknown connectorId",
                )),
                format!("unknown connector {id}"),
            ),
            None => self.respond(&action),
        };
        let reply_payload = match &result {
            Ok(resp) => V1_6::encode_response(resp).unwrap_or(serde_json::Value::Null),
            Err(_) => serde_json::Value::Null,
        };
        let ok = result.is_ok();
        let scope = inbound_scope(&request);
        let messages = self.messages.clone();
        async move {
            // Record the inbound Call, then our reply, both tagged with the connector scope.
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

/// Map a stored config key to the wire `KeyValue`.
fn key_value(c: &super::state::ConfigKey) -> KeyValue {
    KeyValue {
        key: c.key.clone(),
        readonly: c.readonly,
        value: Some(c.value.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ut_unknown_connector_rejected() {
        let mut s = CsState::default();
        s.connectors.clear();
        s.add_connector(1);
        // A present connector, the charge point itself (0) and CS-level Calls are accepted.
        assert_eq!(unknown_connector(&json!({ "connectorId": 1 }), &s), None);
        assert_eq!(unknown_connector(&json!({ "connectorId": 0 }), &s), None);
        assert_eq!(unknown_connector(&json!({}), &s), None);
        // An unknown connector id is reported for rejection.
        assert_eq!(unknown_connector(&json!({ "connectorId": 7 }), &s), Some(7));
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
        let action = V1_6::default_action("Reset").expect("Reset is a known action");
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
        let action = V1_6::decode_call(name, payload).expect("action decodes");
        assert!(h.respond(&action).0.is_ok(), "{name} rejected");
    }

    fn two_connectors() -> CsState {
        let mut s = CsState::default();
        s.connectors.clear();
        s.add_connector(1);
        s.add_connector(2);
        s
    }

    #[test]
    fn ut_reserve_now_targets_connector_not_cs() {
        let h = handler_with(two_connectors());
        drive(
            &h,
            "ReserveNow",
            json!({ "connectorId": 2, "expiryDate": "2030-01-01T00:00:00Z",
                    "idTag": "TAG1", "reservationId": 42 }),
        );
        let st = h.state.read().unwrap();
        assert_eq!(
            st.connector(2).unwrap().reserved_rfid.as_deref(),
            Some("TAG1")
        );
        assert_eq!(st.connector(2).unwrap().reservation_id, Some(42));
        // The CS level and the untargeted connector are untouched.
        assert!(st.reserved_rfid.is_none());
        assert!(st.connector(1).unwrap().reserved_rfid.is_none());
    }

    #[test]
    fn ut_reserve_now_connector_zero_is_cs_level() {
        let h = handler_with(CsState::default());
        drive(
            &h,
            "ReserveNow",
            json!({ "connectorId": 0, "expiryDate": "2030-01-01T00:00:00Z",
                    "idTag": "CP", "reservationId": 1 }),
        );
        let st = h.state.read().unwrap();
        assert_eq!(st.reserved_rfid.as_deref(), Some("CP"));
        assert_eq!(st.reservation_id, Some(1));
        assert!(st.connector(1).unwrap().reserved_rfid.is_none());
    }

    #[test]
    fn ut_cancel_reservation_clears_matching_connector() {
        let h = handler_with(two_connectors());
        drive(
            &h,
            "ReserveNow",
            json!({ "connectorId": 2, "expiryDate": "2030-01-01T00:00:00Z",
                    "idTag": "T", "reservationId": 7 }),
        );
        drive(&h, "CancelReservation", json!({ "reservationId": 7 }));
        let st = h.state.read().unwrap();
        assert!(st.connector(2).unwrap().reserved_rfid.is_none());
        assert!(st.connector(2).unwrap().reservation_id.is_none());
    }

    #[test]
    fn ut_change_availability_status_and_zero_targets_all() {
        let h = handler_with(two_connectors());
        drive(
            &h,
            "ChangeAvailability",
            json!({ "connectorId": 2, "type": "Inoperative" }),
        );
        assert_eq!(
            h.state.read().unwrap().connector(2).unwrap().status,
            "Unavailable"
        );
        drive(
            &h,
            "ChangeAvailability",
            json!({ "connectorId": 0, "type": "Operative" }),
        );
        let st = h.state.read().unwrap();
        assert!(st.connectors.iter().all(|c| c.status == "Available"));
    }

    #[test]
    fn ut_remote_start_then_stop_transaction() {
        let h = handler_with(two_connectors());
        drive(
            &h,
            "RemoteStartTransaction",
            json!({ "connectorId": 1, "idTag": "T" }),
        );
        let tx = {
            let st = h.state.read().unwrap();
            let c = st.connector(1).unwrap();
            assert_eq!(c.status, "Charging");
            c.transaction_id.expect("transaction assigned")
        };
        drive(&h, "RemoteStopTransaction", json!({ "transactionId": tx }));
        let st = h.state.read().unwrap();
        assert!(st.connector(1).unwrap().transaction_id.is_none());
        assert_eq!(st.connector(1).unwrap().status, "Available");
    }

    #[test]
    fn ut_clear_profile_and_unlock_connector() {
        let mut s = two_connectors();
        s.connector_mut(1).unwrap().limit = Some(16.0);
        s.connector_mut(1).unwrap().status = "Unavailable".to_string();
        let h = handler_with(s);
        drive(&h, "ClearChargingProfile", json!({}));
        assert!(
            h.state
                .read()
                .unwrap()
                .connector(1)
                .unwrap()
                .limit
                .is_none()
        );
        drive(&h, "UnlockConnector", json!({ "connectorId": 1 }));
        assert_eq!(
            h.state.read().unwrap().connector(1).unwrap().status,
            "Available"
        );
    }

    #[test]
    fn ut_clear_profile_erases_only_named_purpose() {
        let mut s = two_connectors();
        {
            let c = s.connector_mut(1).unwrap();
            c.limit = Some(16.0);
            c.default_limit = Some(10.0);
            c.max_limit = Some(32.0);
        }
        let h = handler_with(s);
        // Clearing TxDefaultProfile erases only default_limit; the others stay.
        drive(
            &h,
            "ClearChargingProfile",
            json!({ "chargingProfilePurpose": "TxDefaultProfile" }),
        );
        {
            let st = h.state.read().unwrap();
            let c = st.connector(1).unwrap();
            assert_eq!(c.limit, Some(16.0));
            assert_eq!(c.default_limit, None);
            assert_eq!(c.max_limit, Some(32.0));
        }
        // No purpose criterion clears every per-purpose limit.
        drive(&h, "ClearChargingProfile", json!({}));
        let st = h.state.read().unwrap();
        let c = st.connector(1).unwrap();
        assert_eq!(c.limit, None);
        assert_eq!(c.max_limit, None);
    }

    /// Build a SetChargingProfile wire payload for connector 1 with the given purpose/stack/limit.
    fn set_profile(purpose: &str, stack: i64, limit: f64) -> serde_json::Value {
        json!({
            "connectorId": 1,
            "csChargingProfiles": {
                "chargingProfileId": 1,
                "stackLevel": stack,
                "chargingProfilePurpose": purpose,
                "chargingProfileKind": "Absolute",
                "chargingSchedule": {
                    "chargingRateUnit": "A",
                    "chargingSchedulePeriod": [{ "startPeriod": 0, "limit": limit }],
                },
            },
        })
    }

    #[test]
    fn ut_set_charging_profile_rejects_above_max_stack() {
        // Default config seeds ChargeProfileMaxStackLevel = 10.
        let h = handler_with(two_connectors());
        let action = V1_6::decode_call("SetChargingProfile", set_profile("TxProfile", 11, 16.0))
            .expect("action decodes");
        match h.respond(&action).0.expect("response built") {
            Response16::SetChargingProfile(r) => {
                assert_eq!(r.status, ChargingProfileStatus::Rejected)
            }
            other => panic!("unexpected response {other:?}"),
        }
        // Nothing applied to the connector.
        assert_eq!(h.state.read().unwrap().connector(1).unwrap().limit, None);
    }

    #[test]
    fn ut_set_charging_profile_routes_by_purpose() {
        let h = handler_with(two_connectors());
        drive(&h, "SetChargingProfile", set_profile("TxProfile", 0, 16.0));
        drive(
            &h,
            "SetChargingProfile",
            set_profile("TxDefaultProfile", 0, 10.0),
        );
        drive(
            &h,
            "SetChargingProfile",
            set_profile("ChargePointMaxProfile", 0, 32.0),
        );
        let st = h.state.read().unwrap();
        let c = st.connector(1).unwrap();
        assert_eq!(c.limit, Some(16.0));
        assert_eq!(c.default_limit, Some(10.0));
        assert_eq!(c.max_limit, Some(32.0));
    }

    #[test]
    fn ut_stop_clears_only_tx_limit() {
        let mut s = two_connectors();
        {
            let c = s.connector_mut(1).unwrap();
            c.transaction_id = Some(42);
            c.limit = Some(16.0);
            c.default_limit = Some(10.0);
            c.max_limit = Some(32.0);
        }
        let h = handler_with(s);
        drive(&h, "RemoteStopTransaction", json!({ "transactionId": 42 }));
        let st = h.state.read().unwrap();
        let c = st.connector(1).unwrap();
        assert_eq!(c.limit, None, "TxProfile limit cleared on stop");
        assert_eq!(c.default_limit, Some(10.0), "default limit persists");
        assert_eq!(c.max_limit, Some(32.0), "max limit persists");
    }
}
