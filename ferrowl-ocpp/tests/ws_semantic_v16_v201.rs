//! Semantic layer: the same simulation logic, written once against the version-agnostic semantic
//! traits, runs against both OCPP 1.6 and 2.0.1. Two near-identical test functions (`_v16` /
//! `_v201`) share generic helpers.

#![cfg(all(feature = "v1_6", feature = "v2_0_1"))]

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ferrowl_ocpp::Version;
use ferrowl_ocpp::cs::{self, CsActionHandler, CsHandler, CsOps};
use ferrowl_ocpp::csms::{self, CsmsActionHandler, CsmsHandler, CsmsOps};
use ferrowl_ocpp::types::*;
use ferrowl_ocpp::{CallError, CallErrorCode};
use tokio::time::sleep;

const TS: &str = "2026-01-01T00:00:00Z";

fn sink() -> impl ferrowl_ocpp::LogFn + Clone {
    |_s: String| async move {}
}

// ----- low-level recording handlers (capture the wire action name, then reject) -----------------

struct RecordCsms<V> {
    seen: Arc<Mutex<Option<String>>>,
    _v: PhantomData<fn() -> V>,
}

impl<V: Version> CsmsActionHandler<V> for RecordCsms<V> {
    async fn handle_call(
        &self,
        _conn: csms::ConnectionId,
        action: V::Action,
    ) -> Result<V::Response, CallError> {
        *self.seen.lock().unwrap() = Some(V::action_name(&action).to_owned());
        Err(CallError::new(CallErrorCode::InternalError, "recorded"))
    }
}

struct RecordCs<V> {
    seen: Arc<Mutex<Option<String>>>,
    _v: PhantomData<fn() -> V>,
}

impl<V: Version> CsActionHandler<V> for RecordCs<V> {
    async fn handle_call(&self, action: V::Action) -> Result<V::Response, CallError> {
        *self.seen.lock().unwrap() = Some(V::action_name(&action).to_owned());
        Err(CallError::new(CallErrorCode::InternalError, "recorded"))
    }
}

// ----- semantic handlers (version-agnostic, written once) ---------------------------------------

struct SemanticCsms;

impl CsmsHandler for SemanticCsms {
    async fn on_start_transaction(
        &self,
        _conn: csms::ConnectionId,
        _params: StartTransactionParams,
    ) -> Result<StartTransactionResult, CallError> {
        Ok(StartTransactionResult {
            transaction_id: "42".to_owned(),
            status: "Accepted".to_owned(),
        })
    }
}

struct SemanticCs {
    store: Arc<Mutex<HashMap<String, String>>>,
}

impl CsHandler for SemanticCs {
    async fn on_set_config(&self, params: SetConfigParams) -> Result<SetConfigResult, CallError> {
        let mut store = self.store.lock().unwrap();
        let mut results = Vec::new();
        for entry in params.entries {
            store.insert(entry.key.clone(), entry.value.clone());
            results.push(ConfigSetResult {
                key: entry.key,
                status: "Accepted".to_owned(),
            });
        }
        Ok(SetConfigResult { results })
    }
}

// ----- helpers ----------------------------------------------------------------------------------

async fn first_connection<V: Version>(server: &csms::Server<V>) -> csms::ConnectionId {
    for _ in 0..50 {
        if let Some(id) = server.registry().connection_ids().first().copied() {
            return id;
        }
        sleep(Duration::from_millis(20)).await;
    }
    panic!("no CS connected in time");
}

fn server_config() -> csms::Config {
    csms::Config {
        host: "127.0.0.1".to_owned(),
        port: 0,
        timeout_ms: 2000,
    }
}

fn start_params() -> StartTransactionParams {
    StartTransactionParams {
        connector_id: 1,
        id_tag: "TAG1".to_owned(),
        meter_start: 0,
        timestamp: TS.to_owned(),
    }
}

fn heartbeat_interval_60() -> SetConfigParams {
    SetConfigParams {
        entries: vec![ConfigEntry {
            key: "HeartbeatInterval".to_owned(),
            value: "60".to_owned(),
        }],
    }
}

