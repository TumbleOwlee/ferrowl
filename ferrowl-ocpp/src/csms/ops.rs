//! `CsmsOps`: the version-portable outbound API a CSMS simulation calls to drive a specific
//! connected CS. Mirrors [`CsHandler`](crate::cs::CsHandler), with a leading [`ConnectionId`].
//! Implemented per version on [`Server`](crate::csms::Server).

use std::future::Future;

use super::registry::ConnectionId;
use crate::action::Version;
use crate::error::Error;
use crate::semantic::types::*;

/// CSMS-outbound semantic operations. Methods present in only one OCPP version default to
/// `Err(Error::NotSupported)` on the other.
pub trait CsmsOps: Send + Sync + 'static {
    /// Merged: v1.6 `ChangeConfiguration` (fanned out per key), v2.0.1 `SetVariables` (batch).
    fn set_config(
        &self,
        _conn: ConnectionId,
        _params: SetConfigParams,
    ) -> impl Future<Output = Result<SetConfigResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// Merged: v1.6 `GetConfiguration`, v2.0.1 `GetVariables`.
    fn get_config(
        &self,
        _conn: ConnectionId,
        _params: GetConfigParams,
    ) -> impl Future<Output = Result<GetConfigResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// Merged: v1.6 `RemoteStartTransaction`, v2.0.1 `RequestStartTransaction`.
    fn request_start_transaction(
        &self,
        _conn: ConnectionId,
        _params: RemoteStartParams,
    ) -> impl Future<Output = Result<RemoteStartResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// Merged: v1.6 `RemoteStopTransaction`, v2.0.1 `RequestStopTransaction`.
    fn stop_transaction_requested(
        &self,
        _conn: ConnectionId,
        _params: RemoteStopParams,
    ) -> impl Future<Output = Result<GenericStatusResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// `ChangeAvailability` (both versions).
    fn change_availability(
        &self,
        _conn: ConnectionId,
        _params: ChangeAvailabilityParams,
    ) -> impl Future<Output = Result<GenericStatusResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// `Reset` (both versions).
    fn reset(
        &self,
        _conn: ConnectionId,
        _params: ResetParams,
    ) -> impl Future<Output = Result<GenericStatusResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// `UnlockConnector` (both versions).
    fn unlock_connector(
        &self,
        _conn: ConnectionId,
        _params: UnlockConnectorParams,
    ) -> impl Future<Output = Result<GenericStatusResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// `TriggerMessage` (both versions).
    fn trigger_message(
        &self,
        _conn: ConnectionId,
        _params: TriggerMessageParams,
    ) -> impl Future<Output = Result<GenericStatusResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// `ClearCache` (both versions).
    fn clear_cache(
        &self,
        _conn: ConnectionId,
    ) -> impl Future<Output = Result<GenericStatusResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// `GetLocalListVersion` (both versions).
    fn get_local_list_version(
        &self,
        _conn: ConnectionId,
    ) -> impl Future<Output = Result<LocalListVersionResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// `CancelReservation` (both versions).
    fn cancel_reservation(
        &self,
        _conn: ConnectionId,
        _params: CancelReservationParams,
    ) -> impl Future<Output = Result<GenericStatusResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// `SetChargingProfile` (both versions).
    fn set_charging_profile(
        &self,
        _conn: ConnectionId,
        _params: SetChargingProfileParams,
    ) -> impl Future<Output = Result<GenericStatusResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// `ClearChargingProfile` (both versions).
    fn clear_charging_profile(
        &self,
        _conn: ConnectionId,
        _params: ClearChargingProfileParams,
    ) -> impl Future<Output = Result<GenericStatusResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// `GetCompositeSchedule` (both versions).
    fn get_composite_schedule(
        &self,
        _conn: ConnectionId,
        _params: GetCompositeScheduleParams,
    ) -> impl Future<Output = Result<GetCompositeScheduleResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// `ReserveNow` (both versions).
    fn reserve_now(
        &self,
        _conn: ConnectionId,
        _params: ReserveNowParams,
    ) -> impl Future<Output = Result<GenericStatusResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// `SendLocalList` (both versions).
    fn send_local_list(
        &self,
        _conn: ConnectionId,
        _params: SendLocalListParams,
    ) -> impl Future<Output = Result<GenericStatusResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// `UpdateFirmware` (both versions).
    fn update_firmware(
        &self,
        _conn: ConnectionId,
        _params: UpdateFirmwareParams,
    ) -> impl Future<Output = Result<GenericStatusResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// `DataTransfer` initiated by the CSMS (both versions).
    fn data_transfer(
        &self,
        _conn: ConnectionId,
        _params: DataTransferParams,
    ) -> impl Future<Output = Result<DataTransferResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }
}

