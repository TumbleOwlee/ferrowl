//! `CsmsHandler`: the version-portable inbound API a CSMS simulation implements to answer
//! CS-initiated Calls. Mirrors [`CsOps`](crate::cs::CsOps). Wrapped per version by
//! [`SemanticAdapter`](crate::csms::SemanticAdapter); unimplemented methods default to a
//! `NotSupported` `CallError`.

use std::future::Future;

use super::registry::ConnectionId;
use crate::error::CallError;
use crate::ocppj::CallErrorCode;
use crate::semantic::types::*;

fn not_supported(method: &str) -> CallError {
    CallError::new(
        CallErrorCode::NotSupported,
        format!("{method} not implemented for this simulation"),
    )
}

/// CSMS-inbound semantic operations, scoped to the originating [`ConnectionId`]. Implement the ones
/// your simulation answers; the rest reject with a `NotSupported` `CallError`.
pub trait CsmsHandler: Send + Sync + 'static {
    fn on_boot_notification(
        &self,
        _conn: ConnectionId,
        _params: BootNotificationParams,
    ) -> impl Future<Output = Result<BootNotificationResult, CallError>> + Send {
        async { Err(not_supported("on_boot_notification")) }
    }

    fn on_heartbeat(
        &self,
        _conn: ConnectionId,
    ) -> impl Future<Output = Result<HeartbeatResult, CallError>> + Send {
        async { Err(not_supported("on_heartbeat")) }
    }

    fn on_authorize(
        &self,
        _conn: ConnectionId,
        _params: AuthorizeParams,
    ) -> impl Future<Output = Result<AuthorizeResult, CallError>> + Send {
        async { Err(not_supported("on_authorize")) }
    }

    /// Merged: v1.6 `StartTransaction`, v2.0.1 `TransactionEvent { event_type: Started }`.
    fn on_start_transaction(
        &self,
        _conn: ConnectionId,
        _params: StartTransactionParams,
    ) -> impl Future<Output = Result<StartTransactionResult, CallError>> + Send {
        async { Err(not_supported("on_start_transaction")) }
    }

    /// Merged: v1.6 `StopTransaction`, v2.0.1 `TransactionEvent { event_type: Ended }`.
    fn on_stop_transaction(
        &self,
        _conn: ConnectionId,
        _params: StopTransactionParams,
    ) -> impl Future<Output = Result<StopTransactionResult, CallError>> + Send {
        async { Err(not_supported("on_stop_transaction")) }
    }

    /// v2.0.1-only.
    fn on_notify_event(
        &self,
        _conn: ConnectionId,
        _params: NotifyEventParams,
    ) -> impl Future<Output = Result<NotifyEventResult, CallError>> + Send {
        async { Err(not_supported("on_notify_event")) }
    }

    /// `StatusNotification` (both versions). No response payload.
    fn on_status_notification(
        &self,
        _conn: ConnectionId,
        _params: StatusNotificationParams,
    ) -> impl Future<Output = Result<(), CallError>> + Send {
        async { Err(not_supported("on_status_notification")) }
    }

    /// `MeterValues` (both versions). No response payload.
    fn on_meter_values(
        &self,
        _conn: ConnectionId,
        _params: MeterValuesParams,
    ) -> impl Future<Output = Result<(), CallError>> + Send {
        async { Err(not_supported("on_meter_values")) }
    }

    /// `FirmwareStatusNotification` (both versions). No response payload.
    fn on_firmware_status_notification(
        &self,
        _conn: ConnectionId,
        _params: FirmwareStatusNotificationParams,
    ) -> impl Future<Output = Result<(), CallError>> + Send {
        async { Err(not_supported("on_firmware_status_notification")) }
    }

    /// `DataTransfer` initiated by the CS (both versions).
    fn on_data_transfer(
        &self,
        _conn: ConnectionId,
        _params: DataTransferParams,
    ) -> impl Future<Output = Result<DataTransferResult, CallError>> + Send {
        async { Err(not_supported("on_data_transfer")) }
    }
}
