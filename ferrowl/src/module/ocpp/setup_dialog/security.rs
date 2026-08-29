//! Security-profile domain for the OCPP setup dialog: the websocket security level and the
//! skip-verify choice, plus the mapping between raw field text and an [`OcppSecurityConfig`]
//! (`from_config` infers a level, `build_config` resolves one, `validate_security` checks files).
//! Mirrors `ferrowl::module::modbus::setup_dialog::tls` exactly, plus a Basic Auth level (OCPP's
//! websocket security has a credential level; Modbus/TCP TLS does not) — see `OC-R-037`,
//! `OC-R-039`, `OC-R-040`, `OC-R-096`, `OC-R-111`, `OC-R-113`, `OC-R-115`, `OC-R-116`.

use ferrowl_ui::traits::ToLabel;
use ferrowl_util::tls::{CertSource, CertVerification, ClientTlsPolicy, ServerTlsPolicy};

use crate::dialog::tls_section::EffectiveTlsLevel;
use crate::module::ocpp::config::device::OcppSecurityConfig;
use crate::module::ocpp::config::session::OcppRole;

/// Websocket transport security level, offered only when the protocol is `wss://`. Cumulative:
/// each level's fields are a superset of the one below it (`BasicAuth` fields are also shown, and
/// still apply, at `Tls` and `MutualTls`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SecurityLevel {
    None,
    BasicAuth,
    Tls,
    MutualTls,
}

impl ToLabel for SecurityLevel {
    fn to_label(&self) -> String {
        match self {
            SecurityLevel::None => "None",
            SecurityLevel::BasicAuth => "Basic Auth",
            SecurityLevel::Tls => "TLS",
            SecurityLevel::MutualTls => "mTLS",
        }
        .to_string()
    }
}

/// `None`/`BasicAuth` both collapse to `Off` — neither carries a transport-security guarantee,
/// and the shared widget's own gates never distinguish a credential-only connection from an
/// unauthenticated one (that distinction, `show_credentials`, stays on `OcppSetupDialog` itself).
impl From<SecurityLevel> for EffectiveTlsLevel {
    fn from(level: SecurityLevel) -> Self {
        match level {
            SecurityLevel::None | SecurityLevel::BasicAuth => EffectiveTlsLevel::Off,
            SecurityLevel::Tls => EffectiveTlsLevel::Tls,
            SecurityLevel::MutualTls => EffectiveTlsLevel::MutualTls,
        }
    }
}

/// Raw text of every security input field, plus every toggle, passed by name so the many
/// look-alike path fields cannot be transposed at a call site. `client_ca_files` is the
/// add/remove/edit list widget's current entries (OC-R-113) — already individual paths, no
/// further parsing.
pub struct SecurityInputs<'a> {
    pub username: &'a str,
    pub password: &'a str,
    pub ca_file: &'a str,
    pub cert_file: &'a str,
    pub key_file: &'a str,
    pub client_cert_file: &'a str,
    pub client_key_file: &'a str,
    pub client_ca_files: &'a [String],
    /// Server: server certificate is self-signed (OC-R-110). Client, at `MutualTls` only: the
    /// client's own mTLS identity is self-signed (OC-R-116).
    pub self_signed: bool,
    /// Client: accept any server certificate (OC-R-111).
    pub skip_verify: bool,
    /// Server, at `MutualTls` only: accept any client certificate (OC-R-113).
    pub client_cert_skip_verify: bool,
}