/// Outbound mapping: assert the merged semantic methods produce the version-correct wire action.
async fn assert_outbound_wire<V>(expected_start: &str, expected_config: &str)
where
    V: Version,
    V::Action: Clone,
    cs::Client<V>: CsOps,
    csms::Server<V>: CsmsOps,
{
    let csms_seen = Arc::new(Mutex::new(None));
    let server = csms::ServerBuilder::<V>::new(server_config())
        .spawn(
            RecordCsms::<V> {
                seen: csms_seen.clone(),
                _v: PhantomData,
            },
            sink(),
        )
        .await
        .expect("server bind");
    let url = format!("ws://{}/ocpp/CS001", server.local_addr());

    let cs_seen = Arc::new(Mutex::new(None));
    let client = cs::ClientBuilder::<V>::new(cs::Config {
        url,
        timeout_ms: 2000,
    })
    .spawn(
        RecordCs::<V> {
            seen: cs_seen.clone(),
            _v: PhantomData,
        },
        sink(),
    )
    .await
    .expect("client connect");

    // CS -> CSMS: start_transaction merges to the version's wire action.
    let _ = client.start_transaction(start_params()).await;
    assert_eq!(
        csms_seen.lock().unwrap().as_deref(),
        Some(expected_start),
        "start_transaction wire action"
    );

    // CSMS -> CS: set_config merges to the version's wire action.
    let conn = first_connection(&server).await;
    let _ = server.set_config(conn, heartbeat_interval_60()).await;
    assert_eq!(
        cs_seen.lock().unwrap().as_deref(),
        Some(expected_config),
        "set_config wire action"
    );

    client.terminate().await.expect("client terminate");
    server.terminate().await.expect("server terminate");
}

