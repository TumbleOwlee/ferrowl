//! OCPP per-instance config: name, protocol version, role, websocket endpoint. Serialized
//! into session files (tagged `"type":"ocpp"`) and produced by the OCPP setup dialog.

use serde::{Deserialize, Serialize};

use ferrowl_ui::traits::ToLabel;

use super::device::OcppSecurityConfig;

/// OCPP protocol version a charging station speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OcppVersion {
    #[serde(rename = "1.6")]
    #[default]
    V1_6,
    #[serde(rename = "2.0.1")]
    V2_0_1,
    #[serde(rename = "2.1")]
    V2_1,
}

impl std::fmt::Display for OcppVersion {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OcppVersion::V1_6 => write!(fmt, "1.6"),
            OcppVersion::V2_0_1 => write!(fmt, "2.0.1"),
            OcppVersion::V2_1 => write!(fmt, "2.1"),
        }
    }
}

impl ToLabel for OcppVersion {
    fn to_label(&self) -> String {
        self.to_string()
    }
}

/// Whether the module acts as a charging station (client) or a management system (server).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OcppRole {
    #[default]
    Client,
    Server,
}

impl std::fmt::Display for OcppRole {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OcppRole::Client => write!(fmt, "Client"),
            OcppRole::Server => write!(fmt, "Server"),
        }
    }
}

impl ToLabel for OcppRole {
    fn to_label(&self) -> String {
        self.to_string()
    }
}

/// Websocket scheme for the OCPP endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OcppProtocol {
    #[default]
    Ws,
    Wss,
}

impl std::fmt::Display for OcppProtocol {
    /// Renders the URL scheme prefix, e.g. `ws://`.
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OcppProtocol::Ws => write!(fmt, "ws://"),
            OcppProtocol::Wss => write!(fmt, "wss://"),
        }
    }
}

impl ToLabel for OcppProtocol {
    fn to_label(&self) -> String {
        self.to_string()
    }
}

/// One OCPP module instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OcppSpec {
    pub name: String,
    #[serde(default)]
    pub version: OcppVersion,
    #[serde(default)]
    pub role: OcppRole,
    #[serde(default)]
    pub protocol: OcppProtocol,
    pub ip: String,
    pub port: u16,
    /// URL path appended after the endpoint, e.g. `/ocpp/cp001` (empty = none). The OCPP
    /// charge-point identity is conventionally a trailing path segment.
    #[serde(default)]
    pub path: String,
    /// Awaited-reply timeout (ms); `None` uses the crate default (30_000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Websocket transport security (Basic Auth / TLS / mTLS); defaults to plain `ws://`.
    #[serde(default, skip_serializing_if = "OcppSecurityConfig::is_empty")]
    pub security: OcppSecurityConfig,
}

impl OcppSpec {
    /// Full websocket URL, e.g. `ws://127.0.0.1:9000/ocpp/cp001` (path omitted when empty).
    pub fn url(&self) -> String {
        format!("{}{}:{}{}", self.protocol, self.ip, self.port, self.path)
    }

    /// TLS config a CSMS (server role) should run with: the configured material, or — when the
    /// endpoint is `wss://` but the security section provides neither certificate files nor
    /// `self_signed` — an ephemeral self-signed fallback. A wss-labeled server must never
    /// silently bind plain TCP; this mirrors the setup dialog's semantics for configs that
    /// bypass the dialog (hand-written files, `--ocpp` flags, pre-existing sessions).
    ///
    /// The scheme is authoritative in the other direction too: a `ws://` server binds plain TCP
    /// even when certificate files are configured, so its URL never lies about its transport.
    /// This mirrors a `ws://` client, which likewise ignores its own TLS material.
    pub fn effective_csms_tls(&self) -> Option<ferrowl_ocpp::CsmsTlsConfig> {
        if self.protocol != OcppProtocol::Wss {
            return None;
        }
        self.security.csms_tls().or_else(|| {
            self.csms_self_signed_fallback()
                .then_some(ferrowl_ocpp::CsmsTlsConfig {
                    mode: ferrowl_ocpp::CsmsTlsMode::SelfSigned,
                    client_ca_file: None,
                    require_client_cert: false,
                })
        })
    }

    /// True when [`effective_csms_tls`](Self::effective_csms_tls) falls back to an ephemeral
    /// self-signed certificate (wss endpoint without configured TLS material) — callers use
    /// this to surface the fallback in the module log.
    pub fn csms_self_signed_fallback(&self) -> bool {
        self.protocol == OcppProtocol::Wss && self.security.csms_tls().is_none()
    }

    /// Build the runtime spec from its persistence halves: the endpoint comes from the session
    /// [`OcppModuleSpec`], the version/role/timeout/security from the device config.
    pub fn from_parts(module: &OcppModuleSpec, device: &super::device::OcppDeviceConfig) -> Self {
        Self {
            name: module.name.clone(),
            version: device.ocpp_version,
            role: device.role,
            protocol: module.protocol,
            ip: module.ip.clone(),
            port: module.port,
            path: module.path.clone(),
            timeout_ms: device.timeout_ms,
            security: device.security.clone(),
        }
    }
}

/// Session-level OCPP module entry: the tab name, the device-config file path, and the
/// per-instance websocket endpoint. Mirrors the Modbus `ModuleSpec` split — version/role/timeout
/// and the Lua scripts live in the referenced device file, not here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OcppModuleSpec {
    pub name: String,
    /// Path to the OCPP device-config file.
    pub device: String,
    #[serde(default)]
    pub protocol: OcppProtocol,
    pub ip: String,
    pub port: u16,
    /// URL path appended after the endpoint, e.g. `/ocpp/cp001` (empty = none).
    #[serde(default)]
    pub path: String,
}

