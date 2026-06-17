//! `CsHandler`: the version-portable inbound API a charging-station simulation implements to answer
//! CSMS-initiated Calls. Wrapped per version by [`SemanticAdapter`](crate::cs::SemanticAdapter);
//! unimplemented methods default to a `NotSupported` `CallError`.

use std::future::Future;

use crate::error::CallError;
use crate::ocppj::CallErrorCode;
use crate::semantic::types::*;

/// Default `CallError` for an unimplemented inbound semantic method.
fn not_supported(method: &str) -> CallError {
    CallError::new(
        CallErrorCode::NotSupported,
        format!("{method} not implemented for this simulation"),
    )
}

/// CS-inbound semantic operations. Implement the ones your simulation answers; the rest reject with
/// a `NotSupported` `CallError`.
///
/// This trait currently exposes the wired slice (merged config + merged remote-start); the
/// remaining inbound actions are additive and follow the same shape.
pub trait CsHandler: Send + Sync + 'static {
    /// Merged: v1.6 `ChangeConfiguration` (fanned out per key by the adapter), v2.0.1
    /// `SetVariables` (native batch).
    fn on_set_config(
        &self,
        _params: SetConfigParams,
    ) -> impl Future<Output = Result<SetConfigResult, CallError>> + Send {
        async { Err(not_supported("on_set_config")) }
    }

    /// Merged: v1.6 `GetConfiguration`, v2.0.1 `GetVariables`.
    fn on_get_config(
        &self,
        _params: GetConfigParams,
    ) -> impl Future<Output = Result<GetConfigResult, CallError>> + Send {
        async { Err(not_supported("on_get_config")) }
    }

    /// Merged: v1.6 `RemoteStartTransaction`, v2.0.1 `RequestStartTransaction`.
    fn on_start_transaction_requested(
        &self,
        _params: RemoteStartParams,
    ) -> impl Future<Output = Result<RemoteStartResult, CallError>> + Send {
        async { Err(not_supported("on_start_transaction_requested")) }
    }

    /// Merged: v1.6 `RemoteStopTransaction`, v2.0.1 `RequestStopTransaction`.
    fn on_stop_transaction_requested(
        &self,
        _params: RemoteStopParams,
    ) -> impl Future<Output = Result<GenericStatusResult, CallError>> + Send {
        async { Err(not_supported("on_stop_transaction_requested")) }
    }

    /// `ChangeAvailability` (both versions).
    fn on_change_availability(
        &self,
        _params: ChangeAvailabilityParams,
    ) -> impl Future<Output = Result<GenericStatusResult, CallError>> + Send {
        async { Err(not_supported("on_change_availability")) }
    }

    /// `Reset` (both versions).
    fn on_reset(
        &self,
        _params: ResetParams,
    ) -> impl Future<Output = Result<GenericStatusResult, CallError>> + Send {
        async { Err(not_supported("on_reset")) }
    }

    /// `UnlockConnector` (both versions).
    fn on_unlock_connector(
        &self,
        _params: UnlockConnectorParams,
    ) -> impl Future<Output = Result<GenericStatusResult, CallError>> + Send {
        async { Err(not_supported("on_unlock_connector")) }
    }

    /// `TriggerMessage` (both versions).
    fn on_trigger_message(
        &self,
        _params: TriggerMessageParams,
    ) -> impl Future<Output = Result<GenericStatusResult, CallError>> + Send {
        async { Err(not_supported("on_trigger_message")) }
    }

    /// `ClearCache` (both versions).
    fn on_clear_cache(
        &self,
    ) -> impl Future<Output = Result<GenericStatusResult, CallError>> + Send {
        async { Err(not_supported("on_clear_cache")) }
    }

    /// `GetLocalListVersion` (both versions).
    fn on_get_local_list_version(
        &self,
    ) -> impl Future<Output = Result<LocalListVersionResult, CallError>> + Send {
        async { Err(not_supported("on_get_local_list_version")) }
    }

    /// `CancelReservation` (both versions).
    fn on_cancel_reservation(
        &self,
        _params: CancelReservationParams,
    ) -> impl Future<Output = Result<GenericStatusResult, CallError>> + Send {
        async { Err(not_supported("on_cancel_reservation")) }
    }

    /// `SetChargingProfile` (both versions).
    fn on_set_charging_profile(
        &self,
        _params: SetChargingProfileParams,
    ) -> impl Future<Output = Result<GenericStatusResult, CallError>> + Send {
        async { Err(not_supported("on_set_charging_profile")) }
    }

    /// `ClearChargingProfile` (both versions).
    fn on_clear_charging_profile(
        &self,
        _params: ClearChargingProfileParams,
    ) -> impl Future<Output = Result<GenericStatusResult, CallError>> + Send {
        async { Err(not_supported("on_clear_charging_profile")) }
    }

    /// `GetCompositeSchedule` (both versions).
    fn on_get_composite_schedule(
        &self,
        _params: GetCompositeScheduleParams,
    ) -> impl Future<Output = Result<GetCompositeScheduleResult, CallError>> + Send {
        async { Err(not_supported("on_get_composite_schedule")) }
    }

    /// `ReserveNow` (both versions).
    fn on_reserve_now(
        &self,
        _params: ReserveNowParams,
    ) -> impl Future<Output = Result<GenericStatusResult, CallError>> + Send {
        async { Err(not_supported("on_reserve_now")) }
    }

    /// `SendLocalList` (both versions).
    fn on_send_local_list(
        &self,
        _params: SendLocalListParams,
    ) -> impl Future<Output = Result<GenericStatusResult, CallError>> + Send {
        async { Err(not_supported("on_send_local_list")) }
    }

    /// `UpdateFirmware` (both versions).
    fn on_update_firmware(
        &self,
        _params: UpdateFirmwareParams,
    ) -> impl Future<Output = Result<GenericStatusResult, CallError>> + Send {
        async { Err(not_supported("on_update_firmware")) }
    }

    /// `DataTransfer` initiated by the CSMS (both versions).
    fn on_data_transfer(
        &self,
        _params: DataTransferParams,
    ) -> impl Future<Output = Result<DataTransferResult, CallError>> + Send {
        async { Err(not_supported("on_data_transfer")) }
    }
}
