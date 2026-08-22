//! TLS domain for the Modbus setup dialog: the transport security level and its toggles, plus
//! the mapping between raw field text and a [`ModbusTlsConfig`] (`from_config` infers a level,
//! `build_config` resolves one, `validate_tls` checks files). Mirrors
//! `ferrowl::module::ocpp::setup_dialog::security` exactly, minus Basic Auth (Modbus/TCP TLS has
//! no credential level, only Off/TLS/mTLS) — see `MB-R-104`..`MB-R-112`, `MB-R-136`, `MB-R-139`.

use ferrowl_ui::traits::ToLabel;

use crate::config::ClientOrServer;
use ferrowl_modbus::tcp::ModbusTlsConfig;
use ferrowl_util::tls::{
    ClientCertSource, ClientCertVerification, ClientTlsPolicy, ClientVerification,
    ServerCertSource, ServerTlsPolicy,
};

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

/// "Generate an ephemeral self-signed certificate/identity" toggle, offered whenever `Tls`/
/// `MutualTls` is selected. The *same* widget field is reused for both roles (they are never
/// shown at the same time, since a dialog instance is fixed to one role): for the server role it
/// toggles the presented server certificate's source (MB-R-106) whenever `Tls`/`MutualTls` is
/// selected; for the client role it toggles the client's own mTLS identity (MB-R-138/139)
/// whenever `MutualTls` is selected (the identity only exists under mTLS).
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

/// A binary skip-verify toggle, offered whenever `Tls`/`MutualTls` is selected. Two distinct
/// widget fields use this shape: the client-role "accept any server certificate" toggle (shown
/// at `Tls`+) and the server-role "accept any client certificate" toggle (`client_cert_skip_verify`,
/// MB-R-136, shown at `MutualTls` only) — never the same field, since each role only ever shows
/// one of the two rows this shape backs.
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

/// Raw text of every TLS path field, plus every toggle, passed by name so the many look-alike
/// path fields cannot be transposed at a call site. `client_ca_files` is the add/remove/edit
/// list widget's current entries (MB-R-136) — already individual paths, no further parsing.
pub struct TlsInputs<'a> {
    pub ca_file: &'a str,
    pub cert_file: &'a str,
    pub key_file: &'a str,
    pub client_cert_file: &'a str,
    pub client_key_file: &'a str,
    pub client_ca_files: &'a [String],
    /// Server: server certificate is self-signed (MB-R-106). Client, at `MutualTls` only: the
    /// client's own mTLS identity is self-signed (MB-R-138/139).
    pub self_signed: bool,
    /// Client: accept any server certificate.
    pub skip_verify: bool,
    /// Server, at `MutualTls` only: accept any client certificate (MB-R-136).
    pub client_cert_skip_verify: bool,
}

impl TlsLevel {
    /// Infer the level an existing [`ModbusTlsConfig`] represents, by role, from that role's own
    /// nested policy (`cfg.server`/`cfg.client` — the other role's half is always present on the
    /// wire too, per `device.rs`'s doc comment, but never consulted here).
    pub fn from_config(cfg: &ModbusTlsConfig, role: ClientOrServer) -> TlsLevel {
        match role {
            ClientOrServer::Client => match &cfg.client {
                ClientTlsPolicy::MutualTls { .. } => TlsLevel::MutualTls,
                ClientTlsPolicy::Tls { .. } => TlsLevel::Tls,
                ClientTlsPolicy::NoTls => TlsLevel::Off,
            },
            ClientOrServer::Server => match &cfg.server {
                ServerTlsPolicy::MutualTls { .. } => TlsLevel::MutualTls,
                ServerTlsPolicy::Tls { .. } => TlsLevel::Tls,
                ServerTlsPolicy::NoTls => TlsLevel::Off,
            },
        }
    }

