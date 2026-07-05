//! Layered error types, mirroring `ferrowl-modbus`'s hierarchy.
//!
//! `FramingError` (malformed envelope) -> `WsError` (transport) -> `OcppError` (action/decode/
//! validation) -> top-level [`Error`]. Separately, [`CallError`] is what a handler *returns* to
//! signal a protocol-level rejection that is sent back over the wire as an OCPP-J `CallError`
//! frame -- it is distinct from [`Error`], which tears the connection down.

use crate::ocppj::CallErrorCode;

/// Malformed OCPP-J envelope: not a JSON array, wrong arity, bad message-type id, non-string
/// unique id, etc.
#[derive(Debug, thiserror::Error)]
pub enum FramingError {
    #[error("OCPP-J frame is not a JSON array")]
    NotAnArray,
    #[error("OCPP-J frame has invalid arity: {0} elements")]
    BadArity(usize),
    #[error("OCPP-J frame has unknown message type id: {0}")]
    UnknownMessageType(i64),
    #[error("OCPP-J frame field {field} has the wrong JSON type")]
    BadFieldType { field: &'static str },
    #[error("OCPP-J frame is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// Transport-level failure: tungstenite error, await timeout, or a closed socket.
#[derive(Debug, thiserror::Error)]
pub enum WsError {
    #[error("websocket error: {0}")]
    Tungstenite(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("timed out waiting for reply")]
    Timeout,
    #[error("connection closed")]
    Closed,
    #[error("subprotocol negotiation failed (expected {expected})")]
    Subprotocol { expected: &'static str },
}

/// Wrapper over `validator::ValidationErrors` so it participates in the error hierarchy.
#[derive(Debug, thiserror::Error)]
#[error("validation failed: {0}")]
pub struct ValidationError(#[from] pub validator::ValidationErrors);

/// TLS material (certificate/key/CA) failed to load, parse, or configure. Covers Security
/// Profiles 2 and 3.
#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("io error reading TLS file {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("no certificates found in {0}")]
    NoCertificates(String),
    #[error("no private key found in {0}")]
    NoPrivateKey(String),
    #[error("require_client_cert is set but no client_ca_file was configured")]
    MissingClientCa,
    #[error("client certificate verifier configuration failed: {0}")]
    ClientVerifier(String),
    #[error("rustls configuration error: {0}")]
    Rustls(#[from] rustls::Error),
}

/// OCPP-semantic failure: unknown action, (de)serialization, or request validation.
#[derive(Debug, thiserror::Error)]
pub enum OcppError {
    #[error("unknown action: {0}")]
    UnknownAction(String),
    #[error("payload (de)serialization failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Validation(#[from] ValidationError),
}

/// Top-level error returned by builders and core loops. A returned `Error` means the operation or
/// connection failed -- contrast with [`CallError`], which is a protocol-level rejection that
/// stays on the wire.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Framing(#[from] FramingError),
    #[error("{0}")]
    Ws(#[from] WsError),
    #[error("{0}")]
    Ocpp(#[from] OcppError),
    #[error("{0}")]
    Tls(#[from] TlsError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("operation not supported by this OCPP version")]
    NotSupported,
    #[error("the core task is not running")]
    NotRunning,
    #[error("no connection with id {0}")]
    UnknownConnection(u64),
    #[error("call rejected by peer: {0}")]
    Call(CallError),
    #[error("internal channel closed")]
    ChannelClosed,
}

/// A protocol-level rejection a handler returns; serialized to an OCPP-J `CallError` frame
/// `[4, id, errorCode, errorDescription, details]` and sent back to the peer.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{code:?}: {description}")]
pub struct CallError {
    pub code: CallErrorCode,
    pub description: String,
    pub details: serde_json::Value,
}

impl CallError {
    /// Construct a `CallError` with the given code and description and an empty details object.
    pub fn new(code: CallErrorCode, description: impl Into<String>) -> Self {
        Self {
            code,
            description: description.into(),
            details: serde_json::Value::Object(Default::default()),
        }
    }
}

impl From<OcppError> for CallError {
    fn from(err: OcppError) -> Self {
        let code = match &err {
            OcppError::UnknownAction(_) => CallErrorCode::NotImplemented,
            OcppError::Json(_) => CallErrorCode::FormationViolation,
            OcppError::Validation(_) => CallErrorCode::FormationViolation,
        };
        CallError::new(code, err.to_string())
    }
}
