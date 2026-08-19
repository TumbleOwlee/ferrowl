//! OC-R-117 — `extra_headers` are sent on the WebSocket upgrade request alongside the client's
//! own headers. A raw `accept_hdr_async` acceptor stands in for the peer (mirrors the raw-peer
//! pattern in `ws_loopback_v16.rs`'s `raw_connect`, reversed: a real `cs::Client` dials a raw
//! acceptor instead of a raw client dialing the real `csms::Server`).

// Integration-test crate: an unwrap that fails is the test failing, same as an assertion.
#![allow(clippy::unwrap_used)]
#![cfg(feature = "v1_6")]

use std::sync::{Arc, Mutex};

use ferrowl_ocpp::cs::{self, CsActionHandler};
use ferrowl_ocpp::{CallError, CallErrorCode, HeaderDef, Response16, V1_6};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

/// No-op log/status sink. `LogFn + Clone` is satisfied by a capture-free closure.
fn sink() -> impl ferrowl_ocpp::LogFn + Clone {
    |_s: String| async move {}
}

/// CS handler; this test never receives a server-initiated Call.
struct TestCs;

impl CsActionHandler<V1_6> for TestCs {
    async fn handle_call(&self, _action: ferrowl_ocpp::Action16) -> Result<Response16, CallError> {
        Err(CallError::new(CallErrorCode::NotImplemented, "unsupported"))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// OC-R-117 — extra_headers are sent on the WebSocket upgrade request alongside the client's own
/// headers.
async fn extra_headers_are_sent_on_upgrade_request() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let seen: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_for_acceptor = seen.clone();

    tokio::spawn(async move {
        let (stream, _peer) = listener.accept().await.unwrap();
        #[allow(clippy::result_large_err)]
        let callback = move |req: &Request, resp: Response| {
            let mut headers = seen_for_acceptor.lock().unwrap();
            for (name, value) in req.headers() {
                headers.push((
                    name.as_str().to_string(),
                    value.to_str().unwrap_or_default().to_string(),
                ));
            }
            Ok(resp)
        };
        let ws = accept_hdr_async(stream, callback).await.unwrap();
        // Keep the connection alive until the test has read what it needs.
        let _ = futures_util::StreamExt::next(&mut { ws }).await;
    });

    let config = Arc::new(RwLock::new(cs::Config {
        url: format!("ws://{addr}/ocpp/CS001"),
        timeout_ms: 1000,
        basic_auth: None,
        tls: None,
        extra_headers: vec![
            HeaderDef::new("X-Tenant", "acme-1").unwrap(),
            HeaderDef::new("X-Trace-Id", "abc123").unwrap(),
        ],
        reconnect: false,
    }));

    let _client = cs::ClientBuilder::<V1_6>::new(config, ferrowl_ocpp::new_self_signed_cache())
        .spawn(TestCs, sink(), sink())
        .await
        .expect("spawn always returns Ok now; the dial happens inside the task");

    let mut found = Vec::new();
    for _ in 0..50 {
        {
            let headers = seen.lock().unwrap();
            if !headers.is_empty() {
                found = headers.clone();
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    assert!(
        found.iter().any(|(n, v)| n == "x-tenant" && v == "acme-1"),
        "expected x-tenant header, got {found:?}"
    );
    assert!(
        found
            .iter()
            .any(|(n, v)| n == "x-trace-id" && v == "abc123"),
        "expected x-trace-id header, got {found:?}"
    );
}
