//! Modbus TCP client and server.

mod client;
mod server;
pub(crate) mod tls;

use clap::Args;
use serde::{Deserialize, Serialize};

pub use client::{Client, ClientBuilder};
pub use server::ServerBuilder;
pub use tls::{ModbusTlsConfig, SelfSignedCache, new_self_signed_cache};

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

    /// TLS material for this endpoint: a two-role container, both policies defaulting to
    /// `None` (MB-R-104).
    #[serde(default, skip_serializing_if = "ModbusTlsConfig::is_none")]
    #[arg(skip)]
    pub tls: ModbusTlsConfig,
}

fn default_reconnect() -> bool {
    true
}

impl Config {
    /// This endpoint's server-role TLS policy (MB-R-104): the container's own `server` field —
    /// never read when this endpoint runs as a client, and treated as inert if it is anything
    /// other than `None` while this endpoint runs as a client.
    pub(crate) fn server_tls_policy(&self) -> ferrowl_util::tls::ServerTlsPolicy {
        self.tls.server.clone()
    }

    /// This endpoint's client-role TLS policy (MB-R-104): the container's own `client` field —
    /// never read when this endpoint runs as a server.
    pub(crate) fn client_tls_policy(&self) -> ferrowl_util::tls::ClientTlsPolicy {
        self.tls.client.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::Config;
    use ferrowl_util::tls::{CertSource, CertVerification, ClientTlsPolicy, ServerTlsPolicy};

    /// MB-R-162 — a `tcp::Config` with no `tls` key deserializes both policies to `None`.
    #[test]
    fn ut_tcp_config_tls_absent_deserializes_to_default() {
        let json = r#"{
            "ip": "127.0.0.1",
            "port": 502,
            "timeout_ms": 3000,
            "delay_ms": 0,
            "interval_ms": 0
        }"#;
        let cfg: Config = serde_json::from_str(json).expect("deserializes");
        assert!(cfg.tls.is_none());
    }

    /// MB-R-104, MB-R-105 — a `tcp::Config` with a full block-form `tls` object deserializes
    /// into the matching two-role container, and round-trips back through TOML unchanged.
    #[test]
    fn ut_tcp_config_tls_block_roundtrips() {
        let json = r#"{
            "ip": "127.0.0.1",
            "port": 502,
            "timeout_ms": 3000,
            "delay_ms": 0,
            "interval_ms": 0,
            "tls": {
                "server": {
                    "mode": "mutual",
                    "identity": {"source": "files", "cert_file": "cert.pem", "key_file": "key.pem"},
                    "verification": {"verify": "ca-files", "ca_files": ["client-ca.pem"]}
                },
                "client": {
                    "mode": "tls",
                    "verification": {"verify": "skip"}
                }
            }
        }"#;
        let cfg: Config = serde_json::from_str(json).expect("deserializes");
        assert_eq!(
            cfg.tls.server,
            ServerTlsPolicy::Mutual {
                identity: CertSource::Files {
                    cert_file: "cert.pem".to_string(),
                    key_file: "key.pem".to_string(),
                },
                verification: CertVerification::CaFiles {
                    ca_files: vec!["client-ca.pem".to_string()],
                },
            }
        );
        assert_eq!(
            cfg.tls.client,
            ClientTlsPolicy::Tls {
                verification: CertVerification::Skip {},
            }
        );

        let json = serde_json::to_string(&cfg.tls).expect("serialize json");
        let back: super::ModbusTlsConfig = serde_json::from_str(&json).expect("deserialize json");
        assert_eq!(cfg.tls, back);
    }
}
