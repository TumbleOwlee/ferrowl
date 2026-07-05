//! `ferrowl-ocpp`: OCPP CS/CSMS simulation abstraction crate (work in progress).
//!
//! The top-level [`Error`] intentionally carries a [`CallError`] (which holds a `serde_json::Value`
//! of error details), so it is a comparatively large enum. The crate returns it by value rather
//! than boxing throughout, so `clippy::result_large_err` is allowed crate-wide.
#![allow(clippy::result_large_err)]

mod action;
mod conn;
mod correlation;
mod error;
mod log;
mod ocppj;
mod security;

pub mod cs;
pub mod csms;

pub use action::{ConnectorScope, Version};
pub use error::{CallError, Error, FramingError, OcppError, TlsError, ValidationError, WsError};
pub use log::LogFn;
pub use ocppj::{CallErrorCode, MessageTypeId, OcppJMessage, UniqueId};
pub use security::{BasicAuth, CsTlsConfig, CsmsTlsConfig};

#[cfg(feature = "v1_6")]
pub use rust_ocpp::v1_6;
#[cfg(feature = "v2_0_1")]
pub use rust_ocpp::v2_0_1;
#[cfg(feature = "v2_1")]
pub use rust_ocpp::v2_1;

#[cfg(feature = "v1_6")]
pub use action::v1_6::{Action as Action16, Response as Response16, V1_6};
#[cfg(feature = "v2_0_1")]
pub use action::v2_0_1::{Action as Action201, Response as Response201, V2_0_1};
#[cfg(feature = "v2_1")]
pub use action::v2_1::{Action as Action21, Response as Response21, V2_1};
