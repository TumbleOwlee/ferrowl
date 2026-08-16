//! Modbus TCP client and server.

mod client;
mod server;
pub(crate) mod tls;

use clap::Args;
use serde::{Deserialize, Serialize};

pub use client::{Client, ClientBuilder};
pub use server::ServerBuilder;
pub use tls::ModbusTlsConfig;

/// Modbus TCP connection settings; doubles as the clap argument group for
/// TCP mode.
#[derive(Serialize, Deserialize, Clone, Debug, Default, Args)]
pub struct Config {
    /// The interface to use for the service or the ip to connect to in client mode.
    #[arg(short, long, default_value_t = String::from("127.0.0.1"))]
    pub ip: String,

    /// The port to use for the service or the port to connect to on target host.
    #[arg(short, long, default_value_t = 502)]
    pub port: u16,

    /// The timeout in milliseconds for each Modbus operation
    #[arg(id = "timeout", short, long, default_value_t = 3000)]
    pub timeout_ms: usize,

    /// The delay in milliseconds of first operation after connect
    #[arg(id = "delay", short, long, default_value_t = 0)]
    pub delay_ms: usize,

    /// The interval in milliseconds between successive operations
    #[arg(id = "interval", short('I'), long, default_value_t = 0)]
    pub interval_ms: usize,

    /// Automatically reconnect (with backoff) instead of ending the task: on a lost or refused
    /// connection (client), or a bind/serve failure (server, MB-R-071/MB-R-130–134).
    #[serde(default = "default_reconnect")]
    #[arg(long, default_value_t = true)]
    pub reconnect: bool,

    /// TLS material for this endpoint. Unset (the default) keeps the endpoint on
    /// plain TCP (MB-R-104).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[arg(skip)]
    pub tls: Option<ModbusTlsConfig>,
}

fn default_reconnect() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::Config;

    /// MB-R-104 — a `tcp::Config` with no `tls` key deserializes with `tls: None`.
    #[test]
    fn ut_tcp_config_tls_absent_deserializes_to_none() {
        let json = r#"{
            "ip": "127.0.0.1",
            "port": 502,
            "timeout_ms": 3000,
            "delay_ms": 0,
            "interval_ms": 0
        }"#;
        let cfg: Config = serde_json::from_str(json).expect("deserializes");
        assert_eq!(cfg.tls, None);
    }

    /// MB-R-104, MB-R-105 — a `tcp::Config` with a full `tls` object deserializes
    /// into the matching `Some(ModbusTlsConfig { .. })`.
    #[test]
    fn ut_tcp_config_tls_present_deserializes() {
        let json = r#"{
            "ip": "127.0.0.1",
            "port": 502,
            "timeout_ms": 3000,
            "delay_ms": 0,
            "interval_ms": 0,
            "tls": {
                "ca_file": "ca.pem",
                "cert_file": "cert.pem",
                "key_file": "key.pem",
                "client_cert_file": "client.pem",
                "client_key_file": "client-key.pem",
                "client_ca_file": "client-ca.pem",
                "require_client_cert": true,
                "self_signed": false,
                "insecure_skip_verify": true
            }
        }"#;
        let cfg: Config = serde_json::from_str(json).expect("deserializes");
        let tls = cfg.tls.expect("tls present");
        // Both cert_file/key_file are set and self_signed is false, so server_cert resolves
        // Explicit; ca_file is present too, but insecure_skip_verify wins (MB-R-109), so it's
        // provably discarded rather than combined with skip-verify.
        assert_eq!(
            tls.server_cert,
            ferrowl_util::tls::ServerCertSource::Explicit {
                cert_file: "cert.pem".to_string(),
                key_file: "key.pem".to_string(),
            }
        );
        assert_eq!(
            tls.client_verification,
            ferrowl_util::tls::ClientVerification::SkipVerify
        );
        assert_eq!(tls.client_cert_file.as_deref(), Some("client.pem"));
        assert_eq!(tls.client_key_file.as_deref(), Some("client-key.pem"));
        assert_eq!(tls.client_ca_file.as_deref(), Some("client-ca.pem"));
        assert!(tls.require_client_cert);
    }
}
