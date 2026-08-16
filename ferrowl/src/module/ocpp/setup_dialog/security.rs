//! Security-profile domain for the OCPP setup dialog: the websocket security level and the
//! skip-verify choice, plus the mapping between raw field text and an [`OcppSecurityConfig`]
//! (`from_config` infers a level, `build_config` resolves one, `validate_security` checks files).

use ferrowl_ui::traits::ToLabel;
use ferrowl_util::tls::{ClientVerification, ServerCertSource};

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

/// Client-only "accept any server certificate" toggle, offered whenever `wss://` is selected
/// (orthogonal to the security level — even a Basic-Auth-only connection may need it against a
/// self-signed CSMS). Mirrors `ReconnectChoice` in the Modbus dialog.
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

/// Server-only "generate an ephemeral self-signed certificate" toggle, offered whenever `Tls`/
/// `MutualTls` is selected. Mirrors `SelfSignedChoice` in the Modbus dialog's `tls` module
/// byte-for-byte (see `ferrowl/src/module/modbus/setup_dialog/tls.rs`).
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

/// Raw text of every security input field, passed by name so the many look-alike path fields
/// cannot be transposed at a call site (a swapped positional pair would compile and only fail at
/// TLS-handshake time).
pub struct SecurityInputs<'a> {
    pub username: &'a str,
    pub password: &'a str,
    pub ca_file: &'a str,
    pub cert_file: &'a str,
    pub key_file: &'a str,
    pub client_cert_file: &'a str,
    pub client_key_file: &'a str,
    pub client_ca_file: &'a str,
}

impl SecurityLevel {
    /// Infer the level an existing [`OcppSecurityConfig`] represents, by role. Precedence (highest
    /// first): client cert (client) / require-client-cert or client CA (server) → `MutualTls`;
    /// cert+key (server) / CA file (client) → `Tls`; username → `BasicAuth`; else `None`.
    pub fn from_config(cfg: &OcppSecurityConfig, role: OcppRole) -> SecurityLevel {
        match role {
            OcppRole::Client => {
                if cfg.client_cert_file.is_some() {
                    SecurityLevel::MutualTls
                } else if !matches!(
                    cfg.client_verification,
                    ClientVerification::Verify { ca_file: None }
                ) {
                    SecurityLevel::Tls
                } else if cfg.username.is_some() {
                    SecurityLevel::BasicAuth
                } else {
                    SecurityLevel::None
                }
            }
            OcppRole::Server => {
                if cfg.require_client_cert || cfg.client_ca_file.is_some() {
                    SecurityLevel::MutualTls
                } else if matches!(cfg.server_cert, ServerCertSource::Explicit { .. }) {
                    // Deliberately not `!matches!(.., Unset)`: `SelfSigned` alone must not imply
                    // `Tls` here, since `resolve()`'s below-`Tls` auto-fallback (OC-R-095) also
                    // sets `server_cert: SelfSigned` -- treating that as `Tls` on a later
                    // `edit()` would promote the level and make the fallback irreversible.
                    // Matches the pre-OC-R-110 behavior, which ignored `self_signed` here too.
                    SecurityLevel::Tls
                } else if cfg.username.is_some() {
                    SecurityLevel::BasicAuth
                } else {
                    SecurityLevel::None
                }
            }
        }
    }

