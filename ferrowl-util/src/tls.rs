//! Shared TLS-configuration enums, generalizing the server-certificate-source and
//! client-verification axes that both `ferrowl-modbus`'s `ModbusTlsConfig` and `ferrowl-ocpp`'s
//! `cs::Config.tls`/`csms::Config.tls`/the `ferrowl` crate's `OcppSecurityConfig` represent on the
//! wire as `self_signed`/`cert_file`/`key_file` and `insecure_skip_verify`/`ca_file` respectively.
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

/// Where a server verifies an incoming client certificate against, under mTLS
/// (`ServerTlsPolicy::MutualTls`). MB-R-108/OC-R-039: `Verify`'s `ca_files` is checked non-empty
/// at construction (`resolve()`/`Deserialize`) — a certificate signed by any *one* configured CA
/// is sufficient, not all. `SkipVerify` still requires a presented certificate; it only skips
/// the chain/identity check against `ca_files`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientCertVerification {
    Verify { ca_files: Vec<String> },
    SkipVerify,
}

impl ClientCertVerification {
    /// MB-R-108/OC-R-039 precedence (skip-verify wins, `ca_files` ignored rather than combined,
    /// mirroring [`ClientVerification::resolve`]) + the "`Verify` needs at least one CA" check
    /// MB-R-105/108 requires enforced at construction, not later.
    pub fn resolve(skip_verify: bool, ca_files: Vec<String>) -> Result<Self, String> {
        if skip_verify {
            return Ok(ClientCertVerification::SkipVerify);
        }
        if ca_files.is_empty() {
            return Err("ca_files must be non-empty when client_cert_skip_verify is unset".into());
        }
        Ok(ClientCertVerification::Verify { ca_files })
    }
}

/// Where a client's own mTLS identity comes from, under `ClientTlsPolicy::MutualTls`. MB-R-138/
/// OC-R-115: `SelfSigned` wins unconditionally over explicit files present in the same raw
/// object, mirroring [`ServerCertSource`]'s precedence — there is no legal "unset" state (this
/// type only appears nested inside `ClientTlsPolicy::MutualTls`, which is itself only chosen
/// when there is an identity to present).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientCertSource {
    SelfSigned,
    Explicit {
        client_cert_file: String,
        client_key_file: String,
    },
}

impl ClientCertSource {
    /// MB-R-138/139/OC-R-115/116 precedence, mirroring [`ServerCertSource::resolve`]: self-signed
    /// wins unconditionally; exactly one of the file pair set (neither self-signed) is an error;
    /// neither set at all is also an error — unlike `ServerCertSource`, there is no legal "unset"
    /// variant here.
    pub fn resolve(
        self_signed: bool,
        client_cert_file: Option<String>,
        client_key_file: Option<String>,
    ) -> Result<Self, String> {
        if self_signed {
            return Ok(ClientCertSource::SelfSigned);
        }
        match (client_cert_file, client_key_file) {
            (Some(c), Some(k)) => Ok(ClientCertSource::Explicit {
                client_cert_file: c,
                client_key_file: k,
            }),
            _ => Err(
                "client_cert_file and client_key_file must both be set, or client_self_signed set"
                    .to_string(),
            ),
        }
    }
}

/// Wire shadow for [`ClientCertVerification`]'s flattened fields (`client_ca_files`/
/// `client_ca_file` (legacy singular, deserialize-only)/`client_cert_skip_verify`).
#[derive(Serialize, Deserialize, Default)]
struct RawClientCertVerification {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    client_ca_files: Vec<String>,
    /// Legacy singular form (pre-MB-R-136/OC-R-113). Deserialize-only: never emitted by
    /// `Serialize`, and ignored on read whenever `client_ca_files` is non-empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client_ca_file: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    client_cert_skip_verify: bool,
}

