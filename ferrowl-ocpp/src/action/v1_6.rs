//! OCPP 1.6 action set (28 actions), generated via `define_ocpp_version!`.
//!
//! Table rows are derived 1:1 from `rust_ocpp::v1_6::messages`. The variant name is the
//! spec wire action name; module paths use rust_ocpp's own (snake_case) spelling. The
//! trailing flag marks whether the request type derives `validator::Validate`.

use crate::action::macros::define_ocpp_version;

define_ocpp_version! {
    V1_6, "ocpp1.6",
    Authorize => ::rust_ocpp::v1_6::messages::authorize::AuthorizeRequest, ::rust_ocpp::v1_6::messages::authorize::AuthorizeResponse, yes ;
    BootNotification => ::rust_ocpp::v1_6::messages::boot_notification::BootNotificationRequest, ::rust_ocpp::v1_6::messages::boot_notification::BootNotificationResponse, yes ;
    CancelReservation => ::rust_ocpp::v1_6::messages::cancel_reservation::CancelReservationRequest, ::rust_ocpp::v1_6::messages::cancel_reservation::CancelReservationResponse, no ;
    ChangeAvailability => ::rust_ocpp::v1_6::messages::change_availability::ChangeAvailabilityRequest, ::rust_ocpp::v1_6::messages::change_availability::ChangeAvailabilityResponse, no ;
    ChangeConfiguration => ::rust_ocpp::v1_6::messages::change_configuration::ChangeConfigurationRequest, ::rust_ocpp::v1_6::messages::change_configuration::ChangeConfigurationResponse, yes ;
    ClearCache => ::rust_ocpp::v1_6::messages::clear_cache::ClearCacheRequest, ::rust_ocpp::v1_6::messages::clear_cache::ClearCacheResponse, no ;
    ClearChargingProfile => ::rust_ocpp::v1_6::messages::clear_charging_profile::ClearChargingProfileRequest, ::rust_ocpp::v1_6::messages::clear_charging_profile::ClearChargingProfileResponse, no ;
    DataTransfer => ::rust_ocpp::v1_6::messages::data_transfer::DataTransferRequest, ::rust_ocpp::v1_6::messages::data_transfer::DataTransferResponse, yes ;
    DiagnosticsStatusNotification => ::rust_ocpp::v1_6::messages::diagnostics_status_notification::DiagnosticsStatusNotificationRequest, ::rust_ocpp::v1_6::messages::diagnostics_status_notification::DiagnosticsStatusNotificationResponse, no ;
    FirmwareStatusNotification => ::rust_ocpp::v1_6::messages::firmware_status_notification::FirmwareStatusNotificationRequest, ::rust_ocpp::v1_6::messages::firmware_status_notification::FirmwareStatusNotificationResponse, no ;
    GetCompositeSchedule => ::rust_ocpp::v1_6::messages::get_composite_schedule::GetCompositeScheduleRequest, ::rust_ocpp::v1_6::messages::get_composite_schedule::GetCompositeScheduleResponse, no ;
    GetConfiguration => ::rust_ocpp::v1_6::messages::get_configuration::GetConfigurationRequest, ::rust_ocpp::v1_6::messages::get_configuration::GetConfigurationResponse, yes ;
    GetDiagnostics => ::rust_ocpp::v1_6::messages::get_diagnostics::GetDiagnosticsRequest, ::rust_ocpp::v1_6::messages::get_diagnostics::GetDiagnosticsResponse, yes ;
    GetLocalListVersion => ::rust_ocpp::v1_6::messages::get_local_list_version::GetLocalListVersionRequest, ::rust_ocpp::v1_6::messages::get_local_list_version::GetLocalListVersionResponse, no ;
    Heartbeat => ::rust_ocpp::v1_6::messages::heart_beat::HeartbeatRequest, ::rust_ocpp::v1_6::messages::heart_beat::HeartbeatResponse, yes ;
    MeterValues => ::rust_ocpp::v1_6::messages::meter_values::MeterValuesRequest, ::rust_ocpp::v1_6::messages::meter_values::MeterValuesResponse, no ;
    RemoteStartTransaction => ::rust_ocpp::v1_6::messages::remote_start_transaction::RemoteStartTransactionRequest, ::rust_ocpp::v1_6::messages::remote_start_transaction::RemoteStartTransactionResponse, yes ;
    RemoteStopTransaction => ::rust_ocpp::v1_6::messages::remote_stop_transaction::RemoteStopTransactionRequest, ::rust_ocpp::v1_6::messages::remote_stop_transaction::RemoteStopTransactionResponse, no ;
    ReserveNow => ::rust_ocpp::v1_6::messages::reserve_now::ReserveNowRequest, ::rust_ocpp::v1_6::messages::reserve_now::ReserveNowResponse, yes ;
    Reset => ::rust_ocpp::v1_6::messages::reset::ResetRequest, ::rust_ocpp::v1_6::messages::reset::ResetResponse, no ;
    SendLocalList => ::rust_ocpp::v1_6::messages::send_local_list::SendLocalListRequest, ::rust_ocpp::v1_6::messages::send_local_list::SendLocalListResponse, no ;
    SetChargingProfile => ::rust_ocpp::v1_6::messages::set_charging_profile::SetChargingProfileRequest, ::rust_ocpp::v1_6::messages::set_charging_profile::SetChargingProfileResponse, no ;
    StartTransaction => ::rust_ocpp::v1_6::messages::start_transaction::StartTransactionRequest, ::rust_ocpp::v1_6::messages::start_transaction::StartTransactionResponse, yes ;
    StatusNotification => ::rust_ocpp::v1_6::messages::status_notification::StatusNotificationRequest, ::rust_ocpp::v1_6::messages::status_notification::StatusNotificationResponse, yes ;
    StopTransaction => ::rust_ocpp::v1_6::messages::stop_transaction::StopTransactionRequest, ::rust_ocpp::v1_6::messages::stop_transaction::StopTransactionResponse, yes ;
    TriggerMessage => ::rust_ocpp::v1_6::messages::trigger_message::TriggerMessageRequest, ::rust_ocpp::v1_6::messages::trigger_message::TriggerMessageResponse, no ;
    UnlockConnector => ::rust_ocpp::v1_6::messages::unlock_connector::UnlockConnectorRequest, ::rust_ocpp::v1_6::messages::unlock_connector::UnlockConnectorResponse, yes ;
    UpdateFirmware => ::rust_ocpp::v1_6::messages::update_firmware::UpdateFirmwareRequest, ::rust_ocpp::v1_6::messages::update_firmware::UpdateFirmwareResponse, yes ;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Version;
    use ::rust_ocpp::v1_6::messages::boot_notification::BootNotificationRequest;

    fn boot_req(model: &str) -> BootNotificationRequest {
        BootNotificationRequest {
            charge_point_model: model.to_owned(),
            charge_point_vendor: "Ferrowl".to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn ut_action_name_matches_wire() {
        let a = Action::BootNotification(boot_req("M"));
        assert_eq!(V1_6::action_name(&a), "BootNotification");
        assert_eq!(V1_6::subprotocol(), "ocpp1.6");
    }

    #[test]
    fn ut_call_encode_decode_round_trip() {
        let action = Action::BootNotification(boot_req("Model-1"));
        let payload = V1_6::encode_action(&action).unwrap();
        let decoded = V1_6::decode_call("BootNotification", payload).unwrap();
        assert_eq!(action, decoded);
    }

    #[test]
    fn ut_decode_result_uses_originating_action() {
        let action = Action::BootNotification(boot_req("M"));
        let resp_json = serde_json::json!({
            "currentTime": "2026-01-01T00:00:00Z",
            "interval": 300,
            "status": "Accepted"
        });
        let resp = V1_6::decode_result(&action, resp_json).unwrap();
        assert!(matches!(resp, Response::BootNotification(_)));
    }

    #[test]
    fn ut_unknown_action_errors() {
        assert!(matches!(
            V1_6::decode_call("NoSuchAction", serde_json::json!({})),
            Err(crate::error::OcppError::UnknownAction(_))
        ));
    }

    #[test]
    fn ut_validate_rejects_oversized_field() {
        let bad = Action::BootNotification(boot_req(&"x".repeat(21)));
        assert!(V1_6::validate(&bad).is_err());
        let good = Action::BootNotification(boot_req("ok"));
        assert!(V1_6::validate(&good).is_ok());
    }
}