    /// Build the resolved [`OcppSecurityConfig`] for this level/role from raw field text, so a
    /// field not visible at this level/role (e.g. `client_cert_file` at `Tls`) is dropped rather
    /// than smuggled through from a stale input.
    pub fn build_config(self, role: OcppRole, inputs: SecurityInputs<'_>) -> OcppSecurityConfig {
        let SecurityInputs {
            username,
            password,
            ca_file,
            cert_file,
            key_file,
            client_cert_file,
            client_key_file,
            client_ca_file,
        } = inputs;
        let opt = |s: &str| {
            let t = s.trim();
            (!t.is_empty()).then(|| t.to_string())
        };
        let basic = self >= SecurityLevel::BasicAuth;
        let mtls = self == SecurityLevel::MutualTls;
        let is_client = role == OcppRole::Client;
        let is_server = role == OcppRole::Server;
        let _ = ca_file; // consulted by the caller (`resolve`) instead; see below
        let _ = (cert_file, key_file); // ditto
        OcppSecurityConfig {
            username: if basic { opt(username) } else { None },
            password: if basic { opt(password) } else { None },
            client_cert_file: if mtls && is_client {
                opt(client_cert_file)
            } else {
                None
            },
            client_key_file: if mtls && is_client {
                opt(client_key_file)
            } else {
                None
            },
            client_ca_file: if mtls && is_server {
                opt(client_ca_file)
            } else {
                None
            },
            require_client_cert: mtls && is_server,
            // `server_cert`/`client_verification` are overwritten by the caller (`resolve`),
            // which resolves them from the dialog's `self_signed`/`skip_verify` toggle widgets
            // together with the raw field text via `ServerCertSource::resolve`/
            // `ClientVerification::resolve` (OC-R-110/OC-R-111, mirroring MB-R-135) -- neither is
            // derivable from the raw text alone, since the toggle can exclude stale text this
            // function has no visibility into.
            client_verification: ClientVerification::default(),
            server_cert: ServerCertSource::default(),
        }
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

/// Check every required credential/certificate file is present and, for path fields, exists on
/// disk. `level` has already been checked `>= Tls` for the server role by the caller.
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
            // OC-R-110: Self-Signed needs no cert/key files (mirrors Modbus's `validate_tls`
            // fix in s8).
            if level >= SecurityLevel::Tls
                && !matches!(cfg.server_cert, ServerCertSource::SelfSigned)
            {
                match &cfg.server_cert {
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
            if level == SecurityLevel::MutualTls {
                let ca = cfg
                    .client_ca_file
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .ok_or("Client CA file is required for mTLS.")?;
                exists("Client CA file", ca)?;
            }
        }
        OcppRole::Client => {
            if let ClientVerification::Verify { ca_file: Some(ca) } = &cfg.client_verification
                && !ca.is_empty()
            {
                exists("CA file", ca)?;
            }
            if level == SecurityLevel::MutualTls {
                let cert = cfg
                    .client_cert_file
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .ok_or("Client certificate file is required for mTLS.")?;
                exists("Client certificate file", cert)?;
                let key = cfg
                    .client_key_file
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .ok_or("Client key file is required for mTLS.")?;
                exists("Client key file", key)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            client_verification: ClientVerification::Verify {
                ca_file: Some("ca.pem".into()),
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
            server_cert: ServerCertSource::Explicit {
                cert_file: "s.crt".into(),
                key_file: "s.key".into(),
            },
            ..Default::default()
        };
        assert_eq!(
            SecurityLevel::from_config(&cfg, OcppRole::Server),
            SecurityLevel::Tls
        );
    }

    #[test]
    /// UI-R-024 — a mutual-TLS client config loads into the client-cert fields.
    fn ut_from_config_mutual_tls_client_is_client_cert() {
        let cfg = OcppSecurityConfig {
            client_cert_file: Some("c.crt".into()),
            ..Default::default()
        };
        assert_eq!(
            SecurityLevel::from_config(&cfg, OcppRole::Client),
            SecurityLevel::MutualTls
        );
    }

    #[test]
    /// UI-R-024 — a mutual-TLS server config loads into the require-flag/client-CA fields.
    fn ut_from_config_mutual_tls_server_is_require_flag_or_client_ca() {
        let by_flag = OcppSecurityConfig {
            require_client_cert: true,
            ..Default::default()
        };
        assert_eq!(
            SecurityLevel::from_config(&by_flag, OcppRole::Server),
            SecurityLevel::MutualTls
        );
        let by_ca = OcppSecurityConfig {
            client_ca_file: Some("ca.pem".into()),
            ..Default::default()
        };
        assert_eq!(
            SecurityLevel::from_config(&by_ca, OcppRole::Server),
            SecurityLevel::MutualTls
        );
    }

    // --- SecurityLevel::build_config -----------------------------------------------------------

    #[test]
    /// UI-R-024 — building the config drops fields not visible at the chosen security level.
    fn ut_build_config_drops_fields_not_visible_at_level() {
        let cfg = SecurityLevel::BasicAuth.build_config(
            OcppRole::Server,
            SecurityInputs {
                username: "u",
                password: "p",
                ca_file: "ca",
                cert_file: "cert",
                key_file: "key",
                client_cert_file: "ccert",
                client_key_file: "ckey",
                client_ca_file: "cca",
            },
        );
        assert_eq!(cfg.username.as_deref(), Some("u"));
        assert_eq!(cfg.password.as_deref(), Some("p"));
        assert_eq!(cfg.client_ca_file, None);
        assert!(!cfg.require_client_cert);
    }

    #[test]
    /// UI-R-024 — a server TLS build drops client fields (server_cert/client_verification are
    /// resolved by the caller, `resolve()`, not by `build_config` -- see OC-R-110/OC-R-111.)
    fn ut_build_config_tls_server_keeps_cert_key_not_client_fields() {
        let cfg = SecurityLevel::Tls.build_config(
            OcppRole::Server,
            SecurityInputs {
                username: "",
                password: "",
                ca_file: "ca",
                cert_file: "cert",
                key_file: "key",
                client_cert_file: "ccert",
                client_key_file: "ckey",
                client_ca_file: "cca",
            },
        );
        assert_eq!(cfg.client_ca_file, None);
    }

    #[test]
    /// UI-R-024 — a mutual-TLS server build sets require-client-cert.
    fn ut_build_config_mutual_tls_server_sets_require_client_cert() {
        let cfg = SecurityLevel::MutualTls.build_config(
            OcppRole::Server,
            SecurityInputs {
                username: "",
                password: "",
                ca_file: "",
                cert_file: "cert",
                key_file: "key",
                client_cert_file: "",
                client_key_file: "",
                client_ca_file: "cca",
            },
        );
        assert_eq!(cfg.client_ca_file.as_deref(), Some("cca"));
        assert!(cfg.require_client_cert);
        assert_eq!(cfg.client_cert_file, None); // client-only field
    }

    #[test]
    /// UI-R-024 — a mutual-TLS client build keeps the client cert/key.
    fn ut_build_config_mutual_tls_client_keeps_client_cert_key() {
        let cfg = SecurityLevel::MutualTls.build_config(
            OcppRole::Client,
            SecurityInputs {
                username: "",
                password: "",
                ca_file: "ca",
                cert_file: "",
                key_file: "",
                client_cert_file: "ccert",
                client_key_file: "ckey",
                client_ca_file: "",
            },
        );
        assert_eq!(cfg.client_cert_file.as_deref(), Some("ccert"));
        assert_eq!(cfg.client_key_file.as_deref(), Some("ckey"));
        assert_eq!(cfg.client_ca_file, None); // server-only field
        assert!(!cfg.require_client_cert);
    }
}
