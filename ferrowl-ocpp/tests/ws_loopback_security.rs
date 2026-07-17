//! Security-profile loopback tests layered on the same CS/CSMS engine exercised by
//! `ws_loopback_v16.rs`: HTTP Basic Auth (Security Profile 1) and TLS (Security Profile 2).
//!
//! Mutual TLS (Security Profile 3) is not covered here: building a CA-signed client certificate
//! with `rcgen` (as opposed to a single self-signed leaf) needs a second keypair plus a
//! `BasicConstraints::Ca` issuer, which is enough extra scaffolding that it's left for a
//! follow-up rather than bolted on to this pass.

// Integration-test crate: an unwrap that fails is the test failing, same as an assertion.
#![allow(clippy::unwrap_used)]
#![cfg(feature = "v1_6")]

use std::sync::atomic::{AtomicU32, Ordering};

use ferrowl_ocpp::cs::{self, CsActionHandler};
use ferrowl_ocpp::csms::{self, CsmsActionHandler};
use ferrowl_ocpp::{
    Action16, BasicAuth, CallError, CallErrorCode, CsTlsConfig, CsmsTlsConfig, CsmsTlsMode,
    Response16, V1_6,
};
use serde_json::json;

/// No-op log sink.
fn sink() -> impl ferrowl_ocpp::LogFn + Clone {
    |_s: String| async move {}
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
            Action16::BootNotification(_) => Ok(Response16::BootNotification(
                serde_json::from_value(json!({
                    "currentTime": "2026-01-01T00:00:00Z",
                    "interval": 300,
                    "status": "Accepted"
                }))
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

fn boot_action() -> Action16 {
    Action16::BootNotification(
        serde_json::from_value(json!({
            "chargePointModel": "Model-1",
            "chargePointVendor": "Ferrowl"
        }))
        .unwrap(),
    )
}

/// Write `pem` to a fresh file under the OS temp dir; returns its path. Left in place -- the temp
/// dir is ephemeral and there's no shared teardown hook to hang cleanup off.
fn write_pem(label: &str, pem: &str) -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "ferrowl-ocpp-test-{}-{label}-{n}.pem",
        std::process::id()
    ));
    std::fs::write(&path, pem).expect("failed to write test PEM file");
    path.to_string_lossy().into_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// OC-R-030 — a CS with Basic Auth sends the `Authorization: Basic` header and the CSMS accepts matching credentials.
async fn basic_auth_accepts_matching_credentials() {
    let auth = BasicAuth {
        username: "cp001".to_owned(),
        password: "s3cret".to_owned(),
    };

    let server = csms::ServerBuilder::<V1_6>::new(csms::Config {
        host: "127.0.0.1".to_owned(),
        port: 0,
        timeout_ms: 2000,
        basic_auth: Some(auth.clone()),
        tls: None,
    })
    .spawn(TestCsms, sink())
    .await
    .expect("server failed to bind");

    let url = format!("ws://{}/ocpp/CS001", server.local_addr());
    let client = cs::ClientBuilder::<V1_6>::new(cs::Config {
        url,
        timeout_ms: 2000,
        basic_auth: Some(auth),
        tls: None,
    })
    .spawn(TestCs, sink())
    .await
    .expect("client failed to connect with matching credentials");

    let resp = client.call(boot_action()).await.expect("boot call failed");
    assert!(matches!(resp, Response16::BootNotification(_)));

    client.terminate().await.expect("client terminate failed");
    server.terminate().await.expect("server terminate failed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// OC-R-031 — a CSMS rejects an upgrade whose Basic Auth credentials do not match, answering 401.
async fn basic_auth_rejects_mismatched_credentials() {
    let server = csms::ServerBuilder::<V1_6>::new(csms::Config {
        host: "127.0.0.1".to_owned(),
        port: 0,
        timeout_ms: 2000,
        basic_auth: Some(BasicAuth {
            username: "cp001".to_owned(),
            password: "s3cret".to_owned(),
        }),
        tls: None,
    })
    .spawn(TestCsms, sink())
    .await
    .expect("server failed to bind");

    let url = format!("ws://{}/ocpp/CS001", server.local_addr());
    let result = cs::ClientBuilder::<V1_6>::new(cs::Config {
        url,
        timeout_ms: 2000,
        basic_auth: Some(BasicAuth {
            username: "cp001".to_owned(),
            password: "wrong".to_owned(),
        }),
        tls: None,
    })
    .spawn(TestCs, sink())
    .await;

    assert!(
        result.is_err(),
        "connect should fail the websocket handshake on a credential mismatch"
    );

    server.terminate().await.expect("server terminate failed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// OC-R-031 — a CSMS rejects an upgrade with no `Authorization` header, answering 401.
async fn basic_auth_rejects_missing_credentials() {
    let server = csms::ServerBuilder::<V1_6>::new(csms::Config {
        host: "127.0.0.1".to_owned(),
        port: 0,
        timeout_ms: 2000,
        basic_auth: Some(BasicAuth {
            username: "cp001".to_owned(),
            password: "s3cret".to_owned(),
        }),
        tls: None,
    })
    .spawn(TestCsms, sink())
    .await
    .expect("server failed to bind");

    let url = format!("ws://{}/ocpp/CS001", server.local_addr());
    let result = cs::ClientBuilder::<V1_6>::new(cs::Config {
        url,
        timeout_ms: 2000,
        basic_auth: None,
        tls: None,
    })
    .spawn(TestCs, sink())
    .await;

    assert!(
        result.is_err(),
        "connect should fail the websocket handshake with no Authorization header at all"
    );

    server.terminate().await.expect("server terminate failed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// OC-R-034 — a CS trusts a certificate supplied via `ca_file`, completing a TLS loopback to the CSMS.
/// OC-R-050 — when TLS is configured, the CSMS terminates TLS on the accepted socket before the WebSocket handshake (the CS reaches the OCPP layer only through the completed TLS session).
async fn tls_loopback_over_self_signed_cert() {
    let key_pair = rcgen::KeyPair::generate().expect("keypair generation failed");
    let cert = rcgen::CertificateParams::new(vec!["127.0.0.1".to_owned()])
        .expect("cert params")
        .self_signed(&key_pair)
        .expect("self-signed cert");
    let cert_file = write_pem("server-cert", &cert.pem());
    let key_file = write_pem("server-key", &key_pair.serialize_pem());

    let server = csms::ServerBuilder::<V1_6>::new(csms::Config {
        host: "127.0.0.1".to_owned(),
        port: 0,
        timeout_ms: 2000,
        basic_auth: None,
        tls: Some(CsmsTlsConfig {
            mode: CsmsTlsMode::Files {
                cert_file: cert_file.clone(),
                key_file,
            },
            client_ca_file: None,
            require_client_cert: false,
        }),
    })
    .spawn(TestCsms, sink())
    .await
    .expect("TLS server failed to bind");

    let url = format!("wss://{}/ocpp/CS001", server.local_addr());
    let client = cs::ClientBuilder::<V1_6>::new(cs::Config {
        url,
        timeout_ms: 2000,
        basic_auth: None,
        tls: Some(CsTlsConfig {
            ca_file: Some(cert_file),
            client_cert_file: None,
            client_key_file: None,
            insecure_skip_verify: false,
        }),
    })
    .spawn(TestCs, sink())
    .await
    .expect("client failed to connect over TLS");

    let resp = client.call(boot_action()).await.expect("boot call failed");
    assert!(matches!(resp, Response16::BootNotification(_)));

    client.terminate().await.expect("client terminate failed");
    server.terminate().await.expect("server terminate failed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// OC-R-034 — a CS rejects a server certificate not trusted by the webpki roots or its configured `ca_file`.
async fn tls_loopback_rejects_untrusted_cert() {
    let key_pair = rcgen::KeyPair::generate().expect("keypair generation failed");
    let cert = rcgen::CertificateParams::new(vec!["127.0.0.1".to_owned()])
        .expect("cert params")
        .self_signed(&key_pair)
        .expect("self-signed cert");
    let cert_file = write_pem("server-cert-untrusted", &cert.pem());
    let key_file = write_pem("server-key-untrusted", &key_pair.serialize_pem());

    let server = csms::ServerBuilder::<V1_6>::new(csms::Config {
        host: "127.0.0.1".to_owned(),
        port: 0,
        timeout_ms: 2000,
        basic_auth: None,
        tls: Some(CsmsTlsConfig {
            mode: CsmsTlsMode::Files {
                cert_file,
                key_file,
            },
            client_ca_file: None,
            require_client_cert: false,
        }),
    })
    .spawn(TestCsms, sink())
    .await
    .expect("TLS server failed to bind");

    // No `ca_file`: only the webpki root store is trusted, so the self-signed cert must be
    // rejected.
    let url = format!("wss://{}/ocpp/CS001", server.local_addr());
    let result = cs::ClientBuilder::<V1_6>::new(cs::Config {
        url,
        timeout_ms: 2000,
        basic_auth: None,
        tls: Some(CsTlsConfig {
            ca_file: None,
            client_cert_file: None,
            client_key_file: None,
            insecure_skip_verify: false,
        }),
    })
    .spawn(TestCs, sink())
    .await;

    assert!(
        result.is_err(),
        "connect should fail TLS verification against an untrusted self-signed cert"
    );

    server.terminate().await.expect("server terminate failed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// OC-R-036 — a CS with `insecure_skip_verify` accepts any server certificate and connects.
async fn self_signed_csms_with_skip_verify_client_connects() {
    let server = csms::ServerBuilder::<V1_6>::new(csms::Config {
        host: "127.0.0.1".to_owned(),
        port: 0,
        timeout_ms: 2000,
        basic_auth: None,
        tls: Some(CsmsTlsConfig {
            mode: CsmsTlsMode::SelfSigned,
            client_ca_file: None,
            require_client_cert: false,
        }),
    })
    .spawn(TestCsms, sink())
    .await
    .expect("TLS server failed to bind with a self-signed cert");

    let url = format!("wss://{}/ocpp/CS001", server.local_addr());
    let client = cs::ClientBuilder::<V1_6>::new(cs::Config {
        url,
        timeout_ms: 2000,
        basic_auth: None,
        tls: Some(CsTlsConfig {
            ca_file: None,
            client_cert_file: None,
            client_key_file: None,
            insecure_skip_verify: true,
        }),
    })
    .spawn(TestCs, sink())
    .await
    .expect("client with insecure_skip_verify should connect to a self-signed CSMS");

    let resp = client.call(boot_action()).await.expect("boot call failed");
    assert!(matches!(resp, Response16::BootNotification(_)));

    client.terminate().await.expect("client terminate failed");
    server.terminate().await.expect("server terminate failed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// OC-R-036 — without `insecure_skip_verify`, a CS rejects an untrusted self-signed CSMS certificate.
async fn self_signed_csms_without_skip_verify_client_rejects() {
    let server = csms::ServerBuilder::<V1_6>::new(csms::Config {
        host: "127.0.0.1".to_owned(),
        port: 0,
        timeout_ms: 2000,
        basic_auth: None,
        tls: Some(CsmsTlsConfig {
            mode: CsmsTlsMode::SelfSigned,
            client_ca_file: None,
            require_client_cert: false,
        }),
    })
    .spawn(TestCsms, sink())
    .await
    .expect("TLS server failed to bind with a self-signed cert");

    let url = format!("wss://{}/ocpp/CS001", server.local_addr());
    let result = cs::ClientBuilder::<V1_6>::new(cs::Config {
        url,
        timeout_ms: 2000,
        basic_auth: None,
        tls: Some(CsTlsConfig {
            ca_file: None,
            client_cert_file: None,
            client_key_file: None,
            insecure_skip_verify: false,
        }),
    })
    .spawn(TestCs, sink())
    .await;

    assert!(
        result.is_err(),
        "connect should fail without insecure_skip_verify: a per-start self-signed cert can't be pinned"
    );

    server.terminate().await.expect("server terminate failed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// OC-R-031 — Basic Auth credential checking still applies when layered over a self-signed TLS connection.
async fn basic_auth_over_self_signed_tls_checks_credentials() {
    let auth = BasicAuth {
        username: "cp001".to_owned(),
        password: "s3cret".to_owned(),
    };
    let tls = Some(CsmsTlsConfig {
        mode: CsmsTlsMode::SelfSigned,
        client_ca_file: None,
        require_client_cert: false,
    });

    let server = csms::ServerBuilder::<V1_6>::new(csms::Config {
        host: "127.0.0.1".to_owned(),
        port: 0,
        timeout_ms: 2000,
        basic_auth: Some(auth.clone()),
        tls,
    })
    .spawn(TestCsms, sink())
    .await
    .expect("TLS server failed to bind");

    let url = format!("wss://{}/ocpp/CS001", server.local_addr());
    let client = cs::ClientBuilder::<V1_6>::new(cs::Config {
        url: url.clone(),
        timeout_ms: 2000,
        basic_auth: Some(auth),
        tls: Some(CsTlsConfig {
            ca_file: None,
            client_cert_file: None,
            client_key_file: None,
            insecure_skip_verify: true,
        }),
    })
    .spawn(TestCs, sink())
    .await
    .expect("correct credentials should connect");

    let resp = client.call(boot_action()).await.expect("boot call failed");
    assert!(matches!(resp, Response16::BootNotification(_)));
    client.terminate().await.expect("client terminate failed");

    let wrong = cs::ClientBuilder::<V1_6>::new(cs::Config {
        url,
        timeout_ms: 2000,
        basic_auth: Some(BasicAuth {
            username: "cp001".to_owned(),
            password: "wrong".to_owned(),
        }),
        tls: Some(CsTlsConfig {
            ca_file: None,
            client_cert_file: None,
            client_key_file: None,
            insecure_skip_verify: true,
        }),
    })
    .spawn(TestCs, sink())
    .await;
    assert!(
        wrong.is_err(),
        "mismatched credentials should be rejected even over a self-signed TLS connection"
    );

    server.terminate().await.expect("server terminate failed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// OC-R-040 — `require_client_cert` combined with a self-signed CSMS certificate fails the server's start.
async fn self_signed_with_require_client_cert_is_rejected_at_build() {
    let result = csms::ServerBuilder::<V1_6>::new(csms::Config {
        host: "127.0.0.1".to_owned(),
        port: 0,
        timeout_ms: 2000,
        basic_auth: None,
        tls: Some(CsmsTlsConfig {
            mode: CsmsTlsMode::SelfSigned,
            client_ca_file: None,
            require_client_cert: true,
        }),
    })
    .spawn(TestCsms, sink())
    .await;

    let err = result
        .err()
        .expect("SelfSigned + require_client_cert must be rejected, not silently combined");
    assert!(
        err.to_string().contains("require_client_cert"),
        "unexpected error: {err}"
    );
}
