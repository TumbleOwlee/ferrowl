//! OCPP 1.6 inbound (CSMS→CS) handler, answered from [`CsState`]. GetConfiguration is built from
//! the config store, ChangeConfiguration writes it, Reset mutates state. Connector-scoped Calls are
//! simulated against the targeted connector (or charge-point-wide for connectorId 0): ReserveNow /
//! CancelReservation (matched by reservationId), ChangeAvailability (status), Remote Start/Stop
//! (transaction), SetChargingProfile / ClearChargingProfile (limit), UnlockConnector. Every other
//! inbound Call is default-accepted (see `UNHANDLED.md`). Each inbound Call and our reply are recorded.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::RwLock;

use ferrowl_ocpp::cs::CsActionHandler;
use ferrowl_ocpp::v1_6::messages::change_configuration::ChangeConfigurationResponse;
use ferrowl_ocpp::v1_6::messages::get_configuration::GetConfigurationResponse;
use ferrowl_ocpp::v1_6::messages::reset::ResetResponse;
use ferrowl_ocpp::v1_6::messages::set_charging_profile::SetChargingProfileResponse;
use ferrowl_ocpp::v1_6::types::{
    AvailabilityType, ChargingProfileStatus, ConfigurationStatus, KeyValue, ResetResponseStatus,
};
use ferrowl_ocpp::{Action16, CallError, CallErrorCode, Response16, V1_6, Version};

use crate::module::ocpp::client::backend::{Dir, Messages, OcppMessage, OcppSender, push_capped};
use crate::module::ocpp::client::v1_6::state::CsState;
use crate::module::ocpp::lock::HasState;
use crate::module::ocpp::scope::Scope;
use crate::module::ocpp::wire_log::{encode_action_or_log, encode_response_or_log};

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

/// Inbound handler for an OCPP 1.6 charging station, backed by shared [`CsState`]. `sender` lets an
/// accepted remote-start (OC-R-070) dispatch a `StartTransaction` through the same send path the
/// RFID/operator flow uses — see `spawn_remote_transaction_start`.
pub struct CsStateHandler {
    online: Arc<AtomicBool>,
    messages: Messages,
    state: Arc<RwLock<CsState>>,
    sender: OcppSender<V1_6>,
}

