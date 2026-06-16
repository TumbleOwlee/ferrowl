//! Error types for instance lifecycle operations.

use tokio::sync::mpsc::error::SendError;

/// Errors from instance lifecycle management (start/stop/command).
#[derive(Debug, thiserror::Error)]
pub enum InstanceError {
    #[error("Instance is already active")]
    AlreadyActive,
    #[error("Instance is not running")]
    NotRunning,
    #[error("Failed to cancel instance")]
    CancelFailed,
    #[error("Failed to send command to instance: {0}")]
    SendError(SendError<ferrowl_modbus::Command>),
    #[error("Invalid operation specified")]
    InvalidOperation,
}

/// Combined error type: network errors from ferrowl-modbus or local
/// [`InstanceError`]s.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Network error: {0}")]
    Net(#[from] ferrowl_modbus::Error),
    #[error("Instance error: {0}")]
    Instance(#[from] InstanceError),
}
