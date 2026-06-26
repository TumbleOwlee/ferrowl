//! Low-level layer: a CSMS server and a CS client exchange Calls in both directions over a real
//! websocket loopback, OCPP 2.1. Mirrors `ws_loopback_v201.rs`; its primary job is to prove the
//! `ocpp2.1` subprotocol is negotiated end-to-end (regression guard for the 400-Bad-Request bug).

#![cfg(feature = "v2_1")]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ferrowl_ocpp::cs::{self, CsActionHandler};
use ferrowl_ocpp::csms::{self, CsmsActionHandler};
use ferrowl_ocpp::{Action21, CallError, CallErrorCode, Response21, V2_1};
use serde_json::json;
use tokio::time::sleep;

/// No-op log sink.
fn sink() -> impl ferrowl_ocpp::LogFn + Clone {
    |_s: String| async move {}
}

/// CSMS handler answering the two CS-initiated actions used by this test.
struct TestCsms;

impl CsmsActionHandler<V2_1> for TestCsms {
    async fn handle_call(
        &self,
        _conn: csms::ConnectionId,
        action: Action21,
    ) -> Result<Response21, CallError> {
        match action {
            Action21::BootNotification(_) => Ok(Response21::BootNotification(
                serde_json::from_value(json!({
                    "currentTime": "2026-01-01T00:00:00Z",
                    "interval": 300,
                    "status": "Accepted"
                }))
                .unwrap(),
            )),
            Action21::Heartbeat(_) => Ok(Response21::Heartbeat(
                serde_json::from_value(json!({ "currentTime": "2026-01-01T00:00:00Z" })).unwrap(),
            )),
            _ => Err(CallError::new(CallErrorCode::NotImplemented, "unsupported")),
        }
    }
}

/// CS handler that records whether it received a server-initiated `ClearCache`.
#[derive(Clone)]
struct TestCs {
    clear_cache_seen: Arc<AtomicBool>,
}

impl CsActionHandler<V2_1> for TestCs {
    async fn handle_call(&self, action: Action21) -> Result<Response21, CallError> {
        match action {
            Action21::ClearCache(_) => {
                self.clear_cache_seen.store(true, Ordering::SeqCst);
                Ok(Response21::ClearCache(
                    serde_json::from_value(json!({ "status": "Accepted" })).unwrap(),
                ))
            }
            _ => Err(CallError::new(CallErrorCode::NotImplemented, "unsupported")),
        }
    }
}

/// Spawn a CSMS server on an OS-assigned port and return it.
async fn start_server() -> csms::Server<V2_1> {
    csms::ServerBuilder::<V2_1>::new(csms::Config {
        host: "127.0.0.1".to_owned(),
        port: 0,
        timeout_ms: 2000,
    })
    .spawn(TestCsms, sink())
    .await
    .expect("server failed to bind")
}

/// Wait until the server registry reports at least one connection, then return its id.
async fn first_connection(server: &csms::Server<V2_1>) -> csms::ConnectionId {
    for _ in 0..50 {
        if let Some(id) = server.registry().connection_ids().first().copied() {
            return id;
        }
        sleep(Duration::from_millis(20)).await;
    }
    panic!("no CS connected in time");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cs_calls_csms_and_csms_calls_cs() {
    let server = start_server().await;
    let url = format!("ws://{}/ocpp/CS001", server.local_addr());

    let clear_cache_seen = Arc::new(AtomicBool::new(false));
    // A successful connect here is the subprotocol regression guard: the client advertises
    // `ocpp2.1` and the server must accept it (previously it could end up bound as `ocpp1.6`).
    let client = cs::ClientBuilder::<V2_1>::new(cs::Config {
        url,
        timeout_ms: 2000,
    })
    .spawn(
        TestCs {
            clear_cache_seen: clear_cache_seen.clone(),
        },
        sink(),
    )
    .await
    .expect("client failed to connect");

    // CS -> CSMS: BootNotification.
    let boot = Action21::BootNotification(
        serde_json::from_value(json!({
            "reason": "PowerUp",
            "chargingStation": {
                "model": "Model-1",
                "vendorName": "Ferrowl"
            }
        }))
        .unwrap(),
    );
    let resp = client.call(boot).await.expect("boot call failed");
    match resp {
        Response21::BootNotification(r) => {
            let v = serde_json::to_value(&r).unwrap();
            assert_eq!(v["status"], "Accepted");
        }
        _ => panic!("unexpected response variant"),
    }

    // CS -> CSMS: Heartbeat.
    let hb = Action21::Heartbeat(serde_json::from_value(json!({})).unwrap());
    assert!(matches!(
        client.call(hb).await.unwrap(),
        Response21::Heartbeat(_)
    ));

    // CSMS -> CS: server-initiated ClearCache (reverse direction).
    let conn = first_connection(&server).await;
    let clear = Action21::ClearCache(serde_json::from_value(json!({})).unwrap());
    let resp = server
        .call(conn, clear)
        .await
        .expect("clear cache call failed");
    assert!(matches!(resp, Response21::ClearCache(_)));
    assert!(
        clear_cache_seen.load(Ordering::SeqCst),
        "CS handler should have seen the server-initiated call"
    );

    // Graceful shutdown of both sides.
    client.terminate().await.expect("client terminate failed");
    server.terminate().await.expect("server terminate failed");
}