/// The `(ca_files, skip_verify)` wire pair for a [`ClientCertVerification`], shared by its own
/// `Serialize` impl and by [`ServerTlsPolicy`]'s `MutualTls` arm (issue #207 item 2).
fn client_cert_verification_fields(v: &ClientCertVerification) -> (Vec<String>, bool) {
    match v {
        ClientCertVerification::SkipVerify => (Vec::new(), true),
        ClientCertVerification::Verify { ca_files } => (ca_files.clone(), false),
    }
}

impl Serialize for ClientCertVerification {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (client_ca_files, client_cert_skip_verify) = client_cert_verification_fields(self);
        RawClientCertVerification {
            client_ca_files,
            client_cert_skip_verify,
            ..Default::default()
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ClientCertVerification {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawClientCertVerification::deserialize(deserializer)?;
        let ca_files = if !raw.client_ca_files.is_empty() {
            raw.client_ca_files
        } else {
            raw.client_ca_file.into_iter().collect()
        };
        ClientCertVerification::resolve(raw.client_cert_skip_verify, ca_files)
            .map_err(D::Error::custom)
    }
}

/// Wire shadow for [`ClientCertSource`]'s flattened fields (`client_cert_file`/
/// `client_key_file`/`client_self_signed`).
#[derive(Serialize, Deserialize, Default)]
struct RawClientCertSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client_cert_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client_key_file: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    client_self_signed: bool,
}

/// The `(self_signed, cert_file, key_file)` wire triple for a [`ClientCertSource`], shared by
/// its own `Serialize` impl and by [`ClientTlsPolicy`]'s `MutualTls` arm (issue #207 item 2).
fn client_cert_source_fields(source: &ClientCertSource) -> (bool, Option<String>, Option<String>) {
    match source {
        ClientCertSource::SelfSigned => (true, None, None),
        ClientCertSource::Explicit {
            client_cert_file,
            client_key_file,
        } => (
            false,
            Some(client_cert_file.clone()),
            Some(client_key_file.clone()),
        ),
    }
}

impl Serialize for ClientCertSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (client_self_signed, client_cert_file, client_key_file) =
            client_cert_source_fields(self);
        RawClientCertSource {
            client_cert_file,
            client_key_file,
            client_self_signed,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ClientCertSource {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawClientCertSource::deserialize(deserializer)?;
        ClientCertSource::resolve(
            raw.client_self_signed,
            raw.client_cert_file,
            raw.client_key_file,
        )
        .map_err(D::Error::custom)
    }
}

/// A server-role endpoint's TLS configuration (MB-R-105). `NoTls` is never produced by this
/// type's own resolution logic — it is only ever constructed by the wrapping `Option`-shaped
/// accessor on the containing wire config (`None` there means "no `tls` block at all"); a
/// `ServerTlsPolicy` nested inside a *present* TLS block always resolves to `Tls`/`MutualTls`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerTlsPolicy {
    NoTls,
    Tls {
        server_cert: ServerCertSource,
    },
    MutualTls {
        server_cert: ServerCertSource,
        client_verification: ClientCertVerification,
    },
}

/// A client-role endpoint's TLS configuration (MB-R-105). Same `NoTls` convention as
/// [`ServerTlsPolicy`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientTlsPolicy {
    NoTls,
    Tls {
        client_verification: ClientVerification,
    },
    MutualTls {
        client_verification: ClientVerification,
        client_identity: ClientCertSource,
    },
}

/// Wire shadow for [`ServerTlsPolicy`]'s flattened fields: [`ServerCertSource`]'s own three
/// (`self_signed`/`cert_file`/`key_file`) plus `require_client_cert` (the `MutualTls` trigger)
/// and [`ClientCertVerification`]'s own three. `require_client_cert` gates whether the
/// verification fields are even consulted — unlike a bare flattened `ClientCertVerification`
/// (which errors on an empty `ca_files` unconditionally), a `Tls`-level document legitimately
/// carries none of them at all.
#[derive(Serialize, Deserialize, Default)]
struct RawServerTlsPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cert_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key_file: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    self_signed: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    require_client_cert: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    client_ca_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client_ca_file: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    client_cert_skip_verify: bool,
}