impl SecurityLevel {
    /// Infer the level an existing [`OcppSecurityConfig`] represents, by role, from that role's
    /// own nested policy (`cfg.tls.server`/`cfg.tls.client` — the other role's half is always
    /// present on the wire too, per `device.rs`'s doc comment, but never consulted here).
    pub fn from_config(cfg: &OcppSecurityConfig, role: OcppRole) -> SecurityLevel {
        // OC-R-126: the level is decided by matching the role's own policy variant directly,
        // never by comparing against an all-unset field-presence baseline -- the variant *is*
        // the state (api-contract.md §9.1). `Ephemeral` (OC-R-095's "nothing configured, fall
        // back and log" identity) still resolves to `Tls` here, same as `SelfSigned`/`Files`:
        // it is distinguished from `None {}` by the outer `mode`, not by which identity a `Tls`
        // policy carries.
        let tls_level = match role {
            OcppRole::Client => match &cfg.tls.client {
                ClientTlsPolicy::Mutual { .. } => Some(SecurityLevel::MutualTls),
                ClientTlsPolicy::Tls { .. } => Some(SecurityLevel::Tls),
                ClientTlsPolicy::None {} => None,
            },
            OcppRole::Server => match &cfg.tls.server {
                ServerTlsPolicy::Mutual { .. } => Some(SecurityLevel::MutualTls),
                ServerTlsPolicy::Tls { .. } => Some(SecurityLevel::Tls),
                ServerTlsPolicy::None {} => None,
            },
        };
        tls_level.unwrap_or(if cfg.username.is_some() {
            SecurityLevel::BasicAuth
        } else {
            SecurityLevel::None
        })
    }

    /// Build the active role's resolved policy from raw field text and toggle state (OC-R-111/
    /// OC-R-113/OC-R-116), producing a full [`OcppSecurityConfig`] whose *inactive* role's half
    /// is left at [`OcppSecurityConfig::default`]'s placeholder — the caller
    /// (`OcppSetupDialog::resolve`) overwrites that half from the original config, if any, so a
    /// role toggle preserves the other role's previously-saved settings. Basic Auth fields
    /// (`username`/`password`) are role-independent and always set at `BasicAuth` level or above.
    pub fn build_config(
        self,
        role: OcppRole,
        inputs: SecurityInputs<'_>,
    ) -> Result<OcppSecurityConfig, String> {
        let SecurityInputs {
            username,
            password,
            ca_file,
            cert_file,
            key_file,
            client_cert_file,
            client_key_file,
            client_ca_files,
            self_signed,
            skip_verify,
            client_cert_skip_verify,
        } = inputs;
        let opt = |s: &str| {
            let t = s.trim();
            (!t.is_empty()).then(|| t.to_string())
        };
        let basic = self >= SecurityLevel::BasicAuth;
        let tls = self >= SecurityLevel::Tls;
        let mtls = self == SecurityLevel::MutualTls;

        let mut cfg = OcppSecurityConfig {
            username: if basic { opt(username) } else { None },
            password: if basic { opt(password) } else { None },
            ..OcppSecurityConfig::default()
        };

        match role {
            OcppRole::Server => {
                // Below `Tls`, `cert_file`/`key_file` are not a visible field for this level (a
                // `BasicAuth`-only server still resolves *some* `ServerTlsPolicy`, per
                // `OcppSecurityConfig`'s always-present-on-the-wire shape, but never from stale
                // text a lower level never showed).
                let (cert_file, key_file) = if tls {
                    (opt(cert_file), opt(key_file))
                } else {
                    (None, None)
                };
                let identity = if self_signed {
                    CertSource::SelfSigned {}
                } else {
                    match (cert_file, key_file) {
                        (Some(c), Some(k)) => CertSource::Files {
                            cert_file: c,
                            key_file: k,
                        },
                        (None, None) => CertSource::Ephemeral {},
                        _ => {
                            return Err(
                                "cert_file and key_file must both be set, or neither".to_string()
                            );
                        }
                    }
                };
                cfg.tls.server = if mtls {
                    let ca_files = client_ca_files.to_vec();
                    if !client_cert_skip_verify && ca_files.is_empty() {
                        return Err(
                            "Client CA list is required for mTLS (or enable Skip Verify)."
                                .to_string(),
                        );
                    }
                    let verification = if client_cert_skip_verify {
                        CertVerification::Skip {}
                    } else {
                        CertVerification::CaFiles { ca_files }
                    };
                    ServerTlsPolicy::Mutual {
                        identity,
                        verification,
                    }
                } else {
                    ServerTlsPolicy::Tls { identity }
                };
            }
            OcppRole::Client => {
                let verification = if skip_verify {
                    CertVerification::Skip {}
                } else {
                    CertVerification::RootStore {
                        extra_ca_files: opt(ca_file).into_iter().collect(),
                    }
                };
                cfg.tls.client = if mtls {
                    let identity = if self_signed {
                        CertSource::SelfSigned {}
                    } else {
                        match (opt(client_cert_file), opt(client_key_file)) {
                            (Some(c), Some(k)) => CertSource::Files {
                                cert_file: c,
                                key_file: k,
                            },
                            _ => {
                                return Err(
                                    "client_cert_file and client_key_file must both be set, or self-signed set"
                                        .to_string(),
                                );
                            }
                        }
                    };
                    ClientTlsPolicy::Mutual {
                        verification,
                        identity,
                    }
                } else {
                    ClientTlsPolicy::Tls { verification }
                };
            }
        }
        Ok(cfg)
    }

