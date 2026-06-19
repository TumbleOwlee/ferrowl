//! OCPP-J framing: the version-agnostic envelope layer over the websocket transport.

pub mod codec;
mod error_code;
mod message;

pub use error_code::CallErrorCode;
pub use message::{MessageTypeId, OcppJMessage, UniqueId};