impl Serialize for ServerTlsPolicy {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let raw = match self {
            // issue #207 item 1: this arm serializes to the same all-defaults wire form as
            // `Tls { server_cert: Unset }` below, and deserializes back as `Tls`, never `NoTls`
            // — see `ut_server_tls_policy_notls_serializes_as_tls_unset_asymmetry`. Harmless
            // today: `NoTls` is only ever produced by the containing wire config's
            // `Option`-shaped accessor, never nested inside a present `tls` block, so this arm
            // is unreachable in practice — but a future caller that round-trips through it
            // directly would silently get `Tls` back.
            ServerTlsPolicy::NoTls => RawServerTlsPolicy::default(),
            ServerTlsPolicy::Tls { server_cert } => {
                let (self_signed, cert_file, key_file) = server_cert_fields(server_cert);
                RawServerTlsPolicy {
                    self_signed,
                    cert_file,
                    key_file,
                    ..Default::default()
                }
            }
            ServerTlsPolicy::MutualTls {
                server_cert,
                client_verification,
            } => {
                let (self_signed, cert_file, key_file) = server_cert_fields(server_cert);
                let (client_ca_files, client_cert_skip_verify) =
                    client_cert_verification_fields(client_verification);
                RawServerTlsPolicy {
                    self_signed,
                    cert_file,
                    key_file,
                    require_client_cert: true,
                    client_ca_files,
                    client_ca_file: None,
                    client_cert_skip_verify,
                }
            }
        };
        raw.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ServerTlsPolicy {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawServerTlsPolicy::deserialize(deserializer)?;
        let server_cert = ServerCertSource::resolve(raw.self_signed, raw.cert_file, raw.key_file)
            .map_err(D::Error::custom)?;
        if !raw.require_client_cert {
            return Ok(ServerTlsPolicy::Tls { server_cert });
        }
        let ca_files = if !raw.client_ca_files.is_empty() {
            raw.client_ca_files
        } else {
            raw.client_ca_file.into_iter().collect()
        };
        let client_verification =
            ClientCertVerification::resolve(raw.client_cert_skip_verify, ca_files)
                .map_err(D::Error::custom)?;
        Ok(ServerTlsPolicy::MutualTls {
            server_cert,
            client_verification,
        })
    }
}

/// Wire shadow for [`ClientTlsPolicy`]'s flattened fields: [`ClientVerification`]'s own two
/// (`ca_file`/`insecure_skip_verify`) plus `client_self_signed`/`client_cert_file`/
/// `client_key_file` (the `MutualTls` identity, [`ClientCertSource`]'s own three) — no explicit
/// "is this mTLS" trigger field of its own; `MutualTls` is inferred from any client-identity
/// field being present, mirroring how the dialog layer already treats "client cert file set" as
/// the mTLS trigger (`TlsLevel::from_config`).
#[derive(Serialize, Deserialize, Default)]
struct RawClientTlsPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ca_file: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    insecure_skip_verify: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client_cert_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client_key_file: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    client_self_signed: bool,
}

