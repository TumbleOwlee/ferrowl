//! Shared helpers for rendering an OCPP action or response to JSON for the message log.
//!
//! Both the client and the server encode payloads for the in-memory message log (and the outbound
//! command replies that become log entries). An encode failure there would otherwise silently
//! degrade to `Value::Null` with no trace; these helpers log the failure to stderr — the crate's
//! existing error-reporting channel (see `main.rs`/`headless.rs`) — before falling back to `Null`,
//! so callers keep their "empty payload" degradation while the failure is no longer invisible
//! (OC-R-101). They live here, outside either role's module, so both sides can reach them without
//! one depending on the other's internals.

use ferrowl_ocpp::Version;

/// Encode an `action` to JSON for the message log, logging any encode failure before degrading to
/// `Value::Null`.
pub(crate) fn encode_action_or_log<V: Version>(action: &V::Action) -> serde_json::Value {
    V::encode_action(action).unwrap_or_else(|e| {
        eprintln!("ocpp: failed to encode action to JSON: {e}");
        serde_json::Value::Null
    })
}

/// [`encode_action_or_log`]'s twin for responses.
pub(crate) fn encode_response_or_log<V: Version>(response: &V::Response) -> serde_json::Value {
    V::encode_response(response).unwrap_or_else(|e| {
        eprintln!("ocpp: failed to encode response to JSON: {e}");
        serde_json::Value::Null
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrowl_ocpp::{ConnectorScope, OcppError, ValidationError};

    /// Minimal [`Version`] mock whose `encode_action`/`encode_response` always fail, to exercise the
    /// helpers' fallback path without depending on a real action type ever failing to serialize.
    struct FailingVersion;

    impl Version for FailingVersion {
        type Action = ();
        type Response = ();

        fn action_name(_action: &()) -> &'static str {
            "Failing"
        }
        fn action_names() -> &'static [&'static str] {
            &[]
        }
        fn cs_actions() -> &'static [&'static str] {
            &[]
        }
        fn csms_actions() -> &'static [(&'static str, ConnectorScope)] {
            &[]
        }
        fn default_action(_name: &str) -> Option<()> {
            None
        }
        fn default_response(_name: &str) -> Option<()> {
            None
        }
        fn subprotocol() -> &'static str {
            "failing"
        }
        fn decode_call(_action_name: &str, _payload: serde_json::Value) -> Result<(), OcppError> {
            unimplemented!("not exercised by this test")
        }
        fn validate(_action: &()) -> Result<(), ValidationError> {
            Ok(())
        }
        fn encode_response(_response: &()) -> Result<serde_json::Value, OcppError> {
            Err(OcppError::UnknownAction("Failing".to_string()))
        }
        fn encode_action(_action: &()) -> Result<serde_json::Value, OcppError> {
            Err(OcppError::UnknownAction("Failing".to_string()))
        }
        fn decode_result(_action: &(), _payload: serde_json::Value) -> Result<(), OcppError> {
            unimplemented!("not exercised by this test")
        }
    }

    #[test]
    /// OC-R-101 — an action that fails to encode degrades to `null` instead of vanishing untraced.
    fn ut_encode_action_or_log_returns_null_on_encode_failure() {
        assert_eq!(
            encode_action_or_log::<FailingVersion>(&()),
            serde_json::Value::Null
        );
    }

    #[test]
    /// OC-R-101 — a response that fails to encode degrades to `null` instead of vanishing untraced.
    fn ut_encode_response_or_log_returns_null_on_encode_failure() {
        assert_eq!(
            encode_response_or_log::<FailingVersion>(&()),
            serde_json::Value::Null
        );
    }
}
