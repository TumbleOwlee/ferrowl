//! Modbus/TCP TLS configuration (MB-R-104 .. MB-R-112).

use serde::{Deserialize, Serialize};

use crate::TcpError;

/// TLS material for a Modbus/TCP endpoint, client or server (MB-R-105).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ModbusTlsConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cert_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_cert_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_key_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_ca_file: Option<String>,
    #[serde(default)]
    pub require_client_cert: bool,
    #[serde(default)]
    pub self_signed: bool,
    #[serde(default)]
    pub insecure_skip_verify: bool,
}

/// Read a PEM file's raw bytes, mapping a missing/unreadable file to the TLS
/// configuration-error tier (MB-R-107/108, edge-cases.md "malformed PEM").
///
/// Unused until s2 (client.rs) and s3 (server.rs) wire it into their connect/serve
/// paths; allowed dead here rather than deferred, since both consume it without
/// otherwise touching this file.
#[allow(dead_code)]
pub(crate) fn read_pem(path: &str) -> Result<Vec<u8>, TcpError> {
    std::fs::read(path).map_err(|e| TcpError::Configuration(format!("failed to read {path}: {e}")))
}

/// Map any `rust_modbus` failure encountered while assembling TLS material (a bad
/// PEM, an untrusted root, ...) onto the same configuration-error tier.
///
/// Unused until s2/s3 wire it in — see `read_pem` above.
#[allow(dead_code)]
pub(crate) fn map_tls_err(e: rust_modbus::Error) -> TcpError {
    TcpError::Configuration(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::ModbusTlsConfig;

    /// MB-R-105 — `ModbusTlsConfig` carries exactly nine fields, six optional
    /// strings unset and three bools false, by default.
    #[test]
    fn ut_modbus_tls_config_defaults() {
        let cfg = ModbusTlsConfig::default();
        assert_eq!(cfg.ca_file, None);
        assert_eq!(cfg.cert_file, None);
        assert_eq!(cfg.key_file, None);
        assert_eq!(cfg.client_cert_file, None);
        assert_eq!(cfg.client_key_file, None);
        assert_eq!(cfg.client_ca_file, None);
        assert!(!cfg.require_client_cert);
        assert!(!cfg.self_signed);
        assert!(!cfg.insecure_skip_verify);
    }
}