impl Serialize for ClientTlsPolicy {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let raw = match self {
            // issue #207 item 1: same asymmetry as `ServerTlsPolicy::NoTls` — see
            // `ut_client_tls_policy_notls_serializes_as_tls_default_asymmetry`.
            ClientTlsPolicy::NoTls => RawClientTlsPolicy::default(),
            ClientTlsPolicy::Tls {
                client_verification,
            } => {
                let (ca_file, insecure_skip_verify) =
                    client_verification_fields(client_verification);
                RawClientTlsPolicy {
                    ca_file,
                    insecure_skip_verify,
                    ..Default::default()
                }
            }
            ClientTlsPolicy::MutualTls {
                client_verification,
                client_identity,
            } => {
                let (ca_file, insecure_skip_verify) =
                    client_verification_fields(client_verification);
                let (client_self_signed, client_cert_file, client_key_file) =
                    client_cert_source_fields(client_identity);
                RawClientTlsPolicy {
                    ca_file,
                    insecure_skip_verify,
                    client_self_signed,
                    client_cert_file,
                    client_key_file,
                }
            }
        };
        raw.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ClientTlsPolicy {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawClientTlsPolicy::deserialize(deserializer)?;
        let client_verification =
            ClientVerification::resolve(raw.insecure_skip_verify, raw.ca_file);
        let mtls = raw.client_self_signed
            || raw.client_cert_file.is_some()
            || raw.client_key_file.is_some();
        if !mtls {
            return Ok(ClientTlsPolicy::Tls {
                client_verification,
            });
        }
        let client_identity = ClientCertSource::resolve(
            raw.client_self_signed,
            raw.client_cert_file,
            raw.client_key_file,
        )
        .map_err(D::Error::custom)?;
        Ok(ClientTlsPolicy::MutualTls {
            client_verification,
            client_identity,
        })
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

/// The `(self_signed, cert_file, key_file)` wire triple for a [`ServerCertSource`], shared by
/// its own `Serialize` impl and by [`ServerTlsPolicy`]'s (issue #207 item 2).
fn server_cert_fields(source: &ServerCertSource) -> (bool, Option<String>, Option<String>) {
    match source {
        ServerCertSource::SelfSigned => (true, None, None),
        ServerCertSource::Explicit {
            cert_file,
            key_file,
        } => (false, Some(cert_file.clone()), Some(key_file.clone())),
        ServerCertSource::Unset => (false, None, None),
    }
}

impl Serialize for ServerCertSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (self_signed, cert_file, key_file) = server_cert_fields(self);
        RawServerCert {
            cert_file,
            key_file,
            self_signed,
        }
        .serialize(serializer)
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

/// The `(ca_file, insecure_skip_verify)` wire pair for a [`ClientVerification`], shared by its
/// own `Serialize` impl and by [`ClientTlsPolicy`]'s `Tls`/`MutualTls` arms (issue #207 item 2).
fn client_verification_fields(v: &ClientVerification) -> (Option<String>, bool) {
    match v {
        ClientVerification::SkipVerify => (None, true),
        ClientVerification::Verify { ca_file } => (ca_file.clone(), false),
    }
}

impl Serialize for ClientVerification {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (ca_file, insecure_skip_verify) = client_verification_fields(self);
        RawClientVerification {
            ca_file,
            insecure_skip_verify,
        }
        .serialize(serializer)
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

    #[derive(Serialize, Deserialize)]
    struct ServerTlsPolicyWrapper {
        #[serde(flatten)]
        server: ServerTlsPolicy,
    }

    #[derive(Serialize, Deserialize)]
    struct ClientTlsPolicyWrapper {
        #[serde(flatten)]
        client: ClientTlsPolicy,
    }

    // --- ServerTlsPolicy serde --------------------------------------------------------------

    /// MB-R-105 — a bare `{}` (no `require_client_cert`) deserializes `Tls { server_cert: Unset }`
    /// (the MB-R-106 fallback state), never `NoTls` — `NoTls` is accessor-only, unreachable via
    /// `Deserialize`.
    #[test]
    fn ut_server_tls_policy_deserialize_empty_is_tls_unset() {
        let w: ServerTlsPolicyWrapper = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(
            w.server,
            ServerTlsPolicy::Tls {
                server_cert: ServerCertSource::Unset
            }
        );
    }

    /// MB-R-105/108 — `require_client_cert: true` with a non-empty `client_ca_files` resolves
    /// `MutualTls` with `ClientCertVerification::Verify` holding exactly those files.
    #[test]
    fn ut_server_tls_policy_deserialize_mutual_tls_verify() {
        let json = serde_json::json!({
            "self_signed": true,
            "require_client_cert": true,
            "client_ca_files": ["a.pem", "b.pem"],
        });
        let w: ServerTlsPolicyWrapper = serde_json::from_value(json).unwrap();
        assert_eq!(
            w.server,
            ServerTlsPolicy::MutualTls {
                server_cert: ServerCertSource::SelfSigned,
                client_verification: ClientCertVerification::Verify {
                    ca_files: vec!["a.pem".to_string(), "b.pem".to_string()]
                }
            }
        );
    }

    /// MB-R-105/108 — `require_client_cert: true` with `client_cert_skip_verify: true` resolves
    /// `MutualTls` with `ClientCertVerification::SkipVerify`, `client_ca_files` ignored.
    #[test]
    fn ut_server_tls_policy_deserialize_mutual_tls_skip_verify() {
        let json = serde_json::json!({
            "self_signed": true,
            "require_client_cert": true,
            "client_cert_skip_verify": true,
        });
        let w: ServerTlsPolicyWrapper = serde_json::from_value(json).unwrap();
        assert_eq!(
            w.server,
            ServerTlsPolicy::MutualTls {
                server_cert: ServerCertSource::SelfSigned,
                client_verification: ClientCertVerification::SkipVerify,
            }
        );
    }

    /// MB-R-105/108 — `require_client_cert: true` with neither `client_ca_files` nor
    /// `client_cert_skip_verify` is a deserialize error (empty `ca_files`, MB-R-108).
    #[test]
    fn ut_server_tls_policy_deserialize_mutual_tls_empty_ca_files_is_error() {
        let json = serde_json::json!({"self_signed": true, "require_client_cert": true});
        let result: Result<ServerTlsPolicyWrapper, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    /// Backward compat — a legacy document (`client_ca_file` singular + `require_client_cert`)
    /// still deserializes onto `MutualTls { client_verification: Verify { ca_files: [path] }, .. }`.
    #[test]
    fn ut_server_tls_policy_deserialize_legacy_singular_client_ca_file() {
        let json = serde_json::json!({
            "cert_file": "s.crt",
            "key_file": "s.key",
            "require_client_cert": true,
            "client_ca_file": "legacy-ca.pem",
        });
        let w: ServerTlsPolicyWrapper = serde_json::from_value(json).unwrap();
        assert_eq!(
            w.server,
            ServerTlsPolicy::MutualTls {
                server_cert: ServerCertSource::Explicit {
                    cert_file: "s.crt".to_string(),
                    key_file: "s.key".to_string(),
                },
                client_verification: ClientCertVerification::Verify {
                    ca_files: vec!["legacy-ca.pem".to_string()]
                }
            }
        );
    }

    /// struct/type rework — `Tls`/`MutualTls` round-trip through their flattened wire shape.
    #[test]
    fn ut_server_tls_policy_serde_round_trip() {
        let tls = ServerTlsPolicyWrapper {
            server: ServerTlsPolicy::Tls {
                server_cert: ServerCertSource::SelfSigned,
            },
        };
        let value = serde_json::to_value(&tls).unwrap();
        assert_eq!(value, serde_json::json!({"self_signed": true}));
        let back: ServerTlsPolicyWrapper = serde_json::from_value(value).unwrap();
        assert_eq!(back.server, tls.server);

        let mtls = ServerTlsPolicyWrapper {
            server: ServerTlsPolicy::MutualTls {
                server_cert: ServerCertSource::SelfSigned,
                client_verification: ClientCertVerification::Verify {
                    ca_files: vec!["ca.pem".to_string()],
                },
            },
        };
        let value = serde_json::to_value(&mtls).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "self_signed": true,
                "require_client_cert": true,
                "client_ca_files": ["ca.pem"],
            })
        );
        let back: ServerTlsPolicyWrapper = serde_json::from_value(value).unwrap();
        assert_eq!(back.server, mtls.server);
    }

