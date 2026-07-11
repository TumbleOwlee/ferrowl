//! OCPP 2.1 action set (90 actions), generated via `define_ocpp_version!`.
//!
//! Table rows are derived 1:1 from `rust_ocpp::v2_1::messages`. The variant name is the
//! spec wire action name; module paths use rust_ocpp's own (snake_case) spelling. The
//! trailing flag marks whether the request type derives `validator::Validate` (the 2.1
//! codegen derives it on most requests, unlike the hand-written 2.0.1 set).
//!
//! 2.1 is a strict superset of 2.0.1: the 64 shared actions carry over, plus 26 new ones
//! (tariffs, DER control, battery swap, periodic event streams, dynamic schedules, …).
//! The one-way streaming datagram `NotifyPeriodicEventStream` has no Request/Response pair
//! and so cannot be a table row; it is intentionally omitted.

use crate::action::macros::define_ocpp_version;

define_ocpp_version! {
    V2_1, "ocpp2.1",
    cs = [
        Authorize, BootNotification, ClearedChargingLimit, DataTransfer,
        FirmwareStatusNotification, Get15118EVCertificate, GetCertificateStatus, Heartbeat,
        LogStatusNotification, MeterValues, NotifyChargingLimit, NotifyCustomerInformation,
        NotifyDisplayMessages, NotifyEVChargingNeeds, NotifyEVChargingSchedule, NotifyEvent,
        NotifyMonitoringReport, NotifyReport, PublishFirmwareStatusNotification,
        ReportChargingProfiles, ReservationStatusUpdate, SecurityEventNotification, SignCertificate,
        StatusNotification, TransactionEvent,
        // 2.1 additions originated by the Charging Station (CS→CSMS).
        BatterySwap, GetCertificateChainStatus, NotifyDERAlarm, NotifyDERStartStop,
        NotifyPriorityCharging, NotifySettlement, NotifyWebPaymentStarted, OpenPeriodicEventStream,
        ClosePeriodicEventStream, PullDynamicScheduleUpdate, ReportDERControl, VatNumberValidation,
    ];
    // CSMS-originated actions, tagged by evse/evseId presence in the rust_ocpp request (the
    // connector target). Nested-optional evse fields are treated as None — charge-point-wide.
    csms = [
        CancelReservation => None,
        CertificateSigned => None,
        ChangeAvailability => Optional,
        ClearCache => None,
        ClearChargingProfile => None,
        ClearDisplayMessage => None,
        ClearVariableMonitoring => None,
        CostUpdated => None,
        CustomerInformation => None,
        DeleteCertificate => None,
        GetBaseReport => None,
        GetChargingProfiles => Optional,
        GetCompositeSchedule => Required,
        GetDisplayMessages => None,
        GetInstalledCertificateIds => None,
        GetLocalListVersion => None,
        GetLog => None,
        GetMonitoringReport => None,
        GetReport => None,
        GetTransactionStatus => None,
        GetVariables => None,
        InstallCertificate => None,
        PublishFirmware => None,
        RequestStartTransaction => Optional,
        RequestStopTransaction => None,
        ReserveNow => Optional,
        Reset => Optional,
        SendLocalList => None,
        SetChargingProfile => Required,
        SetDisplayMessage => None,
        SetMonitoringBase => None,
        SetMonitoringLevel => None,
        SetNetworkProfile => None,
        SetVariableMonitoring => None,
        SetVariables => None,
        TriggerMessage => Optional,
        UnlockConnector => Required,
        UnpublishFirmware => None,
        UpdateFirmware => None,
        // 2.1 additions originated by the CSMS (CSMS→CS).
        AFRRSignal => None,
        AdjustPeriodicEventStream => None,
        ChangeTransactionTariff => None,
        ClearDERControl => None,
        ClearTariffs => Optional,
        GetDERControl => None,
        GetPeriodicEventStream => None,
        GetTariffs => Required,
        NotifyAllowedEnergyTransfer => None,
        RequestBatterySwap => None,
        SetDERControl => None,
        SetDefaultTariff => Required,
        UpdateDynamicSchedule => None,
        UsePriorityCharging => None,
    ];
    AFRRSignal => ::rust_ocpp::v2_1::messages::afrr_signal::AFRRSignalRequest, ::rust_ocpp::v2_1::messages::afrr_signal::AFRRSignalResponse, no ;
    AdjustPeriodicEventStream => ::rust_ocpp::v2_1::messages::adjust_periodic_event_stream::AdjustPeriodicEventStreamRequest, ::rust_ocpp::v2_1::messages::adjust_periodic_event_stream::AdjustPeriodicEventStreamResponse, yes ;
    Authorize => ::rust_ocpp::v2_1::messages::authorize::AuthorizeRequest, ::rust_ocpp::v2_1::messages::authorize::AuthorizeResponse, yes ;
    BatterySwap => ::rust_ocpp::v2_1::messages::battery_swap::BatterySwapRequest, ::rust_ocpp::v2_1::messages::battery_swap::BatterySwapResponse, yes ;
    BootNotification => ::rust_ocpp::v2_1::messages::boot_notification::BootNotificationRequest, ::rust_ocpp::v2_1::messages::boot_notification::BootNotificationResponse, no ;
    CancelReservation => ::rust_ocpp::v2_1::messages::cancel_reservation::CancelReservationRequest, ::rust_ocpp::v2_1::messages::cancel_reservation::CancelReservationResponse, yes ;
    CertificateSigned => ::rust_ocpp::v2_1::messages::certificate_signed::CertificateSignedRequest, ::rust_ocpp::v2_1::messages::certificate_signed::CertificateSignedResponse, yes ;
    ChangeAvailability => ::rust_ocpp::v2_1::messages::change_availability::ChangeAvailabilityRequest, ::rust_ocpp::v2_1::messages::change_availability::ChangeAvailabilityResponse, no ;
    ChangeTransactionTariff => ::rust_ocpp::v2_1::messages::change_transaction_tariff::ChangeTransactionTariffRequest, ::rust_ocpp::v2_1::messages::change_transaction_tariff::ChangeTransactionTariffResponse, yes ;
    ClearCache => ::rust_ocpp::v2_1::messages::clear_cache::ClearCacheRequest, ::rust_ocpp::v2_1::messages::clear_cache::ClearCacheResponse, no ;
    ClearChargingProfile => ::rust_ocpp::v2_1::messages::clear_charging_profile::ClearChargingProfileRequest, ::rust_ocpp::v2_1::messages::clear_charging_profile::ClearChargingProfileResponse, no ;
    ClearDERControl => ::rust_ocpp::v2_1::messages::clear_der_control::ClearDERControlRequest, ::rust_ocpp::v2_1::messages::clear_der_control::ClearDERControlResponse, yes ;
    ClearDisplayMessage => ::rust_ocpp::v2_1::messages::clear_display_message::ClearDisplayMessageRequest, ::rust_ocpp::v2_1::messages::clear_display_message::ClearDisplayMessageResponse, yes ;
    ClearTariffs => ::rust_ocpp::v2_1::messages::clear_tariffs::ClearTariffsRequest, ::rust_ocpp::v2_1::messages::clear_tariffs::ClearTariffsResponse, yes ;
    ClearVariableMonitoring => ::rust_ocpp::v2_1::messages::clear_variable_monitoring::ClearVariableMonitoringRequest, ::rust_ocpp::v2_1::messages::clear_variable_monitoring::ClearVariableMonitoringResponse, yes ;
    ClearedChargingLimit => ::rust_ocpp::v2_1::messages::cleared_charging_limit::ClearedChargingLimitRequest, ::rust_ocpp::v2_1::messages::cleared_charging_limit::ClearedChargingLimitResponse, yes ;
    ClosePeriodicEventStream => ::rust_ocpp::v2_1::messages::close_periodic_event_stream::ClosePeriodicEventStreamRequest, ::rust_ocpp::v2_1::messages::close_periodic_event_stream::ClosePeriodicEventStreamResponse, yes ;
    CostUpdated => ::rust_ocpp::v2_1::messages::cost_updated::CostUpdatedRequest, ::rust_ocpp::v2_1::messages::cost_updated::CostUpdatedResponse, yes ;
    CustomerInformation => ::rust_ocpp::v2_1::messages::customer_information::CustomerInformationRequest, ::rust_ocpp::v2_1::messages::customer_information::CustomerInformationResponse, yes ;
    DataTransfer => ::rust_ocpp::v2_1::messages::datatransfer::DataTransferRequest, ::rust_ocpp::v2_1::messages::datatransfer::DataTransferResponse, yes ;
    DeleteCertificate => ::rust_ocpp::v2_1::messages::delete_certificate::DeleteCertificateRequest, ::rust_ocpp::v2_1::messages::delete_certificate::DeleteCertificateResponse, no ;
    FirmwareStatusNotification => ::rust_ocpp::v2_1::messages::firmware_status_notification::FirmwareStatusNotificationRequest, ::rust_ocpp::v2_1::messages::firmware_status_notification::FirmwareStatusNotificationResponse, no ;
    Get15118EVCertificate => ::rust_ocpp::v2_1::messages::get_15118ev_certificate::Get15118EVCertificateRequest, ::rust_ocpp::v2_1::messages::get_15118ev_certificate::Get15118EVCertificateResponse, yes ;
    GetBaseReport => ::rust_ocpp::v2_1::messages::get_base_report::GetBaseReportRequest, ::rust_ocpp::v2_1::messages::get_base_report::GetBaseReportResponse, no ;
    GetCertificateChainStatus => ::rust_ocpp::v2_1::messages::get_certificate_chain_status::GetCertificateChainStatusRequest, ::rust_ocpp::v2_1::messages::get_certificate_chain_status::GetCertificateChainStatusResponse, yes ;
    GetCertificateStatus => ::rust_ocpp::v2_1::messages::get_certificate_status::GetCertificateStatusRequest, ::rust_ocpp::v2_1::messages::get_certificate_status::GetCertificateStatusResponse, no ;
    GetChargingProfiles => ::rust_ocpp::v2_1::messages::get_charging_profiles::GetChargingProfilesRequest, ::rust_ocpp::v2_1::messages::get_charging_profiles::GetChargingProfilesResponse, yes ;
    GetCompositeSchedule => ::rust_ocpp::v2_1::messages::get_composite_schedule::GetCompositeScheduleRequest, ::rust_ocpp::v2_1::messages::get_composite_schedule::GetCompositeScheduleResponse, yes ;
    GetDERControl => ::rust_ocpp::v2_1::messages::get_der_control::GetDERControlRequest, ::rust_ocpp::v2_1::messages::get_der_control::GetDERControlResponse, yes ;
    GetDisplayMessages => ::rust_ocpp::v2_1::messages::get_display_message::GetDisplayMessagesRequest, ::rust_ocpp::v2_1::messages::get_display_message::GetDisplayMessagesResponse, yes ;
    GetInstalledCertificateIds => ::rust_ocpp::v2_1::messages::get_installed_certificate_ids::GetInstalledCertificateIdsRequest, ::rust_ocpp::v2_1::messages::get_installed_certificate_ids::GetInstalledCertificateIdsResponse, yes ;
    GetLocalListVersion => ::rust_ocpp::v2_1::messages::get_local_list_version::GetLocalListVersionRequest, ::rust_ocpp::v2_1::messages::get_local_list_version::GetLocalListVersionResponse, no ;
    GetLog => ::rust_ocpp::v2_1::messages::get_log::GetLogRequest, ::rust_ocpp::v2_1::messages::get_log::GetLogResponse, yes ;
    GetMonitoringReport => ::rust_ocpp::v2_1::messages::get_monitoring_report::GetMonitoringReportRequest, ::rust_ocpp::v2_1::messages::get_monitoring_report::GetMonitoringReportResponse, yes ;
    GetPeriodicEventStream => ::rust_ocpp::v2_1::messages::get_periodic_event_stream::GetPeriodicEventStreamRequest, ::rust_ocpp::v2_1::messages::get_periodic_event_stream::GetPeriodicEventStreamResponse, no ;
    GetReport => ::rust_ocpp::v2_1::messages::get_report::GetReportRequest, ::rust_ocpp::v2_1::messages::get_report::GetReportResponse, yes ;
    GetTariffs => ::rust_ocpp::v2_1::messages::get_tariffs::GetTariffsRequest, ::rust_ocpp::v2_1::messages::get_tariffs::GetTariffsResponse, yes ;
    GetTransactionStatus => ::rust_ocpp::v2_1::messages::get_transaction_status::GetTransactionStatusRequest, ::rust_ocpp::v2_1::messages::get_transaction_status::GetTransactionStatusResponse, yes ;
    GetVariables => ::rust_ocpp::v2_1::messages::get_variables::GetVariablesRequest, ::rust_ocpp::v2_1::messages::get_variables::GetVariablesResponse, yes ;
    Heartbeat => ::rust_ocpp::v2_1::messages::heartbeat::HeartbeatRequest, ::rust_ocpp::v2_1::messages::heartbeat::HeartbeatResponse, no ;
    InstallCertificate => ::rust_ocpp::v2_1::messages::install_certificate::InstallCertificateRequest, ::rust_ocpp::v2_1::messages::install_certificate::InstallCertificateResponse, yes ;
    LogStatusNotification => ::rust_ocpp::v2_1::messages::log_status_notification::LogStatusNotificationRequest, ::rust_ocpp::v2_1::messages::log_status_notification::LogStatusNotificationResponse, no ;
    MeterValues => ::rust_ocpp::v2_1::messages::meter_values::MeterValuesRequest, ::rust_ocpp::v2_1::messages::meter_values::MeterValuesResponse, yes ;
    NotifyAllowedEnergyTransfer => ::rust_ocpp::v2_1::messages::notify_allowed_energy_transfer::NotifyAllowedEnergyTransferRequest, ::rust_ocpp::v2_1::messages::notify_allowed_energy_transfer::NotifyAllowedEnergyTransferResponse, yes ;
    NotifyChargingLimit => ::rust_ocpp::v2_1::messages::notify_charging_limit::NotifyChargingLimitRequest, ::rust_ocpp::v2_1::messages::notify_charging_limit::NotifyChargingLimitResponse, yes ;
    NotifyCustomerInformation => ::rust_ocpp::v2_1::messages::notify_customer_information::NotifyCustomerInformationRequest, ::rust_ocpp::v2_1::messages::notify_customer_information::NotifyCustomerInformationResponse, yes ;
    NotifyDERAlarm => ::rust_ocpp::v2_1::messages::notify_der_alarm::NotifyDERAlarmRequest, ::rust_ocpp::v2_1::messages::notify_der_alarm::NotifyDERAlarmResponse, yes ;
    NotifyDERStartStop => ::rust_ocpp::v2_1::messages::notify_der_start_stop::NotifyDERStartStopRequest, ::rust_ocpp::v2_1::messages::notify_der_start_stop::NotifyDERStartStopResponse, yes ;
    NotifyDisplayMessages => ::rust_ocpp::v2_1::messages::notify_display_messages::NotifyDisplayMessagesRequest, ::rust_ocpp::v2_1::messages::notify_display_messages::NotifyDisplayMessagesResponse, yes ;
    NotifyEVChargingNeeds => ::rust_ocpp::v2_1::messages::notify_ev_charging_needs::NotifyEVChargingNeedsRequest, ::rust_ocpp::v2_1::messages::notify_ev_charging_needs::NotifyEVChargingNeedsResponse, yes ;
    NotifyEVChargingSchedule => ::rust_ocpp::v2_1::messages::notify_ev_charging_schedule::NotifyEVChargingScheduleRequest, ::rust_ocpp::v2_1::messages::notify_ev_charging_schedule::NotifyEVChargingScheduleResponse, yes ;
    NotifyEvent => ::rust_ocpp::v2_1::messages::notify_event::NotifyEventRequest, ::rust_ocpp::v2_1::messages::notify_event::NotifyEventResponse, yes ;
    NotifyMonitoringReport => ::rust_ocpp::v2_1::messages::notify_monitoring_report::NotifyMonitoringReportRequest, ::rust_ocpp::v2_1::messages::notify_monitoring_report::NotifyMonitoringReportResponse, yes ;
    NotifyPriorityCharging => ::rust_ocpp::v2_1::messages::notify_priority_charging::NotifyPriorityChargingRequest, ::rust_ocpp::v2_1::messages::notify_priority_charging::NotifyPriorityChargingResponse, yes ;
    NotifyReport => ::rust_ocpp::v2_1::messages::notify_report::NotifyReportRequest, ::rust_ocpp::v2_1::messages::notify_report::NotifyReportResponse, yes ;
    NotifySettlement => ::rust_ocpp::v2_1::messages::notify_settlement::NotifySettlementRequest, ::rust_ocpp::v2_1::messages::notify_settlement::NotifySettlementResponse, yes ;
    NotifyWebPaymentStarted => ::rust_ocpp::v2_1::messages::notify_web_payment_started::NotifyWebPaymentStartedRequest, ::rust_ocpp::v2_1::messages::notify_web_payment_started::NotifyWebPaymentStartedResponse, yes ;
    OpenPeriodicEventStream => ::rust_ocpp::v2_1::messages::open_periodic_event_stream::OpenPeriodicEventStreamRequest, ::rust_ocpp::v2_1::messages::open_periodic_event_stream::OpenPeriodicEventStreamResponse, no ;
    PublishFirmwareStatusNotification => ::rust_ocpp::v2_1::messages::publish_firmware_status_notification::PublishFirmwareStatusNotificationRequest, ::rust_ocpp::v2_1::messages::publish_firmware_status_notification::PublishFirmwareStatusNotificationResponse, yes ;
    PublishFirmware => ::rust_ocpp::v2_1::messages::publish_firmware::PublishFirmwareRequest, ::rust_ocpp::v2_1::messages::publish_firmware::PublishFirmwareResponse, yes ;
    PullDynamicScheduleUpdate => ::rust_ocpp::v2_1::messages::pull_dynamic_schedule_update::PullDynamicScheduleUpdateRequest, ::rust_ocpp::v2_1::messages::pull_dynamic_schedule_update::PullDynamicScheduleUpdateResponse, no ;
    ReportChargingProfiles => ::rust_ocpp::v2_1::messages::report_charging_profiles::ReportChargingProfilesRequest, ::rust_ocpp::v2_1::messages::report_charging_profiles::ReportChargingProfilesResponse, yes ;
    ReportDERControl => ::rust_ocpp::v2_1::messages::report_der_control::ReportDERControlRequest, ::rust_ocpp::v2_1::messages::report_der_control::ReportDERControlResponse, yes ;
    RequestBatterySwap => ::rust_ocpp::v2_1::messages::request_battery_swap::RequestBatterySwapRequest, ::rust_ocpp::v2_1::messages::request_battery_swap::RequestBatterySwapResponse, no ;
    RequestStartTransaction => ::rust_ocpp::v2_1::messages::request_start_transaction::RequestStartTransactionRequest, ::rust_ocpp::v2_1::messages::request_start_transaction::RequestStartTransactionResponse, yes ;
    RequestStopTransaction => ::rust_ocpp::v2_1::messages::request_stop_transaction::RequestStopTransactionRequest, ::rust_ocpp::v2_1::messages::request_stop_transaction::RequestStopTransactionResponse, yes ;
    ReservationStatusUpdate => ::rust_ocpp::v2_1::messages::reservation_status_update::ReservationStatusUpdateRequest, ::rust_ocpp::v2_1::messages::reservation_status_update::ReservationStatusUpdateResponse, yes ;
    ReserveNow => ::rust_ocpp::v2_1::messages::reserve_now::ReserveNowRequest, ::rust_ocpp::v2_1::messages::reserve_now::ReserveNowResponse, yes ;
    Reset => ::rust_ocpp::v2_1::messages::reset::ResetRequest, ::rust_ocpp::v2_1::messages::reset::ResetResponse, yes ;
    SecurityEventNotification => ::rust_ocpp::v2_1::messages::security_event_notification::SecurityEventNotificationRequest, ::rust_ocpp::v2_1::messages::security_event_notification::SecurityEventNotificationResponse, yes ;
    SendLocalList => ::rust_ocpp::v2_1::messages::send_local_list::SendLocalListRequest, ::rust_ocpp::v2_1::messages::send_local_list::SendLocalListResponse, yes ;
    SetChargingProfile => ::rust_ocpp::v2_1::messages::set_charging_profile::SetChargingProfileRequest, ::rust_ocpp::v2_1::messages::set_charging_profile::SetChargingProfileResponse, yes ;
    SetDERControl => ::rust_ocpp::v2_1::messages::set_der_control::SetDERControlRequest, ::rust_ocpp::v2_1::messages::set_der_control::SetDERControlResponse, yes ;
    SetDefaultTariff => ::rust_ocpp::v2_1::messages::set_default_tariff::SetDefaultTariffRequest, ::rust_ocpp::v2_1::messages::set_default_tariff::SetDefaultTariffResponse, yes ;
    SetDisplayMessage => ::rust_ocpp::v2_1::messages::set_display_message::SetDisplayMessageRequest, ::rust_ocpp::v2_1::messages::set_display_message::SetDisplayMessageResponse, no ;
    SetMonitoringBase => ::rust_ocpp::v2_1::messages::set_monitoring_base::SetMonitoringBaseRequest, ::rust_ocpp::v2_1::messages::set_monitoring_base::SetMonitoringBaseResponse, no ;
    SetMonitoringLevel => ::rust_ocpp::v2_1::messages::set_monitoring_level::SetMonitoringLevelRequest, ::rust_ocpp::v2_1::messages::set_monitoring_level::SetMonitoringLevelResponse, yes ;
    SetNetworkProfile => ::rust_ocpp::v2_1::messages::set_network_profile::SetNetworkProfileRequest, ::rust_ocpp::v2_1::messages::set_network_profile::SetNetworkProfileResponse, no ;
    SetVariableMonitoring => ::rust_ocpp::v2_1::messages::set_variable_monitoring::SetVariableMonitoringRequest, ::rust_ocpp::v2_1::messages::set_variable_monitoring::SetVariableMonitoringResponse, yes ;
    SetVariables => ::rust_ocpp::v2_1::messages::set_variables::SetVariablesRequest, ::rust_ocpp::v2_1::messages::set_variables::SetVariablesResponse, yes ;
    SignCertificate => ::rust_ocpp::v2_1::messages::sign_certificate::SignCertificateRequest, ::rust_ocpp::v2_1::messages::sign_certificate::SignCertificateResponse, yes ;
    StatusNotification => ::rust_ocpp::v2_1::messages::status_notification::StatusNotificationRequest, ::rust_ocpp::v2_1::messages::status_notification::StatusNotificationResponse, yes ;
    TransactionEvent => ::rust_ocpp::v2_1::messages::transaction_event::TransactionEventRequest, ::rust_ocpp::v2_1::messages::transaction_event::TransactionEventResponse, yes ;
    TriggerMessage => ::rust_ocpp::v2_1::messages::trigger_message::TriggerMessageRequest, ::rust_ocpp::v2_1::messages::trigger_message::TriggerMessageResponse, yes ;
    UnlockConnector => ::rust_ocpp::v2_1::messages::unlock_connector::UnlockConnectorRequest, ::rust_ocpp::v2_1::messages::unlock_connector::UnlockConnectorResponse, yes ;
    UnpublishFirmware => ::rust_ocpp::v2_1::messages::unpublish_firmware::UnpublishFirmwareRequest, ::rust_ocpp::v2_1::messages::unpublish_firmware::UnpublishFirmwareResponse, yes ;
    UpdateDynamicSchedule => ::rust_ocpp::v2_1::messages::update_dynamic_schedule::UpdateDynamicScheduleRequest, ::rust_ocpp::v2_1::messages::update_dynamic_schedule::UpdateDynamicScheduleResponse, no ;
    UpdateFirmware => ::rust_ocpp::v2_1::messages::update_firmware::UpdateFirmwareRequest, ::rust_ocpp::v2_1::messages::update_firmware::UpdateFirmwareResponse, yes ;
    UsePriorityCharging => ::rust_ocpp::v2_1::messages::use_priority_charging::UsePriorityChargingRequest, ::rust_ocpp::v2_1::messages::use_priority_charging::UsePriorityChargingResponse, yes ;
    VatNumberValidation => ::rust_ocpp::v2_1::messages::vat_number_validation::VatNumberValidationRequest, ::rust_ocpp::v2_1::messages::vat_number_validation::VatNumberValidationResponse, yes ;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Version;

    #[test]
    fn ut_subprotocol() {
        assert_eq!(V2_1::subprotocol(), "ocpp2.1");
    }

    #[test]
    fn ut_csms_actions_partition_and_scopes() {
        use crate::action::ConnectorScope::*;
        let cs: std::collections::HashSet<_> = V2_1::cs_actions().iter().copied().collect();
        let csms: std::collections::HashSet<_> =
            V2_1::csms_actions().iter().map(|(n, _)| *n).collect();
        // cs and csms partition the full action set: disjoint and together complete.
        assert!(cs.is_disjoint(&csms));
        assert_eq!(cs.len() + csms.len(), V2_1::action_names().len());
        for n in V2_1::action_names() {
            assert!(cs.contains(n) || csms.contains(n), "{n} uncategorized");
        }
        let scope = |name: &str| {
            V2_1::csms_actions()
                .iter()
                .find(|(n, _)| *n == name)
                .unwrap()
                .1
        };
        assert_eq!(scope("Reset"), Optional);
        assert_eq!(scope("UnlockConnector"), Required);
        assert_eq!(scope("GetTariffs"), Required);
        assert_eq!(scope("SetDERControl"), None);
    }

    #[test]
    fn ut_round_trip_boot_notification() {
        use ::rust_ocpp::v2_1::datatypes::charging_station::ChargingStationType;
        use ::rust_ocpp::v2_1::enumerations::boot_reason::BootReasonEnumType;
        use ::rust_ocpp::v2_1::enumerations::registration_status::RegistrationStatusEnumType;
        use ::rust_ocpp::v2_1::messages::boot_notification::BootNotificationResponse;

        let req = ::rust_ocpp::v2_1::messages::boot_notification::BootNotificationRequest {
            charging_station: ChargingStationType {
                model: "Model-X".into(),
                vendor_name: "Acme".into(),
                ..Default::default()
            },
            reason: BootReasonEnumType::PowerUp,
            ..Default::default()
        };
        let action = Action::BootNotification(Box::new(req));
        let payload = V2_1::encode_action(&action).unwrap();
        let decoded = V2_1::decode_call("BootNotification", payload).unwrap();
        assert_eq!(decoded, action);

        let resp = BootNotificationResponse {
            interval: 300,
            status: RegistrationStatusEnumType::Accepted,
            ..Default::default()
        };
        let response = Response::BootNotification(Box::new(resp));
        let payload = V2_1::encode_response(&response).unwrap();
        let decoded = V2_1::decode_result(&action, payload).unwrap();
        assert_eq!(decoded, response);
    }

    #[test]
    fn ut_round_trip_authorize() {
        use ::rust_ocpp::v2_1::datatypes::id_token::IdTokenType;
        use ::rust_ocpp::v2_1::messages::authorize::AuthorizeRequest;

        let req = AuthorizeRequest {
            id_token: IdTokenType {
                id_token: "DEADBEEF".into(),
                kind: "ISO14443".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let action = Action::Authorize(Box::new(req));
        assert!(V2_1::validate(&action).is_ok());
        let payload = V2_1::encode_action(&action).unwrap();
        let decoded = V2_1::decode_call("Authorize", payload).unwrap();
        assert_eq!(decoded, action);
    }

    #[test]
    fn ut_round_trip_meter_values() {
        use ::rust_ocpp::v2_1::datatypes::meter_value::MeterValueType;
        use ::rust_ocpp::v2_1::datatypes::sampled_value::SampledValueType;
        use ::rust_ocpp::v2_1::messages::meter_values::MeterValuesRequest;

        let sampled = SampledValueType::default();
        let meter_value = MeterValueType {
            sampled_value: vec![sampled],
            ..Default::default()
        };
        let req = MeterValuesRequest {
            evse_id: 1,
            meter_value: vec![meter_value],
            ..Default::default()
        };
        let action = Action::MeterValues(Box::new(req));
        assert!(V2_1::validate(&action).is_ok());
        let payload = V2_1::encode_action(&action).unwrap();
        let decoded = V2_1::decode_call("MeterValues", payload).unwrap();
        assert_eq!(decoded, action);
    }

    #[test]
    fn ut_validate_rejects_meter_values_empty_meter_value() {
        use ::rust_ocpp::v2_1::messages::meter_values::MeterValuesRequest;

        // `meter_value` requires at least 1 entry per `MeterValuesRequest`'s `Validate` impl —
        // 2.1's codegen derives `Validate` on this request, unlike hand-written 2.0.1.
        let req = MeterValuesRequest {
            evse_id: 1,
            meter_value: vec![],
            ..Default::default()
        };
        let action = Action::MeterValues(Box::new(req));
        assert!(V2_1::validate(&action).is_err());
    }

    #[test]
    fn ut_validate_rejects_authorize_certificate_too_long() {
        use ::rust_ocpp::v2_1::datatypes::id_token::IdTokenType;
        use ::rust_ocpp::v2_1::messages::authorize::AuthorizeRequest;

        // `certificate` is capped at 10000 chars by `AuthorizeRequest`'s `Validate` impl.
        let req = AuthorizeRequest {
            certificate: Some("A".repeat(10001)),
            id_token: IdTokenType::default(),
            ..Default::default()
        };
        let action = Action::Authorize(Box::new(req));
        assert!(V2_1::validate(&action).is_err());
    }
}
