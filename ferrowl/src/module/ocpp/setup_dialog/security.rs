//! Security-profile domain for the OCPP setup dialog: the websocket security level and the
//! skip-verify choice, plus the mapping between raw field text and an [`OcppSecurityConfig`]
//! (`from_config` infers a level, `build_config` resolves one, `validate_security` checks files).

use ferrowl_ui::traits::ToLabel;

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
                } else if cfg.ca_file.is_some() {
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
                } else if cfg.cert_file.is_some() || cfg.key_file.is_some() {
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
        let tls = self >= SecurityLevel::Tls;
        let mtls = self == SecurityLevel::MutualTls;
        let is_client = role == OcppRole::Client;
        let is_server = role == OcppRole::Server;
        OcppSecurityConfig {
            username: if basic { opt(username) } else { None },
            password: if basic { opt(password) } else { None },
            ca_file: if tls && is_client { opt(ca_file) } else { None },
            cert_file: if tls && is_server {
                opt(cert_file)
            } else {
                None
            },
            key_file: if tls && is_server {
                opt(key_file)
            } else {
                None
            },
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
            // Set by the caller (`resolve`), which knows the role/level rule for `self_signed`
            // and reads the dialog's `skip_verify` toggle for `insecure_skip_verify` — neither is
            // derivable from the raw field text this function works from.
            self_signed: false,
            insecure_skip_verify: false,
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
            if level >= SecurityLevel::Tls {
                let cert = cfg
                    .cert_file
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .ok_or("Certificate file is required for TLS.")?;
                exists("Certificate file", cert)?;
                let key = cfg
                    .key_file
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .ok_or("Key file is required for TLS.")?;
                exists("Key file", key)?;
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
            if let Some(ca) = cfg.ca_file.as_deref()
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
    fn ut_from_config_tls_client_is_ca_file() {
        let cfg = OcppSecurityConfig {
            ca_file: Some("ca.pem".into()),
            ..Default::default()
        };
        assert_eq!(
            SecurityLevel::from_config(&cfg, OcppRole::Client),
            SecurityLevel::Tls
        );
    }

    #[test]
    fn ut_from_config_tls_server_is_cert_and_key() {
        let cfg = OcppSecurityConfig {
            cert_file: Some("s.crt".into()),
            key_file: Some("s.key".into()),
            ..Default::default()
        };
        assert_eq!(
            SecurityLevel::from_config(&cfg, OcppRole::Server),
            SecurityLevel::Tls
        );
    }

    #[test]
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
        assert_eq!(cfg.cert_file, None);
        assert_eq!(cfg.key_file, None);
        assert_eq!(cfg.client_ca_file, None);
        assert!(!cfg.require_client_cert);
    }

    #[test]
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
        assert_eq!(cfg.cert_file.as_deref(), Some("cert"));
        assert_eq!(cfg.key_file.as_deref(), Some("key"));
        assert_eq!(cfg.ca_file, None); // client-only field
        assert_eq!(cfg.client_ca_file, None);
    }

    #[test]
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
        assert_eq!(cfg.ca_file.as_deref(), Some("ca"));
        assert_eq!(cfg.client_cert_file.as_deref(), Some("ccert"));
        assert_eq!(cfg.client_key_file.as_deref(), Some("ckey"));
        assert_eq!(cfg.client_ca_file, None); // server-only field
        assert!(!cfg.require_client_cert);
    }
}
