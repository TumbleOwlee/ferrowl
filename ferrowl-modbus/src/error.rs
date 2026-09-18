//! Error types for Modbus protocol and transport failures.

use rust_modbus::ExceptionCode;

/// Errors from Modbus protocol operations.
#[derive(Debug, thiserror::Error)]
pub enum ModbusError {
    #[error("Modbus exception: {0:?}")]
    Exception(ExceptionCode),
    #[error("Modbus error: {0}")]
    Error(rust_modbus::Error),
    #[error("Modbus timeout: {0}")]
    Timeout(tokio::time::error::Elapsed),
}

/// Errors from the serial (RTU) transport.
#[derive(Debug, thiserror::Error)]
pub enum SerialError {
    #[error("Serial error: {0}")]
    Error(rust_modbus::Error),
    #[error("Serial configuration error: {0}")]
    Configuration(String),
}

/// Errors from the TCP transport.
#[derive(Debug, thiserror::Error)]
pub enum TcpError {
    #[error("TCP address error: {0}")]
    Address(std::net::AddrParseError),
    #[error("TCP configuration error: {0}")]
    Configuration(String),
    #[error("TCP configuration error: {0}")]
    Tls(#[from] TlsError),
    #[error("TCP error: {0}")]
    Error(rust_modbus::Error),
    #[error("TCP timeout: {0}")]
    Timeout(tokio::time::error::Elapsed),
}

/// The stage of self-signed certificate generation a [`TlsError::SelfSigned`] failed at
/// (MB-R-251).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfSignedStep {
    CertGeneration,
    KeyGeneration,
    KeyEncoding,
}

impl std::fmt::Display for SelfSignedStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SelfSignedStep::CertGeneration => "cert generation",
            SelfSignedStep::KeyGeneration => "key generation",
            SelfSignedStep::KeyEncoding => "key encoding",
        })
    }
}

/// A TLS configuration failure raised while building a Modbus endpoint's TLS material
/// (MB-R-250), covering both the shared-policy construction rejections and the PEM-loading
/// tier (MB-R-251).
#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("{source}")]
    Pem {
        path: String,
        source: rust_modbus::Error,
    },
    #[error("{path} contains no certificate")]
    NoCertificates { path: String },
    #[error("verify = \"ca-files\": ca_files must be non-empty")]
    EmptyCaFiles,
    #[error("source = \"ephemeral\" is not a valid client identity")]
    EphemeralClientIdentity,
    #[error("verify = \"root-store\" is client-only, not valid on a server")]
    RootStoreOnServer,
    #[error("self-signed {step} failed: {detail}")]
    SelfSigned {
        step: SelfSignedStep,
        detail: String,
    },
}

impl From<ferrowl_util::tls::PolicyError> for TlsError {
    fn from(err: ferrowl_util::tls::PolicyError) -> Self {
        match err {
            ferrowl_util::tls::PolicyError::EmptyCaFiles => TlsError::EmptyCaFiles,
            ferrowl_util::tls::PolicyError::EphemeralClientIdentity => {
                TlsError::EphemeralClientIdentity
            }
            ferrowl_util::tls::PolicyError::RootStoreOnServer => TlsError::RootStoreOnServer,
        }
    }
}

