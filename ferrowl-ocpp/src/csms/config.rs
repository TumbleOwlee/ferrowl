//! CSMS listener configuration (serde only; no CLI binding yet — Decision 8).

use std::time::Duration;

/// Configuration for a CSMS server's listening socket.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    /// Listen address (e.g. `"127.0.0.1"`).
    pub host: String,
    /// Listen port. Use `0` to let the OS assign one.
    pub port: u16,
    /// How long to wait for a correlated reply before failing an awaited Call.
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_timeout_ms() -> u64 {
    30_000
}

impl Config {
    pub(crate) fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }
}
