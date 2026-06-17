//! Version-neutral parameter/result structs shared by the semantic trait layer.
//!
//! These are `ferrowl-ocpp`'s own types, deliberately independent of `rust_ocpp`'s per-version
//! structs so the semantic traits can be version-agnostic (no `V` parameter). Per-version adapters
//! translate between these and the wire types. Enum-valued fields are carried as `String` so a
//! single neutral type spans both versions' (differing) enum sets.
//!
//! This module currently covers the methods wired end-to-end in the semantic layer (the both-version
//! basics plus the genuine cross-version merges). The remaining actions follow the identical neutral
//! struct + adapter pattern and are additive.

use serde::{Deserialize, Serialize};

/// `boot_notification` request: a charging station announcing itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BootNotificationParams {
    pub model: String,
    pub vendor: String,
}

/// `boot_notification` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BootNotificationResult {
    pub status: String,
    pub current_time: String,
    pub interval: i64,
}

/// `authorize` request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthorizeParams {
    pub id_tag: String,
}

/// `authorize` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthorizeResult {
    pub status: String,
}

/// `heartbeat` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeartbeatResult {
    pub current_time: String,
}

/// `start_transaction` request. Merged: v1.6 `StartTransaction`, v2.0.1 `TransactionEvent`
/// (`event_type: Started`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartTransactionParams {
    pub connector_id: i64,
    pub id_tag: String,
    pub meter_start: i64,
    /// RFC3339 timestamp.
    pub timestamp: String,
}

/// `start_transaction` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartTransactionResult {
    /// v1.6: the CSMS-assigned numeric id (as a string). v2.0.1: the CS-chosen transaction id.
    pub transaction_id: String,
    pub status: String,
}

/// `stop_transaction` request. Merged: v1.6 `StopTransaction`, v2.0.1 `TransactionEvent`
/// (`event_type: Ended`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StopTransactionParams {
    pub transaction_id: String,
    pub meter_stop: i64,
    /// RFC3339 timestamp.
    pub timestamp: String,
    pub id_tag: Option<String>,
}

/// `stop_transaction` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StopTransactionResult {
    pub status: Option<String>,
}

/// A single configuration key/value pair to write.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
}

/// `on_set_config` request. Merged: v1.6 `ChangeConfiguration` (one key per call; the adapter
/// fans out), v2.0.1 `SetVariables` (native batch).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetConfigParams {
    pub entries: Vec<ConfigEntry>,
}

/// Per-key result of a set-config operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigSetResult {
    pub key: String,
    pub status: String,
}

/// `on_set_config` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetConfigResult {
    pub results: Vec<ConfigSetResult>,
}

/// `on_get_config` request. Merged: v1.6 `GetConfiguration`, v2.0.1 `GetVariables`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GetConfigParams {
    pub keys: Vec<String>,
}

/// A single read-back configuration value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigReadEntry {
    pub key: String,
    pub value: Option<String>,
    pub readonly: bool,
}

/// `on_get_config` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GetConfigResult {
    pub entries: Vec<ConfigReadEntry>,
}

/// `on_start_transaction_requested` request. Merged: v1.6 `RemoteStartTransaction`, v2.0.1
/// `RequestStartTransaction`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteStartParams {
    pub id_tag: String,
    pub connector_id: Option<i64>,
}

/// `on_start_transaction_requested` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteStartResult {
    pub status: String,
}

/// `notify_event` request (v2.0.1-only). The event records are carried as raw JSON for now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotifyEventParams {
    pub generated_at: String,
    pub seq_no: i64,
    pub event_data: serde_json::Value,
}

/// `notify_event` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotifyEventResult {}

// ----- both-version actions (added beyond the initial slice) -------------------------------------
//
// Scalar fields are mapped natively per version. Deeply version-shaped structures (charging
// profiles, composite schedules, local-auth lists, firmware descriptors, meter values) are carried
// as `serde_json::Value` so a single method spans both versions; their inner shape follows the
// chosen OCPP version's schema. Enum-valued `String` fields use the spec spelling of the relevant
// version (e.g. `reset` `kind` is `"Hard"`/`"Soft"` for v1.6, `"Immediate"`/`"OnIdle"` for v2.0.1).