/// Top-level error type unifying protocol, transport, and server errors.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Modbus(#[from] ModbusError),
    #[error("{0}")]
    Serial(#[from] SerialError),
    #[error("{0}")]
    Tcp(#[from] TcpError),
    #[error("Server error: {0}")]
    Server(rust_modbus::Error),
    /// MB-R-150 — another module instance in the same session already claims this Rtu/Ascii
    /// serial path; the OS-level open was skipped for this attempt.
    #[error("Serial path '{path}' is already in use by module '{other}' in this session")]
    PathConflict { path: String, other: String },
}

#[cfg(test)]
mod tests {
    use super::{Error, ModbusError, SelfSignedStep, SerialError, TcpError, TlsError};
    use rust_modbus::ExceptionCode;

    #[test]
    /// MB-R-251 — an unreadable cert/key/CA file carries its path and I/O cause.
    fn ut_tls_error_io_display() {
        let e = TlsError::Io {
            path: "cert.pem".to_string(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"),
        };
        assert_eq!(e.to_string(), "failed to read cert.pem: no such file");
    }

    #[test]
    /// MB-R-251 — a readable file whose PEM cannot be parsed carries its path and parse cause.
    fn ut_tls_error_pem_display() {
        let e = TlsError::Pem {
            path: "cert.pem".to_string(),
            source: rust_modbus::Error::Configuration { field: "pem" },
        };
        assert_eq!(e.to_string(), "invalid configuration for pem");
    }

    #[test]
    /// MB-R-251 — a readable file holding no certificate names the path.
    fn ut_tls_error_no_certificates_display() {
        let e = TlsError::NoCertificates {
            path: "cert.pem".to_string(),
        };
        assert_eq!(e.to_string(), "cert.pem contains no certificate");
    }

    #[test]
    /// MB-R-251 — the three shared-policy construction rejections keep their existing text.
    fn ut_tls_error_empty_ca_files_display() {
        assert_eq!(
            TlsError::EmptyCaFiles.to_string(),
            "verify = \"ca-files\": ca_files must be non-empty"
        );
    }

    #[test]
    /// MB-R-251 — the three shared-policy construction rejections keep their existing text.
    fn ut_tls_error_ephemeral_client_identity_display() {
        assert_eq!(
            TlsError::EphemeralClientIdentity.to_string(),
            "source = \"ephemeral\" is not a valid client identity"
        );
    }

    #[test]
    /// MB-R-251 — the three shared-policy construction rejections keep their existing text.
    fn ut_tls_error_root_store_on_server_display() {
        assert_eq!(
            TlsError::RootStoreOnServer.to_string(),
            "verify = \"root-store\" is client-only, not valid on a server"
        );
    }

    #[test]
    /// MB-R-251 — a self-signed generation failure names its step and detail.
    fn ut_tls_error_self_signed_display() {
        let e = TlsError::SelfSigned {
            step: SelfSignedStep::CertGeneration,
            detail: "boom".to_string(),
        };
        assert_eq!(e.to_string(), "self-signed cert generation failed: boom");

        let e = TlsError::SelfSigned {
            step: SelfSignedStep::KeyGeneration,
            detail: "boom".to_string(),
        };
        assert_eq!(e.to_string(), "self-signed key generation failed: boom");

        let e = TlsError::SelfSigned {
            step: SelfSignedStep::KeyEncoding,
            detail: "boom".to_string(),
        };
        assert_eq!(e.to_string(), "self-signed key encoding failed: boom");
    }

    #[test]
    /// MB-R-251 — the shared TLS policy error maps to the matching `TlsError` case, one to one.
    fn ut_tls_error_from_policy_error_maps_each_case() {
        assert!(matches!(
            TlsError::from(ferrowl_util::tls::PolicyError::EmptyCaFiles),
            TlsError::EmptyCaFiles
        ));
        assert!(matches!(
            TlsError::from(ferrowl_util::tls::PolicyError::EphemeralClientIdentity),
            TlsError::EphemeralClientIdentity
        ));
        assert!(matches!(
            TlsError::from(ferrowl_util::tls::PolicyError::RootStoreOnServer),
            TlsError::RootStoreOnServer
        ));
    }

    #[test]
    /// MB-R-250 — a TLS configuration failure crosses the crate boundary with its typed cause
    /// reachable through `source()`, not flattened into a string.
    fn ut_tcp_error_tls_source_reachable_and_display_unchanged() {
        let e = TcpError::from(TlsError::NoCertificates {
            path: "x.pem".to_string(),
        });
        let source = std::error::Error::source(&e)
            .expect("Tls variant carries a source")
            .downcast_ref::<TlsError>();
        assert!(source.is_some());
        assert_eq!(
            e.to_string(),
            "TCP configuration error: x.pem contains no certificate"
        );
    }

    #[test]
    fn ut_serial_error_configuration_display() {
        let e = SerialError::Configuration("bad baud rate".to_string());
        assert_eq!(e.to_string(), "Serial configuration error: bad baud rate");
    }

    #[test]
    fn ut_tcp_error_configuration_display() {
        let e = TcpError::Configuration("missing host".to_string());
        assert_eq!(e.to_string(), "TCP configuration error: missing host");
    }

    #[test]
    fn ut_modbus_error_exception_display() {
        let e = ModbusError::Exception(ExceptionCode::IllegalFunction);
        assert!(e.to_string().contains("Modbus exception"));
    }

    #[test]
    fn ut_error_wraps_serial_display() {
        let inner = SerialError::Configuration("oops".to_string());
        let e = Error::from(inner);
        assert!(e.to_string().contains("Serial configuration error: oops"));
    }

    #[test]
    fn ut_error_wraps_tcp_display() {
        let inner = TcpError::Configuration("no host".to_string());
        let e = Error::from(inner);
        assert!(e.to_string().contains("TCP configuration error: no host"));
    }

    #[test]
    /// MB-R-150 — the conflict error names both the path and the other instance, distinguishing
    /// it from an ordinary open-failure message.
    fn ut_error_path_conflict_display() {
        let e = Error::PathConflict {
            path: "/dev/ttyUSB0".to_string(),
            other: "PLC Sim".to_string(),
        };
        assert_eq!(
            e.to_string(),
            "Serial path '/dev/ttyUSB0' is already in use by module 'PLC Sim' in this session"
        );
    }
}
