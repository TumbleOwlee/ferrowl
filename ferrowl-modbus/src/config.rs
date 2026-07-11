//! Shared configuration for Modbus clients and servers.

use clap::Args;
use serde::{Deserialize, Serialize};

/// Common timing and behavior settings for Modbus TCP and RTU transports.
#[derive(Serialize, Deserialize, Clone, Debug, Default, Args)]
pub struct CommonTimingConfig {
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
