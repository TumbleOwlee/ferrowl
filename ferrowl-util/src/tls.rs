//! Shared TLS-configuration enums, generalizing the server-certificate-source and
//! client-verification axes that both `ferrowl-modbus`'s `ModbusTlsConfig` and `ferrowl-ocpp`'s
//! `CsTlsConfig`/`CsmsTlsMode`/the `ferrowl` crate's `OcppSecurityConfig` represent on the wire
//! as `self_signed`/`cert_file`/`key_file` and `insecure_skip_verify`/`ca_file` respectively.
//! No requirement ID of its own — supports MB-R-106/MB-R-107/MB-R-109/OC-R-036/OC-R-096/
//! OC-R-112 ("struct/type rework" in the approved spec text).

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Where a server's presented TLS certificate comes from. `SelfSigned` and `Explicit` are
/// mutually exclusive by construction (MB-R-106/OC-R-096): self_signed wins unconditionally,
/// making the illegal both-set combination unrepresentable rather than merely
/// unreachable-by-precedence.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ServerCertSource {
    SelfSigned,
    Explicit {
        cert_file: String,
        key_file: String,
    },
    #[default]
    Unset,
}

impl ServerCertSource {
    /// MB-R-106/OC-R-096 precedence (self_signed wins unconditionally) + MB-R-107/OC-R-112
    /// ("alone" is a configuration error). The single "config resolution" implementation,
    /// called from this type's `Deserialize` impl and from every non-serde construction site.
    pub fn resolve(
        self_signed: bool,
        cert_file: Option<String>,
        key_file: Option<String>,
    ) -> Result<Self, String> {
        if self_signed {
            return Ok(ServerCertSource::SelfSigned);
        }
        match (cert_file, key_file) {
            (Some(c), Some(k)) => Ok(ServerCertSource::Explicit {
                cert_file: c,
                key_file: k,
            }),
            (None, None) => Ok(ServerCertSource::Unset),
            _ => Err("cert_file and key_file must both be set, or neither".to_string()),
        }
    }
}

/// Where a client verifies the server's certificate against. MB-R-109/OC-R-036 precedence
/// (skip-verify wins, `ca_file` ignored rather than combined) — already-correct precedence, now
/// type-enforced too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientVerification {
    SkipVerify,
    Verify { ca_file: Option<String> },
}

impl Default for ClientVerification {
    fn default() -> Self {
        ClientVerification::Verify { ca_file: None }
    }
}

impl ClientVerification {
    pub fn resolve(skip_verify: bool, ca_file: Option<String>) -> Self {
        if skip_verify {
            ClientVerification::SkipVerify
        } else {
            ClientVerification::Verify { ca_file }
        }
    }
}

/// Wire shadow for [`ServerCertSource`]'s flattened fields (`cert_file`/`key_file`/
/// `self_signed`), used by its manual `Serialize`/`Deserialize` so the type stays
/// `#[serde(flatten)]`-compatible.
#[derive(Serialize, Deserialize, Default)]
struct RawServerCert {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cert_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key_file: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    self_signed: bool,
}

impl Serialize for ServerCertSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let raw = match self {
            ServerCertSource::SelfSigned => RawServerCert {
                self_signed: true,
                ..Default::default()
            },
            ServerCertSource::Explicit {
                cert_file,
                key_file,
            } => RawServerCert {
                cert_file: Some(cert_file.clone()),
                key_file: Some(key_file.clone()),
                self_signed: false,
            },
            ServerCertSource::Unset => RawServerCert::default(),
        };
        raw.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ServerCertSource {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawServerCert::deserialize(deserializer)?;
        ServerCertSource::resolve(raw.self_signed, raw.cert_file, raw.key_file)
            .map_err(D::Error::custom)
    }
}

/// Wire shadow for [`ClientVerification`]'s flattened fields (`ca_file`/
/// `insecure_skip_verify`), used by its manual `Serialize`/`Deserialize` so the type stays
/// `#[serde(flatten)]`-compatible.
#[derive(Serialize, Deserialize, Default)]
struct RawClientVerification {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ca_file: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    insecure_skip_verify: bool,
}

