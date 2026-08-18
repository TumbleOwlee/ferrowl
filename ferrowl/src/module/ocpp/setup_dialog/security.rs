//! Security-profile domain for the OCPP setup dialog: the websocket security level and the
//! skip-verify choice, plus the mapping between raw field text and an [`OcppSecurityConfig`]
//! (`from_config` infers a level, `build_config` resolves one, `validate_security` checks files).
//! Mirrors `ferrowl::module::modbus::setup_dialog::tls` exactly, plus a Basic Auth level (OCPP's
//! websocket security has a credential level; Modbus/TCP TLS does not) — see `OC-R-037`,
//! `OC-R-039`, `OC-R-040`, `OC-R-096`, `OC-R-111`, `OC-R-113`, `OC-R-115`, `OC-R-116`.

use ferrowl_ui::traits::ToLabel;
use ferrowl_util::tls::{
    ClientCertSource, ClientCertVerification, ClientTlsPolicy, ClientVerification,
    ServerCertSource, ServerTlsPolicy,
};

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

/// Client-only "accept any server certificate" toggle. **OC-R-111**: shown only at `Tls`/
/// `MutualTls` (not at every wss level as before this spec diff) — an out-of-band credential
/// check (Basic Auth) has nothing to do with certificate verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipVerifyChoice {
    Off,
    On,
}

impl ToLabel for SkipVerifyChoice {
    fn to_label(&self) -> String {
        match self {
            SkipVerifyChoice::Off => "Off",
            SkipVerifyChoice::On => "On",
        }
        .to_string()
    }
}

/// "Generate an ephemeral self-signed certificate/identity" toggle, offered whenever `Tls`/
/// `MutualTls` is selected. The *same* widget field is reused for both roles (they are never
/// shown at the same time, since a dialog instance is fixed to one role): for the server role it
/// toggles the presented server certificate's source (OC-R-110) whenever `Tls`/`MutualTls` is
/// selected; for the client role it toggles the client's own mTLS identity (OC-R-116) whenever
/// `MutualTls` is selected (the identity only exists under mTLS). Mirrors the Modbus dialog's
/// `SelfSignedChoice` byte-for-byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfSignedChoice {
    Off,
    On,
}

impl ToLabel for SelfSignedChoice {
    fn to_label(&self) -> String {
        match self {
            SelfSignedChoice::Off => "Off",
            SelfSignedChoice::On => "On",
        }
        .to_string()
    }
}

/// Raw text of every security input field, plus every toggle, passed by name so the many
/// look-alike path fields cannot be transposed at a call site. `client_ca_files` is raw
/// comma-separated text (OC-R-113), parsed by [`parse_ca_list`].
pub struct SecurityInputs<'a> {
    pub username: &'a str,
    pub password: &'a str,
    pub ca_file: &'a str,
    pub cert_file: &'a str,
    pub key_file: &'a str,
    pub client_cert_file: &'a str,
    pub client_key_file: &'a str,
    pub client_ca_files: &'a str,
    /// Server: server certificate is self-signed (OC-R-110). Client, at `MutualTls` only: the
    /// client's own mTLS identity is self-signed (OC-R-116).
    pub self_signed: bool,
    /// Client: accept any server certificate (OC-R-111).
    pub skip_verify: bool,
    /// Server, at `MutualTls` only: accept any client certificate (OC-R-113).
    pub client_cert_skip_verify: bool,
}

/// Split `raw` on commas into a list of trimmed, non-empty CA file paths (OC-R-113). Duplicated
/// from the Modbus dialog's identical helper rather than shared — the two setup dialogs are
/// independent module types with no shared UI-choices module (see this file's own doc comment).
pub(super) fn parse_ca_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

