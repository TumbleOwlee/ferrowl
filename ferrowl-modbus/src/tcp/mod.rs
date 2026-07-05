//! Modbus TCP client and server.

mod client;
mod server;

use clap::Args;
use serde::{Deserialize, Serialize};

pub use client::{Client, ClientBuilder};
pub use server::ServerBuilder;

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
}

fn default_reconnect() -> bool {
    true
}
