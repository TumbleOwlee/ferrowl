//! Modbus TCP client and server.

mod client;
mod server;
mod tls;

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

    /// Client-only: automatically reconnect (with backoff) on a lost or refused connection
    /// instead of ending the client task. Ignored by the server.
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
        assert_eq!(tls.ca_file.as_deref(), Some("ca.pem"));
        assert_eq!(tls.cert_file.as_deref(), Some("cert.pem"));
        assert_eq!(tls.key_file.as_deref(), Some("key.pem"));
        assert_eq!(tls.client_cert_file.as_deref(), Some("client.pem"));
        assert_eq!(tls.client_key_file.as_deref(), Some("client-key.pem"));
        assert_eq!(tls.client_ca_file.as_deref(), Some("client-ca.pem"));
        assert!(tls.require_client_cert);
        assert!(!tls.self_signed);
        assert!(tls.insecure_skip_verify);
    }
}
