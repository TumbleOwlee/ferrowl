//! The version-portable semantic trait layer, built on top of the low-level action layer.
//!
//! Consumers write simulation logic once against the semantic traits ([`CsOps`]/[`CsHandler`] and
//! their CSMS mirrors) using the version-neutral [`types`], and it runs unmodified against either
//! OCPP version. Outbound methods are implemented on `Client`/`Server` per version; inbound methods
//! are implemented by per-version `SemanticAdapter`s that wrap a user handler.
//!
//! [`CsOps`]: crate::cs::CsOps
//! [`CsHandler`]: crate::cs::CsHandler

pub mod types;

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::{CallError, Error};
use crate::ocppj::CallErrorCode;

/// Decode an adapter-built JSON value into a wire type, mapping failure to an inbound `CallError`.
pub(crate) fn decode_call<T: DeserializeOwned>(value: Value) -> Result<T, CallError> {
    serde_json::from_value(value).map_err(|e| {
        CallError::new(
            CallErrorCode::InternalError,
            format!("semantic decode: {e}"),
        )
    })
}

/// Decode an adapter-built JSON value into a wire type, mapping failure to an outbound `Error`.
pub(crate) fn decode_out<T: DeserializeOwned>(value: Value) -> Result<T, Error> {
    serde_json::from_value(value).map_err(|e| crate::error::OcppError::Json(e).into())
}

/// Render a serializable (typically `rust_ocpp` enum) value as its wire string, for reading an
/// inbound enum field into a neutral `String`.
pub(crate) fn enum_str<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}
