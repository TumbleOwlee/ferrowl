//! OCPP CS reconnect-with-backoff (OC-R-048, OC-R-105–107, NF-R-021). Mirrors
//! `ferrowl-modbus/tests/tcp_reconnect.rs` in shape: a closed ephemeral port stands in for "no
//! peer answers", and a real `csms::ServerBuilder` loopback target proves recovery once one
//! becomes reachable.

// Integration-test crate: an unwrap that fails is the test failing, same as an assertion.
#![allow(clippy::unwrap_used)]
#![cfg(feature = "v1_6")]

use std::sync::Arc;
use std::time::Duration;

use ferrowl_ocpp::cs::{self, CsActionHandler};
use ferrowl_ocpp::csms::{self, CsmsActionHandler};
use ferrowl_ocpp::{Action16, CallError, CallErrorCode, Response16, V1_6};
use tokio::sync::RwLock;
use tokio::time::sleep;

/// No-op log/status sink. `LogFn + Clone` is satisfied by a capture-free closure.
fn sink() -> impl ferrowl_ocpp::LogFn + Clone {
    |_s: String| async move {}
}

/// Poll until the CSMS listener has bound: `spawn` no longer binds synchronously, retrying a
/// failed bind with backoff instead (OC-R-083), so `local_addr()` is `None` until the first
/// successful bind lands.
async fn bound_addr<V: ferrowl_ocpp::Version>(server: &csms::Server<V>) -> std::net::SocketAddr {
    for _ in 0..50 {
        if let Some(addr) = server.local_addr() {
            return addr;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("CSMS listener never bound");
}

/// CSMS handler answering the single action these tests exercise.
struct TestCsms;

impl CsmsActionHandler<V1_6> for TestCsms {
    async fn handle_call(
        &self,
        _conn: csms::ConnectionId,
        action: Action16,
    ) -> Result<Response16, CallError> {
        match action {
            Action16::Heartbeat(_) => Ok(Response16::Heartbeat(
                serde_json::from_value(
                    serde_json::json!({ "currentTime": "2026-01-01T00:00:00Z" }),
                )
                .unwrap(),
            )),
            _ => Err(CallError::new(CallErrorCode::NotImplemented, "unsupported")),
        }
    }
}

/// CS handler; these tests never receive a server-initiated Call.
struct TestCs;

impl CsActionHandler<V1_6> for TestCs {
    async fn handle_call(&self, _action: Action16) -> Result<Response16, CallError> {
        Err(CallError::new(CallErrorCode::NotImplemented, "unsupported"))
    }
}

/// An OS-assigned free TCP port (bind to :0, read the port, drop the listener) — nothing answers
/// on it afterward, standing in for a refused dial.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn config(url: String, reconnect: bool) -> Arc<RwLock<cs::Config>> {
    Arc::new(RwLock::new(cs::Config {
        url,
        reconnect,
        timeout_ms: 1000,
        basic_auth: None,
        tls: None,
    }))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// OC-R-048 (revised) — with `reconnect: true` (the default), a CS whose very first dial fails
/// never ends its task: it stays alive, backing off and retrying.
async fn cs_dial_failure_retries_while_reconnect_enabled() {
    let dead_port = free_port();
    let mut client = cs::ClientBuilder::<V1_6>::new(
        config(format!("ws://127.0.0.1:{dead_port}/ocpp/CS001"), true),
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCs, sink(), sink())
    .await
    .expect("spawn always returns Ok now; the dial happens inside the task");

    sleep(Duration::from_millis(200)).await;

    // The task is still running: `join()` under a short timeout must not resolve.
    let joined = tokio::time::timeout(Duration::from_millis(50), client.join()).await;
    assert!(
        joined.is_err(),
        "the task must still be alive and backing off, not ended"
    );

    let _ = client.terminate().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// OC-R-048 (revised) — with `reconnect: false`, a CS whose dial fails ends its task with that
/// error (after emitting a disconnected status), instead of retrying.
async fn cs_dial_failure_reconnect_false_ends_task() {
    let dead_port = free_port();
    let mut client = cs::ClientBuilder::<V1_6>::new(
        config(format!("ws://127.0.0.1:{dead_port}/ocpp/CS001"), false),
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCs, sink(), sink())
    .await
    .expect("spawn always returns Ok now; the dial error surfaces from the task");

    let result = tokio::time::timeout(Duration::from_secs(2), client.join())
        .await
        .expect("the task must end promptly, not retry, with reconnect disabled");
    assert!(result.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// OC-R-106 — terminating a CS (or closing its command channel) while it is backing off aborts
/// the wait immediately and ends the task with success, extending OC-R-047 to the backing-off
/// state.
async fn cs_terminate_while_backing_off_ends_task_ok() {
    let dead_port = free_port();
    let client = cs::ClientBuilder::<V1_6>::new(
        config(format!("ws://127.0.0.1:{dead_port}/ocpp/CS001"), true),
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCs, sink(), sink())
    .await
    .expect("spawn always returns Ok now; the dial happens inside the task");

    // Give the first (failing) dial attempt time to run and enter its backoff wait.
    sleep(Duration::from_millis(100)).await;

    let result = tokio::time::timeout(Duration::from_secs(2), client.terminate())
        .await
        .expect("terminate() must not hang while the task is backing off");
    assert!(result.is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// OC-R-107 — the endpoint (and `reconnect`/security config) is re-read from the shared config on
/// every dial attempt, so pointing it at a real CSMS while backing off is picked up on the next
/// attempt without a restart (mirrors `ferrowl-modbus`'s `tcp_client_rereads_config_on_reconnect`,
/// MB-R-056).
async fn cs_config_reread_on_every_dial() {
    let dead_port = free_port();
    let shared_config = config(format!("ws://127.0.0.1:{dead_port}/ocpp/CS001"), true);

    let client = cs::ClientBuilder::<V1_6>::new(
        shared_config.clone(),
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCs, sink(), sink())
    .await
    .expect("spawn always returns Ok now; the dial happens inside the task");

    // First connect attempt fails; while it backs off, repoint the config at a live CSMS.
    sleep(Duration::from_millis(200)).await;
    let server = csms::ServerBuilder::<V1_6>::new(
        csms::Config {
            host: "127.0.0.1".to_owned(),
            port: 0,
            timeout_ms: 1000,
            reconnect: true,
            basic_auth: None,
            tls: None,
        },
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCsms, sink())
    .await
    .expect("server failed to bind");
    shared_config.write().await.url = format!("ws://{}/ocpp/CS001", bound_addr(&server).await);

    // The 1s initial backoff plus a margin must elapse before the re-read connect succeeds.
    sleep(Duration::from_millis(1500)).await;
    let hb = Action16::Heartbeat(serde_json::from_value(serde_json::json!({})).unwrap());
    let resp = tokio::time::timeout(Duration::from_secs(1), client.call(hb))
        .await
        .expect("call must not hang once the reconnect landed")
        .expect("the reconnected CS must be able to complete a Call against the new endpoint");
    assert!(matches!(resp, Response16::Heartbeat(_)));

    let _ = client.terminate().await;
    server.terminate().await.expect("server terminate failed");
}
