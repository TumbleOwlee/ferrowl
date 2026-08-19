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
use ferrowl_ocpp::{Action16, BasicAuth, CallError, CallErrorCode, Response16, V1_6};
use ferrowl_util::tls::{
    ClientCertSource, ClientCertVerification, ClientTlsPolicy, ClientVerification,
    ServerCertSource, ServerTlsPolicy,
};
use serde_json::json;

/// No-op log sink.
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

    let server = csms::ServerBuilder::<V1_6>::new(
        csms::Config {
            host: "127.0.0.1".to_owned(),
            port: 0,
            timeout_ms: 2000,
            reconnect: true,
            basic_auth: Some(auth.clone()),
            tls: None,
        },
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCsms, sink())
    .await
    .expect("server failed to bind");

    let url = format!("ws://{}/ocpp/CS001", bound_addr(&server).await);
    let client = cs::ClientBuilder::<V1_6>::new(
        std::sync::Arc::new(tokio::sync::RwLock::new(cs::Config {
            extra_headers: Vec::new(),
            url,
            reconnect: true,
            timeout_ms: 2000,
            basic_auth: Some(auth),
            tls: None,
        })),
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCs, sink(), sink())
    .await
    .expect("spawn always returns Ok now; the dial happens inside the task");

    let resp = client.call(boot_action()).await.expect("boot call failed");
    assert!(matches!(resp, Response16::BootNotification(_)));

    client.terminate().await.expect("client terminate failed");
    server.terminate().await.expect("server terminate failed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// OC-R-031 — a CSMS rejects an upgrade whose Basic Auth credentials do not match, answering 401.
async fn basic_auth_rejects_mismatched_credentials() {
    let server = csms::ServerBuilder::<V1_6>::new(
        csms::Config {
            host: "127.0.0.1".to_owned(),
            port: 0,
            timeout_ms: 2000,
            reconnect: true,
            basic_auth: Some(BasicAuth {
                username: "cp001".to_owned(),
                password: "s3cret".to_owned(),
            }),
            tls: None,
        },
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCsms, sink())
    .await
    .expect("server failed to bind");

    let url = format!("ws://{}/ocpp/CS001", bound_addr(&server).await);
    // OC-R-048/OC-R-105: `spawn` always succeeds now (the dial moved inside the retried task);
    // `reconnect: false` ends the task on the first failed dial instead of retrying forever, and
    // the credential-mismatch error surfaces from `join()`.
    let mut result = cs::ClientBuilder::<V1_6>::new(
        std::sync::Arc::new(tokio::sync::RwLock::new(cs::Config {
            extra_headers: Vec::new(),
            url,
            reconnect: false,
            timeout_ms: 2000,
            basic_auth: Some(BasicAuth {
                username: "cp001".to_owned(),
                password: "wrong".to_owned(),
            }),
            tls: None,
        })),
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCs, sink(), sink())
    .await
    .expect("spawn always returns Ok now; the dial happens inside the task");

    assert!(
        result.join().await.is_err(),
        "connect should fail the websocket handshake on a credential mismatch"
    );

    server.terminate().await.expect("server terminate failed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// OC-R-031 — a CSMS rejects an upgrade with no `Authorization` header, answering 401.
async fn basic_auth_rejects_missing_credentials() {
    let server = csms::ServerBuilder::<V1_6>::new(
        csms::Config {
            host: "127.0.0.1".to_owned(),
            port: 0,
            timeout_ms: 2000,
            reconnect: true,
            basic_auth: Some(BasicAuth {
                username: "cp001".to_owned(),
                password: "s3cret".to_owned(),
            }),
            tls: None,
        },
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCsms, sink())
    .await
    .expect("server failed to bind");

    let url = format!("ws://{}/ocpp/CS001", bound_addr(&server).await);
    let mut result = cs::ClientBuilder::<V1_6>::new(
        std::sync::Arc::new(tokio::sync::RwLock::new(cs::Config {
            extra_headers: Vec::new(),
            url,
            reconnect: false,
            timeout_ms: 2000,
            basic_auth: None,
            tls: None,
        })),
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCs, sink(), sink())
    .await
    .expect("spawn always returns Ok now; the dial happens inside the task");

    assert!(
        result.join().await.is_err(),
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

    let server = csms::ServerBuilder::<V1_6>::new(
        csms::Config {
            host: "127.0.0.1".to_owned(),
            port: 0,
            timeout_ms: 2000,
            reconnect: true,
            basic_auth: None,
            tls: Some(ServerTlsPolicy::Tls {
                server_cert: ServerCertSource::Explicit {
                    cert_file: cert_file.clone(),
                    key_file,
                },
            }),
        },
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCsms, sink())
    .await
    .expect("server failed to bind");

    let url = format!("wss://{}/ocpp/CS001", bound_addr(&server).await);
    let client = cs::ClientBuilder::<V1_6>::new(
        std::sync::Arc::new(tokio::sync::RwLock::new(cs::Config {
            extra_headers: Vec::new(),
            url,
            reconnect: true,
            timeout_ms: 2000,
            basic_auth: None,
            tls: Some(ClientTlsPolicy::Tls {
                client_verification: ClientVerification::resolve(false, Some(cert_file)),
            }),
        })),
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCs, sink(), sink())
    .await
    .expect("spawn always returns Ok now; the dial happens inside the task");

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

    let server = csms::ServerBuilder::<V1_6>::new(
        csms::Config {
            host: "127.0.0.1".to_owned(),
            port: 0,
            timeout_ms: 2000,
            reconnect: true,
            basic_auth: None,
            tls: Some(ServerTlsPolicy::Tls {
                server_cert: ServerCertSource::Explicit {
                    cert_file,
                    key_file,
                },
            }),
        },
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCsms, sink())
    .await
    .expect("server failed to bind");

    // No `ca_file`: only the webpki root store is trusted, so the self-signed cert must be
    // rejected.
    let url = format!("wss://{}/ocpp/CS001", bound_addr(&server).await);
    let mut result = cs::ClientBuilder::<V1_6>::new(
        std::sync::Arc::new(tokio::sync::RwLock::new(cs::Config {
            extra_headers: Vec::new(),
            url,
            reconnect: false,
            timeout_ms: 2000,
            basic_auth: None,
            tls: Some(ClientTlsPolicy::Tls {
                client_verification: ClientVerification::resolve(false, None),
            }),
        })),
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCs, sink(), sink())
    .await
    .expect("spawn always returns Ok now; the dial happens inside the task");

    assert!(
        result.join().await.is_err(),
        "connect should fail TLS verification against an untrusted self-signed cert"
    );

    server.terminate().await.expect("server terminate failed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// OC-R-036 — a CS with `insecure_skip_verify` accepts any server certificate and connects.
async fn self_signed_csms_with_skip_verify_client_connects() {
    let server = csms::ServerBuilder::<V1_6>::new(
        csms::Config {
            host: "127.0.0.1".to_owned(),
            port: 0,
            timeout_ms: 2000,
            reconnect: true,
            basic_auth: None,
            tls: Some(ServerTlsPolicy::Tls {
                server_cert: ServerCertSource::SelfSigned,
            }),
        },
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCsms, sink())
    .await
    .expect("server failed to bind");

    let url = format!("wss://{}/ocpp/CS001", bound_addr(&server).await);
    let client = cs::ClientBuilder::<V1_6>::new(
        std::sync::Arc::new(tokio::sync::RwLock::new(cs::Config {
            extra_headers: Vec::new(),
            url,
            reconnect: true,
            timeout_ms: 2000,
            basic_auth: None,
            tls: Some(ClientTlsPolicy::Tls {
                client_verification: ClientVerification::resolve(true, None),
            }),
        })),
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCs, sink(), sink())
    .await
    .expect("spawn always returns Ok now; the dial happens inside the task");

    let resp = client.call(boot_action()).await.expect("boot call failed");
    assert!(matches!(resp, Response16::BootNotification(_)));

    client.terminate().await.expect("client terminate failed");
    server.terminate().await.expect("server terminate failed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// OC-R-036 — without `insecure_skip_verify`, a CS rejects an untrusted self-signed CSMS certificate.
async fn self_signed_csms_without_skip_verify_client_rejects() {
    let server = csms::ServerBuilder::<V1_6>::new(
        csms::Config {
            host: "127.0.0.1".to_owned(),
            port: 0,
            timeout_ms: 2000,
            reconnect: true,
            basic_auth: None,
            tls: Some(ServerTlsPolicy::Tls {
                server_cert: ServerCertSource::SelfSigned,
            }),
        },
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCsms, sink())
    .await
    .expect("server failed to bind");

    let url = format!("wss://{}/ocpp/CS001", bound_addr(&server).await);
    let mut result = cs::ClientBuilder::<V1_6>::new(
        std::sync::Arc::new(tokio::sync::RwLock::new(cs::Config {
            extra_headers: Vec::new(),
            url,
            reconnect: false,
            timeout_ms: 2000,
            basic_auth: None,
            tls: Some(ClientTlsPolicy::Tls {
                client_verification: ClientVerification::resolve(false, None),
            }),
        })),
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCs, sink(), sink())
    .await
    .expect("spawn always returns Ok now; the dial happens inside the task");

    assert!(
        result.join().await.is_err(),
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
    let tls = Some(ServerTlsPolicy::Tls {
        server_cert: ServerCertSource::SelfSigned,
    });

    let server = csms::ServerBuilder::<V1_6>::new(
        csms::Config {
            host: "127.0.0.1".to_owned(),
            port: 0,
            timeout_ms: 2000,
            reconnect: true,
            basic_auth: Some(auth.clone()),
            tls,
        },
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCsms, sink())
    .await
    .expect("server failed to bind");

    let url = format!("wss://{}/ocpp/CS001", bound_addr(&server).await);
    let client = cs::ClientBuilder::<V1_6>::new(
        std::sync::Arc::new(tokio::sync::RwLock::new(cs::Config {
            extra_headers: Vec::new(),
            url: url.clone(),
            reconnect: true,
            timeout_ms: 2000,
            basic_auth: Some(auth),
            tls: Some(ClientTlsPolicy::Tls {
                client_verification: ClientVerification::resolve(true, None),
            }),
        })),
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCs, sink(), sink())
    .await
    .expect("spawn always returns Ok now; the dial happens inside the task");

    let resp = client.call(boot_action()).await.expect("boot call failed");
    assert!(matches!(resp, Response16::BootNotification(_)));
    client.terminate().await.expect("client terminate failed");

    let mut wrong = cs::ClientBuilder::<V1_6>::new(
        std::sync::Arc::new(tokio::sync::RwLock::new(cs::Config {
            extra_headers: Vec::new(),
            url,
            reconnect: false,
            timeout_ms: 2000,
            basic_auth: Some(BasicAuth {
                username: "cp001".to_owned(),
                password: "wrong".to_owned(),
            }),
            tls: Some(ClientTlsPolicy::Tls {
                client_verification: ClientVerification::resolve(true, None),
            }),
        })),
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCs, sink(), sink())
    .await
    .expect("spawn always returns Ok now; the dial error surfaces from the task");
    assert!(
        wrong.join().await.is_err(),
        "mismatched credentials should be rejected even over a self-signed TLS connection"
    );

    server.terminate().await.expect("server terminate failed");
}

// OC-R-040/OC-R-039's "require_client_cert without a client_ca_file is rejected" is now enforced
// at construction (`ClientCertVerification::resolve`/`Deserialize`), before a `ServerTlsPolicy`
// carrying that combination can even exist -- see
// `ferrowl_util::tls::tests::ut_server_tls_policy_deserialize_mutual_tls_empty_ca_files_is_error`.
// There is no longer a "builds, but is then rejected by `build_server_config`" state to cover
// here.

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// OC-R-040 — `require_client_cert` combined with a self-signed CSMS certificate succeeds,
/// end-to-end over a real TLS+mTLS handshake, when a `client_ca_file` is configured: the
/// server's own self-signed identity and the CA trusted for verifying client certificates are
/// independent (unlike `build_server_config`'s unit-level coverage of the same rule in
/// `ferrowl-ocpp/src/security.rs`, this exercises the full CS/CSMS wire handshake).
async fn it_csms_self_signed_with_require_client_cert_and_client_ca_accepts_connection() {
    let ca_key = rcgen::KeyPair::generate().expect("ca keypair generation failed");
    let ca_params =
        rcgen::CertificateParams::new(vec!["ferrowl-ocpp-test-ca".to_owned()]).expect("ca params");
    let ca_cert = ca_params.self_signed(&ca_key).expect("self-signed ca cert");
    let ca_file = write_pem("mtls-ca", &ca_cert.pem());

    let issuer = rcgen::Issuer::from_params(&ca_params, &ca_key);
    let client_key = rcgen::KeyPair::generate().expect("client keypair generation failed");
    let client_cert = rcgen::CertificateParams::new(vec!["ferrowl-ocpp-test-client".to_owned()])
        .expect("client params")
        .signed_by(&client_key, &issuer)
        .expect("ca-signed client cert");
    let client_cert_file = write_pem("mtls-client-cert", &client_cert.pem());
    let client_key_file = write_pem("mtls-client-key", &client_key.serialize_pem());

    let server = csms::ServerBuilder::<V1_6>::new(
        csms::Config {
            host: "127.0.0.1".to_owned(),
            port: 0,
            timeout_ms: 2000,
            reconnect: true,
            basic_auth: None,
            tls: Some(ServerTlsPolicy::MutualTls {
                server_cert: ServerCertSource::SelfSigned,
                client_verification: ClientCertVerification::Verify {
                    ca_files: vec![ca_file],
                },
            }),
        },
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCsms, sink())
    .await
    .expect(
        "server should start: self-signed + require_client_cert is valid with a client_ca_file",
    );

    let url = format!("wss://{}/ocpp/CS001", bound_addr(&server).await);
    let client = cs::ClientBuilder::<V1_6>::new(
        std::sync::Arc::new(tokio::sync::RwLock::new(cs::Config {
            extra_headers: Vec::new(),
            url,
            reconnect: true,
            timeout_ms: 2000,
            basic_auth: None,
            tls: Some(ClientTlsPolicy::MutualTls {
                // The server's identity is an ephemeral self-signed cert, unpinnable in advance.
                client_verification: ClientVerification::SkipVerify,
                client_identity: ClientCertSource::Explicit {
                    client_cert_file,
                    client_key_file,
                },
            }),
        })),
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCs, sink(), sink())
    .await
    .expect("spawn always returns Ok now; the dial happens inside the task");

    let resp = client.call(boot_action()).await.expect("boot call failed");
    assert!(matches!(resp, Response16::BootNotification(_)));

    client.terminate().await.expect("client terminate failed");
    server.terminate().await.expect("server terminate failed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// OC-R-039/OC-R-113 — a CSMS trusting *any one* of several configured CAs accepts a client
/// certificate signed by the second CA in the list, not just the first.
async fn it_csms_multi_ca_accepts_cert_signed_by_either_ca() {
    let ca1_key = rcgen::KeyPair::generate().expect("ca1 keypair");
    let ca1_params = rcgen::CertificateParams::new(vec!["ferrowl-ocpp-test-ca1".to_owned()])
        .expect("ca1 params");
    let ca1_file = write_pem(
        "multi-ca-1",
        &ca1_params
            .clone()
            .self_signed(&ca1_key)
            .expect("self-signed ca1")
            .pem(),
    );

    let ca2_key = rcgen::KeyPair::generate().expect("ca2 keypair");
    let ca2_params = rcgen::CertificateParams::new(vec!["ferrowl-ocpp-test-ca2".to_owned()])
        .expect("ca2 params");
    let ca2_cert = ca2_params
        .clone()
        .self_signed(&ca2_key)
        .expect("self-signed ca2");
    let ca2_file = write_pem("multi-ca-2", &ca2_cert.pem());

    // The client certificate is signed by CA2 only -- CA1 is present in the trust store but
    // irrelevant to this handshake.
    let issuer = rcgen::Issuer::from_params(&ca2_params, &ca2_key);
    let client_key = rcgen::KeyPair::generate().expect("client keypair");
    let client_cert = rcgen::CertificateParams::new(vec!["ferrowl-ocpp-test-client2".to_owned()])
        .expect("client params")
        .signed_by(&client_key, &issuer)
        .expect("ca2-signed client cert");
    let client_cert_file = write_pem("multi-ca-client-cert", &client_cert.pem());
    let client_key_file = write_pem("multi-ca-client-key", &client_key.serialize_pem());

    let server = csms::ServerBuilder::<V1_6>::new(
        csms::Config {
            host: "127.0.0.1".to_owned(),
            port: 0,
            timeout_ms: 2000,
            reconnect: true,
            basic_auth: None,
            tls: Some(ServerTlsPolicy::MutualTls {
                server_cert: ServerCertSource::SelfSigned,
                client_verification: ClientCertVerification::Verify {
                    ca_files: vec![ca1_file, ca2_file],
                },
            }),
        },
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCsms, sink())
    .await
    .expect("server should start with a multi-CA trust store");

    let url = format!("wss://{}/ocpp/CS001", bound_addr(&server).await);
    let client = cs::ClientBuilder::<V1_6>::new(
        std::sync::Arc::new(tokio::sync::RwLock::new(cs::Config {
            extra_headers: Vec::new(),
            url,
            reconnect: true,
            timeout_ms: 2000,
            basic_auth: None,
            tls: Some(ClientTlsPolicy::MutualTls {
                client_verification: ClientVerification::SkipVerify,
                client_identity: ClientCertSource::Explicit {
                    client_cert_file,
                    client_key_file,
                },
            }),
        })),
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCs, sink(), sink())
    .await
    .expect("spawn always returns Ok now; the dial happens inside the task");

    let resp = client.call(boot_action()).await.expect("boot call failed");
    assert!(matches!(resp, Response16::BootNotification(_)));

    client.terminate().await.expect("client terminate failed");
    server.terminate().await.expect("server terminate failed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// OC-R-039 — server-role `SkipVerify` still requires a client certificate be presented (a
/// handshake with none fails, same as `Verify`), but performs no chain/identity validation: a
/// client presenting a self-signed identity trusted by nobody the server knows about is still
/// accepted. OC-R-115 — the client's self-signed identity is generated via the same
/// cache/regenerate rule as the server side.
async fn it_csms_skip_verify_accepts_untrusted_self_signed_client_identity() {
    let server = csms::ServerBuilder::<V1_6>::new(
        csms::Config {
            host: "127.0.0.1".to_owned(),
            port: 0,
            timeout_ms: 2000,
            reconnect: true,
            basic_auth: None,
            tls: Some(ServerTlsPolicy::MutualTls {
                server_cert: ServerCertSource::SelfSigned,
                client_verification: ClientCertVerification::SkipVerify,
            }),
        },
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCsms, sink())
    .await
    .expect("server failed to bind");

    let url = format!("wss://{}/ocpp/CS001", bound_addr(&server).await);
    let client = cs::ClientBuilder::<V1_6>::new(
        std::sync::Arc::new(tokio::sync::RwLock::new(cs::Config {
            extra_headers: Vec::new(),
            url,
            reconnect: true,
            timeout_ms: 2000,
            basic_auth: None,
            tls: Some(ClientTlsPolicy::MutualTls {
                client_verification: ClientVerification::SkipVerify,
                client_identity: ClientCertSource::SelfSigned,
            }),
        })),
        ferrowl_ocpp::new_self_signed_cache(),
    )
    .spawn(TestCs, sink(), sink())
    .await
    .expect("spawn always returns Ok now; the dial happens inside the task");

    let resp = client.call(boot_action()).await.expect("boot call failed");
    assert!(matches!(resp, Response16::BootNotification(_)));

    client.terminate().await.expect("client terminate failed");
    server.terminate().await.expect("server terminate failed");
}
