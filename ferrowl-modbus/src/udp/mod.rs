//! Modbus UDP client and server.

mod client;
mod server;

use clap::Args;
use serde::{Deserialize, Serialize};

pub use client::{Client, ClientBuilder};
pub use server::ServerBuilder;

/// Modbus UDP connection settings (MB-R-116): the same fields as `tcp::Config` except
/// `tls`, which this type does not carry at all — the upstream UDP transport performs no
/// handshake and offers no DTLS option to configure.
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

    /// Client-only: automatically reconnect (with backoff) on a lost association instead of
    /// ending the client task. Ignored by the server.
    #[serde(default = "default_reconnect")]
    #[arg(long, default_value_t = true)]
    pub reconnect: bool,
}

fn default_reconnect() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::Config;

    /// MB-R-116 — a `udp::Config` carries exactly the TCP field set minus `tls`: no `tls`
    /// field exists on this type at all (not merely unset), unlike `tcp::Config`.
    #[test]
    fn ut_udp_config_has_no_tls_field() {
        let json =
            r#"{"ip":"127.0.0.1","port":502,"timeout_ms":3000,"delay_ms":0,"interval_ms":0}"#;
        let cfg: Config = serde_json::from_str(json).expect("deserializes with no tls key");
        assert_eq!(cfg.ip, "127.0.0.1");
        assert_eq!(cfg.port, 502);
        assert!(cfg.reconnect);
    }
}