    /// issue #207 item 1 — `NoTls` serializes to the same all-defaults wire form as
    /// `Tls { server_cert: Unset }`, and round-trips back as `Tls`, never `NoTls`. Pinned so a
    /// future change to this asymmetry is deliberate, not accidental.
    #[test]
    fn ut_server_tls_policy_notls_serializes_as_tls_unset_asymmetry() {
        let no_tls = ServerTlsPolicyWrapper {
            server: ServerTlsPolicy::NoTls,
        };
        let tls_unset = ServerTlsPolicyWrapper {
            server: ServerTlsPolicy::Tls {
                server_cert: ServerCertSource::Unset,
            },
        };
        let no_tls_value = serde_json::to_value(&no_tls).unwrap();
        let tls_unset_value = serde_json::to_value(&tls_unset).unwrap();
        assert_eq!(no_tls_value, tls_unset_value);

        let back: ServerTlsPolicyWrapper = serde_json::from_value(no_tls_value).unwrap();
        assert_eq!(back.server, tls_unset.server);
    }

    // --- ClientTlsPolicy serde --------------------------------------------------------------

    /// MB-R-105 — a bare `{}` deserializes `Tls { client_verification: Verify { ca_file: None } }`
    /// (the "no TLS options set" state), never `NoTls`.
    #[test]
    fn ut_client_tls_policy_deserialize_empty_is_tls_default_verify() {
        let w: ClientTlsPolicyWrapper = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(
            w.client,
            ClientTlsPolicy::Tls {
                client_verification: ClientVerification::Verify { ca_file: None }
            }
        );
    }

