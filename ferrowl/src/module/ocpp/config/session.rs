//! OCPP per-instance config: name, protocol version, role, websocket endpoint. Serialized
//! into session files (tagged `"type":"ocpp"`) and produced by the OCPP setup dialog.

use serde::{Deserialize, Serialize};

use ferrowl_ui::traits::ToLabel;

/// OCPP protocol version a charging station speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OcppVersion {
    #[serde(rename = "1.6")]
    #[default]
    V1_6,
    #[serde(rename = "2.0.1")]
    V2_0_1,
}

impl std::fmt::Display for OcppVersion {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OcppVersion::V1_6 => write!(fmt, "1.6"),
            OcppVersion::V2_0_1 => write!(fmt, "2.0.1"),
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
    /// Awaited-reply timeout (ms); `None` uses the crate default (30_000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

impl OcppSpec {
    /// Full websocket URL, e.g. `ws://127.0.0.1:9000`. The OCPP charge-point identity is
    /// conventionally a trailing path segment; not modelled yet.
    pub fn url(&self) -> String {
        format!("{}{}:{}", self.protocol, self.ip, self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_protocol_display_and_url() {
        let spec = OcppSpec {
            name: "cs-1".into(),
            version: OcppVersion::V1_6,
            role: OcppRole::Client,
            protocol: OcppProtocol::Ws,
            ip: "127.0.0.1".into(),
            port: 9000,
            timeout_ms: None,
        };
        assert_eq!(spec.url(), "ws://127.0.0.1:9000");

        let secure = OcppSpec {
            protocol: OcppProtocol::Wss,
            ..spec.clone()
        };
        assert_eq!(secure.url(), "wss://127.0.0.1:9000");
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
            timeout_ms: Some(5000),
        };
        let mut v = serde_json::to_value(&spec).unwrap();
        v.as_object_mut()
            .unwrap()
            .insert("type".into(), "ocpp".into());
        let back: OcppSpec = serde_json::from_value(v).unwrap();
        assert_eq!(spec, back);
    }
}
