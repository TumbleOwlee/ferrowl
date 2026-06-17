//! OCPP-J envelope types, version-agnostic across 1.6 and 2.0.1.

use serde_json::Value;

use super::CallErrorCode;

/// OCPP-J message type id (the first element of every envelope array).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub enum MessageTypeId {
    Call = 2,
    CallResult = 3,
    CallError = 4,
}

/// A correlation id. For our outbound Calls this is a UUID v4 string; for inbound Calls it is
/// whatever string the peer chose, so the underlying storage is always a `String`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UniqueId(pub String);

impl UniqueId {
    /// Generate a fresh UUID v4 unique id for an outbound Call.
    pub fn generate() -> Self {
        UniqueId(uuid::Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for UniqueId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for UniqueId {
    fn from(s: String) -> Self {
        UniqueId(s)
    }
}

/// A decoded OCPP-J envelope. `action`/`payload` are kept as raw values so this layer stays
/// version-agnostic; turning them into typed actions/responses is the [`Version`] trait's job.
///
/// [`Version`]: crate::action::Version
#[derive(Debug, Clone, PartialEq)]
pub enum OcppJMessage {
    /// `[2, id, action, payload]`
    Call {
        id: UniqueId,
        action: String,
        payload: Value,
    },
    /// `[3, id, payload]`
    CallResult { id: UniqueId, payload: Value },
    /// `[4, id, errorCode, errorDescription, details]`
    CallError {
        id: UniqueId,
        code: CallErrorCode,
        description: String,
        details: Value,
    },
}

impl OcppJMessage {
    /// The correlation id of this envelope.
    pub fn id(&self) -> &UniqueId {
        match self {
            OcppJMessage::Call { id, .. }
            | OcppJMessage::CallResult { id, .. }
            | OcppJMessage::CallError { id, .. } => id,
        }
    }
}