/// Full round trip through both inbound adapters and both outbound impls, written once.
async fn assert_semantic_round_trip<V>()
where
    V: Version,
    V::Action: Clone,
    cs::Client<V>: CsOps,
    csms::Server<V>: CsmsOps,
    cs::SemanticAdapter<V, SemanticCs>: CsActionHandler<V>,
    csms::SemanticAdapter<V, SemanticCsms>: CsmsActionHandler<V>,
{
    let server = csms::ServerBuilder::<V>::new(server_config())
        .spawn(csms::SemanticAdapter::<V, _>::new(SemanticCsms), sink())
        .await
        .expect("server bind");
    let url = format!("ws://{}/ocpp/CS001", server.local_addr());

    let store = Arc::new(Mutex::new(HashMap::new()));
    let client = cs::ClientBuilder::<V>::new(cs::Config {
        url,
        timeout_ms: 2000,
    })
    .spawn(
        cs::SemanticAdapter::<V, _>::new(SemanticCs {
            store: store.clone(),
        }),
        sink(),
    )
    .await
    .expect("client connect");

    // CS -> CSMS start_transaction: succeeds on both versions via the inbound CsmsHandler adapter.
    let result = client
        .start_transaction(start_params())
        .await
        .expect("start_transaction should succeed");
    assert!(!result.transaction_id.is_empty());
    assert_eq!(result.status, "Accepted");

    // CSMS -> CS set_config: reaches the CsHandler adapter and updates its store on both versions.
    let conn = first_connection(&server).await;
    let set = server
        .set_config(conn, heartbeat_interval_60())
        .await
        .expect("set_config should succeed");
    assert_eq!(
        set.results.first().map(|r| r.status.as_str()),
        Some("Accepted")
    );
    assert_eq!(
        store
            .lock()
            .unwrap()
            .get("HeartbeatInterval")
            .map(String::as_str),
        Some("60")
    );

    client.terminate().await.expect("client terminate");
    server.terminate().await.expect("server terminate");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn semantic_wire_mapping_v16() {
    use ferrowl_ocpp::V1_6;
    assert_outbound_wire::<V1_6>("StartTransaction", "ChangeConfiguration").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn semantic_wire_mapping_v201() {
    use ferrowl_ocpp::V2_0_1;
    assert_outbound_wire::<V2_0_1>("TransactionEvent", "SetVariables").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn semantic_round_trip_v16() {
    use ferrowl_ocpp::V1_6;
    assert_semantic_round_trip::<V1_6>().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn semantic_round_trip_v201() {
    use ferrowl_ocpp::V2_0_1;
    assert_semantic_round_trip::<V2_0_1>().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn v201_only_method_not_supported_under_v16() {
    use ferrowl_ocpp::V1_6;
    let server = csms::ServerBuilder::<V1_6>::new(server_config())
        .spawn(csms::SemanticAdapter::<V1_6, _>::new(SemanticCsms), sink())
        .await
        .expect("server bind");
    let url = format!("ws://{}/ocpp/CS001", server.local_addr());
    let store = Arc::new(Mutex::new(HashMap::new()));
    let client = cs::ClientBuilder::<V1_6>::new(cs::Config {
        url,
        timeout_ms: 2000,
    })
    .spawn(
        cs::SemanticAdapter::<V1_6, _>::new(SemanticCs { store }),
        sink(),
    )
    .await
    .expect("client connect");

    // notify_event is v2.0.1-only; on a V1_6 client it must return NotSupported, not hang or panic.
    let result = client
        .notify_event(NotifyEventParams {
            generated_at: TS.to_owned(),
            seq_no: 0,
            event_data: serde_json::json!([]),
        })
        .await;
    assert!(matches!(result, Err(ferrowl_ocpp::Error::NotSupported)));

    client.terminate().await.expect("client terminate");
    server.terminate().await.expect("server terminate");
}

// ----- SetChargingProfile (both versions support it) --------------------------------------------

struct NoopCsms<V> {
    _v: PhantomData<fn() -> V>,
}

impl<V: Version> CsmsActionHandler<V> for NoopCsms<V> {
    async fn handle_call(
        &self,
        _conn: csms::ConnectionId,
        _action: V::Action,
    ) -> Result<V::Response, CallError> {
        Err(CallError::new(CallErrorCode::NotImplemented, "noop"))
    }
}

struct ChargingCs {
    seen: Arc<Mutex<Option<serde_json::Value>>>,
}

impl CsHandler for ChargingCs {
    async fn on_set_charging_profile(
        &self,
        params: SetChargingProfileParams,
    ) -> Result<GenericStatusResult, CallError> {
        *self.seen.lock().unwrap() = Some(params.charging_profile);
        Ok(GenericStatusResult {
            status: "Accepted".to_owned(),
        })
    }
}

async fn assert_set_charging_profile<V>(profile: serde_json::Value, id_key: &str)
where
    V: Version,
    V::Action: Clone,
    csms::Server<V>: CsmsOps,
    cs::SemanticAdapter<V, ChargingCs>: CsActionHandler<V>,
{
    let server = csms::ServerBuilder::<V>::new(server_config())
        .spawn(NoopCsms::<V> { _v: PhantomData }, sink())
        .await
        .expect("server bind");
    let url = format!("ws://{}/ocpp/CS001", server.local_addr());

    let seen = Arc::new(Mutex::new(None));
    let client = cs::ClientBuilder::<V>::new(cs::Config {
        url,
        timeout_ms: 2000,
    })
    .spawn(
        cs::SemanticAdapter::<V, _>::new(ChargingCs { seen: seen.clone() }),
        sink(),
    )
    .await
    .expect("client connect");

    let conn = first_connection(&server).await;
    let result = server
        .set_charging_profile(
            conn,
            SetChargingProfileParams {
                connector_id: Some(1),
                evse_id: Some(1),
                charging_profile: profile,
            },
        )
        .await
        .expect("set_charging_profile should succeed");
    assert_eq!(result.status, "Accepted");

    let received = seen
        .lock()
        .unwrap()
        .clone()
        .expect("CS handler saw the profile");
    assert_eq!(
        received[id_key].as_i64(),
        Some(1),
        "profile id round-tripped"
    );

    client.terminate().await.expect("client terminate");
    server.terminate().await.expect("server terminate");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn set_charging_profile_v16() {
    use ferrowl_ocpp::V1_6;
    let profile = serde_json::json!({
        "chargingProfileId": 1,
        "stackLevel": 0,
        "chargingProfilePurpose": "TxProfile",
        "chargingProfileKind": "Absolute",
        "chargingSchedule": {
            "chargingRateUnit": "A",
            "chargingSchedulePeriod": [{ "startPeriod": 0, "limit": 16.0 }]
        }
    });
    assert_set_charging_profile::<V1_6>(profile, "chargingProfileId").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn set_charging_profile_v201() {
    use ferrowl_ocpp::V2_0_1;
    let profile = serde_json::json!({
        "id": 1,
        "stackLevel": 0,
        "chargingProfilePurpose": "TxProfile",
        "chargingProfileKind": "Absolute",
        "chargingSchedule": [{
            "id": 1,
            "chargingRateUnit": "A",
            "chargingSchedulePeriod": [{ "startPeriod": 0, "limit": 16.0 }]
        }]
    });
    assert_set_charging_profile::<V2_0_1>(profile, "id").await;
}