    /// MB-R-105/138 — `client_self_signed: true` resolves `MutualTls` with
    /// `ClientCertSource::SelfSigned`, even with no cert/key files present.
    #[test]
    fn ut_client_tls_policy_deserialize_mutual_tls_self_signed() {
        let json = serde_json::json!({"client_self_signed": true});
        let w: ClientTlsPolicyWrapper = serde_json::from_value(json).unwrap();
        assert_eq!(
            w.client,
            ClientTlsPolicy::MutualTls {
                client_verification: ClientVerification::Verify { ca_file: None },
                client_identity: ClientCertSource::SelfSigned,
            }
        );
    }

    /// MB-R-105 — both `client_cert_file`/`client_key_file` set (no `client_self_signed`)
    /// resolves `MutualTls` with `ClientCertSource::Explicit`.
    #[test]
    fn ut_client_tls_policy_deserialize_mutual_tls_explicit() {
        let json = serde_json::json!({
            "client_cert_file": "c.pem",
            "client_key_file": "k.pem",
        });
        let w: ClientTlsPolicyWrapper = serde_json::from_value(json).unwrap();
        assert_eq!(
            w.client,
            ClientTlsPolicy::MutualTls {
                client_verification: ClientVerification::Verify { ca_file: None },
                client_identity: ClientCertSource::Explicit {
                    client_cert_file: "c.pem".to_string(),
                    client_key_file: "k.pem".to_string(),
                },
            }
        );
    }

    /// struct/type rework — `Tls`/`MutualTls` round-trip through their flattened wire shape.
    #[test]
    fn ut_client_tls_policy_serde_round_trip() {
        let tls = ClientTlsPolicyWrapper {
            client: ClientTlsPolicy::Tls {
                client_verification: ClientVerification::SkipVerify,
            },
        };
        let value = serde_json::to_value(&tls).unwrap();
        assert_eq!(value, serde_json::json!({"insecure_skip_verify": true}));
        let back: ClientTlsPolicyWrapper = serde_json::from_value(value).unwrap();
        assert_eq!(back.client, tls.client);

        let mtls = ClientTlsPolicyWrapper {
            client: ClientTlsPolicy::MutualTls {
                client_verification: ClientVerification::Verify { ca_file: None },
                client_identity: ClientCertSource::SelfSigned,
            },
        };
        let value = serde_json::to_value(&mtls).unwrap();
        assert_eq!(value, serde_json::json!({"client_self_signed": true}));
        let back: ClientTlsPolicyWrapper = serde_json::from_value(value).unwrap();
        assert_eq!(back.client, mtls.client);
    }