impl SecurityLevel {
    /// Infer the level an existing [`OcppSecurityConfig`] represents, by role, from that role's
    /// own nested policy (`cfg.server`/`cfg.client` — the other role's half is always present on
    /// the wire too, per `device.rs`'s doc comment, but never consulted here).
    pub fn from_config(cfg: &OcppSecurityConfig, role: OcppRole) -> SecurityLevel {
        let tls_level = match role {
            OcppRole::Client => match &cfg.client {
                ClientTlsPolicy::MutualTls { .. } => Some(SecurityLevel::MutualTls),
                ClientTlsPolicy::Tls {
                    client_verification,
                } if !matches!(
                    client_verification,
                    ClientVerification::Verify { ca_file: None }
                ) =>
                {
                    Some(SecurityLevel::Tls)
                }
                _ => None,
            },
            OcppRole::Server => match &cfg.server {
                ServerTlsPolicy::MutualTls { .. } => Some(SecurityLevel::MutualTls),
                // Deliberately not `!matches!(.., Unset)`: `SelfSigned` alone must not imply
                // `Tls` here, since `resolve()`'s below-`Tls` auto-fallback (OC-R-095) also sets
                // `server_cert: SelfSigned` -- treating that as `Tls` on a later `edit()` would
                // promote the level and make the fallback irreversible. Matches the
                // pre-OC-R-110 behavior, which ignored `self_signed` here too.
                ServerTlsPolicy::Tls {
                    server_cert: ServerCertSource::Explicit { .. },
                } => Some(SecurityLevel::Tls),
                _ => None,
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
                let server_cert = ServerCertSource::resolve(self_signed, cert_file, key_file)?;
                cfg.server = if mtls {
                    let ca_files = parse_ca_list(client_ca_files);
                    if !client_cert_skip_verify && ca_files.is_empty() {
                        return Err(
                            "Client CA list is required for mTLS (or enable Skip Verify)."
                                .to_string(),
                        );
                    }
                    let client_verification =
                        ClientCertVerification::resolve(client_cert_skip_verify, ca_files)?;
                    ServerTlsPolicy::MutualTls {
                        server_cert,
                        client_verification,
                    }
                } else {
                    ServerTlsPolicy::Tls { server_cert }
                };
            }
            OcppRole::Client => {
                let client_verification = ClientVerification::resolve(skip_verify, opt(ca_file));
                cfg.client = if mtls {
                    let client_identity = ClientCertSource::resolve(
                        self_signed,
                        opt(client_cert_file),
                        opt(client_key_file),
                    )?;
                    ClientTlsPolicy::MutualTls {
                        client_verification,
                        client_identity,
                    }
                } else {
                    ClientTlsPolicy::Tls {
                        client_verification,
                    }
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
            let server_cert = match &cfg.server {
                ServerTlsPolicy::Tls { server_cert }
                | ServerTlsPolicy::MutualTls { server_cert, .. } => server_cert,
                ServerTlsPolicy::NoTls => &ServerCertSource::Unset,
            };
            // OC-R-110: Self-Signed needs no cert/key files.
            if level >= SecurityLevel::Tls && !matches!(server_cert, ServerCertSource::SelfSigned) {
                match server_cert {
                    ServerCertSource::Explicit {
                        cert_file,
                        key_file,
                    } => {
                        exists("Certificate file", cert_file)?;
                        exists("Key file", key_file)?;
                    }
                    ServerCertSource::Unset => {
                        return Err("Certificate file is required for TLS.".to_string());
                    }
                    ServerCertSource::SelfSigned => unreachable!("excluded by the outer !matches!"),
                }
            }
            if level == SecurityLevel::MutualTls
                && let ServerTlsPolicy::MutualTls {
                    client_verification,
                    ..
                } = &cfg.server
            {
                match client_verification {
                    ClientCertVerification::Verify { ca_files } => {
                        for ca in ca_files {
                            exists("Client CA file", ca)?;
                        }
                    }
                    ClientCertVerification::SkipVerify => {}
                }
            }
        }
        OcppRole::Client => {
            let client_verification = match &cfg.client {
                ClientTlsPolicy::Tls {
                    client_verification,
                }
                | ClientTlsPolicy::MutualTls {
                    client_verification,
                    ..
                } => client_verification,
                ClientTlsPolicy::NoTls => &ClientVerification::Verify { ca_file: None },
            };
            if let ClientVerification::Verify { ca_file: Some(ca) } = client_verification
                && !ca.is_empty()
            {
                exists("CA file", ca)?;
            }
            if level == SecurityLevel::MutualTls
                && let ClientTlsPolicy::MutualTls {
                    client_identity, ..
                } = &cfg.client
                && let ClientCertSource::Explicit {
                    client_cert_file,
                    client_key_file,
                } = client_identity
            {
                exists("Client certificate file", client_cert_file)?;
                exists("Client key file", client_key_file)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    fn inputs<'a>(
        username: &'a str,
        password: &'a str,
        ca_file: &'a str,
        cert_file: &'a str,
        key_file: &'a str,
        client_cert_file: &'a str,
        client_key_file: &'a str,
        client_ca_files: &'a str,
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
            client: ClientTlsPolicy::Tls {
                client_verification: ClientVerification::Verify {
                    ca_file: Some("ca.pem".into()),
                },
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
            server: ServerTlsPolicy::Tls {
                server_cert: ServerCertSource::Explicit {
                    cert_file: "s.crt".into(),
                    key_file: "s.key".into(),
                },
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
            client: ClientTlsPolicy::MutualTls {
                client_verification: ClientVerification::default(),
                client_identity: ClientCertSource::Explicit {
                    client_cert_file: "c.crt".into(),
                    client_key_file: "c.key".into(),
                },
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
            server: ServerTlsPolicy::MutualTls {
                server_cert: ServerCertSource::SelfSigned,
                client_verification: ClientCertVerification::Verify {
                    ca_files: vec!["ca.pem".to_string()],
                },
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
                inputs("u", "p", "ca", "cert", "key", "ccert", "ckey", "cca"),
            )
            .unwrap();
        assert_eq!(cfg.username.as_deref(), Some("u"));
        assert_eq!(cfg.password.as_deref(), Some("p"));
        assert_eq!(
            cfg.server,
            ServerTlsPolicy::Tls {
                server_cert: ServerCertSource::Unset
            }
        );
    }

    #[test]
    /// OC-R-113 — a mutual-TLS server build parses the comma-separated CA list and sets a
    /// `MutualTls` policy with `Verify{ca_files}`.
    fn ut_build_config_mutual_tls_server_parses_ca_list() {
        let cfg = SecurityLevel::MutualTls
            .build_config(
                OcppRole::Server,
                inputs("", "", "", "cert", "key", "", "", "ca1.pem, ca2.pem"),
            )
            .unwrap();
        match cfg.server {
            ServerTlsPolicy::MutualTls {
                client_verification: ClientCertVerification::Verify { ca_files },
                ..
            } => assert_eq!(ca_files, vec!["ca1.pem".to_string(), "ca2.pem".to_string()]),
            other => panic!("expected MutualTls with Verify, got {other:?}"),
        }
    }

    #[test]
    /// OC-R-113 — a mutual-TLS server build with an empty CA list and skip-verify off is a
    /// validation error.
    fn ut_build_config_mutual_tls_server_empty_ca_list_and_skip_verify_off_is_validation_error() {
        let err = SecurityLevel::MutualTls
            .build_config(
                OcppRole::Server,
                inputs("", "", "", "cert", "key", "", "", ""),
            )
            .unwrap_err();
        assert!(err.contains("Client CA list is required"));
    }

    #[test]
    /// OC-R-116 — a mutual-TLS client build with the self-signed toggle on excludes the
    /// (possibly stale) client-cert/key text and resolves to `ClientCertSource::SelfSigned`.
    fn ut_build_config_mutual_tls_client_self_signed_excludes_cert_key() {
        let mut i = inputs("", "", "", "", "", "stale.crt", "stale.key", "");
        i.self_signed = true;
        let cfg = SecurityLevel::MutualTls
            .build_config(OcppRole::Client, i)
            .unwrap();
        assert_eq!(
            cfg.client,
            ClientTlsPolicy::MutualTls {
                client_verification: ClientVerification::default(),
                client_identity: ClientCertSource::SelfSigned,
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
                inputs("", "", "ca", "", "", "ccert", "ckey", ""),
            )
            .unwrap();
        assert_eq!(
            cfg.client,
            ClientTlsPolicy::MutualTls {
                client_verification: ClientVerification::Verify {
                    ca_file: Some("ca".to_string())
                },
                client_identity: ClientCertSource::Explicit {
                    client_cert_file: "ccert".to_string(),
                    client_key_file: "ckey".to_string(),
                },
            }
        );
    }

    // --- validate_security -----------------------------------------------------------------------

    #[test]
    /// OC-R-110 — a server at TLS with self_signed set needs no cert/key files.
    fn ut_validate_security_server_self_signed_needs_no_files() {
        let cfg = OcppSecurityConfig {
            server: ServerTlsPolicy::Tls {
                server_cert: ServerCertSource::SelfSigned,
            },
            ..Default::default()
        };
        assert!(validate_security(&cfg, OcppRole::Server, SecurityLevel::Tls, &|_| false).is_ok());
    }

    #[test]
    /// OC-R-113 — a server at mTLS additionally requires every listed client CA file to exist.
    fn ut_validate_security_server_mutual_tls_requires_client_ca_files() {
        let cfg = OcppSecurityConfig {
            server: ServerTlsPolicy::MutualTls {
                server_cert: ServerCertSource::SelfSigned,
                client_verification: ClientCertVerification::Verify {
                    ca_files: vec!["ca1.pem".to_string(), "ca2.pem".to_string()],
                },
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
            server: ServerTlsPolicy::MutualTls {
                server_cert: ServerCertSource::SelfSigned,
                client_verification: ClientCertVerification::SkipVerify,
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
            client: ClientTlsPolicy::Tls {
                client_verification: ClientVerification::Verify {
                    ca_file: Some("ca.pem".into()),
                },
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
            client: ClientTlsPolicy::MutualTls {
                client_verification: ClientVerification::default(),
                client_identity: ClientCertSource::Explicit {
                    client_cert_file: "c.crt".into(),
                    client_key_file: "c.key".into(),
                },
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
            client: ClientTlsPolicy::MutualTls {
                client_verification: ClientVerification::default(),
                client_identity: ClientCertSource::SelfSigned,
            },
            ..Default::default()
        };
        assert!(
            validate_security(&cfg, OcppRole::Client, SecurityLevel::MutualTls, &|_| false).is_ok()
        );
    }

    // --- parse_ca_list ---------------------------------------------------------------------------

    #[test]
    /// OC-R-113 — the CA list parses comma-separated, trims whitespace, and drops empty entries.
    fn ut_parse_ca_list_trims_and_drops_empty() {
        assert_eq!(
            parse_ca_list(" ca1.pem ,ca2.pem,, ca3.pem"),
            vec![
                "ca1.pem".to_string(),
                "ca2.pem".to_string(),
                "ca3.pem".to_string(),
            ]
        );
        assert_eq!(parse_ca_list(""), Vec::<String>::new());
        assert_eq!(parse_ca_list("   "), Vec::<String>::new());
    }
}
