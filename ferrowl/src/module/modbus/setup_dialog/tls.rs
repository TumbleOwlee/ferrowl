//! TLS domain for the Modbus setup dialog: the transport security level and its toggles, plus
//! the mapping between raw field text and a [`ModbusTlsConfig`] (`from_config` infers a level,
//! `build_config` resolves one, `validate_tls` checks files). Mirrors
//! `ferrowl::module::ocpp::setup_dialog::security` exactly, minus Basic Auth (Modbus/TCP TLS has
//! no credential level, only Off/TLS/mTLS) — see `MB-R-104`..`MB-R-112`.

use ferrowl_ui::traits::ToLabel;

use crate::config::Role;
use ferrowl_modbus::tcp::ModbusTlsConfig;

/// Modbus/TCP TLS level. Cumulative: `MutualTls`'s fields are shown in addition to `Tls`'s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TlsLevel {
    Off,
    Tls,
    MutualTls,
}

impl ToLabel for TlsLevel {
    fn to_label(&self) -> String {
        match self {
            TlsLevel::Off => "Off",
            TlsLevel::Tls => "TLS",
            TlsLevel::MutualTls => "mTLS",
        }
        .to_string()
    }
}

/// Server-only "generate an ephemeral self-signed certificate" toggle, offered whenever `Tls`/
/// `MutualTls` is selected (orthogonal to whether `cert_file`/`key_file` are also set — per
/// edge-cases.md, explicit files win over `self_signed` when both are present).
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

/// Client-only "accept any server certificate" toggle, offered whenever `Tls`/`MutualTls` is
/// selected. Mirrors `SkipVerifyChoice` in the OCPP dialog's `security` module.
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

/// Raw text of every TLS path field, passed by name so the many look-alike path fields cannot be
/// transposed at a call site.
pub struct TlsInputs<'a> {
    pub ca_file: &'a str,
    pub cert_file: &'a str,
    pub key_file: &'a str,
    pub client_cert_file: &'a str,
    pub client_key_file: &'a str,
    pub client_ca_file: &'a str,
}

impl TlsLevel {
    /// Infer the level an existing [`ModbusTlsConfig`] represents, by role. Precedence (highest
    /// first): client cert (client) / require-client-cert or client CA (server) → `MutualTls`;
    /// CA file or skip-verify (client) / cert+key or self-signed (server) → `Tls`; else `Off`.
    pub fn from_config(cfg: &ModbusTlsConfig, role: Role) -> TlsLevel {
        match role {
            Role::Client => {
                if cfg.client_cert_file.is_some() {
                    TlsLevel::MutualTls
                } else if cfg.ca_file.is_some() || cfg.insecure_skip_verify {
                    TlsLevel::Tls
                } else {
                    TlsLevel::Off
                }
            }
            Role::Server => {
                if cfg.require_client_cert || cfg.client_ca_file.is_some() {
                    TlsLevel::MutualTls
                } else if cfg.cert_file.is_some() || cfg.key_file.is_some() || cfg.self_signed {
                    TlsLevel::Tls
                } else {
                    TlsLevel::Off
                }
            }
        }
    }

