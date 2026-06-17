//! OCPP 2.0.1 action set (64 actions), generated via `define_ocpp_version!`.
//!
//! Table rows are derived 1:1 from `rust_ocpp::v2_0_1::messages`. The variant name is the
//! spec wire action name; module paths use rust_ocpp's own (snake_case) spelling. The
//! trailing flag marks whether the request type derives `validator::Validate`.

use crate::action::macros::define_ocpp_version;

define_ocpp_version! {
    V2_0_1, "ocpp2.0.1",
    Authorize => ::rust_ocpp::v2_0_1::messages::authorize::AuthorizeRequest, ::rust_ocpp::v2_0_1::messages::authorize::AuthorizeResponse, yes ;
    BootNotification => ::rust_ocpp::v2_0_1::messages::boot_notification::BootNotificationRequest, ::rust_ocpp::v2_0_1::messages::boot_notification::BootNotificationResponse, no ;
    CancelReservation => ::rust_ocpp::v2_0_1::messages::cancel_reservation::CancelReservationRequest, ::rust_ocpp::v2_0_1::messages::cancel_reservation::CancelReservationResponse, no ;
    CertificateSigned => ::rust_ocpp::v2_0_1::messages::certificate_signed::CertificateSignedRequest, ::rust_ocpp::v2_0_1::messages::certificate_signed::CertificateSignedResponse, yes ;
    ChangeAvailability => ::rust_ocpp::v2_0_1::messages::change_availability::ChangeAvailabilityRequest, ::rust_ocpp::v2_0_1::messages::change_availability::ChangeAvailabilityResponse, no ;
    ClearCache => ::rust_ocpp::v2_0_1::messages::clear_cache::ClearCacheRequest, ::rust_ocpp::v2_0_1::messages::clear_cache::ClearCacheResponse, no ;
    ClearChargingProfile => ::rust_ocpp::v2_0_1::messages::clear_charging_profile::ClearChargingProfileRequest, ::rust_ocpp::v2_0_1::messages::clear_charging_profile::ClearChargingProfileResponse, no ;
    ClearDisplayMessage => ::rust_ocpp::v2_0_1::messages::clear_display_message::ClearDisplayMessageRequest, ::rust_ocpp::v2_0_1::messages::clear_display_message::ClearDisplayMessageResponse, no ;
    ClearVariableMonitoring => ::rust_ocpp::v2_0_1::messages::clear_variable_monitoring::ClearVariableMonitoringRequest, ::rust_ocpp::v2_0_1::messages::clear_variable_monitoring::ClearVariableMonitoringResponse, no ;
    ClearedChargingLimit => ::rust_ocpp::v2_0_1::messages::cleared_charging_limit::ClearedChargingLimitRequest, ::rust_ocpp::v2_0_1::messages::cleared_charging_limit::ClearedChargingLimitResponse, no ;
    CostUpdated => ::rust_ocpp::v2_0_1::messages::cost_updated::CostUpdatedRequest, ::rust_ocpp::v2_0_1::messages::cost_updated::CostUpdatedResponse, yes ;
    CustomerInformation => ::rust_ocpp::v2_0_1::messages::customer_information::CustomerInformationRequest, ::rust_ocpp::v2_0_1::messages::customer_information::CustomerInformationResponse, yes ;
    DataTransfer => ::rust_ocpp::v2_0_1::messages::datatransfer::DataTransferRequest, ::rust_ocpp::v2_0_1::messages::datatransfer::DataTransferResponse, yes ;
    DeleteCertificate => ::rust_ocpp::v2_0_1::messages::delete_certificate::DeleteCertificateRequest, ::rust_ocpp::v2_0_1::messages::delete_certificate::DeleteCertificateResponse, no ;
    FirmwareStatusNotification => ::rust_ocpp::v2_0_1::messages::firmware_status_notification::FirmwareStatusNotificationRequest, ::rust_ocpp::v2_0_1::messages::firmware_status_notification::FirmwareStatusNotificationResponse, no ;
    Get15118EVCertificate => ::rust_ocpp::v2_0_1::messages::get_15118ev_certificate::Get15118EVCertificateRequest, ::rust_ocpp::v2_0_1::messages::get_15118ev_certificate::Get15118EVCertificateResponse, yes ;
    GetBaseReport => ::rust_ocpp::v2_0_1::messages::get_base_report::GetBaseReportRequest, ::rust_ocpp::v2_0_1::messages::get_base_report::GetBaseReportResponse, no ;
    GetCertificateStatus => ::rust_ocpp::v2_0_1::messages::get_certificate_status::GetCertificateStatusRequest, ::rust_ocpp::v2_0_1::messages::get_certificate_status::GetCertificateStatusResponse, no ;
    GetChargingProfiles => ::rust_ocpp::v2_0_1::messages::get_charging_profiles::GetChargingProfilesRequest, ::rust_ocpp::v2_0_1::messages::get_charging_profiles::GetChargingProfilesResponse, no ;
    GetCompositeSchedule => ::rust_ocpp::v2_0_1::messages::get_composite_schedule::GetCompositeScheduleRequest, ::rust_ocpp::v2_0_1::messages::get_composite_schedule::GetCompositeScheduleResponse, no ;
    GetDisplayMessages => ::rust_ocpp::v2_0_1::messages::get_display_message::GetDisplayMessagesRequest, ::rust_ocpp::v2_0_1::messages::get_display_message::GetDisplayMessagesResponse, no ;
    GetInstalledCertificateIds => ::rust_ocpp::v2_0_1::messages::get_installed_certificate_ids::GetInstalledCertificateIdsRequest, ::rust_ocpp::v2_0_1::messages::get_installed_certificate_ids::GetInstalledCertificateIdsResponse, no ;
    GetLocalListVersion => ::rust_ocpp::v2_0_1::messages::get_local_list_version::GetLocalListVersionRequest, ::rust_ocpp::v2_0_1::messages::get_local_list_version::GetLocalListVersionResponse, no ;
    GetLog => ::rust_ocpp::v2_0_1::messages::get_log::GetLogRequest, ::rust_ocpp::v2_0_1::messages::get_log::GetLogResponse, no ;
    GetMonitoringReport => ::rust_ocpp::v2_0_1::messages::get_monitoring_report::GetMonitoringReportRequest, ::rust_ocpp::v2_0_1::messages::get_monitoring_report::GetMonitoringReportResponse, no ;
    GetReport => ::rust_ocpp::v2_0_1::messages::get_report::GetReportRequest, ::rust_ocpp::v2_0_1::messages::get_report::GetReportResponse, yes ;
    GetTransactionStatus => ::rust_ocpp::v2_0_1::messages::get_transaction_status::GetTransactionStatusRequest, ::rust_ocpp::v2_0_1::messages::get_transaction_status::GetTransactionStatusResponse, no ;
    GetVariables => ::rust_ocpp::v2_0_1::messages::get_variables::GetVariablesRequest, ::rust_ocpp::v2_0_1::messages::get_variables::GetVariablesResponse, no ;
    Heartbeat => ::rust_ocpp::v2_0_1::messages::heartbeat::HeartbeatRequest, ::rust_ocpp::v2_0_1::messages::heartbeat::HeartbeatResponse, no ;
    InstallCertificate => ::rust_ocpp::v2_0_1::messages::install_certificate::InstallCertificateRequest, ::rust_ocpp::v2_0_1::messages::install_certificate::InstallCertificateResponse, yes ;
    LogStatusNotification => ::rust_ocpp::v2_0_1::messages::log_status_notification::LogStatusNotificationRequest, ::rust_ocpp::v2_0_1::messages::log_status_notification::LogStatusNotificationResponse, no ;
    MeterValues => ::rust_ocpp::v2_0_1::messages::meter_values::MeterValuesRequest, ::rust_ocpp::v2_0_1::messages::meter_values::MeterValuesResponse, no ;
    NotifyChargingLimit => ::rust_ocpp::v2_0_1::messages::notify_charging_limit::NotifyChargingLimitRequest, ::rust_ocpp::v2_0_1::messages::notify_charging_limit::NotifyChargingLimitResponse, no ;
    NotifyCustomerInformation => ::rust_ocpp::v2_0_1::messages::notify_customer_information::NotifyCustomerInformationRequest, ::rust_ocpp::v2_0_1::messages::notify_customer_information::NotifyCustomerInformationResponse, no ;
    NotifyDisplayMessages => ::rust_ocpp::v2_0_1::messages::notify_display_messages::NotifyDisplayMessagesRequest, ::rust_ocpp::v2_0_1::messages::notify_display_messages::NotifyDisplayMessagesResponse, no ;
    NotifyEVChargingNeeds => ::rust_ocpp::v2_0_1::messages::notify_ev_charging_needs::NotifyEVChargingNeedsRequest, ::rust_ocpp::v2_0_1::messages::notify_ev_charging_needs::NotifyEVChargingNeedsResponse, no ;
    NotifyEVChargingSchedule => ::rust_ocpp::v2_0_1::messages::notify_ev_charging_schedule::NotifyEVChargingScheduleRequest, ::rust_ocpp::v2_0_1::messages::notify_ev_charging_schedule::NotifyEVChargingScheduleResponse, no ;
    NotifyEvent => ::rust_ocpp::v2_0_1::messages::notify_event::NotifyEventRequest, ::rust_ocpp::v2_0_1::messages::notify_event::NotifyEventResponse, no ;
    NotifyMonitoringReport => ::rust_ocpp::v2_0_1::messages::notify_monitoring_report::NotifyMonitoringReportRequest, ::rust_ocpp::v2_0_1::messages::notify_monitoring_report::NotifyMonitoringReportResponse, no ;
    NotifyReport => ::rust_ocpp::v2_0_1::messages::notify_report::NotifyReportRequest, ::rust_ocpp::v2_0_1::messages::notify_report::NotifyReportResponse, no ;
    PublishFirmware => ::rust_ocpp::v2_0_1::messages::publish_firmware::PublishFirmwareRequest, ::rust_ocpp::v2_0_1::messages::publish_firmware::PublishFirmwareResponse, no ;
    PublishFirmwareStatusNotification => ::rust_ocpp::v2_0_1::messages::publish_firmware_status_notification::PublishFirmwareStatusNotificationRequest, ::rust_ocpp::v2_0_1::messages::publish_firmware_status_notification::PublishFirmwareStatusNotificationResponse, no ;
    ReportChargingProfiles => ::rust_ocpp::v2_0_1::messages::report_charging_profiles::ReportChargingProfilesRequest, ::rust_ocpp::v2_0_1::messages::report_charging_profiles::ReportChargingProfilesResponse, no ;
    RequestStartTransaction => ::rust_ocpp::v2_0_1::messages::request_start_transaction::RequestStartTransactionRequest, ::rust_ocpp::v2_0_1::messages::request_start_transaction::RequestStartTransactionResponse, no ;
    RequestStopTransaction => ::rust_ocpp::v2_0_1::messages::request_stop_transaction::RequestStopTransactionRequest, ::rust_ocpp::v2_0_1::messages::request_stop_transaction::RequestStopTransactionResponse, no ;
    ReservationStatusUpdate => ::rust_ocpp::v2_0_1::messages::reservation_status_update::ReservationStatusUpdateRequest, ::rust_ocpp::v2_0_1::messages::reservation_status_update::ReservationStatusUpdateResponse, no ;
    ReserveNow => ::rust_ocpp::v2_0_1::messages::reserve_now::ReserveNowRequest, ::rust_ocpp::v2_0_1::messages::reserve_now::ReserveNowResponse, no ;
    Reset => ::rust_ocpp::v2_0_1::messages::reset::ResetRequest, ::rust_ocpp::v2_0_1::messages::reset::ResetResponse, no ;
    SecurityEventNotification => ::rust_ocpp::v2_0_1::messages::security_event_notification::SecurityEventNotificationRequest, ::rust_ocpp::v2_0_1::messages::security_event_notification::SecurityEventNotificationResponse, no ;
    SendLocalList => ::rust_ocpp::v2_0_1::messages::send_local_list::SendLocalListRequest, ::rust_ocpp::v2_0_1::messages::send_local_list::SendLocalListResponse, no ;
    SetChargingProfile => ::rust_ocpp::v2_0_1::messages::set_charging_profile::SetChargingProfileRequest, ::rust_ocpp::v2_0_1::messages::set_charging_profile::SetChargingProfileResponse, no ;
    SetDisplayMessage => ::rust_ocpp::v2_0_1::messages::set_display_message::SetDisplayMessageRequest, ::rust_ocpp::v2_0_1::messages::set_display_message::SetDisplayMessageResponse, no ;
    SetMonitoringBase => ::rust_ocpp::v2_0_1::messages::set_monitoring_base::SetMonitoringBaseRequest, ::rust_ocpp::v2_0_1::messages::set_monitoring_base::SetMonitoringBaseResponse, no ;
    SetMonitoringLevel => ::rust_ocpp::v2_0_1::messages::set_monitoring_level::SetMonitoringLevelRequest, ::rust_ocpp::v2_0_1::messages::set_monitoring_level::SetMonitoringLevelResponse, no ;
    SetNetworkProfile => ::rust_ocpp::v2_0_1::messages::set_network_profile::SetNetworkProfileRequest, ::rust_ocpp::v2_0_1::messages::set_network_profile::SetNetworkProfileResponse, no ;
    SetVariableMonitoring => ::rust_ocpp::v2_0_1::messages::set_variable_monitoring::SetVariableMonitoringRequest, ::rust_ocpp::v2_0_1::messages::set_variable_monitoring::SetVariableMonitoringResponse, no ;
    SetVariables => ::rust_ocpp::v2_0_1::messages::set_variables::SetVariablesRequest, ::rust_ocpp::v2_0_1::messages::set_variables::SetVariablesResponse, no ;
    SignCertificate => ::rust_ocpp::v2_0_1::messages::sign_certificate::SignCertificateRequest, ::rust_ocpp::v2_0_1::messages::sign_certificate::SignCertificateResponse, no ;
    StatusNotification => ::rust_ocpp::v2_0_1::messages::status_notification::StatusNotificationRequest, ::rust_ocpp::v2_0_1::messages::status_notification::StatusNotificationResponse, no ;
    TransactionEvent => ::rust_ocpp::v2_0_1::messages::transaction_event::TransactionEventRequest, ::rust_ocpp::v2_0_1::messages::transaction_event::TransactionEventResponse, no ;
    TriggerMessage => ::rust_ocpp::v2_0_1::messages::trigger_message::TriggerMessageRequest, ::rust_ocpp::v2_0_1::messages::trigger_message::TriggerMessageResponse, no ;
    UnlockConnector => ::rust_ocpp::v2_0_1::messages::unlock_connector::UnlockConnectorRequest, ::rust_ocpp::v2_0_1::messages::unlock_connector::UnlockConnectorResponse, no ;
    UnpublishFirmware => ::rust_ocpp::v2_0_1::messages::unpublish_firmware::UnpublishFirmwareRequest, ::rust_ocpp::v2_0_1::messages::unpublish_firmware::UnpublishFirmwareResponse, no ;
    UpdateFirmware => ::rust_ocpp::v2_0_1::messages::update_firmware::UpdateFirmwareRequest, ::rust_ocpp::v2_0_1::messages::update_firmware::UpdateFirmwareResponse, no ;
}