/// A bare `{ status }` result, shared by the many actions whose response is just a status enum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenericStatusResult {
    pub status: String,
}

/// `status_notification` request (CS -> CSMS).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusNotificationParams {
    pub connector_id: i64,
    pub status: String,
    pub error_code: Option<String>,
    pub evse_id: Option<i64>,
    /// RFC3339 timestamp (required by v2.0.1; ignored by v1.6 if absent).
    pub timestamp: Option<String>,
}

/// `meter_values` request (CS -> CSMS). `meter_value` is the version-shaped array of samples.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeterValuesParams {
    pub connector_id: i64,
    pub meter_value: serde_json::Value,
}

/// `data_transfer` request (either direction).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataTransferParams {
    pub vendor_id: String,
    pub message_id: Option<String>,
    pub data: Option<String>,
}

/// `data_transfer` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataTransferResult {
    pub status: String,
    pub data: Option<String>,
}

/// `firmware_status_notification` request (CS -> CSMS).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FirmwareStatusNotificationParams {
    pub status: String,
}

/// `change_availability` request (CSMS -> CS). `operational` true = Operative.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeAvailabilityParams {
    pub connector_id: i64,
    pub operational: bool,
}

/// `reset` request (CSMS -> CS).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResetParams {
    pub kind: String,
    pub evse_id: Option<i64>,
}

/// `unlock_connector` request (CSMS -> CS).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnlockConnectorParams {
    pub connector_id: i64,
    pub evse_id: Option<i64>,
}

/// `trigger_message` request (CSMS -> CS).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerMessageParams {
    pub requested_message: String,
    pub connector_id: Option<i64>,
}

/// `cancel_reservation` request (CSMS -> CS).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CancelReservationParams {
    pub reservation_id: i64,
}

/// `get_local_list_version` result (CSMS -> CS).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalListVersionResult {
    pub version: i64,
}

/// `set_charging_profile` request (CSMS -> CS). The profile object is version-shaped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetChargingProfileParams {
    pub connector_id: Option<i64>,
    pub evse_id: Option<i64>,
    pub charging_profile: serde_json::Value,
}

/// `clear_charging_profile` request (CSMS -> CS). `criteria` is the version-shaped filter object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClearChargingProfileParams {
    pub id: Option<i64>,
    pub criteria: serde_json::Value,
}

/// `get_composite_schedule` request (CSMS -> CS).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GetCompositeScheduleParams {
    pub connector_id: Option<i64>,
    pub evse_id: Option<i64>,
    pub duration: i64,
}

/// `get_composite_schedule` result. `schedule` is the version-shaped schedule object (or null).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GetCompositeScheduleResult {
    pub status: String,
    pub schedule: serde_json::Value,
}

/// `reserve_now` request (CSMS -> CS).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReserveNowParams {
    pub reservation_id: i64,
    pub connector_id: Option<i64>,
    pub evse_id: Option<i64>,
    /// RFC3339 expiry timestamp.
    pub expiry_date: String,
    pub id_tag: String,
}

/// `send_local_list` request (CSMS -> CS). `local_authorization_list` is version-shaped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SendLocalListParams {
    pub list_version: i64,
    pub update_type: String,
    pub local_authorization_list: serde_json::Value,
}

/// `update_firmware` request (CSMS -> CS). The firmware descriptor differs sharply between
/// versions, so the whole request payload is carried as version-shaped JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateFirmwareParams {
    pub payload: serde_json::Value,
}

/// `on_stop_transaction_requested` request (CSMS -> CS). Merged: v1.6 `RemoteStopTransaction`,
/// v2.0.1 `RequestStopTransaction`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteStopParams {
    pub transaction_id: String,
}