impl Serialize for ClientVerification {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let raw = match self {
            ClientVerification::SkipVerify => RawClientVerification {
                insecure_skip_verify: true,
                ..Default::default()
            },
            ClientVerification::Verify { ca_file } => RawClientVerification {
                ca_file: ca_file.clone(),
                insecure_skip_verify: false,
            },
        };
        raw.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ClientVerification {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawClientVerification::deserialize(deserializer)?;
        Ok(ClientVerification::resolve(
            raw.insecure_skip_verify,
            raw.ca_file,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Deserialize, Default)]
    struct ServerCertWrapper {
        #[serde(flatten)]
        server_cert: ServerCertSource,
    }

    #[derive(Serialize, Deserialize, Default)]
    struct ClientVerificationWrapper {
        #[serde(flatten)]
        client_verification: ClientVerification,
    }

    /// MB-R-106/OC-R-096 — `self_signed` wins unconditionally over explicit cert/key files.
    #[test]
    fn ut_server_cert_source_resolve_self_signed_wins_over_explicit_files() {
        let resolved =
            ServerCertSource::resolve(true, Some("c".to_string()), Some("k".to_string()));
        assert_eq!(resolved, Ok(ServerCertSource::SelfSigned));
    }

    /// MB-R-106/OC-R-096 — both cert_file and key_file set (no self_signed) resolves Explicit.
    #[test]
    fn ut_server_cert_source_resolve_explicit_when_both_set() {
        let resolved =
            ServerCertSource::resolve(false, Some("c".to_string()), Some("k".to_string()));
        assert_eq!(
            resolved,
            Ok(ServerCertSource::Explicit {
                cert_file: "c".to_string(),
                key_file: "k".to_string(),
            })
        );
    }

    /// MB-R-106 — neither cert_file nor key_file nor self_signed resolves Unset (fallback case).
    #[test]
    fn ut_server_cert_source_resolve_unset_when_neither_set() {
        assert_eq!(
            ServerCertSource::resolve(false, None, None),
            Ok(ServerCertSource::Unset)
        );
    }

    /// MB-R-107/OC-R-112 — cert_file or key_file set alone (self_signed unset) is a
    /// configuration-resolution error.
    #[test]
    fn ut_server_cert_source_resolve_alone_is_error() {
        assert!(ServerCertSource::resolve(false, Some("c".to_string()), None).is_err());
        assert!(ServerCertSource::resolve(false, None, Some("k".to_string())).is_err());
    }

    /// struct/type rework — the flattened wire shape for `Explicit` is exactly
    /// `{"cert_file":..,"key_file":..}`, no nested `self_signed` key.
    #[test]
    fn ut_server_cert_source_serde_round_trip_explicit() {
        let w = ServerCertWrapper {
            server_cert: ServerCertSource::Explicit {
                cert_file: "c".to_string(),
                key_file: "k".to_string(),
            },
        };
        let value = serde_json::to_value(&w).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"cert_file": "c", "key_file": "k"})
        );
        let back: ServerCertWrapper = serde_json::from_value(value).unwrap();
        assert_eq!(back.server_cert, w.server_cert);
    }

    /// struct/type rework — the flattened wire shape for `SelfSigned` is exactly
    /// `{"self_signed":true}`.
    #[test]
    fn ut_server_cert_source_serde_round_trip_self_signed() {
        let w = ServerCertWrapper {
            server_cert: ServerCertSource::SelfSigned,
        };
        let value = serde_json::to_value(&w).unwrap();
        assert_eq!(value, serde_json::json!({"self_signed": true}));
        let back: ServerCertWrapper = serde_json::from_value(value).unwrap();
        assert_eq!(back.server_cert, w.server_cert);
    }

    /// struct/type rework — the flattened wire shape for `Unset` is exactly `{}`.
    #[test]
    fn ut_server_cert_source_serde_round_trip_unset() {
        let w = ServerCertWrapper {
            server_cert: ServerCertSource::Unset,
        };
        let value = serde_json::to_value(&w).unwrap();
        assert_eq!(value, serde_json::json!({}));
        let back: ServerCertWrapper = serde_json::from_value(value).unwrap();
        assert_eq!(back.server_cert, w.server_cert);
    }

    /// MB-R-106 — self_signed wins even when cert_file/key_file are present in the raw JSON
    /// (structurally unreachable via Deserialize, not just via `resolve` directly).
    #[test]
    fn ut_server_cert_source_deserialize_self_signed_wins_over_present_cert_file() {
        let json = serde_json::json!({
            "self_signed": true,
            "cert_file": "c",
            "key_file": "k",
        });
        let w: ServerCertWrapper = serde_json::from_value(json).unwrap();
        assert_eq!(w.server_cert, ServerCertSource::SelfSigned);
    }

    /// MB-R-107/OC-R-112 — cert_file set alone (no self_signed, no key_file) fails to
    /// deserialize: config resolution fails at construction, not later.
    #[test]
    fn ut_server_cert_source_deserialize_alone_is_serde_error() {
        let json = serde_json::json!({"cert_file": "c"});
        let result: Result<ServerCertWrapper, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    /// MB-R-109/OC-R-036 — skip-verify wins over ca_file, not combined.
    #[test]
    fn ut_client_verification_resolve_skip_verify_wins_over_ca_file() {
        let resolved = ClientVerification::resolve(true, Some("ca".to_string()));
        assert_eq!(resolved, ClientVerification::SkipVerify);
    }

    /// MB-R-109/OC-R-036 — without skip-verify, ca_file (present or absent) is carried as-is.
    #[test]
    fn ut_client_verification_resolve_verify_carries_ca_file() {
        assert_eq!(
            ClientVerification::resolve(false, Some("ca".to_string())),
            ClientVerification::Verify {
                ca_file: Some("ca".to_string())
            }
        );
        assert_eq!(
            ClientVerification::resolve(false, None),
            ClientVerification::Verify { ca_file: None }
        );
    }

    /// struct/type rework — the flattened wire shape round-trips for both variants.
    #[test]
    fn ut_client_verification_serde_round_trip() {
        let skip = ClientVerificationWrapper {
            client_verification: ClientVerification::SkipVerify,
        };
        let value = serde_json::to_value(&skip).unwrap();
        assert_eq!(value, serde_json::json!({"insecure_skip_verify": true}));
        let back: ClientVerificationWrapper = serde_json::from_value(value).unwrap();
        assert_eq!(back.client_verification, skip.client_verification);

        let verify = ClientVerificationWrapper {
            client_verification: ClientVerification::Verify {
                ca_file: Some("ca".to_string()),
            },
        };
        let value = serde_json::to_value(&verify).unwrap();
        assert_eq!(value, serde_json::json!({"ca_file": "ca"}));
        let back: ClientVerificationWrapper = serde_json::from_value(value).unwrap();
        assert_eq!(back.client_verification, verify.client_verification);
    }
}