    /// Index into the `security` selection's value list (declaration order above).
    pub(super) fn index(self) -> usize {
        match self {
            SecurityLevel::None => 0,
            SecurityLevel::BasicAuth => 1,
            SecurityLevel::Tls => 2,
            SecurityLevel::MutualTls => 3,
        }
    }
}

/// Check every required credential/certificate file is present and exists on disk. `level` has
/// already been checked `>= Tls` for the server role by the caller. `cfg` is assumed
/// already-resolved (the "CA list required" and "cert/key required unless self-signed" *shape*
/// errors are raised by [`SecurityLevel::build_config`] itself, since only it has the raw toggle
/// state needed to phrase them helpfully) — this only adds the file-existence check on top.
pub(super) fn validate_security(
    cfg: &OcppSecurityConfig,
    role: OcppRole,
    level: SecurityLevel,
    file_exists: &dyn Fn(&str) -> bool,
) -> Result<(), String> {
    let exists = |label: &str, path: &str| -> Result<(), String> {
        if !file_exists(path) {
            return Err(format!("{label} not found: {path}"));
        }
        Ok(())
    };

    match role {
        OcppRole::Server => {
            let identity = match &cfg.tls.server {
                ServerTlsPolicy::Tls { identity } | ServerTlsPolicy::Mutual { identity, .. } => {
                    identity
                }
                ServerTlsPolicy::None {} => &CertSource::Ephemeral {},
            };
            // OC-R-110: Self-Signed needs no cert/key files.
            if level >= SecurityLevel::Tls && !matches!(identity, CertSource::SelfSigned {}) {
                match identity {
                    CertSource::Files {
                        cert_file,
                        key_file,
                    } => {
                        exists("Certificate file", cert_file)?;
                        exists("Key file", key_file)?;
                    }
                    CertSource::Ephemeral {} => {
                        return Err("Certificate file is required for TLS.".to_string());
                    }
                    CertSource::SelfSigned {} => unreachable!("excluded by the outer !matches!"),
                }
            }
            if level == SecurityLevel::MutualTls
                && let ServerTlsPolicy::Mutual { verification, .. } = &cfg.tls.server
            {
                match verification {
                    CertVerification::CaFiles { ca_files } => {
                        for ca in ca_files {
                            exists("Client CA file", ca)?;
                        }
                    }
                    CertVerification::Skip {} | CertVerification::RootStore { .. } => {}
                }
            }
        }
        OcppRole::Client => {
            let verification = match &cfg.tls.client {
                ClientTlsPolicy::Tls { verification }
                | ClientTlsPolicy::Mutual { verification, .. } => verification,
                ClientTlsPolicy::None {} => &CertVerification::RootStore {
                    extra_ca_files: vec![],
                },
            };
            let ca_paths: &[String] = match verification {
                CertVerification::RootStore { extra_ca_files } => extra_ca_files,
                CertVerification::CaFiles { ca_files } => ca_files,
                CertVerification::Skip {} => &[],
            };
            for ca in ca_paths {
                if !ca.is_empty() {
                    exists("CA file", ca)?;
                }
            }
            if level == SecurityLevel::MutualTls
                && let ClientTlsPolicy::Mutual { identity, .. } = &cfg.tls.client
                && let CertSource::Files {
                    cert_file,
                    key_file,
                } = identity
            {
                exists("Client certificate file", cert_file)?;
                exists("Client key file", key_file)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::ocpp::config::device::OcppTlsConfig;

    #[test]
    /// UI-R-049 — `SecurityLevel::BasicAuth` collapses to `EffectiveTlsLevel::Off` alongside
    /// `None`: every predicate the shared widget owns only ever tests `>= Tls`/`== MutualTls`,
    /// never distinguishing `None` from `BasicAuth`.
    fn ut_effective_tls_level_from_security_level() {
        assert_eq!(
            EffectiveTlsLevel::from(SecurityLevel::None),
            EffectiveTlsLevel::Off
        );
        assert_eq!(
            EffectiveTlsLevel::from(SecurityLevel::BasicAuth),
            EffectiveTlsLevel::Off
        );
        assert_eq!(
            EffectiveTlsLevel::from(SecurityLevel::Tls),
            EffectiveTlsLevel::Tls
        );
        assert_eq!(
            EffectiveTlsLevel::from(SecurityLevel::MutualTls),
            EffectiveTlsLevel::MutualTls
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn inputs<'a>(
        username: &'a str,
        password: &'a str,
        ca_file: &'a str,
        cert_file: &'a str,
        key_file: &'a str,
        client_cert_file: &'a str,
        client_key_file: &'a str,
        client_ca_files: &'a [String],
    ) -> SecurityInputs<'a> {
        SecurityInputs {
            username,
            password,
            ca_file,
            cert_file,
            key_file,
            client_cert_file,
            client_key_file,
            client_ca_files,
            self_signed: false,
            skip_verify: false,
            client_cert_skip_verify: false,
        }
    }

    // --- SecurityLevel::from_config -----------------------------------------------------------

    #[test]
    /// UI-R-024 — the security fields load from a no-security config for both roles.
    fn ut_from_config_none_both_roles() {
        let cfg = OcppSecurityConfig::default();
        assert_eq!(
            SecurityLevel::from_config(&cfg, OcppRole::Client),
            SecurityLevel::None
        );
        assert_eq!(
            SecurityLevel::from_config(&cfg, OcppRole::Server),
            SecurityLevel::None
        );
    }

    #[test]
    /// UI-R-024 — the security fields load from a basic-auth config for both roles.
    fn ut_from_config_basic_auth_both_roles() {
        let cfg = OcppSecurityConfig {
            username: Some("u".into()),
            password: Some("p".into()),
            ..Default::default()
        };
        assert_eq!(
            SecurityLevel::from_config(&cfg, OcppRole::Client),
            SecurityLevel::BasicAuth
        );
        assert_eq!(
            SecurityLevel::from_config(&cfg, OcppRole::Server),
            SecurityLevel::BasicAuth
        );
    }

    #[test]
    /// UI-R-024 — a client TLS config loads into the CA-file field.
    fn ut_from_config_tls_client_is_ca_file() {
        let cfg = OcppSecurityConfig {
            tls: OcppTlsConfig {
                client: ClientTlsPolicy::Tls {
                    verification: CertVerification::RootStore {
                        extra_ca_files: vec!["ca.pem".into()],
                    },
                },
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            SecurityLevel::from_config(&cfg, OcppRole::Client),
            SecurityLevel::Tls
        );
    }

    #[test]
    /// UI-R-024 — a server TLS config loads into the cert and key fields.
    fn ut_from_config_tls_server_is_cert_and_key() {
        let cfg = OcppSecurityConfig {
            tls: OcppTlsConfig {
                server: ServerTlsPolicy::Tls {
                    identity: CertSource::Files {
                        cert_file: "s.crt".into(),
                        key_file: "s.key".into(),
                    },
                },
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            SecurityLevel::from_config(&cfg, OcppRole::Server),
            SecurityLevel::Tls
        );
    }

    #[test]
    /// OC-R-126 — the level is decided by the policy variant alone, never by comparing against
    /// an all-unset field-presence baseline: a client `Tls` policy whose `RootStore` carries an
    /// empty `extra_ca_files` (the widget's own default state) is still `Tls`, not `Off`.
    fn ut_from_config_tls_client_with_empty_root_store_is_still_tls() {
        let cfg = OcppSecurityConfig {
            tls: OcppTlsConfig {
                client: ClientTlsPolicy::Tls {
                    verification: CertVerification::RootStore {
                        extra_ca_files: vec![],
                    },
                },
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            SecurityLevel::from_config(&cfg, OcppRole::Client),
            SecurityLevel::Tls
        );
    }

    #[test]
    /// OC-R-126 — a server `Tls` policy with a `SelfSigned` identity is still `Tls`, never
    /// falling through to `None`/`BasicAuth`: the variant is the state, distinguishable from the
    /// OC-R-095 `Ephemeral` fallback by construction rather than by a "was anything configured"
    /// comparison.
    fn ut_from_config_tls_server_self_signed_is_still_tls() {
        let cfg = OcppSecurityConfig {
            tls: OcppTlsConfig {
                server: ServerTlsPolicy::Tls {
                    identity: CertSource::SelfSigned {},
                },
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            SecurityLevel::from_config(&cfg, OcppRole::Server),
            SecurityLevel::Tls
        );
    }

    #[test]
    /// OC-R-126 — the OC-R-095 fallback state (`Ephemeral`) is distinguished from a real `Off`
    /// only by whether the policy is `None {}` at all, not by which identity variant it carries;
    /// `Ephemeral` itself is unreachable from the dialog (only `SelfSigned`/`Files` are offered),
    /// but a hand-edited config carrying it still reports `Tls`, matching OC-R-096's fallback.
    fn ut_from_config_tls_server_ephemeral_is_still_tls() {
        let cfg = OcppSecurityConfig {
            tls: OcppTlsConfig {
                server: ServerTlsPolicy::Tls {
                    identity: CertSource::Ephemeral {},
                },
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            SecurityLevel::from_config(&cfg, OcppRole::Server),
            SecurityLevel::Tls
        );
    }

    #[test]
    /// UI-R-024/OC-R-116 — a mutual-TLS client config loads into the client-cert fields.
    fn ut_from_config_mutual_tls_client_is_client_cert() {
        let cfg = OcppSecurityConfig {
            tls: OcppTlsConfig {
                client: ClientTlsPolicy::Mutual {
                    verification: CertVerification::RootStore {
                        extra_ca_files: vec![],
                    },
                    identity: CertSource::Files {
                        cert_file: "c.crt".into(),
                        key_file: "c.key".into(),
                    },
                },
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            SecurityLevel::from_config(&cfg, OcppRole::Client),
            SecurityLevel::MutualTls
        );
    }

    #[test]
    /// UI-R-024/OC-R-113 — a mutual-TLS server config loads at the MutualTls level.
    fn ut_from_config_mutual_tls_server() {
        let cfg = OcppSecurityConfig {
            tls: OcppTlsConfig {
                server: ServerTlsPolicy::Mutual {
                    identity: CertSource::SelfSigned {},
                    verification: CertVerification::CaFiles {
                        ca_files: vec!["ca.pem".to_string()],
                    },
                },
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            SecurityLevel::from_config(&cfg, OcppRole::Server),
            SecurityLevel::MutualTls
        );
    }

    // --- SecurityLevel::build_config -----------------------------------------------------------

    #[test]
    /// UI-R-024 — building the config drops fields not visible at the chosen security level.
    fn ut_build_config_drops_fields_not_visible_at_level() {
        let cfg = SecurityLevel::BasicAuth
            .build_config(
                OcppRole::Server,
                inputs(
                    "u",
                    "p",
                    "ca",
                    "cert",
                    "key",
                    "ccert",
                    "ckey",
                    &["cca".to_string()],
                ),
            )
            .unwrap();
        assert_eq!(cfg.username.as_deref(), Some("u"));
        assert_eq!(cfg.password.as_deref(), Some("p"));
        assert_eq!(
            cfg.tls.server,
            ServerTlsPolicy::Tls {
                identity: CertSource::Ephemeral {}
            }
        );
    }

    #[test]
    /// OC-R-113 — a mutual-TLS server build parses the comma-separated CA list and sets a
    /// `Mutual` policy with `CaFiles{ca_files}`.
    fn ut_build_config_mutual_tls_server_parses_ca_list() {
        let cfg = SecurityLevel::MutualTls
            .build_config(
                OcppRole::Server,
                inputs(
                    "",
                    "",
                    "",
                    "cert",
                    "key",
                    "",
                    "",
                    &["ca1.pem".to_string(), "ca2.pem".to_string()],
                ),
            )
            .unwrap();
        match cfg.tls.server {
            ServerTlsPolicy::Mutual {
                verification: CertVerification::CaFiles { ca_files },
                ..
            } => assert_eq!(ca_files, vec!["ca1.pem".to_string(), "ca2.pem".to_string()]),
            other => panic!("expected Mutual with CaFiles, got {other:?}"),
        }
    }

    #[test]
    /// OC-R-113 — a mutual-TLS server build with an empty CA list and skip-verify off is a
    /// validation error.
    fn ut_build_config_mutual_tls_server_empty_ca_list_and_skip_verify_off_is_validation_error() {
        let err = SecurityLevel::MutualTls
            .build_config(
                OcppRole::Server,
                inputs("", "", "", "cert", "key", "", "", &[]),
            )
            .unwrap_err();
        assert!(err.contains("Client CA list is required"));
    }

    #[test]
    /// OC-R-116 — a mutual-TLS client build with the self-signed toggle on excludes the
    /// (possibly stale) client-cert/key text and resolves to `CertSource::SelfSigned`.
    fn ut_build_config_mutual_tls_client_self_signed_excludes_cert_key() {
        let mut i = inputs("", "", "", "", "", "stale.crt", "stale.key", &[]);
        i.self_signed = true;
        let cfg = SecurityLevel::MutualTls
            .build_config(OcppRole::Client, i)
            .unwrap();
        assert_eq!(
            cfg.tls.client,
            ClientTlsPolicy::Mutual {
                verification: CertVerification::RootStore {
                    extra_ca_files: vec![],
                },
                identity: CertSource::SelfSigned {},
            }
        );
    }

    #[test]
    /// UI-R-024 — a mutual-TLS client build keeps the client cert/key (and any trust-anchor CA
    /// file text set alongside it — the two are independent axes, both legal under mTLS).
    fn ut_build_config_mutual_tls_client_keeps_client_cert_key() {
        let cfg = SecurityLevel::MutualTls
            .build_config(
                OcppRole::Client,
                inputs("", "", "ca", "", "", "ccert", "ckey", &[]),
            )
            .unwrap();
        assert_eq!(
            cfg.tls.client,
            ClientTlsPolicy::Mutual {
                verification: CertVerification::RootStore {
                    extra_ca_files: vec!["ca".to_string()],
                },
                identity: CertSource::Files {
                    cert_file: "ccert".to_string(),
                    key_file: "ckey".to_string(),
                },
            }
        );
    }

    // --- validate_security -----------------------------------------------------------------------

    #[test]
    /// OC-R-110 — a server at TLS with self_signed set needs no cert/key files.
    fn ut_validate_security_server_self_signed_needs_no_files() {
        let cfg = OcppSecurityConfig {
            tls: OcppTlsConfig {
                server: ServerTlsPolicy::Tls {
                    identity: CertSource::SelfSigned {},
                },
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(validate_security(&cfg, OcppRole::Server, SecurityLevel::Tls, &|_| false).is_ok());
    }

    #[test]
    /// OC-R-113 — a server at mTLS additionally requires every listed client CA file to exist.
    fn ut_validate_security_server_mutual_tls_requires_client_ca_files() {
        let cfg = OcppSecurityConfig {
            tls: OcppTlsConfig {
                server: ServerTlsPolicy::Mutual {
                    identity: CertSource::SelfSigned {},
                    verification: CertVerification::CaFiles {
                        ca_files: vec!["ca1.pem".to_string(), "ca2.pem".to_string()],
                    },
                },
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(
            validate_security(&cfg, OcppRole::Server, SecurityLevel::MutualTls, &|_| true).is_ok()
        );
        assert!(
            validate_security(&cfg, OcppRole::Server, SecurityLevel::MutualTls, &|_| false)
                .is_err()
        );
    }

    #[test]
    /// OC-R-113 — a server at mTLS with skip-verify on needs no CA files checked.
    fn ut_validate_security_server_mutual_tls_skip_verify_on_needs_no_ca_files() {
        let cfg = OcppSecurityConfig {
            tls: OcppTlsConfig {
                server: ServerTlsPolicy::Mutual {
                    identity: CertSource::SelfSigned {},
                    verification: CertVerification::Skip {},
                },
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(
            validate_security(&cfg, OcppRole::Server, SecurityLevel::MutualTls, &|_| false).is_ok()
        );
    }

    #[test]
    /// UI-R-024 — a client's CA file, when set, must exist; skip-verify alone needs no file.
    fn ut_validate_security_client_ca_file_must_exist_when_set() {
        let cfg = OcppSecurityConfig {
            tls: OcppTlsConfig {
                client: ClientTlsPolicy::Tls {
                    verification: CertVerification::RootStore {
                        extra_ca_files: vec!["ca.pem".into()],
                    },
                },
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(validate_security(&cfg, OcppRole::Client, SecurityLevel::Tls, &|_| true).is_ok());
        assert!(validate_security(&cfg, OcppRole::Client, SecurityLevel::Tls, &|_| false).is_err());
    }

    #[test]
    /// UI-R-024 — a client at mTLS requires existing client cert and key files.
    fn ut_validate_security_client_mutual_tls_requires_client_cert_key_files() {
        let cfg = OcppSecurityConfig {
            tls: OcppTlsConfig {
                client: ClientTlsPolicy::Mutual {
                    verification: CertVerification::RootStore {
                        extra_ca_files: vec![],
                    },
                    identity: CertSource::Files {
                        cert_file: "c.crt".into(),
                        key_file: "c.key".into(),
                    },
                },
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(
            validate_security(&cfg, OcppRole::Client, SecurityLevel::MutualTls, &|_| true).is_ok()
        );
        assert!(
            validate_security(&cfg, OcppRole::Client, SecurityLevel::MutualTls, &|_| false)
                .is_err()
        );
    }

    #[test]
    /// OC-R-116 — a client at mTLS with self-signed set needs no cert/key files checked.
    fn ut_validate_security_client_self_signed_needs_no_files() {
        let cfg = OcppSecurityConfig {
            tls: OcppTlsConfig {
                client: ClientTlsPolicy::Mutual {
                    verification: CertVerification::RootStore {
                        extra_ca_files: vec![],
                    },
                    identity: CertSource::SelfSigned {},
                },
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(
            validate_security(&cfg, OcppRole::Client, SecurityLevel::MutualTls, &|_| false).is_ok()
        );
    }
}