    /// Build the resolved [`ModbusTlsConfig`] for this level/role from raw field text, so a field
    /// not visible at this level/role (e.g. `client_cert_file` at `Tls`) is dropped rather than
    /// smuggled through from a stale input.
    pub fn build_config(self, role: Role, inputs: TlsInputs<'_>) -> ModbusTlsConfig {
        let TlsInputs {
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
        let tls = self >= TlsLevel::Tls;
        let mtls = self == TlsLevel::MutualTls;
        let is_client = role == Role::Client;
        let is_server = role == Role::Server;
        ModbusTlsConfig {
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
            // Set by the caller (`resolve`), which reads the dialog's `self_signed`/
            // `skip_verify` toggle widgets — neither is derivable from the raw field text this
            // function works from.
            self_signed: false,
            insecure_skip_verify: false,
        }
    }

    /// Index into the `tls_level` selection's value list (declaration order above).
    pub(super) fn index(self) -> usize {
        match self {
            TlsLevel::Off => 0,
            TlsLevel::Tls => 1,
            TlsLevel::MutualTls => 2,
        }
    }
}

/// Check every required certificate file is present and exists on disk. `level` has already been
/// checked `>= Tls` by the caller where relevant.
pub(super) fn validate_tls(
    cfg: &ModbusTlsConfig,
    role: Role,
    level: TlsLevel,
    file_exists: &dyn Fn(&str) -> bool,
) -> Result<(), String> {
    let exists = |label: &str, path: &str| -> Result<(), String> {
        if !file_exists(path) {
            return Err(format!("{label} not found: {path}"));
        }
        Ok(())
    };

    match role {
        Role::Server => {
            if level >= TlsLevel::Tls && !cfg.self_signed {
                let cert = cfg
                    .cert_file
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .ok_or("Certificate file is required for TLS (or enable Self-Signed).")?;
                exists("Certificate file", cert)?;
                let key = cfg
                    .key_file
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .ok_or("Key file is required for TLS (or enable Self-Signed).")?;
                exists("Key file", key)?;
            }
            if level == TlsLevel::MutualTls {
                let ca = cfg
                    .client_ca_file
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .ok_or("Client CA file is required for mTLS.")?;
                exists("Client CA file", ca)?;
            }
        }
        Role::Client => {
            if let Some(ca) = cfg.ca_file.as_deref()
                && !ca.is_empty()
            {
                exists("CA file", ca)?;
            }
            if level == TlsLevel::MutualTls {
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

    // --- TlsLevel::from_config -----------------------------------------------------------------

    #[test]
    /// UI-R-024 — the TLS fields load from a no-TLS config for both roles.
    fn ut_from_config_off_both_roles() {
        let cfg = ModbusTlsConfig::default();
        assert_eq!(TlsLevel::from_config(&cfg, Role::Client), TlsLevel::Off);
        assert_eq!(TlsLevel::from_config(&cfg, Role::Server), TlsLevel::Off);
    }

    #[test]
    /// UI-R-024 — a client TLS config loads into the CA-file field.
    fn ut_from_config_tls_client_is_ca_file() {
        let cfg = ModbusTlsConfig {
            ca_file: Some("ca.pem".into()),
            ..Default::default()
        };
        assert_eq!(TlsLevel::from_config(&cfg, Role::Client), TlsLevel::Tls);
    }

    #[test]
    /// UI-R-024 — a client config with only skip-verify set still loads at the TLS level.
    fn ut_from_config_tls_client_is_skip_verify_alone() {
        let cfg = ModbusTlsConfig {
            insecure_skip_verify: true,
            ..Default::default()
        };
        assert_eq!(TlsLevel::from_config(&cfg, Role::Client), TlsLevel::Tls);
    }

    #[test]
    /// UI-R-024 — a server TLS config loads into the cert and key fields.
    fn ut_from_config_tls_server_is_cert_and_key() {
        let cfg = ModbusTlsConfig {
            cert_file: Some("s.crt".into()),
            key_file: Some("s.key".into()),
            ..Default::default()
        };
        assert_eq!(TlsLevel::from_config(&cfg, Role::Server), TlsLevel::Tls);
    }

    #[test]
    /// UI-R-024 — a server config with only self_signed set still loads at the TLS level.
    fn ut_from_config_tls_server_is_self_signed_alone() {
        let cfg = ModbusTlsConfig {
            self_signed: true,
            ..Default::default()
        };
        assert_eq!(TlsLevel::from_config(&cfg, Role::Server), TlsLevel::Tls);
    }

    #[test]
    /// UI-R-024 — a mutual-TLS client config loads into the client-cert fields.
    fn ut_from_config_mutual_tls_client_is_client_cert() {
        let cfg = ModbusTlsConfig {
            client_cert_file: Some("c.crt".into()),
            ..Default::default()
        };
        assert_eq!(
            TlsLevel::from_config(&cfg, Role::Client),
            TlsLevel::MutualTls
        );
    }

    #[test]
    /// UI-R-024 — a mutual-TLS server config loads into the require-flag/client-CA fields.
    fn ut_from_config_mutual_tls_server_is_require_flag_or_client_ca() {
        let by_flag = ModbusTlsConfig {
            require_client_cert: true,
            ..Default::default()
        };
        assert_eq!(
            TlsLevel::from_config(&by_flag, Role::Server),
            TlsLevel::MutualTls
        );
        let by_ca = ModbusTlsConfig {
            client_ca_file: Some("ca.pem".into()),
            ..Default::default()
        };
        assert_eq!(
            TlsLevel::from_config(&by_ca, Role::Server),
            TlsLevel::MutualTls
        );
    }

    // --- TlsLevel::build_config -----------------------------------------------------------------

    #[test]
    /// UI-R-024 — building the config drops every field when the level is Off.
    fn ut_build_config_drops_fields_when_off() {
        let cfg = TlsLevel::Off.build_config(
            Role::Server,
            TlsInputs {
                ca_file: "ca",
                cert_file: "cert",
                key_file: "key",
                client_cert_file: "ccert",
                client_key_file: "ckey",
                client_ca_file: "cca",
            },
        );
        assert_eq!(cfg.cert_file, None);
        assert_eq!(cfg.key_file, None);
        assert_eq!(cfg.client_ca_file, None);
        assert!(!cfg.require_client_cert);
    }

    #[test]
    /// UI-R-024 — a server TLS build keeps cert/key and drops client fields.
    fn ut_build_config_tls_server_keeps_cert_key_not_client_fields() {
        let cfg = TlsLevel::Tls.build_config(
            Role::Server,
            TlsInputs {
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
    /// UI-R-024 — a mutual-TLS server build sets require-client-cert.
    fn ut_build_config_mutual_tls_server_sets_require_client_cert() {
        let cfg = TlsLevel::MutualTls.build_config(
            Role::Server,
            TlsInputs {
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
        let cfg = TlsLevel::MutualTls.build_config(
            Role::Client,
            TlsInputs {
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

    // --- validate_tls ----------------------------------------------------------------------------

    #[test]
    /// UI-R-024 — a server at TLS with self_signed set needs no cert/key files.
    fn ut_validate_tls_server_self_signed_needs_no_files() {
        let cfg = ModbusTlsConfig {
            self_signed: true,
            ..Default::default()
        };
        assert!(validate_tls(&cfg, Role::Server, TlsLevel::Tls, &|_| false).is_ok());
    }

    #[test]
    /// UI-R-024 — a server at TLS without self_signed requires an existing cert and key file.
    fn ut_validate_tls_server_requires_cert_and_key_files() {
        let missing = ModbusTlsConfig::default();
        assert!(validate_tls(&missing, Role::Server, TlsLevel::Tls, &|_| false).is_err());

        let cfg = ModbusTlsConfig {
            cert_file: Some("s.crt".into()),
            key_file: Some("s.key".into()),
            ..Default::default()
        };
        assert!(validate_tls(&cfg, Role::Server, TlsLevel::Tls, &|_| true).is_ok());
        assert!(validate_tls(&cfg, Role::Server, TlsLevel::Tls, &|_| false).is_err());
    }

    #[test]
    /// UI-R-024 — a server at mTLS additionally requires an existing client CA file.
    fn ut_validate_tls_server_mutual_tls_requires_client_ca_file() {
        let cfg = ModbusTlsConfig {
            self_signed: true,
            client_ca_file: Some("ca.pem".into()),
            ..Default::default()
        };
        assert!(validate_tls(&cfg, Role::Server, TlsLevel::MutualTls, &|_| true).is_ok());
        assert!(validate_tls(&cfg, Role::Server, TlsLevel::MutualTls, &|_| false).is_err());

        let no_ca = ModbusTlsConfig {
            self_signed: true,
            ..Default::default()
        };
        assert!(validate_tls(&no_ca, Role::Server, TlsLevel::MutualTls, &|_| true).is_err());
    }

    #[test]
    /// UI-R-024 — a client's CA file, when set, must exist; skip-verify alone needs no file.
    fn ut_validate_tls_client_ca_file_must_exist_when_set() {
        let cfg = ModbusTlsConfig {
            ca_file: Some("ca.pem".into()),
            ..Default::default()
        };
        assert!(validate_tls(&cfg, Role::Client, TlsLevel::Tls, &|_| true).is_ok());
        assert!(validate_tls(&cfg, Role::Client, TlsLevel::Tls, &|_| false).is_err());

        let skip_verify_only = ModbusTlsConfig {
            insecure_skip_verify: true,
            ..Default::default()
        };
        assert!(validate_tls(&skip_verify_only, Role::Client, TlsLevel::Tls, &|_| false).is_ok());
    }

    #[test]
    /// UI-R-024 — a client at mTLS requires existing client cert and key files.
    fn ut_validate_tls_client_mutual_tls_requires_client_cert_key_files() {
        let missing = ModbusTlsConfig::default();
        assert!(validate_tls(&missing, Role::Client, TlsLevel::MutualTls, &|_| false).is_err());

        let cfg = ModbusTlsConfig {
            client_cert_file: Some("c.crt".into()),
            client_key_file: Some("c.key".into()),
            ..Default::default()
        };
        assert!(validate_tls(&cfg, Role::Client, TlsLevel::MutualTls, &|_| true).is_ok());
        assert!(validate_tls(&cfg, Role::Client, TlsLevel::MutualTls, &|_| false).is_err());
    }
}
