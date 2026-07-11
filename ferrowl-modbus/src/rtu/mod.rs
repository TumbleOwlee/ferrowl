//! Modbus RTU (serial) client and server.

mod client;
mod server;

use clap::Args;
use serde::{Deserialize, Serialize};

pub use client::{Client, ClientBuilder};
pub use server::ServerBuilder;

/// Modbus RTU serial settings; doubles as the clap argument group for RTU
/// mode.
#[derive(Serialize, Deserialize, Clone, Debug, Default, Args)]
pub struct Config {
    /// The device path to use for communication.
    pub path: String,

    /// The baud rate to use for the serial connection.
    #[arg(short, long, default_value_t = 115200)]
    pub baud_rate: u32,

    /// The Modbus slave id to use.
    #[arg(short, long, default_value_t = 1)]
    pub slave: u8,

    /// The Modbus parity bit [values: even, odd, none]
    #[arg(short, long)]
    pub parity: Option<String>,

    /// The Modbus data bits [values: 5, 6, 7, 8]
    #[arg(short, long)]
    pub data_bits: Option<u8>,

    /// The Modbus stop bits [values: 1, 2]
    #[arg(short, long)]
    pub stop_bits: Option<u8>,

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
}

fn default_reconnect() -> bool {
    true
}

// NOTE: `Config` doubles as a clap `Args` group, but flattening it into any `clap::Parser`
// command (as production CLI code must) panics at parse time via clap's debug assertions:
// short option '-s' is claimed by both `slave` and `stop_bits` (and, by the same auto-derived-
// from-field-initial rule, '-d' by both `data_bits` and `delay_ms`). This is a pre-existing bug
// in the `#[arg(short, ...)]` attributes above, outside this test-coverage pass's scope to fix;
// flagging it here rather than silently working around it. Only the serde (de)serialization
// path — unaffected by the clap collision — is unit-tested below.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_config_serde_defaults_reconnect_true_when_absent() {
        let json = r#"{"path":"/dev/ttyUSB0","baud_rate":115200,"slave":1,"parity":null,"data_bits":null,"stop_bits":null,"timeout_ms":3000,"delay_ms":0,"interval_ms":0}"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert!(cfg.reconnect);
    }

    #[test]
    fn ut_config_serde_respects_explicit_reconnect_false() {
        let json = r#"{"path":"/dev/ttyUSB0","baud_rate":115200,"slave":1,"parity":null,"data_bits":null,"stop_bits":null,"timeout_ms":3000,"delay_ms":0,"interval_ms":0,"reconnect":false}"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert!(!cfg.reconnect);
    }
}
