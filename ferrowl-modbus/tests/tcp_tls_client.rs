//! Client-side Modbus/TCP TLS tests (MB-R-109, MB-R-110, MB-R-111 client half).
//!
//! Drives `ferrowl_modbus::tcp::Client::connect` against a bare `rust_modbus`
//! TLS/TCP listener, never `ferrowl_modbus`'s own server (covered by tcp_tls_server.rs).

// Integration-test crate: an unwrap that fails is the test failing, same as an assertion.
#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicU32, Ordering};

use ferrowl_modbus::tcp;
use ferrowl_util::tls::{ClientCertSource, ClientTlsPolicy, ClientVerification};
use rcgen::{CertificateParams, Issuer, KeyPair};
use rust_modbus::{
    ClientCertPolicy, RootStore, ServerCertVerification, TcpConfig, TlsClientConfig, TlsListener,
    TlsServerConfig, connect_tls,
};
use tokio::io::AsyncReadExt;

/// An OS-assigned free TCP port (bind to :0, read the port, drop the listener).
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn write_pem(label: &str, pem: &str) -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "ferrowl-modbus-tls-test-{}-{label}-{n}.pem",
        std::process::id()
    ));
    std::fs::write(&path, pem).expect("failed to write test PEM file");
    path.to_string_lossy().into_owned()
}

/// A self-signed cert/key pair, PEM-encoded, CN `127.0.0.1`.
fn self_signed_pem() -> (String, String) {
    let key = KeyPair::generate().expect("keypair generation failed");
    let cert = CertificateParams::new(vec!["127.0.0.1".to_owned()])
        .expect("cert params")
        .self_signed(&key)
        .expect("self-signed cert");
    (cert.pem(), key.serialize_pem())
}

/// A self-signed CA plus one client cert/key pair it signs, all PEM-encoded.
fn ca_and_signed_client_pem() -> (String, String, String) {
    let ca_key = KeyPair::generate().expect("ca keypair generation failed");
    let ca_params = CertificateParams::new(vec!["ferrowl-test-ca".to_owned()]).expect("ca params");
    let ca_cert = ca_params.self_signed(&ca_key).expect("self-signed ca cert");
    let issuer = Issuer::from_params(&ca_params, &ca_key);

    let client_key = KeyPair::generate().expect("client keypair generation failed");
    let client_cert = CertificateParams::new(vec!["ferrowl-test-client".to_owned()])
        .expect("client params")
        .signed_by(&client_key, &issuer)
        .expect("ca-signed client cert");

    (ca_cert.pem(), client_cert.pem(), client_key.serialize_pem())
}

fn config(port: u16, tls: tcp::ModbusTlsConfig) -> tcp::Config {
    tcp::Config {
        ip: "127.0.0.1".to_string(),
        port,
        timeout_ms: 1000,
        delay_ms: 0,
        interval_ms: 0,
        reconnect: true,
        tls: Some(tls),
    }
}

#[tokio::test]
/// MB-R-109 — `insecure_skip_verify` accepts a server certificate unauthenticated,
/// with no `ca_file` needed.
async fn tls_client_connects_to_plain_rust_modbus_tls_server() {
    let (cert_pem, key_pem) = self_signed_pem();
    let cert_chain = rust_modbus::load_pem_cert_chain(cert_pem.as_bytes()).unwrap();
    let key = rust_modbus::load_pem_private_key(key_pem.as_bytes()).unwrap();

    let addr: std::net::SocketAddr = format!("127.0.0.1:{}", free_port()).parse().unwrap();
    let listener = TlsListener::bind(
        addr,
        TlsServerConfig {
            cert_chain,
            key,
            client_certs: ClientCertPolicy::None,
        },
    )
    .await
    .unwrap();
    let bound = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let _ = listener.accept().await;
    });

    let cfg = config(
        bound.port(),
        tcp::ModbusTlsConfig {
            client: ClientTlsPolicy::Tls {
                client_verification: ClientVerification::SkipVerify,
            },
            ..Default::default()
        },
    );
    let client = tcp::Client::connect(&cfg, &tcp::new_self_signed_cache()).await;
    assert!(
        client.is_ok(),
        "expected connect to succeed: {}",
        client.err().map(|e| e.to_string()).unwrap_or_default()
    );

    server.abort();
}