    /// issue #207 item 1 — same asymmetry as `ServerTlsPolicy::NoTls`: `NoTls` serializes to
    /// the same all-defaults wire form as `Tls { client_verification: Verify { ca_file: None } }`
    /// and round-trips back as `Tls`, never `NoTls`.
    #[test]
    fn ut_client_tls_policy_notls_serializes_as_tls_default_asymmetry() {
        let no_tls = ClientTlsPolicyWrapper {
            client: ClientTlsPolicy::NoTls,
        };
        let tls_default = ClientTlsPolicyWrapper {
            client: ClientTlsPolicy::Tls {
                client_verification: ClientVerification::Verify { ca_file: None },
            },
        };
        let no_tls_value = serde_json::to_value(&no_tls).unwrap();
        let tls_default_value = serde_json::to_value(&tls_default).unwrap();
        assert_eq!(no_tls_value, tls_default_value);

        let back: ClientTlsPolicyWrapper = serde_json::from_value(no_tls_value).unwrap();
        assert_eq!(back.client, tls_default.client);
    }

    #[derive(Serialize, Deserialize)]
    struct ClientCertVerificationWrapper {
        #[serde(flatten)]
        client_cert_verification: ClientCertVerification,
    }

    #[derive(Serialize, Deserialize)]
    struct ClientCertSourceWrapper {
        #[serde(flatten)]
        client_cert_source: ClientCertSource,
    }

    // --- ClientCertVerification::resolve --------------------------------------------------

    /// MB-R-108/OC-R-039 — `skip_verify` wins over `ca_files`, not combined.
    #[test]
    fn ut_client_cert_verification_resolve_skip_verify_wins_over_ca_files() {
        let resolved = ClientCertVerification::resolve(true, vec!["ca.pem".to_string()]);
        assert_eq!(resolved, Ok(ClientCertVerification::SkipVerify));
    }

    /// MB-R-105/108/OC-R-039 — without skip-verify, a non-empty `ca_files` list resolves
    /// `Verify` carrying exactly those files.
    #[test]
    fn ut_client_cert_verification_resolve_verify_carries_ca_files() {
        let resolved =
            ClientCertVerification::resolve(false, vec!["a.pem".to_string(), "b.pem".to_string()]);
        assert_eq!(
            resolved,
            Ok(ClientCertVerification::Verify {
                ca_files: vec!["a.pem".to_string(), "b.pem".to_string()]
            })
        );
    }

    /// MB-R-105/108/OC-R-039 — an empty `ca_files` list without skip-verify is a
    /// construction-time error, not a silent fallback.
    #[test]
    fn ut_client_cert_verification_resolve_empty_ca_files_without_skip_verify_is_error() {
        assert!(ClientCertVerification::resolve(false, vec![]).is_err());
    }

    // --- ClientCertVerification serde -----------------------------------------------------

    /// struct/type rework — the flattened wire shape round-trips for both variants; `Verify`
    /// serializes as `client_ca_files` (plural, MB-R-136/OC-R-113), not the legacy singular.
    #[test]
    fn ut_client_cert_verification_serde_round_trip() {
        let skip = ClientCertVerificationWrapper {
            client_cert_verification: ClientCertVerification::SkipVerify,
        };
        let value = serde_json::to_value(&skip).unwrap();
        assert_eq!(value, serde_json::json!({"client_cert_skip_verify": true}));
        let back: ClientCertVerificationWrapper = serde_json::from_value(value).unwrap();
        assert_eq!(back.client_cert_verification, skip.client_cert_verification);

        let verify = ClientCertVerificationWrapper {
            client_cert_verification: ClientCertVerification::Verify {
                ca_files: vec!["ca.pem".to_string()],
            },
        };
        let value = serde_json::to_value(&verify).unwrap();
        assert_eq!(value, serde_json::json!({"client_ca_files": ["ca.pem"]}));
        let back: ClientCertVerificationWrapper = serde_json::from_value(value).unwrap();
        assert_eq!(
            back.client_cert_verification,
            verify.client_cert_verification
        );
    }

    /// Backward compat — a legacy config with the old singular `client_ca_file` string (plus
    /// `require_client_cert: true`, folded in by the caller) still deserializes, mapping onto
    /// `Verify { ca_files: ["path"] }` when the new plural `client_ca_files` key is absent.
    #[test]
    fn ut_client_cert_verification_deserialize_legacy_singular_ca_file() {
        let json = serde_json::json!({"client_ca_file": "legacy.pem"});
        let w: ClientCertVerificationWrapper = serde_json::from_value(json).unwrap();
        assert_eq!(
            w.client_cert_verification,
            ClientCertVerification::Verify {
                ca_files: vec!["legacy.pem".to_string()]
            }
        );
    }

