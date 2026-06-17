//! Low-level layer: a CSMS server and a CS client exchange Calls in both directions over a real
//! websocket loopback, OCPP 1.6. Mirrors `ferrowl-modbus/tests/tcp_loopback.rs`.

#![cfg(feature = "v1_6")]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ferrowl_ocpp::cs::{self, CsActionHandler};
use ferrowl_ocpp::csms::{self, CsmsActionHandler};
use ferrowl_ocpp::{Action16, CallError, CallErrorCode, Response16, V1_6};
use serde_json::json;
use tokio::time::sleep;

/// No-op log sink.
fn sink() -> impl ferrowl_ocpp::LogFn + Clone {
    |_s: String| async move {}
}

/// CSMS handler answering the three CS-initiated actions used by this test.
struct TestCsms;

impl CsmsActionHandler<V1_6> for TestCsms {
    async fn handle_call(
        &self,
        _conn: csms::ConnectionId,
        action: Action16,
    ) -> Result<Response16, CallError> {
        match action {
            Action16::BootNotification(_) => Ok(Response16::BootNotification(
                serde_json::from_value(json!({
                    "currentTime": "2026-01-01T00:00:00Z",
                    "interval": 300,
                    "status": "Accepted"
                }))
                .unwrap(),
            )),
            Action16::Heartbeat(_) => Ok(Response16::Heartbeat(
                serde_json::from_value(json!({ "currentTime": "2026-01-01T00:00:00Z" })).unwrap(),
            )),
            Action16::StatusNotification(_) => Ok(Response16::StatusNotification(
                serde_json::from_value(json!({})).unwrap(),
            )),
            _ => Err(CallError::new(CallErrorCode::NotImplemented, "unsupported")),
        }
    }
}

/// CS handler that records whether it received a server-initiated `RemoteStartTransaction`.
#[derive(Clone)]
struct TestCs {
    remote_start_seen: Arc<AtomicBool>,
}

impl CsActionHandler<V1_6> for TestCs {
    async fn handle_call(&self, action: Action16) -> Result<Response16, CallError> {
        match action {
            Action16::RemoteStartTransaction(_) => {
                self.remote_start_seen.store(true, Ordering::SeqCst);
                Ok(Response16::RemoteStartTransaction(
                    serde_json::from_value(json!({ "status": "Accepted" })).unwrap(),
                ))
            }
            _ => Err(CallError::new(CallErrorCode::NotImplemented, "unsupported")),
        }
    }
}

/// Spawn a CSMS server on an OS-assigned port and return it.
async fn start_server() -> csms::Server<V1_6> {
    csms::ServerBuilder::<V1_6>::new(csms::Config {
        host: "127.0.0.1".to_owned(),
        port: 0,
        timeout_ms: 2000,
    })
    .spawn(TestCsms, sink())
    .await
    .expect("server failed to bind")
}

/// Wait until the server registry reports at least one connection, then return its id.
async fn first_connection(server: &csms::Server<V1_6>) -> csms::ConnectionId {
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

    let remote_start_seen = Arc::new(AtomicBool::new(false));
    let client = cs::ClientBuilder::<V1_6>::new(cs::Config {
        url,
        timeout_ms: 2000,
    })
    .spawn(
        TestCs {
            remote_start_seen: remote_start_seen.clone(),
        },
        sink(),
    )
    .await
    .expect("client failed to connect");

    // CS -> CSMS: BootNotification.
    let boot = Action16::BootNotification(
        serde_json::from_value(json!({
            "chargePointModel": "Model-1",
            "chargePointVendor": "Ferrowl"
        }))
        .unwrap(),
    );
    let resp = client.call(boot).await.expect("boot call failed");
    match resp {
        Response16::BootNotification(r) => {
            let v = serde_json::to_value(&r).unwrap();
            assert_eq!(v["status"], "Accepted");
        }
        _ => panic!("unexpected response variant"),
    }

    // CS -> CSMS: Heartbeat and StatusNotification (prove macro dispatch over several variants).
    let hb = Action16::Heartbeat(serde_json::from_value(json!({})).unwrap());
    assert!(matches!(
        client.call(hb).await.unwrap(),
        Response16::Heartbeat(_)
    ));

    let status = Action16::StatusNotification(
        serde_json::from_value(json!({
            "connectorId": 1,
            "errorCode": "NoError",
            "status": "Available"
        }))
        .unwrap(),
    );
    assert!(matches!(
        client.call(status).await.unwrap(),
        Response16::StatusNotification(_)
    ));

    // CSMS -> CS: server-initiated RemoteStartTransaction (reverse direction).
    let conn = first_connection(&server).await;
    let remote_start = Action16::RemoteStartTransaction(
        serde_json::from_value(json!({ "idTag": "TAG1" })).unwrap(),
    );
    let resp = server
        .call(conn, remote_start)
        .await
        .expect("remote start call failed");
    assert!(matches!(resp, Response16::RemoteStartTransaction(_)));
    assert!(
        remote_start_seen.load(Ordering::SeqCst),
        "CS handler should have seen the server-initiated call"
    );

    // Graceful shutdown of both sides.
    client.terminate().await.expect("client terminate failed");
    server.terminate().await.expect("server terminate failed");
}