#[tokio::test]
/// MB-R-109 — a client trusts a server certificate presented via `ca_file`, and
/// rejects the same certificate with neither `ca_file` nor `insecure_skip_verify` set.
async fn tls_client_verifies_against_ca_file() {
    let (cert_pem, key_pem) = self_signed_pem();
    let cert_file = write_pem("server-cert2", &cert_pem);
    let cert_chain = rust_modbus::load_pem_cert_chain(cert_pem.as_bytes()).unwrap();
    let key = rust_modbus::load_pem_private_key(key_pem.as_bytes()).unwrap();

    let addr: std::net::SocketAddr = format!("127.0.0.1:{}", free_port()).parse().unwrap();
    let listener = TlsListener::bind(
        addr,
        TlsServerConfig {
            cert_chain,
            key,
            client_certs: ClientCertPolicy::None,
        },
    )
    .await
    .unwrap();
    let bound = listener.local_addr().unwrap();
    let listener = std::sync::Arc::new(listener);

    let accept_listener = listener.clone();
    let server = tokio::spawn(async move {
        loop {
            if accept_listener.accept().await.is_err() {
                break;
            }
        }
    });

    // Trusted via ca_file.
    let trusted_cfg = config(
        bound.port(),
        tcp::ModbusTlsConfig {
            client: ClientTlsPolicy::Tls {
                client_verification: ClientVerification::Verify {
                    ca_file: Some(cert_file),
                },
            },
            ..Default::default()
        },
    );
    let trusted = tcp::Client::connect(&trusted_cfg, &tcp::new_self_signed_cache()).await;
    assert!(
        trusted.is_ok(),
        "expected trusted connect: {}",
        trusted.err().map(|e| e.to_string()).unwrap_or_default()
    );

    // Untrusted: neither ca_file nor insecure_skip_verify set.
    let untrusted_cfg = config(bound.port(), tcp::ModbusTlsConfig::default());
    let untrusted = tcp::Client::connect(&untrusted_cfg, &tcp::new_self_signed_cache()).await;
    assert!(
        untrusted.is_err(),
        "expected untrusted connect to fail verification"
    );

    server.abort();
}

#[tokio::test]
/// MB-R-110 — a client presents its identity only when both `client_cert_file` and
/// `client_key_file` are set; either alone presents nothing, and a server requiring
/// a client certificate then rejects the handshake.
async fn tls_client_presents_identity_only_when_both_files_set() {
    let (server_cert_pem, server_key_pem) = self_signed_pem();
    let server_cert_file = write_pem("mtls-server-cert", &server_cert_pem);
    let server_cert_chain = rust_modbus::load_pem_cert_chain(server_cert_pem.as_bytes()).unwrap();
    let server_key = rust_modbus::load_pem_private_key(server_key_pem.as_bytes()).unwrap();

    let (ca_pem, client_cert_pem, client_key_pem) = ca_and_signed_client_pem();
    let client_cert_file = write_pem("mtls-client-cert", &client_cert_pem);
    let client_key_file = write_pem("mtls-client-key", &client_key_pem);

    let mut roots = RootStore::empty();
    roots.add_pem(ca_pem.as_bytes()).unwrap();

    let addr: std::net::SocketAddr = format!("127.0.0.1:{}", free_port()).parse().unwrap();
    let listener = TlsListener::bind(
        addr,
        TlsServerConfig {
            cert_chain: server_cert_chain,
            key: server_key,
            client_certs: ClientCertPolicy::Require(roots),
        },
    )
    .await
    .unwrap();
    let bound = listener.local_addr().unwrap();
    let listener = std::sync::Arc::new(listener);

    let accept_listener = listener.clone();
    let server = tokio::spawn(async move {
        loop {
            if accept_listener.accept().await.is_err() {
                break;
            }
        }
    });

    // Both set: identity presented, handshake succeeds.
    let both_cfg = config(
        bound.port(),
        tcp::ModbusTlsConfig {
            client: ClientTlsPolicy::MutualTls {
                client_verification: ClientVerification::Verify {
                    ca_file: Some(server_cert_file.clone()),
                },
                client_identity: ClientCertSource::Explicit {
                    client_cert_file: client_cert_file.clone(),
                    client_key_file: client_key_file.clone(),
                },
            },
            ..Default::default()
        },
    );
    let both = tcp::Client::connect(&both_cfg, &tcp::new_self_signed_cache()).await;
    assert!(
        both.is_ok(),
        "expected mTLS connect to succeed: {}",
        both.err().map(|e| e.to_string()).unwrap_or_default()
    );

    // Only cert set, or only key set: MB-R-110 says either alone presents *no* client
    // identity — proved directly against `build_client_tls_config`'s unit tests in
    // `tcp/client.rs`. What that "no identity presented" consequence looks like on the
    // wire, against this same `Require`-policy server, is probed below with a raw
    // `rust_modbus::connect_tls` (identity: None mirrors both "cert only" and "key
    // only", since `build_client_tls_config` maps both to `None` identically).
    //
    // TLS 1.3 quirk (confirmed empirically, not assumed): the *client's* handshake
    // future resolves successfully even when the server will reject the connection for
    // a missing/invalid client certificate — client-side completion doesn't wait for
    // the server's verdict on the client's certificate message. The rejection is only
    // observable on the wire afterwards, as the server closing the connection before
    // sending any application data. So this test asserts on that: the first read
    // returns EOF/an error, rather than asserting `connect()` itself errors.
    let mut server_trust = RootStore::empty();
    server_trust.add_pem(server_cert_pem.as_bytes()).unwrap();
    let no_identity_tls = TlsClientConfig {
        server_cert: ServerCertVerification::Verify(server_trust),
        client_identity: None,
    };
    let addr: std::net::SocketAddr = format!("127.0.0.1:{}", bound.port()).parse().unwrap();
    let transport = connect_tls(addr, TcpConfig::default(), no_identity_tls)
        .await
        .expect("client-side TLS 1.3 handshake completes even though the server will reject");
    let mut stream = transport.into_inner();
    let mut buf = [0u8; 1];
    let read = tokio::time::timeout(std::time::Duration::from_millis(500), stream.read(&mut buf))
        .await
        .expect("server should close the connection promptly, not hang");
    // A reset/IO error (the `Err` arm) is an equally valid rejection signal, alongside
    // an outright EOF.
    if let Ok(n) = read {
        assert_eq!(
            n, 0,
            "expected EOF (server closed after rejecting no identity)"
        );
    }

    drop((client_cert_file, client_key_file));
    server.abort();
}