// ---------------------------------------------------------------------------------------------
// Per-version implementations. CSMS-outbound semantic commands target one connection. v1.6
// `set_config` fans out one `ChangeConfiguration` per key (v1.6 allows a single key per call)
// and aggregates; v2.0.1 sends one native `SetVariables` batch.
// ---------------------------------------------------------------------------------------------

#[cfg(feature = "v1_6")]
impl CsmsOps for super::Server<crate::action::v1_6::V1_6> {
    async fn set_config(
        &self,
        conn: ConnectionId,
        params: SetConfigParams,
    ) -> Result<SetConfigResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let mut results = Vec::with_capacity(params.entries.len());
        for entry in params.entries {
            let action =
                Action::ChangeConfiguration(crate::semantic::decode_out(serde_json::json!({
                    "key": entry.key,
                    "value": entry.value,
                }))?);
            let v = V1_6::encode_response(&self.call(conn, action).await?)?;
            results.push(ConfigSetResult {
                key: entry.key,
                status: v["status"].as_str().unwrap_or_default().to_owned(),
            });
        }
        Ok(SetConfigResult { results })
    }

    async fn get_config(
        &self,
        conn: ConnectionId,
        params: GetConfigParams,
    ) -> Result<GetConfigResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let action = Action::GetConfiguration(crate::semantic::decode_out(serde_json::json!({
            "key": params.keys,
        }))?);
        let v = V1_6::encode_response(&self.call(conn, action).await?)?;
        let entries = v["configurationKey"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|e| ConfigReadEntry {
                        key: e["key"].as_str().unwrap_or_default().to_owned(),
                        value: e["value"].as_str().map(str::to_owned),
                        readonly: e["readonly"].as_bool().unwrap_or(false),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(GetConfigResult { entries })
    }

    async fn request_start_transaction(
        &self,
        conn: ConnectionId,
        params: RemoteStartParams,
    ) -> Result<RemoteStartResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let action =
            Action::RemoteStartTransaction(crate::semantic::decode_out(serde_json::json!({
                "idTag": params.id_tag,
                "connectorId": params.connector_id,
            }))?);
        let v = V1_6::encode_response(&self.call(conn, action).await?)?;
        Ok(RemoteStartResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn stop_transaction_requested(
        &self,
        conn: ConnectionId,
        params: RemoteStopParams,
    ) -> Result<GenericStatusResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let transaction_id: i64 = params.transaction_id.parse().unwrap_or_default();
        let action = Action::RemoteStopTransaction(crate::semantic::decode_out(
            serde_json::json!({ "transactionId": transaction_id }),
        )?);
        let v = V1_6::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn change_availability(
        &self,
        conn: ConnectionId,
        params: ChangeAvailabilityParams,
    ) -> Result<GenericStatusResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let kind = if params.operational {
            "Operative"
        } else {
            "Inoperative"
        };
        let action = Action::ChangeAvailability(crate::semantic::decode_out(serde_json::json!({
            "connectorId": params.connector_id,
            "type": kind,
        }))?);
        let v = V1_6::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn reset(
        &self,
        conn: ConnectionId,
        params: ResetParams,
    ) -> Result<GenericStatusResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let action = Action::Reset(crate::semantic::decode_out(
            serde_json::json!({ "type": params.kind }),
        )?);
        let v = V1_6::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn unlock_connector(
        &self,
        conn: ConnectionId,
        params: UnlockConnectorParams,
    ) -> Result<GenericStatusResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let action = Action::UnlockConnector(crate::semantic::decode_out(
            serde_json::json!({ "connectorId": params.connector_id }),
        )?);
        let v = V1_6::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn trigger_message(
        &self,
        conn: ConnectionId,
        params: TriggerMessageParams,
    ) -> Result<GenericStatusResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let action = Action::TriggerMessage(crate::semantic::decode_out(serde_json::json!({
            "requestedMessage": params.requested_message,
            "connectorId": params.connector_id,
        }))?);
        let v = V1_6::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn clear_cache(&self, conn: ConnectionId) -> Result<GenericStatusResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let action = Action::ClearCache(crate::semantic::decode_out(serde_json::json!({}))?);
        let v = V1_6::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn get_local_list_version(
        &self,
        conn: ConnectionId,
    ) -> Result<LocalListVersionResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let action =
            Action::GetLocalListVersion(crate::semantic::decode_out(serde_json::json!({}))?);
        let v = V1_6::encode_response(&self.call(conn, action).await?)?;
        Ok(LocalListVersionResult {
            version: v["listVersion"].as_i64().unwrap_or_default(),
        })
    }

    async fn cancel_reservation(
        &self,
        conn: ConnectionId,
        params: CancelReservationParams,
    ) -> Result<GenericStatusResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let action = Action::CancelReservation(crate::semantic::decode_out(
            serde_json::json!({ "reservationId": params.reservation_id }),
        )?);
        let v = V1_6::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn set_charging_profile(
        &self,
        conn: ConnectionId,
        params: SetChargingProfileParams,
    ) -> Result<GenericStatusResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let action = Action::SetChargingProfile(crate::semantic::decode_out(serde_json::json!({
            "connectorId": params.connector_id.unwrap_or_default(),
            "csChargingProfiles": params.charging_profile,
        }))?);
        let v = V1_6::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn clear_charging_profile(
        &self,
        conn: ConnectionId,
        params: ClearChargingProfileParams,
    ) -> Result<GenericStatusResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let mut req = params.criteria;
        if !req.is_object() {
            req = serde_json::json!({});
        }
        req["id"] = serde_json::json!(params.id);
        let action = Action::ClearChargingProfile(crate::semantic::decode_out(req)?);
        let v = V1_6::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn get_composite_schedule(
        &self,
        conn: ConnectionId,
        params: GetCompositeScheduleParams,
    ) -> Result<GetCompositeScheduleResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let action =
            Action::GetCompositeSchedule(crate::semantic::decode_out(serde_json::json!({
                "connectorId": params.connector_id.unwrap_or_default(),
                "duration": params.duration,
            }))?);
        let v = V1_6::encode_response(&self.call(conn, action).await?)?;
        Ok(GetCompositeScheduleResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
            schedule: v["chargingSchedule"].clone(),
        })
    }

    async fn reserve_now(
        &self,
        conn: ConnectionId,
        params: ReserveNowParams,
    ) -> Result<GenericStatusResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let action = Action::ReserveNow(crate::semantic::decode_out(serde_json::json!({
            "connectorId": params.connector_id.unwrap_or_default(),
            "expiryDate": params.expiry_date,
            "idTag": params.id_tag,
            "reservationId": params.reservation_id,
        }))?);
        let v = V1_6::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn send_local_list(
        &self,
        conn: ConnectionId,
        params: SendLocalListParams,
    ) -> Result<GenericStatusResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let action = Action::SendLocalList(crate::semantic::decode_out(serde_json::json!({
            "listVersion": params.list_version,
            "updateType": params.update_type,
            "localAuthorizationList": params.local_authorization_list,
        }))?);
        let v = V1_6::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn update_firmware(
        &self,
        conn: ConnectionId,
        params: UpdateFirmwareParams,
    ) -> Result<GenericStatusResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let action = Action::UpdateFirmware(crate::semantic::decode_out(params.payload)?);
        let v = V1_6::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or("Accepted").to_owned(),
        })
    }

    async fn data_transfer(
        &self,
        conn: ConnectionId,
        params: DataTransferParams,
    ) -> Result<DataTransferResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let action = Action::DataTransfer(crate::semantic::decode_out(serde_json::json!({
            "vendorId": params.vendor_id,
            "messageId": params.message_id,
            "data": params.data,
        }))?);
        let v = V1_6::encode_response(&self.call(conn, action).await?)?;
        Ok(DataTransferResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
            data: v["data"].as_str().map(str::to_owned),
        })
    }
}