impl CsStateHandler {
    pub fn new(
        online: Arc<AtomicBool>,
        messages: Messages,
        state: Arc<RwLock<CsState>>,
        sender: OcppSender<V1_6>,
    ) -> Self {
        Self {
            online,
            messages,
            state,
            sender,
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
    /// Build the response for an inbound action from state (or default-accept), and a log context.
    fn respond(&self, action: &Action16) -> (Result<Response16, CallError>, String) {
        match action {
            Action16::GetConfiguration(req) => self.with_state(|state| {
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
                    Ok(Response16::GetConfiguration(Box::new(resp))),
                    "answered from config".to_string(),
                )
            }),
            Action16::ChangeConfiguration(req) => self.with_state_mut(|state| {
                let status = match state.config.iter_mut().find(|c| c.key == req.key) {
                    Some(c) if c.readonly => ConfigurationStatus::Rejected,
                    Some(c) => {
                        c.value.clone_from(&req.value);
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
                    Ok(Response16::ChangeConfiguration(Box::new(
                        ChangeConfigurationResponse { status },
                    ))),
                    format!("{} = {}", req.key, req.value),
                )
            }),
            Action16::Reset(_) => self.with_state_mut(|state| {
                for c in &mut state.connectors {
                    c.status = "Available".to_string();
                    c.transaction_id = None;
                    c.session_energy = 0.0;
                }
                (
                    Ok(Response16::Reset(Box::new(ResetResponse {
                        status: ResetResponseStatus::Accepted,
                    }))),
                    "state reset".to_string(),
                )
            }),
            Action16::SetChargingProfile(req) => {
                let json = encode_action_or_log::<V1_6>(action);
                let profile = &json["csChargingProfiles"];
                let stack = profile["stackLevel"].as_i64().unwrap_or(0);
                let purpose = profile["chargingProfilePurpose"]
                    .as_str()
                    .unwrap_or("TxProfile")
                    .to_string();
                let schedule = &profile["chargingSchedule"];
                let period = &schedule["chargingSchedulePeriod"][0];
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
                            Response16::SetChargingProfile(Box::new(SetChargingProfileResponse {
                                status: ChargingProfileStatus::Rejected,
                            }));
                        (
                            Ok(resp),
                            format!("rejected: stackLevel {stack} > max {max}"),
                        )
                    } else {
                        // Apply the limit to the targeted connector (fall back to the first),
                        // routed by charging-profile purpose into the matching per-purpose field.
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
                                        c.default_limit_unit.clone_from(&unit);
                                    }
                                    "ChargePointMaxProfile" => {
                                        c.max_limit = Some(limit);
                                        c.max_limit_unit.clone_from(&unit);
                                    }
                                    _ => {
                                        c.limit = Some(limit);
                                        c.limit_unit.clone_from(&unit);
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
                })
            }
            Action16::ReserveNow(req) => {
                let id = req.reservation_id as i64;
                self.with_state_mut(|state| {
                    // connectorId 0 reserves the charge point itself (CS-level); any other id
                    // targets that connector.
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
                })
            }
            Action16::CancelReservation(req) => {
                let id = req.reservation_id as i64;
                self.with_state_mut(|state| {
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
                })
            }
            Action16::ChangeAvailability(req) => {
                let status = match req.kind {
                    AvailabilityType::Operative => "Available",
                    AvailabilityType::Inoperative => "Unavailable",
                };
                self.with_state_mut(|state| {
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
                })
            }
            Action16::RemoteStartTransaction(req) => {
                // OC-R-070 — the response is still built synchronously (Accepted, from the
                // resolved connector), but the actual `StartTransaction` + coupled
                // `StatusNotification` (OC-R-122) are sent asynchronously through the same send
                // path the RFID/operator flow uses, so the CSMS observes them as real Calls
                // rather than a state mutation invisible to the wire.
                let (context, has_target) = self.with_state(|state| {
                    let idx = req
                        .connector_id
                        .and_then(|t| {
                            state
                                .connectors
                                .iter()
                                .position(|c| c.connector_id == t as i64)
                        })
                        .or((!state.connectors.is_empty()).then_some(0));
                    match idx {
                        Some(i) => (
                            format!(
                                "starting tx on connector {}",
                                state.connectors[i].connector_id
                            ),
                            true,
                        ),
                        None => ("no connector to start".to_string(), false),
                    }
                });
                if has_target {
                    let scope = req
                        .connector_id
                        .map_or(Scope::CS, |c| Scope::connector(c as i64));
                    drop(
                        crate::module::ocpp::client::backend::spawn_remote_transaction_start(
                            self.sender.clone(),
                            self.state.clone(),
                            scope,
                        ),
                    );
                }
                let resp = V1_6::default_response("RemoteStartTransaction")
                    .expect("RemoteStartTransaction is a known action");
                (Ok(resp), context)
            }
            Action16::RemoteStopTransaction(req) => {
                let tx = req.transaction_id as i64;
                self.with_state_mut(|state| {
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
                })
            }
            Action16::ClearChargingProfile(req) => {
                let json = encode_action_or_log::<V1_6>(action);
                let purpose = json["chargingProfilePurpose"].as_str().map(str::to_owned);
                self.with_state_mut(|state| {
                    // Optional connectorId; absent clears every connector. The purpose criterion
                    // (when given) selects which per-purpose limit is erased; absent clears all.
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
                })
            }
            Action16::UnlockConnector(req) => self.with_state_mut(|state| {
                if let Some(c) = state.connector_mut(req.connector_id as i64) {
                    c.status = "Available".to_string();
                }
                let resp = V1_6::default_response("UnlockConnector")
                    .expect("UnlockConnector is a known action");
                (Ok(resp), format!("connector {} unlocked", req.connector_id))
            }),
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
        let request = encode_action_or_log::<V1_6>(&action);
        // Reject Calls targeting a connector this station does not have. `with_state` drops the
        // read guard before `respond()` runs (which takes its own write lock) — holding both
        // deadlocks.
        let unknown = self.with_state(|s| unknown_connector(&request, s));
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
            Ok(resp) => encode_response_or_log::<V1_6>(resp),
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
    /// OC-R-063 — an inbound Call naming a connector this CS does not have is rejected with PropertyConstraintViolation.
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
        let messages = Arc::new(tokio::sync::RwLock::new(Vec::<OcppMessage>::new()));
        let handler = CsStateHandler::new(
            Arc::new(AtomicBool::new(false)),
            messages.clone(),
            Arc::new(RwLock::new(CsState::default())),
            OcppSender::<V1_6>::detached(messages),
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
        handler_over(Arc::new(RwLock::new(state)))
    }

    /// A `CsStateHandler` sharing `state` with the caller (a `RemoteStopTransaction` drive doesn't
    /// need a live sender — it's a synchronous state mutation, unlike `RemoteStartTransaction`).
    fn handler_over(state: Arc<RwLock<CsState>>) -> CsStateHandler {
        use std::sync::atomic::AtomicBool;
        let messages = Arc::new(tokio::sync::RwLock::new(Vec::<OcppMessage>::new()));
        CsStateHandler::new(
            Arc::new(AtomicBool::new(false)),
            messages.clone(),
            state,
            OcppSender::<V1_6>::detached(messages),
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
    /// OC-R-069 — a reservation is recorded at the connector level the request targets.
    fn ut_reserve_now_targets_connector_not_cs() {
        let h = handler_with(two_connectors());
        drive(
            &h,
            "ReserveNow",
            json!({ "connectorId": 2, "expiryDate": "2030-01-01T00:00:00Z",
                    "idTag": "TAG1", "reservationId": 42 }),
        );
        let st = h.state.read();
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
    /// OC-R-069 — connector id 0 records the reservation at the charge-point level (OC-R-063: id 0 is the charge point).
    fn ut_reserve_now_connector_zero_is_cs_level() {
        let h = handler_with(CsState::default());
        drive(
            &h,
            "ReserveNow",
            json!({ "connectorId": 0, "expiryDate": "2030-01-01T00:00:00Z",
                    "idTag": "CP", "reservationId": 1 }),
        );
        let st = h.state.read();
        assert_eq!(st.reserved_rfid.as_deref(), Some("CP"));
        assert_eq!(st.reservation_id, Some(1));
        assert!(st.connector(1).unwrap().reserved_rfid.is_none());
    }

    #[test]
    /// OC-R-069 — a cancellation carrying the same reservation id clears the reservation at whichever level holds it.
    fn ut_cancel_reservation_clears_matching_connector() {
        let h = handler_with(two_connectors());
        drive(
            &h,
            "ReserveNow",
            json!({ "connectorId": 2, "expiryDate": "2030-01-01T00:00:00Z",
                    "idTag": "T", "reservationId": 7 }),
        );
        drive(&h, "CancelReservation", json!({ "reservationId": 7 }));
        let st = h.state.read();
        assert!(st.connector(2).unwrap().reserved_rfid.is_none());
        assert!(st.connector(2).unwrap().reservation_id.is_none());
    }

    #[test]
    /// OC-R-063 — connector id 0 means the charge point itself, so ChangeAvailability at 0 targets every connector.
    fn ut_change_availability_status_and_zero_targets_all() {
        let h = handler_with(two_connectors());
        drive(
            &h,
            "ChangeAvailability",
            json!({ "connectorId": 2, "type": "Inoperative" }),
        );
        assert_eq!(h.state.read().connector(2).unwrap().status, "Unavailable");
        drive(
            &h,
            "ChangeAvailability",
            json!({ "connectorId": 0, "type": "Operative" }),
        );
        let st = h.state.read();
        assert!(st.connectors.iter().all(|c| c.status == "Available"));
    }

    // --- OC-R-070 / OC-R-122: remote-start is visible to the CSMS -----------------------------
    //
    // A real websocket loopback (mirrors `ferrowl-ocpp/tests/ws_loopback_v16.rs`): the CSMS side
    // sends the inbound `RemoteStartTransaction` Call and records every Call it receives back
    // from the CS in order, notifying once it has seen `StatusNotification`.

    /// No-op log sink for the CSMS side.
    fn sink() -> impl ferrowl_ocpp::LogFn + Clone {
        |_s: String| async move {}
    }

    /// Poll until the CSMS listener has bound (`spawn` retries the bind in the background).
    async fn bound_addr(server: &ferrowl_ocpp::csms::Server<V1_6>) -> std::net::SocketAddr {
        for _ in 0..50 {
            if let Some(addr) = server.local_addr() {
                return addr;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("CSMS listener never bound");
    }

    /// Wait until the server registry reports at least one connection, then return its id.
    async fn first_connection(
        server: &ferrowl_ocpp::csms::Server<V1_6>,
    ) -> ferrowl_ocpp::csms::ConnectionId {
        for _ in 0..50 {
            if let Some(id) = server.registry().connection_ids().first().copied() {
                return id;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("no CS connected in time");
    }

    /// CSMS handler answering `StartTransaction`/`StatusNotification`, recording the ordered list
    /// of Call names it receives into `calls` and notifying `notify` once it sees
    /// `StatusNotification`. Any other action is left unanswered by `notify`, letting a caller
    /// that only cares about the send-failure path leave the send hanging (never Ok, never
    /// recorded) by simply not spawning this handler at all.
    struct RecordingCsms {
        calls: Arc<parking_lot::Mutex<Vec<String>>>,
        notify: Arc<tokio::sync::Notify>,
    }

    impl ferrowl_ocpp::csms::CsmsActionHandler<V1_6> for RecordingCsms {
        async fn handle_call(
            &self,
            _conn: ferrowl_ocpp::csms::ConnectionId,
            action: Action16,
        ) -> Result<Response16, CallError> {
            let name = match &action {
                Action16::StartTransaction(_) => "StartTransaction",
                Action16::StatusNotification(_) => "StatusNotification",
                _ => "Other",
            };
            self.calls.lock().push(name.to_string());
            if name == "StatusNotification" {
                self.notify.notify_one();
            }
            match action {
                Action16::StartTransaction(_) => Ok(Response16::StartTransaction(
                    serde_json::from_value(json!({
                        "idTagInfo": { "status": "Accepted" },
                        "transactionId": 42,
                    }))
                    .unwrap(),
                )),
                Action16::StatusNotification(_) => Ok(Response16::StatusNotification(
                    serde_json::from_value(json!({})).unwrap(),
                )),
                _ => Err(CallError::new(CallErrorCode::NotImplemented, "unsupported")),
            }
        }
    }

    /// Spawn a CSMS server on an OS-assigned port, recording Calls into `calls`/`notify`.
    async fn start_server(
        calls: Arc<parking_lot::Mutex<Vec<String>>>,
        notify: Arc<tokio::sync::Notify>,
    ) -> ferrowl_ocpp::csms::Server<V1_6> {
        ferrowl_ocpp::csms::ServerBuilder::<V1_6>::new(
            ferrowl_ocpp::csms::Config {
                host: "127.0.0.1".to_owned(),
                port: 0,
                timeout_ms: 2000,
                reconnect: true,
                basic_auth: None,
                tls: None,
            },
            ferrowl_ocpp::new_self_signed_cache(),
        )
        .spawn(RecordingCsms { calls, notify }, sink())
        .await
        .expect("server failed to bind")
    }

    /// Connect a real `OcppClient<V1_6>` to `server`, backed by `state`. The handler's `sender`
    /// is captured before `start()`, so the returned client is usable straight away.
    async fn connected_client(
        server: &ferrowl_ocpp::csms::Server<V1_6>,
        state: Arc<RwLock<CsState>>,
    ) -> crate::module::ocpp::client::backend::OcppClient<V1_6> {
        use crate::module::ocpp::client::backend::OcppClient;
        use crate::module::ocpp::config::device::OcppDeviceConfig;
        use crate::module::ocpp::config::session::{OcppProtocol, OcppRole, OcppSpec, OcppVersion};

        let addr = bound_addr(server).await;
        let spec = OcppSpec {
            name: "cs".into(),
            version: OcppVersion::V1_6,
            role: OcppRole::Client,
            protocol: OcppProtocol::Ws,
            ip: "127.0.0.1".into(),
            port: addr.port(),
            path: String::new(),
            timeout_ms: None,
            reconnect: None,
            security: Default::default(),
        };
        let device = OcppDeviceConfig::default();
        let log: crate::module::view::SharedLog =
            Arc::new(tokio::sync::RwLock::new(crate::app::LogRing::init()));
        let mut client = OcppClient::<V1_6>::new();
        let sender = client.sender();
        let handler = CsStateHandler::new(
            client.online_handle(),
            client.messages_handle(),
            state,
            sender,
        );
        client
            .start(&spec, &device, &log, handler)
            .await
            .expect("client failed to connect");
        for _ in 0..100 {
            if client.is_online() {
                return client;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("client never came online");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    /// OC-R-070 — a remote start mints a transaction id and sets the connector charging, sending a
    /// real `StartTransaction` to the CSMS (not just a local state mutation).
    /// OC-R-122 — the `StartTransaction` is followed by a coupled `StatusNotification`.
    /// A remote stop clears the transaction and returns to available.
    async fn ut_remote_start_then_stop_transaction() {
        let calls = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let notify = Arc::new(tokio::sync::Notify::new());
        let server = start_server(calls.clone(), notify.clone()).await;
        let state = Arc::new(RwLock::new(two_connectors()));
        let mut client = connected_client(&server, state.clone()).await;

        let conn = first_connection(&server).await;
        let remote_start = Action16::RemoteStartTransaction(
            serde_json::from_value(json!({ "connectorId": 1, "idTag": "T" })).unwrap(),
        );
        let resp = server
            .call(conn, remote_start)
            .await
            .expect("remote start call failed");
        assert!(matches!(resp, Response16::RemoteStartTransaction(_)));

        tokio::time::timeout(std::time::Duration::from_secs(5), notify.notified())
            .await
            .expect("StatusNotification never sent");
        assert_eq!(
            *calls.lock(),
            vec![
                "StartTransaction".to_string(),
                "StatusNotification".to_string()
            ],
        );
        let tx = {
            let st = state.read();
            let c = st.connector(1).unwrap();
            assert_eq!(c.status, "Charging");
            c.transaction_id.expect("transaction assigned")
        };

        drive(
            &handler_over(state.clone()),
            "RemoteStopTransaction",
            json!({ "transactionId": tx }),
        );
        {
            let st = state.read();
            assert!(st.connector(1).unwrap().transaction_id.is_none());
            assert_eq!(st.connector(1).unwrap().status, "Available");
        }

        client.stop().await.expect("client stop");
        server.terminate().await.expect("server terminate");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    /// OC-R-070 — a remote start with no `connectorId` targets the first connector, matching
    /// `ClientVersion::state_payload`'s existing "no scope → first connector" fallback.
    async fn ut_remote_start_with_no_target_uses_first_connector() {
        let calls = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let notify = Arc::new(tokio::sync::Notify::new());
        let server = start_server(calls.clone(), notify.clone()).await;
        let state = Arc::new(RwLock::new(two_connectors()));
        let mut client = connected_client(&server, state.clone()).await;

        let conn = first_connection(&server).await;
        let remote_start = Action16::RemoteStartTransaction(
            serde_json::from_value(json!({ "idTag": "T" })).unwrap(),
        );
        let resp = server
            .call(conn, remote_start)
            .await
            .expect("remote start call failed");
        assert!(matches!(resp, Response16::RemoteStartTransaction(_)));

        tokio::time::timeout(std::time::Duration::from_secs(5), notify.notified())
            .await
            .expect("StatusNotification never sent");
        {
            let st = state.read();
            assert_eq!(st.connector(1).unwrap().status, "Charging");
            assert!(st.connector(1).unwrap().transaction_id.is_some());
            assert_eq!(st.connector(2).unwrap().status, "Available");
        }

        client.stop().await.expect("client stop");
        server.terminate().await.expect("server terminate");
    }

    /// CSMS handler that rejects `StartTransaction` (simulating a send failure), recording it and
    /// notifying immediately — there is no coupled `StatusNotification` to wait for, since the
    /// transaction-start itself failed.
    struct FailingStartCsms {
        calls: Arc<parking_lot::Mutex<Vec<String>>>,
        notify: Arc<tokio::sync::Notify>,
    }

    impl ferrowl_ocpp::csms::CsmsActionHandler<V1_6> for FailingStartCsms {
        async fn handle_call(
            &self,
            _conn: ferrowl_ocpp::csms::ConnectionId,
            action: Action16,
        ) -> Result<Response16, CallError> {
            if let Action16::StartTransaction(_) = &action {
                self.calls.lock().push("StartTransaction".to_string());
                self.notify.notify_one();
            }
            Err(CallError::new(CallErrorCode::InternalError, "refused"))
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    /// OC-R-070 — when the CSMS rejects the resulting `StartTransaction`, the connector is left
    /// exactly as it was (1.6 never optimistically mutates state before the response, unlike 2.x's
    /// `started_tx` fast-path, so there is nothing for `rollback_tx` to undo here — this proves the
    /// negative: no stale `transaction_id`/`"Charging"` status is left behind).
    async fn ut_remote_start_send_failure_does_not_leave_stale_transaction() {
        let calls = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let notify = Arc::new(tokio::sync::Notify::new());
        let server = ferrowl_ocpp::csms::ServerBuilder::<V1_6>::new(
            ferrowl_ocpp::csms::Config {
                host: "127.0.0.1".to_owned(),
                port: 0,
                timeout_ms: 2000,
                reconnect: true,
                basic_auth: None,
                tls: None,
            },
            ferrowl_ocpp::new_self_signed_cache(),
        )
        .spawn(
            FailingStartCsms {
                calls: calls.clone(),
                notify: notify.clone(),
            },
            sink(),
        )
        .await
        .expect("server failed to bind");
        let state = Arc::new(RwLock::new(two_connectors()));
        let mut client = connected_client(&server, state.clone()).await;

        let conn = first_connection(&server).await;
        let remote_start = Action16::RemoteStartTransaction(
            serde_json::from_value(json!({ "connectorId": 1, "idTag": "T" })).unwrap(),
        );
        let resp = server
            .call(conn, remote_start)
            .await
            .expect("remote start call failed");
        assert!(matches!(resp, Response16::RemoteStartTransaction(_)));

        tokio::time::timeout(std::time::Duration::from_secs(5), notify.notified())
            .await
            .expect("StartTransaction attempt never observed");
        assert_eq!(*calls.lock(), vec!["StartTransaction".to_string()]);

        // Give the failed send's rollback a moment to run before asserting nothing changed.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        {
            let st = state.read();
            assert!(st.connector(1).unwrap().transaction_id.is_none());
            assert_eq!(st.connector(1).unwrap().status, "Available");
        }

        client.stop().await.expect("client stop");
        server.terminate().await.expect("server terminate");
    }

    #[test]
    /// OC-R-068 — clearing charging profiles with no purpose erases every per-purpose limit on the connector.
    fn ut_clear_profile_and_unlock_connector() {
        let mut s = two_connectors();
        s.connector_mut(1).unwrap().limit = Some(16.0);
        s.connector_mut(1).unwrap().status = "Unavailable".to_string();
        let h = handler_with(s);
        drive(&h, "ClearChargingProfile", json!({}));
        assert!(h.state.read().connector(1).unwrap().limit.is_none());
        drive(&h, "UnlockConnector", json!({ "connectorId": 1 }));
        assert_eq!(h.state.read().connector(1).unwrap().status, "Available");
    }

    #[test]
    /// OC-R-068 — clearing charging profiles by purpose erases only the per-purpose limit matching that criterion.
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
            let st = h.state.read();
            let c = st.connector(1).unwrap();
            assert_eq!(c.limit, Some(16.0));
            assert_eq!(c.default_limit, None);
            assert_eq!(c.max_limit, Some(32.0));
        }
        // No purpose criterion clears every per-purpose limit.
        drive(&h, "ClearChargingProfile", json!({}));
        let st = h.state.read();
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
    /// OC-R-067 — a charging-profile installation whose stack level exceeds the configured maximum is rejected.
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
        assert_eq!(h.state.read().connector(1).unwrap().limit, None);
    }

    #[test]
    /// OC-R-067 — an accepted charging profile applies its limit to the targeted connector under the field matching its purpose.
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
        let st = h.state.read();
        let c = st.connector(1).unwrap();
        assert_eq!(c.limit, Some(16.0));
        assert_eq!(c.default_limit, Some(10.0));
        assert_eq!(c.max_limit, Some(32.0));
    }

    #[test]
    /// OC-R-072 — ending a transaction clears only the transaction-scoped limit; the default and maximum limits persist.
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
        let st = h.state.read();
        let c = st.connector(1).unwrap();
        assert_eq!(c.limit, None, "TxProfile limit cleared on stop");
        assert_eq!(c.default_limit, Some(10.0), "default limit persists");
        assert_eq!(c.max_limit, Some(32.0), "max limit persists");
    }
}