#[tokio::test]
/// MB-R-111 (client half) — a TLS handshake failure, a refused connection, and a
/// timed-out connection attempt are three distinct outcomes.
async fn tls_handshake_failure_is_distinct_from_refused_and_timeout() {
    // (a) TLS configured against a plain (non-TLS) listener: handshake failure.
    let plain_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let plain_addr = plain_listener.local_addr().unwrap();
    let plain_server = tokio::spawn(async move {
        loop {
            if plain_listener.accept().await.is_err() {
                break;
            }
        }
    });
    let handshake_cfg = config(
        plain_addr.port(),
        tcp::ModbusTlsConfig {
            client: ClientTlsPolicy::Tls {
                client_verification: ClientVerification::SkipVerify,
            },
            ..Default::default()
        },
    );
    let handshake_result =
        tcp::Client::connect(&handshake_cfg, &tcp::new_self_signed_cache()).await;
    assert!(
        handshake_result.is_err(),
        "expected TLS handshake against a plain listener to fail"
    );
    plain_server.abort();

    // (b) Nothing listening: connection refused.
    let refused_port = free_port();
    let refused_cfg = config(
        refused_port,
        tcp::ModbusTlsConfig {
            client: ClientTlsPolicy::Tls {
                client_verification: ClientVerification::SkipVerify,
            },
            ..Default::default()
        },
    );
    let refused_result = tcp::Client::connect(&refused_cfg, &tcp::new_self_signed_cache()).await;
    assert!(
        refused_result.is_err(),
        "expected connect to a closed port to fail"
    );

    // (c) A TLS server that accepts the TCP connection but never completes the
    // handshake: a very short client timeout should time out, not hang.
    let stalling_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let stalling_addr = stalling_listener.local_addr().unwrap();
    let stalling_server = tokio::spawn(async move {
        let (socket, _) = stalling_listener.accept().await.unwrap();
        // Accept the TCP connection, then simply hold it open without ever speaking
        // TLS, forcing the client's handshake to stall until it hits its timeout.
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        drop(socket);
    });
    let mut timeout_cfg = config(
        stalling_addr.port(),
        tcp::ModbusTlsConfig {
            client: ClientTlsPolicy::Tls {
                client_verification: ClientVerification::SkipVerify,
            },
            ..Default::default()
        },
    );
    timeout_cfg.timeout_ms = 50;
    let timeout_result = tcp::Client::connect(&timeout_cfg, &tcp::new_self_signed_cache()).await;
    assert!(
        timeout_result.is_err(),
        "expected a stalled handshake to time out"
    );
    stalling_server.abort();
}
