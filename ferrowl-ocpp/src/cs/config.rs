//! CS connection configuration (serde only; no CLI binding yet — Decision 8).

use std::time::Duration;

/// Configuration for dialing a CSMS. The OCPP-J identity is conventionally the last path segment
/// of `url` (e.g. `ws://host:9000/ocpp/CS001`); the websocket subprotocol is fixed by the chosen
/// [`Version`](crate::Version), not configured here.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    /// Full websocket URL to dial, including scheme, host, port, and path.
    pub url: String,
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