    /// Build the active role's resolved policy from raw field text and toggle state
    /// (MB-R-135/136/139), producing a full [`ModbusTlsConfig`] whose *inactive* role's half is
    /// left at [`ModbusTlsConfig::default`]'s placeholder — the caller (`SetupDialog::resolve`)
    /// overwrites that half from the original config, if any, so a role toggle preserves the
    /// other role's previously-saved settings.
    pub fn build_config(
        self,
        role: ClientOrServer,
        inputs: TlsInputs<'_>,
    ) -> Result<ModbusTlsConfig, String> {
        let TlsInputs {
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
        let mtls = self == TlsLevel::MutualTls;

        let mut cfg = ModbusTlsConfig::default();
        match role {
            ClientOrServer::Server => {
                let server_cert =
                    ServerCertSource::resolve(self_signed, opt(cert_file), opt(key_file))?;
                cfg.server = if mtls {
                    let ca_files = client_ca_files.to_vec();
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
            ClientOrServer::Client => {
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
/// checked `>= Tls` by the caller where relevant. `cfg` is assumed already-resolved (the
/// "CA list required" and "cert/key required unless self-signed" *shape* errors are raised by
/// [`TlsLevel::build_config`] itself, since only it has the raw toggle state needed to phrase
/// them helpfully) — this only adds the file-existence check on top.
pub(super) fn validate_tls(
    cfg: &ModbusTlsConfig,
    role: ClientOrServer,
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
        ClientOrServer::Server => {
            let server_cert = match &cfg.server {
                ServerTlsPolicy::Tls { server_cert }
                | ServerTlsPolicy::MutualTls { server_cert, .. } => server_cert,
                ServerTlsPolicy::NoTls => &ServerCertSource::Unset,
            };
            if level >= TlsLevel::Tls && !matches!(server_cert, ServerCertSource::SelfSigned) {
                match server_cert {
                    ServerCertSource::Explicit {
                        cert_file,
                        key_file,
                    } => {
                        exists("Certificate file", cert_file)?;
                        exists("Key file", key_file)?;
                    }
                    ServerCertSource::Unset => {
                        return Err(
                            "Certificate file is required for TLS (or enable Self-Signed)."
                                .to_string(),
                        );
                    }
                    ServerCertSource::SelfSigned => unreachable!("excluded by the outer !matches!"),
                }
            }
            if level == TlsLevel::MutualTls
                && let ServerTlsPolicy::MutualTls {
                    client_verification,
                    ..
                } = &cfg.server
                && let ClientCertVerification::Verify { ca_files } = client_verification
            {
                for ca in ca_files {
                    exists("Client CA file", ca)?;
                }
            }
        }
        ClientOrServer::Client => {
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
            if level == TlsLevel::MutualTls
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

    fn inputs<'a>(
        ca_file: &'a str,
        cert_file: &'a str,
        key_file: &'a str,
        client_cert_file: &'a str,
        client_key_file: &'a str,
        client_ca_files: &'a [String],
    ) -> TlsInputs<'a> {
        TlsInputs {
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

    // --- TlsLevel::from_config -----------------------------------------------------------------

    #[test]
    /// UI-R-024 — the TLS fields load from a no-TLS-block config for both roles (Unset server
    /// cert, no client identity) — still `Tls` level, since `NoTls` never appears within a
    /// present `ModbusTlsConfig` (only the wrapping `Option<ModbusTlsConfig>` represents "off").
    fn ut_from_config_default_both_roles_is_tls() {
        let cfg = ModbusTlsConfig::default();
        assert_eq!(
            TlsLevel::from_config(&cfg, ClientOrServer::Client),
            TlsLevel::Tls
        );
        assert_eq!(
            TlsLevel::from_config(&cfg, ClientOrServer::Server),
            TlsLevel::Tls
        );
    }

    #[test]
    /// UI-R-024 — a mutual-TLS client config loads at the MutualTls level.
    fn ut_from_config_mutual_tls_client() {
        let cfg = ModbusTlsConfig {
            client: ClientTlsPolicy::MutualTls {
                client_verification: ClientVerification::default(),
                client_identity: ClientCertSource::SelfSigned,
            },
            ..Default::default()
        };
        assert_eq!(
            TlsLevel::from_config(&cfg, ClientOrServer::Client),
            TlsLevel::MutualTls
        );
    }

    #[test]
    /// UI-R-024/MB-R-136 — a mutual-TLS server config loads at the MutualTls level.
    fn ut_from_config_mutual_tls_server() {
        let cfg = ModbusTlsConfig {
            server: ServerTlsPolicy::MutualTls {
                server_cert: ServerCertSource::SelfSigned,
                client_verification: ClientCertVerification::Verify {
                    ca_files: vec!["ca.pem".to_string()],
                },
            },
            ..Default::default()
        };
        assert_eq!(
            TlsLevel::from_config(&cfg, ClientOrServer::Server),
            TlsLevel::MutualTls
        );
    }

    // --- TlsLevel::build_config -----------------------------------------------------------------

    #[test]
    /// UI-R-024 — a server TLS build resolves cert/key from raw text when self-signed is off.
    fn ut_build_config_tls_server_resolves_cert_key() {
        let cfg = TlsLevel::Tls
            .build_config(
                ClientOrServer::Server,
                inputs("", "cert", "key", "", "", &[]),
            )
            .unwrap();
        assert_eq!(
            cfg.server,
            ServerTlsPolicy::Tls {
                server_cert: ServerCertSource::Explicit {
                    cert_file: "cert".to_string(),
                    key_file: "key".to_string(),
                }
            }
        );
    }

    #[test]
    /// MB-R-136 — a mutual-TLS server build parses the comma-separated CA list and sets
    /// `require`-shaped `MutualTls` with `Verify{ca_files}`.
    fn ut_build_config_mutual_tls_server_parses_ca_list() {
        let cfg = TlsLevel::MutualTls
            .build_config(
                ClientOrServer::Server,
                inputs(
                    "",
                    "cert",
                    "key",
                    "",
                    "",
                    &["ca1.pem".to_string(), "ca2.pem".to_string()],
                ),
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
    /// MB-R-136 — a mutual-TLS server build with an empty CA list and skip-verify off is a
    /// validation error (rather than silently constructing an unrepresentable
    /// `ClientCertVerification::Verify { ca_files: vec![] }`).
    fn ut_build_config_mutual_tls_server_empty_ca_list_and_skip_verify_off_is_validation_error() {
        let err = TlsLevel::MutualTls
            .build_config(
                ClientOrServer::Server,
                inputs("", "cert", "key", "", "", &[]),
            )
            .unwrap_err();
        assert!(err.contains("Client CA list is required"));
    }

    #[test]
    /// MB-R-136 — a mutual-TLS server build with skip-verify on needs no CA list at all.
    fn ut_build_config_mutual_tls_server_skip_verify_needs_no_ca_list() {
        let mut i = inputs("", "cert", "key", "", "", &[]);
        i.client_cert_skip_verify = true;
        let cfg = TlsLevel::MutualTls
            .build_config(ClientOrServer::Server, i)
            .unwrap();
        assert_eq!(
            cfg.server,
            ServerTlsPolicy::MutualTls {
                server_cert: ServerCertSource::Explicit {
                    cert_file: "cert".to_string(),
                    key_file: "key".to_string(),
                },
                client_verification: ClientCertVerification::SkipVerify,
            }
        );
    }

    #[test]
    /// MB-R-139 — a mutual-TLS client build with the self-signed toggle on excludes the
    /// (possibly stale) client-cert/key text and resolves to `ClientCertSource::SelfSigned`.
    fn ut_build_config_mutual_tls_client_self_signed_excludes_cert_key() {
        let mut i = inputs("", "", "", "stale.crt", "stale.key", &[]);
        i.self_signed = true;
        let cfg = TlsLevel::MutualTls
            .build_config(ClientOrServer::Client, i)
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
    /// UI-R-024 — building the config for the inactive role leaves it at `ModbusTlsConfig`'s
    /// default placeholder (the caller stitches in the real inactive-role config, if any).
    fn ut_build_config_leaves_inactive_role_at_default() {
        let cfg = TlsLevel::Tls
            .build_config(
                ClientOrServer::Server,
                inputs("", "cert", "key", "", "", &[]),
            )
            .unwrap();
        assert_eq!(cfg.client, ModbusTlsConfig::default().client);
    }

    // --- validate_tls ----------------------------------------------------------------------------

    #[test]
    /// UI-R-024 — a server at TLS with self_signed set needs no cert/key files.
    fn ut_validate_tls_server_self_signed_needs_no_files() {
        let cfg = ModbusTlsConfig {
            server: ServerTlsPolicy::Tls {
                server_cert: ServerCertSource::SelfSigned,
            },
            ..Default::default()
        };
        assert!(validate_tls(&cfg, ClientOrServer::Server, TlsLevel::Tls, &|_| false).is_ok());
    }

    #[test]
    /// UI-R-024 — a server at TLS without self_signed requires an existing cert and key file.
    fn ut_validate_tls_server_requires_cert_and_key_files() {
        let missing = ModbusTlsConfig::default();
        assert!(validate_tls(&missing, ClientOrServer::Server, TlsLevel::Tls, &|_| false).is_err());

        let cfg = ModbusTlsConfig {
            server: ServerTlsPolicy::Tls {
                server_cert: ServerCertSource::Explicit {
                    cert_file: "s.crt".into(),
                    key_file: "s.key".into(),
                },
            },
            ..Default::default()
        };
        assert!(validate_tls(&cfg, ClientOrServer::Server, TlsLevel::Tls, &|_| true).is_ok());
        assert!(validate_tls(&cfg, ClientOrServer::Server, TlsLevel::Tls, &|_| false).is_err());
    }

    #[test]
    /// MB-R-136 — a server at mTLS additionally requires every listed client CA file to exist.
    fn ut_validate_tls_server_mutual_tls_requires_client_ca_files() {
        let cfg = ModbusTlsConfig {
            server: ServerTlsPolicy::MutualTls {
                server_cert: ServerCertSource::SelfSigned,
                client_verification: ClientCertVerification::Verify {
                    ca_files: vec!["ca1.pem".to_string(), "ca2.pem".to_string()],
                },
            },
            ..Default::default()
        };
        assert!(validate_tls(&cfg, ClientOrServer::Server, TlsLevel::MutualTls, &|_| true).is_ok());
        assert!(
            validate_tls(&cfg, ClientOrServer::Server, TlsLevel::MutualTls, &|_| {
                false
            })
            .is_err()
        );
    }

    #[test]
    /// MB-R-136 — a server at mTLS with skip-verify on needs no CA files checked.
    fn ut_validate_tls_server_mutual_tls_skip_verify_on_needs_no_ca_files() {
        let cfg = ModbusTlsConfig {
            server: ServerTlsPolicy::MutualTls {
                server_cert: ServerCertSource::SelfSigned,
                client_verification: ClientCertVerification::SkipVerify,
            },
            ..Default::default()
        };
        assert!(
            validate_tls(&cfg, ClientOrServer::Server, TlsLevel::MutualTls, &|_| {
                false
            })
            .is_ok()
        );
    }

    #[test]
    /// UI-R-024 — a client's CA file, when set, must exist; skip-verify alone needs no file.
    fn ut_validate_tls_client_ca_file_must_exist_when_set() {
        let cfg = ModbusTlsConfig {
            client: ClientTlsPolicy::Tls {
                client_verification: ClientVerification::Verify {
                    ca_file: Some("ca.pem".into()),
                },
            },
            ..Default::default()
        };
        assert!(validate_tls(&cfg, ClientOrServer::Client, TlsLevel::Tls, &|_| true).is_ok());
        assert!(validate_tls(&cfg, ClientOrServer::Client, TlsLevel::Tls, &|_| false).is_err());

        let skip_verify_only = ModbusTlsConfig {
            client: ClientTlsPolicy::Tls {
                client_verification: ClientVerification::SkipVerify,
            },
            ..Default::default()
        };
        assert!(
            validate_tls(
                &skip_verify_only,
                ClientOrServer::Client,
                TlsLevel::Tls,
                &|_| false
            )
            .is_ok()
        );
    }

    #[test]
    /// UI-R-024 — a client at mTLS requires existing client cert and key files.
    fn ut_validate_tls_client_mutual_tls_requires_client_cert_key_files() {
        let cfg = ModbusTlsConfig {
            client: ClientTlsPolicy::MutualTls {
                client_verification: ClientVerification::default(),
                client_identity: ClientCertSource::Explicit {
                    client_cert_file: "c.crt".into(),
                    client_key_file: "c.key".into(),
                },
            },
            ..Default::default()
        };
        assert!(validate_tls(&cfg, ClientOrServer::Client, TlsLevel::MutualTls, &|_| true).is_ok());
        assert!(
            validate_tls(&cfg, ClientOrServer::Client, TlsLevel::MutualTls, &|_| {
                false
            })
            .is_err()
        );
    }

    #[test]
    /// MB-R-139 — a client at mTLS with self-signed set needs no cert/key files checked.
    fn ut_validate_tls_client_self_signed_needs_no_files() {
        let cfg = ModbusTlsConfig {
            client: ClientTlsPolicy::MutualTls {
                client_verification: ClientVerification::default(),
                client_identity: ClientCertSource::SelfSigned,
            },
            ..Default::default()
        };
        assert!(
            validate_tls(&cfg, ClientOrServer::Client, TlsLevel::MutualTls, &|_| {
                false
            })
            .is_ok()
        );
    }
}
