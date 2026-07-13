//! Hand-written OCPP-J `<->` JSON-array codec.
//!
//! OCPP-J envelopes are heterogeneous-arity JSON arrays (`[2, id, action, payload]`,
//! `[3, id, payload]`, `[4, id, code, desc, details]`), which serde's enum derives can't express,
//! so encoding/decoding is done by hand against `serde_json::Value`.

use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use super::{CallErrorCode, MessageTypeId, OcppJMessage, UniqueId};
use crate::error::FramingError;

/// Encode an envelope into a text websocket message.
pub fn encode(msg: &OcppJMessage) -> Message {
    let value = match msg {
        OcppJMessage::Call {
            id,
            action,
            payload,
        } => json!([MessageTypeId::Call as i64, id.as_str(), action, payload]),
        OcppJMessage::CallResult { id, payload } => {
            json!([MessageTypeId::CallResult as i64, id.as_str(), payload])
        }
        OcppJMessage::CallError {
            id,
            code,
            description,
            details,
        } => json!([
            MessageTypeId::CallError as i64,
            id.as_str(),
            code.as_str(),
            description,
            details
        ]),
    };
    Message::text(value.to_string())
}

/// Decode a text websocket payload into an envelope.
pub fn decode(text: &str) -> Result<OcppJMessage, FramingError> {
    let value: Value = serde_json::from_str(text)?;
    let arr = value.as_array().ok_or(FramingError::NotAnArray)?;

    let type_id = arr
        .first()
        .and_then(Value::as_i64)
        .ok_or(FramingError::BadFieldType {
            field: "messageTypeId",
        })?;

    let str_at = |idx: usize, field: &'static str| -> Result<String, FramingError> {
        arr.get(idx)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(FramingError::BadFieldType { field })
    };

    match type_id {
        x if x == MessageTypeId::Call as i64 => {
            if arr.len() != 4 {
                return Err(FramingError::BadArity(arr.len()));
            }
            Ok(OcppJMessage::Call {
                id: UniqueId(str_at(1, "uniqueId")?),
                action: str_at(2, "action")?,
                payload: arr[3].clone(),
            })
        }
        x if x == MessageTypeId::CallResult as i64 => {
            if arr.len() != 3 {
                return Err(FramingError::BadArity(arr.len()));
            }
            Ok(OcppJMessage::CallResult {
                id: UniqueId(str_at(1, "uniqueId")?),
                payload: arr[2].clone(),
            })
        }
        x if x == MessageTypeId::CallError as i64 => {
            if arr.len() != 5 {
                return Err(FramingError::BadArity(arr.len()));
            }
            Ok(OcppJMessage::CallError {
                id: UniqueId(str_at(1, "uniqueId")?),
                code: CallErrorCode::from_wire(&str_at(2, "errorCode")?),
                description: str_at(3, "errorDescription")?,
                details: arr[4].clone(),
            })
        }
        other => Err(FramingError::UnknownMessageType(other)),
    }
}

/// Recover the `uniqueId` of a Call frame that [`decode`] rejected, so the peer can be answered
/// with a `CallError` instead of being left to time out (OCPP-J requires a reply whenever the id
/// is recoverable).
///
/// Only Call frames qualify: a malformed CallResult or CallError must never be answered, since a
/// `CallError` about a `CallError` is not a thing the protocol allows. Returns `None` when the
/// text is not JSON, is not an array, has no `messageTypeId` of 2, or carries no string id.
pub fn recover_call_id(text: &str) -> Option<UniqueId> {
    let value: Value = serde_json::from_str(text).ok()?;
    let arr = value.as_array()?;
    if arr.first().and_then(Value::as_i64)? != MessageTypeId::Call as i64 {
        return None;
    }
    Some(UniqueId(arr.get(1).and_then(Value::as_str)?.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode to a text frame and decode it back.
    fn round_trip(msg: &OcppJMessage) -> OcppJMessage {
        match encode(msg) {
            Message::Text(text) => decode(text.as_str()).unwrap(),
            _ => panic!("encode must produce a text frame"),
        }
    }

    #[test]
    fn ut_call_round_trip() {
        let msg = OcppJMessage::Call {
            id: UniqueId("abc".into()),
            action: "Heartbeat".into(),
            payload: json!({}),
        };
        assert_eq!(msg, round_trip(&msg));
    }

    #[test]
    fn ut_call_result_round_trip() {
        let msg = OcppJMessage::CallResult {
            id: UniqueId("xyz".into()),
            payload: json!({"currentTime": "2026-01-01T00:00:00Z"}),
        };
        assert_eq!(msg, round_trip(&msg));
    }

    #[test]
    fn ut_call_error_round_trip() {
        let msg = OcppJMessage::CallError {
            id: UniqueId("e1".into()),
            code: CallErrorCode::FormationViolation,
            description: "bad".into(),
            details: json!({"k": "v"}),
        };
        assert_eq!(msg, round_trip(&msg));
    }

    #[test]
    fn ut_reject_non_array() {
        assert!(matches!(decode("{}"), Err(FramingError::NotAnArray)));
    }

    #[test]
    fn ut_reject_unknown_type() {
        assert!(matches!(
            decode("[9, \"id\", \"x\", {}]"),
            Err(FramingError::UnknownMessageType(9))
        ));
    }

    #[test]
    fn ut_reject_bad_arity() {
        assert!(matches!(
            decode("[2, \"id\"]"),
            Err(FramingError::BadArity(2))
        ));
    }

    #[test]
    fn ut_recover_call_id_from_malformed_call() {
        // Short-arity Call, and a Call whose action isn't a string: both rejected by `decode`,
        // both still owed a CallError, so the id must come back.
        assert_eq!(
            recover_call_id("[2, \"id-1\"]"),
            Some(UniqueId("id-1".into()))
        );
        assert_eq!(
            recover_call_id("[2, \"id-2\", 7, {}]"),
            Some(UniqueId("id-2".into()))
        );
    }

    #[test]
    fn ut_recover_call_id_gives_up_when_unanswerable() {
        assert_eq!(recover_call_id("not json"), None);
        assert_eq!(recover_call_id("{\"a\": 1}"), None); // not an array
        assert_eq!(recover_call_id("[]"), None); // no messageTypeId
        assert_eq!(recover_call_id("[2, 42, \"Heartbeat\", {}]"), None); // id not a string
        // A malformed CallResult/CallError must never be answered -- you can't CallError a
        // CallError, and a CallResult has no pending call on the peer's side to fail.
        assert_eq!(recover_call_id("[3, \"id-3\"]"), None);
        assert_eq!(recover_call_id("[4, \"id-4\"]"), None);
    }
}