    /// Backward compat — when both the legacy singular and the new plural key are present, the
    /// new plural key wins (it is the current wire shape; the singular is deserialize-only).
    #[test]
    fn ut_client_cert_verification_deserialize_plural_wins_over_legacy_singular() {
        let json = serde_json::json!({
            "client_ca_file": "legacy.pem",
            "client_ca_files": ["new.pem"],
        });
        let w: ClientCertVerificationWrapper = serde_json::from_value(json).unwrap();
        assert_eq!(
            w.client_cert_verification,
            ClientCertVerification::Verify {
                ca_files: vec!["new.pem".to_string()]
            }
        );
    }

    /// MB-R-105/108/OC-R-039 — an absent `ca_files`/`ca_file` with skip-verify unset fails to
    /// deserialize: the empty-`ca_files` construction error is enforced through `Deserialize`
    /// too, not just via `resolve` directly.
    #[test]
    fn ut_client_cert_verification_deserialize_empty_is_error() {
        let json = serde_json::json!({});
        let result: Result<ClientCertVerificationWrapper, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    // --- ClientCertSource -------------------------------------------------------------------

    /// MB-R-138/OC-R-115 — `SelfSigned` wins unconditionally over explicit client cert/key
    /// files present in the same raw object, mirroring `ServerCertSource`'s precedence.
    #[test]
    fn ut_client_cert_source_deserialize_self_signed_wins_over_explicit_files() {
        let json = serde_json::json!({
            "client_self_signed": true,
            "client_cert_file": "c.pem",
            "client_key_file": "k.pem",
        });
        let w: ClientCertSourceWrapper = serde_json::from_value(json).unwrap();
        assert_eq!(w.client_cert_source, ClientCertSource::SelfSigned);
    }

    /// MB-R-105/139/OC-R-116 — `client_cert_file`/`client_key_file` set alone (no
    /// `client_self_signed`), while the other of the pair is absent, is a deserialize error.
    #[test]
    fn ut_client_cert_source_deserialize_one_file_alone_is_error() {
        let json = serde_json::json!({"client_cert_file": "c.pem"});
        let result: Result<ClientCertSourceWrapper, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    /// MB-R-105/139/OC-R-116 — neither `client_self_signed` nor a cert/key pair set is a
    /// deserialize error (unlike `ServerCertSource`, `ClientCertSource` has no legal "unset"
    /// state of its own — it only ever appears nested inside `ClientTlsPolicy::MutualTls`).
    #[test]
    fn ut_client_cert_source_deserialize_neither_set_is_error() {
        let json = serde_json::json!({});
        let result: Result<ClientCertSourceWrapper, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    /// struct/type rework — the flattened wire shape round-trips for both variants.
    #[test]
    fn ut_client_cert_source_serde_round_trip() {
        let self_signed = ClientCertSourceWrapper {
            client_cert_source: ClientCertSource::SelfSigned,
        };
        let value = serde_json::to_value(&self_signed).unwrap();
        assert_eq!(value, serde_json::json!({"client_self_signed": true}));
        let back: ClientCertSourceWrapper = serde_json::from_value(value).unwrap();
        assert_eq!(back.client_cert_source, self_signed.client_cert_source);

        let explicit = ClientCertSourceWrapper {
            client_cert_source: ClientCertSource::Explicit {
                client_cert_file: "c.pem".to_string(),
                client_key_file: "k.pem".to_string(),
            },
        };
        let value = serde_json::to_value(&explicit).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"client_cert_file": "c.pem", "client_key_file": "k.pem"})
        );
        let back: ClientCertSourceWrapper = serde_json::from_value(value).unwrap();
        assert_eq!(back.client_cert_source, explicit.client_cert_source);
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