impl OcppModuleSpec {
    /// Build the session entry from a runtime spec plus the device-config path.
    pub fn from_spec(spec: &OcppSpec, device: &str) -> Self {
        Self {
            name: spec.name.clone(),
            device: device.to_string(),
            protocol: spec.protocol,
            ip: spec.ip.clone(),
            port: spec.port,
            path: spec.path.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// OC-R-081 — the session entry carries the endpoint (scheme, ip, port, path), from which the connection URL is built.
    fn ut_protocol_display_and_url() {
        let spec = OcppSpec {
            name: "cs-1".into(),
            version: OcppVersion::V1_6,
            role: OcppRole::Client,
            protocol: OcppProtocol::Ws,
            ip: "127.0.0.1".into(),
            port: 9000,
            path: String::new(),
            timeout_ms: None,
            security: OcppSecurityConfig::default(),
        };
        assert_eq!(spec.url(), "ws://127.0.0.1:9000");

        let secure = OcppSpec {
            protocol: OcppProtocol::Wss,
            ..spec.clone()
        };
        assert_eq!(secure.url(), "wss://127.0.0.1:9000");

        let with_path = OcppSpec {
            path: "/ocpp/cp001".into(),
            ..spec.clone()
        };
        assert_eq!(with_path.url(), "ws://127.0.0.1:9000/ocpp/cp001");
    }

    #[test]
    fn ut_labels() {
        assert_eq!(OcppVersion::V2_0_1.to_label(), "2.0.1");
        assert_eq!(OcppRole::Server.to_label(), "Server");
        assert_eq!(OcppProtocol::Wss.to_label(), "wss://");
    }

    #[test]
    fn ut_spec_roundtrip_with_type_tag() {
        let spec = OcppSpec {
            name: "cs-1".into(),
            version: OcppVersion::V2_0_1,
            role: OcppRole::Client,
            protocol: OcppProtocol::Ws,
            ip: "10.0.0.5".into(),
            port: 8080,
            path: "/ocpp/cp001".into(),
            timeout_ms: Some(5000),
            security: OcppSecurityConfig::default(),
        };
        let mut v = serde_json::to_value(&spec).unwrap();
        v.as_object_mut()
            .unwrap()
            .insert("type".into(), "ocpp".into());
        let back: OcppSpec = serde_json::from_value(v).unwrap();
        assert_eq!(spec, back);
    }

    fn spec_with(
        protocol: OcppProtocol,
        security: crate::config::ocpp::OcppSecurityConfig,
    ) -> OcppSpec {
        OcppSpec {
            name: "csms".into(),
            version: OcppVersion::V1_6,
            role: OcppRole::Server,
            protocol,
            ip: "127.0.0.1".into(),
            port: 9000,
            path: String::new(),
            timeout_ms: None,
            security,
        }
    }

    // A wss endpoint without TLS material must never yield `None` (would bind plain TCP):
    // it falls back to the ephemeral self-signed mode.
    #[test]
    /// OC-R-095 — a wss CSMS with no TLS material configured falls back to an ephemeral self-signed certificate.
    fn ut_effective_csms_tls_wss_without_material_falls_back_to_self_signed() {
        let spec = spec_with(OcppProtocol::Wss, Default::default());
        assert!(spec.csms_self_signed_fallback());
        let tls = spec.effective_csms_tls().expect("fallback TLS");
        assert!(matches!(tls.mode, ferrowl_ocpp::CsmsTlsMode::SelfSigned));
        assert!(!tls.require_client_cert);
    }

    #[test]
    /// OC-R-042 — a ws:// CSMS endpoint binds plain TCP; the endpoint scheme is authoritative for the transport.
    fn ut_effective_csms_tls_ws_stays_plain() {
        let spec = spec_with(OcppProtocol::Ws, Default::default());
        assert!(!spec.csms_self_signed_fallback());
        assert!(spec.effective_csms_tls().is_none());
    }

    // The scheme wins: a ws endpoint binds plain TCP even with certificate files configured, so
    // the URL never advertises a transport the listener doesn't speak.
    #[test]
    /// OC-R-042 — a ws:// CSMS endpoint leaves any configured TLS material inert.
    fn ut_effective_csms_tls_ws_ignores_configured_certificates() {
        let security = crate::config::ocpp::OcppSecurityConfig {
            cert_file: Some("certs/csms.pem".into()),
            key_file: Some("certs/csms.key".into()),
            ..Default::default()
        };
        let spec = spec_with(OcppProtocol::Ws, security);
        assert!(spec.effective_csms_tls().is_none());
    }

    #[test]
    /// OC-R-096 — explicit server cert + key files take precedence over the ephemeral self-signed fallback.
    fn ut_effective_csms_tls_explicit_files_win_over_fallback() {
        let security = crate::config::ocpp::OcppSecurityConfig {
            cert_file: Some("certs/csms.pem".into()),
            key_file: Some("certs/csms.key".into()),
            ..Default::default()
        };
        let spec = spec_with(OcppProtocol::Wss, security);
        assert!(!spec.csms_self_signed_fallback());
        let tls = spec.effective_csms_tls().expect("configured TLS");
        assert!(matches!(tls.mode, ferrowl_ocpp::CsmsTlsMode::Files { .. }));
    }
}
