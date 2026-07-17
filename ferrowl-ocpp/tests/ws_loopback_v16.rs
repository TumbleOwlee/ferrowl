//! Low-level layer: a CSMS server and a CS client exchange Calls in both directions over a real
//! websocket loopback, OCPP 1.6. Mirrors `ferrowl-modbus/tests/tcp_loopback.rs`.

// Integration-test crate: an unwrap that fails is the test failing, same as an assertion.
#![allow(clippy::unwrap_used)]
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
        basic_auth: None,
        tls: None,
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
/// OC-R-014 — the 1.6 connection is full-duplex: CS and CSMS each originate Calls on the same socket.
/// OC-R-043 — the CS dials the full websocket URL advertising its version's subprotocol token (the CSMS only accepts it because the token matches).
/// OC-R-045 — the CS accepts commands while connected: send a Call and await its typed reply, and terminate.
/// OC-R-046 — the CS answers CSMS-originated Calls through its handler.
/// OC-R-047 — terminating the CS tears the connection down and ends the client task successfully.
/// OC-R-049 — the CSMS binds a TCP listener on a configured host/port (port 0 → OS-assigned) and its bound address is retrievable via `local_addr`.
/// OC-R-056 — the CSMS handler is told which connection each Call arrived on (its `ConnectionId` argument).
async fn cs_calls_csms_and_csms_calls_cs() {
    let server = start_server().await;
    let url = format!("ws://{}/ocpp/CS001", server.local_addr());

    let remote_start_seen = Arc::new(AtomicBool::new(false));
    let client = cs::ClientBuilder::<V1_6>::new(cs::Config {
        url,
        timeout_ms: 2000,
        basic_auth: None,
        tls: None,
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

/// A malformed Call whose `uniqueId` is still readable must come back as a `CallError` — leaving
/// it unanswered strands the peer until its own call timeout fires. Driven over a raw websocket,
/// since the typed client cannot produce a malformed frame.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// OC-R-098 — a malformed but identifiable inbound Call is answered with a CallError carrying its recovered id.
async fn malformed_call_with_recoverable_id_gets_call_error() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let server = start_server().await;
    let url = format!("ws://{}/ocpp/CS001", server.local_addr());
    let mut request = url.into_client_request().expect("bad url");
    request
        .headers_mut()
        .insert("Sec-WebSocket-Protocol", "ocpp1.6".parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("raw websocket connect failed");

    // Arity-3 Call: the decoder rejects it, but "bad-1" survives.
    ws.send(Message::text("[2, \"bad-1\", \"Heartbeat\"]"))
        .await
        .expect("send failed");

    let reply = tokio::time::timeout(Duration::from_secs(2), ws.next())
        .await
        .expect("no reply: the peer was left to time out")
        .expect("stream closed without a reply")
        .expect("websocket error");
    let v: serde_json::Value =
        serde_json::from_str(reply.into_text().unwrap().as_str()).expect("reply is not JSON");

    assert_eq!(v[0], 4, "expected a CallError frame");
    assert_eq!(v[1], "bad-1", "CallError must carry the recovered id");
    assert_eq!(v[2], "FormationViolation");

    server.terminate().await.expect("server terminate failed");
}

/// A CSMS whose handler records the actions it saw and can be told to sleep on Heartbeat, so tests
/// can observe fire-and-forget delivery and that a slow handler does not block the read pump.
#[derive(Clone)]
struct RecordingCsms {
    seen: Arc<std::sync::Mutex<Vec<String>>>,
    slow_heartbeat: bool,
}

impl CsmsActionHandler<V1_6> for RecordingCsms {
    async fn handle_call(
        &self,
        _conn: csms::ConnectionId,
        action: Action16,
    ) -> Result<Response16, CallError> {
        match action {
            Action16::Heartbeat(_) => {
                if self.slow_heartbeat {
                    sleep(Duration::from_millis(700)).await;
                }
                self.seen.lock().unwrap().push("Heartbeat".to_owned());
                Ok(Response16::Heartbeat(
                    serde_json::from_value(json!({ "currentTime": "2026-01-01T00:00:00Z" }))
                        .unwrap(),
                ))
            }
            Action16::StatusNotification(_) => {
                self.seen
                    .lock()
                    .unwrap()
                    .push("StatusNotification".to_owned());
                Ok(Response16::StatusNotification(
                    serde_json::from_value(json!({})).unwrap(),
                ))
            }
            _ => Err(CallError::new(CallErrorCode::NotImplemented, "unsupported")),
        }
    }
}

/// Connect a raw websocket (advertising the 1.6 subprotocol) to `url`, bypassing the typed client so
/// a test can send hand-crafted frames.
async fn raw_connect(
    url: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut request = url.into_client_request().expect("bad url");
    request
        .headers_mut()
        .insert("Sec-WebSocket-Protocol", "ocpp1.6".parse().unwrap());
    let (ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("raw websocket connect failed");
    ws
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// OC-R-025 — a handler rejecting an inbound Call sends a CallError back and leaves the connection intact (a later Call still succeeds).
async fn handler_rejection_is_call_error_and_keeps_connection() {
    let server = start_server().await;
    let url = format!("ws://{}/ocpp/CS001", server.local_addr());
    let client = cs::ClientBuilder::<V1_6>::new(cs::Config {
        url,
        timeout_ms: 2000,
        basic_auth: None,
        tls: None,
    })
    .spawn(
        TestCs {
            remote_start_seen: Arc::new(AtomicBool::new(false)),
        },
        sink(),
    )
    .await
    .expect("client failed to connect");

    // TestCsms rejects Authorize (its `_` arm) with a CallError.
    let authorize =
        Action16::Authorize(serde_json::from_value(json!({ "idTag": "TAG1" })).unwrap());
    assert!(client.call(authorize).await.is_err());

    // The connection survived: a supported Call still works.
    let hb = Action16::Heartbeat(serde_json::from_value(json!({})).unwrap());
    assert!(matches!(
        client.call(hb).await.unwrap(),
        Response16::Heartbeat(_)
    ));

    client.terminate().await.expect("terminate");
    server.terminate().await.expect("server terminate");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// OC-R-027 — an inbound Call whose payload fails to deserialize into the action's request type is answered with CallError `FormationViolation`.
async fn bad_payload_is_formation_violation() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let server = start_server().await;
    let url = format!("ws://{}/ocpp/CS001", server.local_addr());
    let mut ws = raw_connect(&url).await;

    // Valid frame + valid action name, but the payload is missing BootNotification's required fields.
    ws.send(Message::text("[2, \"p1\", \"BootNotification\", {}]"))
        .await
        .unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(2), ws.next())
        .await
        .expect("no reply")
        .expect("stream closed")
        .expect("ws error");
    let v: serde_json::Value = serde_json::from_str(reply.into_text().unwrap().as_str()).unwrap();
    assert_eq!(v[0], 4, "expected a CallError");
    assert_eq!(v[1], "p1");
    assert_eq!(v[2], "FormationViolation");

    server.terminate().await.expect("server terminate");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// OC-R-024 — a malformed inbound frame that cannot be answered is logged and skipped without tearing the connection down.
/// OC-R-013 — non-text frames (binary) are ignored rather than treated as OCPP-J payloads.
async fn malformed_and_binary_frames_do_not_tear_down() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let server = start_server().await;
    let url = format!("ws://{}/ocpp/CS001", server.local_addr());
    let mut ws = raw_connect(&url).await;

    // Unrecoverable text (not JSON) — logged and skipped, no reply, no teardown.
    ws.send(Message::text("this is not json")).await.unwrap();
    // A binary frame — ignored as a non-OCPP-J payload.
    ws.send(Message::binary(vec![1u8, 2, 3])).await.unwrap();

    // The connection still works: a valid Heartbeat Call is answered.
    ws.send(Message::text("[2, \"ok1\", \"Heartbeat\", {}]"))
        .await
        .unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(2), ws.next())
        .await
        .expect("no reply: connection was torn down")
        .expect("stream closed")
        .expect("ws error");
    let v: serde_json::Value = serde_json::from_str(reply.into_text().unwrap().as_str()).unwrap();
    assert_eq!(v[0], 3, "expected a CallResult");
    assert_eq!(v[1], "ok1");

    server.terminate().await.expect("server terminate");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// OC-R-020 — an awaited outbound Call that the peer never answers in time is rejected once the reply timeout expires.
async fn awaited_call_times_out() {
    // Server whose Heartbeat handler sleeps well past the client's reply timeout.
    let server = csms::ServerBuilder::<V1_6>::new(csms::Config {
        host: "127.0.0.1".to_owned(),
        port: 0,
        timeout_ms: 2000,
        basic_auth: None,
        tls: None,
    })
    .spawn(
        RecordingCsms {
            seen: Arc::new(std::sync::Mutex::new(Vec::new())),
            slow_heartbeat: true,
        },
        sink(),
    )
    .await
    .expect("server bind");
    let url = format!("ws://{}/ocpp/CS001", server.local_addr());

    // Short client timeout: the slow (700ms) handler cannot answer before it fires.
    let client = cs::ClientBuilder::<V1_6>::new(cs::Config {
        url,
        timeout_ms: 150,
        basic_auth: None,
        tls: None,
    })
    .spawn(
        TestCs {
            remote_start_seen: Arc::new(AtomicBool::new(false)),
        },
        sink(),
    )
    .await
    .expect("client connect");

    let hb = Action16::Heartbeat(serde_json::from_value(json!({})).unwrap());
    assert!(client.call(hb).await.is_err(), "call should have timed out");

    client.terminate().await.expect("terminate");
    server.terminate().await.expect("server terminate");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// OC-R-021 — a Call sent fire-and-forget (`notify`) delivers to the peer with no reply awaited by the caller.
/// OC-R-016 — a slow handler does not block the read pump: a later Call is answered while an earlier fire-and-forget is still being handled.
/// OC-R-015 — the two outbound frames (the fire and the awaited Call) are serialized through the single writer, so both arrive well-formed and are answered.
async fn fire_and_forget_delivers_without_blocking_reads() {
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let server = csms::ServerBuilder::<V1_6>::new(csms::Config {
        host: "127.0.0.1".to_owned(),
        port: 0,
        timeout_ms: 2000,
        basic_auth: None,
        tls: None,
    })
    .spawn(
        RecordingCsms {
            seen: seen.clone(),
            slow_heartbeat: true,
        },
        sink(),
    )
    .await
    .expect("server bind");
    let url = format!("ws://{}/ocpp/CS001", server.local_addr());
    let client = cs::ClientBuilder::<V1_6>::new(cs::Config {
        url,
        timeout_ms: 2000,
        basic_auth: None,
        tls: None,
    })
    .spawn(
        TestCs {
            remote_start_seen: Arc::new(AtomicBool::new(false)),
        },
        sink(),
    )
    .await
    .expect("client connect");

    // Fire a slow Heartbeat without awaiting, then immediately await a fast StatusNotification.
    let hb = Action16::Heartbeat(serde_json::from_value(json!({})).unwrap());
    client.notify(hb).await.expect("notify");
    let status = Action16::StatusNotification(
        serde_json::from_value(json!({
            "connectorId": 1, "errorCode": "NoError", "status": "Available"
        }))
        .unwrap(),
    );
    // The status reply comes back even though the heartbeat handler is still sleeping — proving the
    // read pump is not blocked by the in-flight slow handler.
    assert!(matches!(
        client.call(status).await.unwrap(),
        Response16::StatusNotification(_)
    ));

    // Give the slow heartbeat time to finish; the fire-and-forget did reach the server.
    sleep(Duration::from_millis(900)).await;
    assert!(seen.lock().unwrap().iter().any(|a| a == "Heartbeat"));

    client.terminate().await.expect("terminate");
    server.terminate().await.expect("server terminate");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// OC-R-032 — a CSMS rejects an upgrade that does not advertise the version's subprotocol token (HTTP 400), and echoes the token on acceptance.
async fn csms_requires_and_echoes_subprotocol() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let server = start_server().await;
    let url = format!("ws://{}/ocpp/CS001", server.local_addr());

    // No Sec-WebSocket-Protocol header → the upgrade is rejected.
    let bare = url.clone().into_client_request().expect("bad url");
    assert!(
        tokio_tungstenite::connect_async(bare).await.is_err(),
        "upgrade without the subprotocol token must be rejected"
    );

    // With the token → accepted, and the response echoes it.
    let mut request = url.into_client_request().expect("bad url");
    request
        .headers_mut()
        .insert("Sec-WebSocket-Protocol", "ocpp1.6".parse().unwrap());
    let (_ws, resp) = tokio_tungstenite::connect_async(request)
        .await
        .expect("upgrade with the token should succeed");
    assert_eq!(
        resp.headers()
            .get("sec-websocket-protocol")
            .and_then(|v| v.to_str().ok()),
        Some("ocpp1.6")
    );

    server.terminate().await.expect("server terminate");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// OC-R-051 — each accepted connection gets an opaque, monotonically increasing id from 1; the charge-point identity is kept as metadata, not the key, so duplicate identities never collide.
async fn connection_ids_are_monotonic_and_identity_is_metadata() {
    let server = start_server().await;
    let addr = server.local_addr();

    // Two clients dialing the *same* charge-point identity path.
    let make_client = || async {
        cs::ClientBuilder::<V1_6>::new(cs::Config {
            url: format!("ws://{addr}/ocpp/DUP"),
            timeout_ms: 2000,
            basic_auth: None,
            tls: None,
        })
        .spawn(
            TestCs {
                remote_start_seen: Arc::new(AtomicBool::new(false)),
            },
            sink(),
        )
        .await
        .expect("client connect")
    };
    let c1 = make_client().await;
    let c2 = make_client().await;

    // Wait for both to register.
    for _ in 0..50 {
        if server.registry().connection_ids().len() >= 2 {
            break;
        }
        sleep(Duration::from_millis(20)).await;
    }
    let mut ids = server.registry().connection_ids();
    ids.sort();
    assert_eq!(
        ids.len(),
        2,
        "both duplicate-identity clients registered separately"
    );
    // Ids start at 1 and increase; the two are distinct despite the shared identity.
    assert!(ids[0] < ids[1]);
    assert_eq!(ids[0], csms::ConnectionId(1));
    // The identity is retained as metadata against each id.
    assert_eq!(server.registry().identity(ids[0]).as_deref(), Some("DUP"));
    assert_eq!(server.registry().identity(ids[1]).as_deref(), Some("DUP"));

    c1.terminate().await.expect("terminate c1");
    c2.terminate().await.expect("terminate c2");
    server.terminate().await.expect("server terminate");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// OC-R-052 — a CSMS broadcasts a fire-and-forget Call to every live connection and can disconnect one connection.
/// OC-R-054 — a connection is deregistered from the registry when its connection loop ends.
async fn csms_broadcast_and_disconnect() {
    use ferrowl_ocpp::csms::Command;

    let seen_a = Arc::new(AtomicBool::new(false));
    let seen_b = Arc::new(AtomicBool::new(false));
    let server = start_server().await;
    let addr = server.local_addr();

    let mk = |flag: Arc<AtomicBool>| async move {
        cs::ClientBuilder::<V1_6>::new(cs::Config {
            url: format!("ws://{addr}/ocpp/CS"),
            timeout_ms: 2000,
            basic_auth: None,
            tls: None,
        })
        .spawn(
            TestCs {
                remote_start_seen: flag,
            },
            sink(),
        )
        .await
        .expect("client connect")
    };
    let ca = mk(seen_a.clone()).await;
    let cb = mk(seen_b.clone()).await;

    for _ in 0..50 {
        if server.registry().connection_ids().len() >= 2 {
            break;
        }
        sleep(Duration::from_millis(20)).await;
    }

    // Broadcast a server-initiated RemoteStartTransaction to every connected CS.
    let remote_start = Action16::RemoteStartTransaction(
        serde_json::from_value(json!({ "idTag": "TAG1" })).unwrap(),
    );
    server
        .send(Command::Broadcast(remote_start))
        .await
        .expect("broadcast");
    sleep(Duration::from_millis(300)).await;
    assert!(seen_a.load(Ordering::SeqCst) && seen_b.load(Ordering::SeqCst));

    // Disconnect one connection; the registry drops it when its loop ends.
    let mut ids = server.registry().connection_ids();
    ids.sort();
    server
        .send(Command::DisconnectConnection(ids[0]))
        .await
        .expect("disconnect");
    let mut deregistered = false;
    for _ in 0..50 {
        if !server.registry().connection_ids().contains(&ids[0]) {
            deregistered = true;
            break;
        }
        sleep(Duration::from_millis(20)).await;
    }
    assert!(deregistered, "disconnected connection was not deregistered");

    let _ = ca.terminate().await;
    cb.terminate().await.expect("terminate cb");
    server.terminate().await.expect("server terminate");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// OC-R-055 — a command addressing an unknown connection id fails that command alone (an awaited Call is rejected) and the server keeps running.
async fn command_to_unknown_connection_fails_alone() {
    let server = start_server().await;
    let url = format!("ws://{}/ocpp/CS001", server.local_addr());
    let client = cs::ClientBuilder::<V1_6>::new(cs::Config {
        url,
        timeout_ms: 2000,
        basic_auth: None,
        tls: None,
    })
    .spawn(
        TestCs {
            remote_start_seen: Arc::new(AtomicBool::new(false)),
        },
        sink(),
    )
    .await
    .expect("client connect");
    let conn = first_connection(&server).await;

    // An awaited Call to a nonexistent id is rejected, but the server stays up.
    let remote_start = Action16::RemoteStartTransaction(
        serde_json::from_value(json!({ "idTag": "TAG1" })).unwrap(),
    );
    assert!(
        server
            .call(csms::ConnectionId(9999), remote_start.clone())
            .await
            .is_err()
    );

    // The real connection still works after the failed command.
    let resp = server.call(conn, remote_start).await.expect("live call ok");
    assert!(matches!(resp, Response16::RemoteStartTransaction(_)));

    client.terminate().await.expect("terminate");
    server.terminate().await.expect("server terminate");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// OC-R-053 — terminating a CSMS ends the accept loop, so no further connections are accepted.
async fn terminated_csms_stops_accepting() {
    let server = start_server().await;
    let addr = server.local_addr();
    server.terminate().await.expect("server terminate");

    // After termination the listener is gone: a new client cannot connect.
    sleep(Duration::from_millis(100)).await;
    let res = cs::ClientBuilder::<V1_6>::new(cs::Config {
        url: format!("ws://{addr}/ocpp/CS001"),
        timeout_ms: 1000,
        basic_auth: None,
        tls: None,
    })
    .spawn(
        TestCs {
            remote_start_seen: Arc::new(AtomicBool::new(false)),
        },
        sink(),
    )
    .await;
    assert!(
        res.is_err(),
        "no connection should be accepted after terminate"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// OC-R-048 — a CS does not reconnect on its own: once the CSMS drops, the connection stays down.
async fn cs_does_not_reconnect() {
    let server = start_server().await;
    let addr = server.local_addr();
    let client = cs::ClientBuilder::<V1_6>::new(cs::Config {
        url: format!("ws://{addr}/ocpp/CS001"),
        timeout_ms: 1000,
        basic_auth: None,
        tls: None,
    })
    .spawn(
        TestCs {
            remote_start_seen: Arc::new(AtomicBool::new(false)),
        },
        sink(),
    )
    .await
    .expect("client connect");

    // Drop the server; the CS must not silently re-establish a connection.
    server.terminate().await.expect("server terminate");
    sleep(Duration::from_millis(300)).await;

    // A Call now fails and keeps failing (no reconnect brought the link back).
    let hb = Action16::Heartbeat(serde_json::from_value(json!({})).unwrap());
    assert!(client.call(hb).await.is_err());

    let _ = client.terminate().await;
}

/// A CS handler that records its connect/disconnect lifecycle hooks firing.
#[derive(Clone)]
struct HookCs {
    connected: Arc<AtomicBool>,
    disconnected: Arc<AtomicBool>,
}

impl CsActionHandler<V1_6> for HookCs {
    async fn handle_call(&self, _action: Action16) -> Result<Response16, CallError> {
        Err(CallError::new(CallErrorCode::NotImplemented, "unsupported"))
    }
    async fn on_connected(&self) {
        self.connected.store(true, Ordering::SeqCst);
    }
    async fn on_disconnected(&self) {
        self.disconnected.store(true, Ordering::SeqCst);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
/// OC-R-023 — a peer's WebSocket close ends the connection: the connection is torn down and the CS's disconnect hook fires.
/// OC-R-046 — the CS exposes connect and disconnect lifecycle hooks (both observed here).
async fn peer_close_ends_connection_and_fires_disconnect_hook() {
    let connected = Arc::new(AtomicBool::new(false));
    let disconnected = Arc::new(AtomicBool::new(false));
    let server = start_server().await;
    let url = format!("ws://{}/ocpp/CS001", server.local_addr());
    let client = cs::ClientBuilder::<V1_6>::new(cs::Config {
        url,
        timeout_ms: 1000,
        basic_auth: None,
        tls: None,
    })
    .spawn(
        HookCs {
            connected: connected.clone(),
            disconnected: disconnected.clone(),
        },
        sink(),
    )
    .await
    .expect("client connect");

    // The connect hook fired on establishment.
    for _ in 0..50 {
        if connected.load(Ordering::SeqCst) {
            break;
        }
        sleep(Duration::from_millis(20)).await;
    }
    assert!(
        connected.load(Ordering::SeqCst),
        "connect hook did not fire"
    );

    // The CSMS closes: the connection ends and the disconnect hook fires.
    server.terminate().await.expect("server terminate");
    let mut fired = false;
    for _ in 0..50 {
        if disconnected.load(Ordering::SeqCst) {
            fired = true;
            break;
        }
        sleep(Duration::from_millis(20)).await;
    }
    assert!(fired, "disconnect hook did not fire on peer close");

    let _ = client.terminate().await;
}