#[cfg(feature = "v2_0_1")]
impl CsmsOps for super::Server<crate::action::v2_0_1::V2_0_1> {
    async fn set_config(
        &self,
        conn: ConnectionId,
        params: SetConfigParams,
    ) -> Result<SetConfigResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let set_variable_data: Vec<_> = params
            .entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "attributeValue": e.value,
                    "component": { "name": e.key },
                    "variable": { "name": e.key },
                })
            })
            .collect();
        let action = Action::SetVariables(crate::semantic::decode_out(serde_json::json!({
            "setVariableData": set_variable_data,
        }))?);
        let v = V2_0_1::encode_response(&self.call(conn, action).await?)?;
        let results = v["setVariableResult"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|r| ConfigSetResult {
                        key: r["variable"]["name"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                        status: r["attributeStatus"].as_str().unwrap_or_default().to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(SetConfigResult { results })
    }

    async fn get_config(
        &self,
        conn: ConnectionId,
        params: GetConfigParams,
    ) -> Result<GetConfigResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let get_variable_data: Vec<_> = params
            .keys
            .iter()
            .map(|k| serde_json::json!({ "component": { "name": k }, "variable": { "name": k } }))
            .collect();
        let action = Action::GetVariables(crate::semantic::decode_out(serde_json::json!({
            "getVariableData": get_variable_data,
        }))?);
        let v = V2_0_1::encode_response(&self.call(conn, action).await?)?;
        let entries = v["getVariableResult"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|r| ConfigReadEntry {
                        key: r["variable"]["name"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                        value: r["attributeValue"].as_str().map(str::to_owned),
                        readonly: false,
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(GetConfigResult { entries })
    }

    async fn request_start_transaction(
        &self,
        conn: ConnectionId,
        params: RemoteStartParams,
    ) -> Result<RemoteStartResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let action =
            Action::RequestStartTransaction(crate::semantic::decode_out(serde_json::json!({
                "idToken": { "idToken": params.id_tag, "type": "Central" },
                "evseId": params.connector_id,
                "remoteStartId": 1,
            }))?);
        let v = V2_0_1::encode_response(&self.call(conn, action).await?)?;
        Ok(RemoteStartResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn stop_transaction_requested(
        &self,
        conn: ConnectionId,
        params: RemoteStopParams,
    ) -> Result<GenericStatusResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let action = Action::RequestStopTransaction(crate::semantic::decode_out(
            serde_json::json!({ "transactionId": params.transaction_id }),
        )?);
        let v = V2_0_1::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn change_availability(
        &self,
        conn: ConnectionId,
        params: ChangeAvailabilityParams,
    ) -> Result<GenericStatusResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let status = if params.operational {
            "Operative"
        } else {
            "Inoperative"
        };
        let action = Action::ChangeAvailability(crate::semantic::decode_out(serde_json::json!({
            "operationalStatus": status,
            "evse": { "id": params.connector_id },
        }))?);
        let v = V2_0_1::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn reset(
        &self,
        conn: ConnectionId,
        params: ResetParams,
    ) -> Result<GenericStatusResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let action = Action::Reset(crate::semantic::decode_out(serde_json::json!({
            "type": params.kind,
            "evseId": params.evse_id,
        }))?);
        let v = V2_0_1::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn unlock_connector(
        &self,
        conn: ConnectionId,
        params: UnlockConnectorParams,
    ) -> Result<GenericStatusResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let action = Action::UnlockConnector(crate::semantic::decode_out(serde_json::json!({
            "evseId": params.evse_id.unwrap_or_default(),
            "connectorId": params.connector_id,
        }))?);
        let v = V2_0_1::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn trigger_message(
        &self,
        conn: ConnectionId,
        params: TriggerMessageParams,
    ) -> Result<GenericStatusResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let mut req = serde_json::json!({ "requestedMessage": params.requested_message });
        if let Some(id) = params.connector_id {
            req["evse"] = serde_json::json!({ "id": id });
        }
        let action = Action::TriggerMessage(crate::semantic::decode_out(req)?);
        let v = V2_0_1::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn clear_cache(&self, conn: ConnectionId) -> Result<GenericStatusResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let action = Action::ClearCache(crate::semantic::decode_out(serde_json::json!({}))?);
        let v = V2_0_1::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn get_local_list_version(
        &self,
        conn: ConnectionId,
    ) -> Result<LocalListVersionResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let action =
            Action::GetLocalListVersion(crate::semantic::decode_out(serde_json::json!({}))?);
        let v = V2_0_1::encode_response(&self.call(conn, action).await?)?;
        Ok(LocalListVersionResult {
            version: v["versionNumber"].as_i64().unwrap_or_default(),
        })
    }

    async fn cancel_reservation(
        &self,
        conn: ConnectionId,
        params: CancelReservationParams,
    ) -> Result<GenericStatusResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let action = Action::CancelReservation(crate::semantic::decode_out(
            serde_json::json!({ "reservationId": params.reservation_id }),
        )?);
        let v = V2_0_1::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn set_charging_profile(
        &self,
        conn: ConnectionId,
        params: SetChargingProfileParams,
    ) -> Result<GenericStatusResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let action = Action::SetChargingProfile(crate::semantic::decode_out(serde_json::json!({
            "evseId": params.evse_id.unwrap_or_default(),
            "chargingProfile": params.charging_profile,
        }))?);
        let v = V2_0_1::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn clear_charging_profile(
        &self,
        conn: ConnectionId,
        params: ClearChargingProfileParams,
    ) -> Result<GenericStatusResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let mut req = serde_json::json!({ "chargingProfileId": params.id });
        if !params.criteria.is_null() {
            req["chargingProfileCriteria"] = params.criteria;
        }
        let action = Action::ClearChargingProfile(crate::semantic::decode_out(req)?);
        let v = V2_0_1::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn get_composite_schedule(
        &self,
        conn: ConnectionId,
        params: GetCompositeScheduleParams,
    ) -> Result<GetCompositeScheduleResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let action =
            Action::GetCompositeSchedule(crate::semantic::decode_out(serde_json::json!({
                "evseId": params.evse_id.unwrap_or_default(),
                "duration": params.duration,
            }))?);
        let v = V2_0_1::encode_response(&self.call(conn, action).await?)?;
        Ok(GetCompositeScheduleResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
            schedule: v["schedule"].clone(),
        })
    }

    async fn reserve_now(
        &self,
        conn: ConnectionId,
        params: ReserveNowParams,
    ) -> Result<GenericStatusResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let action = Action::ReserveNow(crate::semantic::decode_out(serde_json::json!({
            "id": params.reservation_id,
            "expiryDateTime": params.expiry_date,
            "idToken": { "idToken": params.id_tag, "type": "Central" },
            "evseId": params.evse_id,
        }))?);
        let v = V2_0_1::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn send_local_list(
        &self,
        conn: ConnectionId,
        params: SendLocalListParams,
    ) -> Result<GenericStatusResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let action = Action::SendLocalList(crate::semantic::decode_out(serde_json::json!({
            "versionNumber": params.list_version,
            "updateType": params.update_type,
            "localAuthorizationList": params.local_authorization_list,
        }))?);
        let v = V2_0_1::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn update_firmware(
        &self,
        conn: ConnectionId,
        params: UpdateFirmwareParams,
    ) -> Result<GenericStatusResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let action = Action::UpdateFirmware(crate::semantic::decode_out(params.payload)?);
        let v = V2_0_1::encode_response(&self.call(conn, action).await?)?;
        Ok(GenericStatusResult {
            status: v["status"].as_str().unwrap_or("Accepted").to_owned(),
        })
    }

    async fn data_transfer(
        &self,
        conn: ConnectionId,
        params: DataTransferParams,
    ) -> Result<DataTransferResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let action = Action::DataTransfer(crate::semantic::decode_out(serde_json::json!({
            "vendorId": params.vendor_id,
            "messageId": params.message_id,
            "data": params.data,
        }))?);
        let v = V2_0_1::encode_response(&self.call(conn, action).await?)?;
        Ok(DataTransferResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
            data: v["data"].as_str().map(str::to_owned),
        })
    }
}
