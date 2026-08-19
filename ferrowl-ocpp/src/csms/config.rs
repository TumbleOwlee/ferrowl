//! CSMS listener configuration (serde only; no CLI binding yet — Decision 8).

use std::time::Duration;

use ferrowl_util::tls::ServerTlsPolicy;

use crate::security::BasicAuth;

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
    /// Automatically retry a failed listener bind (with backoff) instead of ending the module
    /// task (OC-R-083). Mirrors the CS role's own `reconnect` field.
    #[serde(default = "default_reconnect")]
    pub reconnect: bool,
    /// Expected HTTP Basic Auth credentials (Security Profile 1). When set, the handshake is
    /// rejected with HTTP 401 unless the request presents a matching `Authorization` header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basic_auth: Option<BasicAuth>,
    /// TLS material for the listening socket (Security Profiles 2 and 3). When set, every
    /// accepted connection is TLS-terminated before the OCPP-J handshake.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls: Option<ServerTlsPolicy>,
}

fn default_timeout_ms() -> u64 {
    30_000
}

fn default_reconnect() -> bool {
    true
}

impl Config {
    pub(crate) fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }
}
