//! `SemanticAdapter`: wraps a version-agnostic [`CsHandler`] and implements the low-level
//! [`CsActionHandler`] for each OCPP version, translating CSMS-initiated wire actions to/from the
//! neutral semantic types.

use std::marker::PhantomData;
use std::sync::Arc;

use serde_json::{Value, json};

use super::action_handler::CsActionHandler;
use super::handler::CsHandler;
use crate::action::Version;
use crate::error::CallError;
use crate::ocppj::CallErrorCode;
use crate::semantic::types::*;
use crate::semantic::{decode_call, enum_str};

/// Adapts a [`CsHandler`] into a per-version [`CsActionHandler`]. Construct with
/// [`SemanticAdapter::new`] and pass to [`ClientBuilder::spawn`](crate::cs::ClientBuilder::spawn).
pub struct SemanticAdapter<V, H> {
    handler: Arc<H>,
    _v: PhantomData<fn() -> V>,
}

impl<V, H: CsHandler> SemanticAdapter<V, H> {
    pub fn new(handler: H) -> Self {
        Self {
            handler: Arc::new(handler),
            _v: PhantomData,
        }
    }
}

fn unmapped<S: AsRef<str>>(action: S) -> CallError {
    CallError::new(
        CallErrorCode::NotImplemented,
        format!("no CS semantic mapping for {}", action.as_ref()),
    )
}

#[cfg(feature = "v1_6")]
impl<H: CsHandler> CsActionHandler<crate::action::v1_6::V1_6>
    for SemanticAdapter<crate::action::v1_6::V1_6, H>
{
    async fn handle_call(
        &self,
        action: crate::action::v1_6::Action,
    ) -> Result<crate::action::v1_6::Response, CallError> {
        use crate::action::v1_6::{Action, Response};

        match action {
            Action::ChangeConfiguration(req) => {
                let params = SetConfigParams {
                    entries: vec![ConfigEntry {
                        key: req.key,
                        value: req.value,
                    }],
                };
                let result = self.handler.on_set_config(params).await?;
                let status = result
                    .results
                    .first()
                    .map(|e| e.status.clone())
                    .unwrap_or_else(|| "Accepted".to_owned());
                Ok(Response::ChangeConfiguration(decode_call(
                    json!({ "status": status }),
                )?))
            }
            Action::GetConfiguration(req) => {
                let params = GetConfigParams {
                    keys: req.key.unwrap_or_default(),
                };
                let result = self.handler.on_get_config(params).await?;
                let configuration_key: Vec<_> = result
                    .entries
                    .iter()
                    .map(|e| json!({ "key": e.key, "value": e.value, "readonly": e.readonly }))
                    .collect();
                Ok(Response::GetConfiguration(decode_call(
                    json!({ "configurationKey": configuration_key }),
                )?))
            }
            Action::RemoteStartTransaction(req) => {
                let params = RemoteStartParams {
                    id_tag: req.id_tag,
                    connector_id: req.connector_id.map(|c| c as i64),
                };
                let result = self.handler.on_start_transaction_requested(params).await?;
                Ok(Response::RemoteStartTransaction(decode_call(
                    json!({ "status": result.status }),
                )?))
            }
            Action::ChangeAvailability(req) => {
                let params = ChangeAvailabilityParams {
                    connector_id: req.connector_id as i64,
                    operational: enum_str(&req.kind) == "Operative",
                };
                let r = self.handler.on_change_availability(params).await?;
                Ok(Response::ChangeAvailability(decode_call(
                    json!({ "status": r.status }),
                )?))
            }
            Action::Reset(req) => {
                let params = ResetParams {
                    kind: enum_str(&req.kind),
                    evse_id: None,
                };
                let r = self.handler.on_reset(params).await?;
                Ok(Response::Reset(decode_call(json!({ "status": r.status }))?))
            }
            Action::UnlockConnector(req) => {
                let params = UnlockConnectorParams {
                    connector_id: req.connector_id as i64,
                    evse_id: None,
                };
                let r = self.handler.on_unlock_connector(params).await?;
                Ok(Response::UnlockConnector(decode_call(
                    json!({ "status": r.status }),
                )?))
            }
            Action::TriggerMessage(req) => {
                let params = TriggerMessageParams {
                    requested_message: enum_str(&req.requested_message),
                    connector_id: req.connector_id.map(|c| c as i64),
                };
                let r = self.handler.on_trigger_message(params).await?;
                Ok(Response::TriggerMessage(decode_call(
                    json!({ "status": r.status }),
                )?))
            }
            Action::ClearCache(_) => {
                let r = self.handler.on_clear_cache().await?;
                Ok(Response::ClearCache(decode_call(
                    json!({ "status": r.status }),
                )?))
            }
            Action::GetLocalListVersion(_) => {
                let r = self.handler.on_get_local_list_version().await?;
                Ok(Response::GetLocalListVersion(decode_call(
                    json!({ "listVersion": r.version }),
                )?))
            }
            Action::CancelReservation(req) => {
                let params = CancelReservationParams {
                    reservation_id: req.reservation_id as i64,
                };
                let r = self.handler.on_cancel_reservation(params).await?;
                Ok(Response::CancelReservation(decode_call(
                    json!({ "status": r.status }),
                )?))
            }
            Action::SetChargingProfile(req) => {
                let params = SetChargingProfileParams {
                    connector_id: Some(req.connector_id as i64),
                    evse_id: None,
                    charging_profile: serde_json::to_value(&req.cs_charging_profiles)
                        .unwrap_or(Value::Null),
                };
                let r = self.handler.on_set_charging_profile(params).await?;
                Ok(Response::SetChargingProfile(decode_call(
                    json!({ "status": r.status }),
                )?))
            }
            Action::ClearChargingProfile(req) => {
                let params = ClearChargingProfileParams {
                    id: req.id.map(|v| v as i64),
                    criteria: json!({
                        "connectorId": req.connector_id,
                        "chargingProfilePurpose": req.charging_profile_purpose,
                        "stackLevel": req.stack_level,
                    }),
                };
                let r = self.handler.on_clear_charging_profile(params).await?;
                Ok(Response::ClearChargingProfile(decode_call(
                    json!({ "status": r.status }),
                )?))
            }
            Action::GetCompositeSchedule(req) => {
                let params = GetCompositeScheduleParams {
                    connector_id: Some(req.connector_id as i64),
                    evse_id: None,
                    duration: req.duration as i64,
                };
                let r = self.handler.on_get_composite_schedule(params).await?;
                let mut resp = json!({ "status": r.status });
                if !r.schedule.is_null() {
                    resp["chargingSchedule"] = r.schedule;
                }
                Ok(Response::GetCompositeSchedule(decode_call(resp)?))
            }
            Action::ReserveNow(req) => {
                let params = ReserveNowParams {
                    reservation_id: req.reservation_id as i64,
                    connector_id: Some(req.connector_id as i64),
                    evse_id: None,
                    expiry_date: req.expiry_date.to_rfc3339(),
                    id_tag: req.id_tag,
                };
                let r = self.handler.on_reserve_now(params).await?;
                Ok(Response::ReserveNow(decode_call(
                    json!({ "status": r.status }),
                )?))
            }
            Action::SendLocalList(req) => {
                let params = SendLocalListParams {
                    list_version: req.list_version as i64,
                    update_type: enum_str(&req.update_type),
                    local_authorization_list: serde_json::to_value(&req.local_authorization_list)
                        .unwrap_or(Value::Null),
                };
                let r = self.handler.on_send_local_list(params).await?;
                Ok(Response::SendLocalList(decode_call(
                    json!({ "status": r.status }),
                )?))
            }
            Action::UpdateFirmware(req) => {
                let params = UpdateFirmwareParams {
                    payload: serde_json::to_value(&req).unwrap_or(Value::Null),
                };
                let _ = self.handler.on_update_firmware(params).await?;
                Ok(Response::UpdateFirmware(decode_call(json!({}))?))
            }
            Action::DataTransfer(req) => {
                let params = DataTransferParams {
                    vendor_id: req.vendor_string,
                    message_id: req.message_id,
                    data: req.data,
                };
                let r = self.handler.on_data_transfer(params).await?;
                Ok(Response::DataTransfer(decode_call(
                    json!({ "status": r.status, "data": r.data }),
                )?))
            }
            Action::RemoteStopTransaction(req) => {
                let params = RemoteStopParams {
                    transaction_id: req.transaction_id.to_string(),
                };
                let r = self.handler.on_stop_transaction_requested(params).await?;
                Ok(Response::RemoteStopTransaction(decode_call(
                    json!({ "status": r.status }),
                )?))
            }
            other => Err(unmapped(crate::action::v1_6::V1_6::action_name(&other))),
        }
    }
}

#[cfg(feature = "v2_0_1")]
impl<H: CsHandler> CsActionHandler<crate::action::v2_0_1::V2_0_1>
    for SemanticAdapter<crate::action::v2_0_1::V2_0_1, H>
{
    async fn handle_call(
        &self,
        action: crate::action::v2_0_1::Action,
    ) -> Result<crate::action::v2_0_1::Response, CallError> {
        use crate::action::v2_0_1::{Action, Response};

        match action {
            Action::SetVariables(req) => {
                let entries = req
                    .set_variable_data
                    .iter()
                    .map(|d| ConfigEntry {
                        key: d.variable.name.clone(),
                        value: d.attribute_value.clone(),
                    })
                    .collect();
                let result = self
                    .handler
                    .on_set_config(SetConfigParams { entries })
                    .await?;
                let set_variable_result: Vec<_> = req
                    .set_variable_data
                    .iter()
                    .map(|d| {
                        let status = result
                            .results
                            .iter()
                            .find(|r| r.key == d.variable.name)
                            .map(|r| r.status.clone())
                            .unwrap_or_else(|| "Accepted".to_owned());
                        json!({
                            "attributeStatus": status,
                            "component": { "name": d.component.name },
                            "variable": { "name": d.variable.name },
                        })
                    })
                    .collect();
                Ok(Response::SetVariables(decode_call(
                    json!({ "setVariableResult": set_variable_result }),
                )?))
            }
            Action::GetVariables(req) => {
                let keys = req
                    .get_variable_data
                    .iter()
                    .map(|d| d.variable.name.clone())
                    .collect();
                let result = self.handler.on_get_config(GetConfigParams { keys }).await?;
                let get_variable_result: Vec<_> = req
                    .get_variable_data
                    .iter()
                    .map(|d| {
                        let entry = result.entries.iter().find(|e| e.key == d.variable.name);
                        let (status, value) = match entry {
                            Some(e) => ("Accepted", e.value.clone()),
                            None => ("UnknownVariable", None),
                        };
                        json!({
                            "attributeStatus": status,
                            "attributeValue": value,
                            "component": { "name": d.component.name },
                            "variable": { "name": d.variable.name },
                        })
                    })
                    .collect();
                Ok(Response::GetVariables(decode_call(
                    json!({ "getVariableResult": get_variable_result }),
                )?))
            }
            Action::RequestStartTransaction(req) => {
                let params = RemoteStartParams {
                    id_tag: req.id_token.id_token,
                    connector_id: req.evse_id.map(|c| c as i64),
                };
                let result = self.handler.on_start_transaction_requested(params).await?;
                Ok(Response::RequestStartTransaction(decode_call(
                    json!({ "status": result.status }),
                )?))
            }
            Action::ChangeAvailability(req) => {
                let params = ChangeAvailabilityParams {
                    connector_id: req.evse.as_ref().map(|e| e.id as i64).unwrap_or_default(),
                    operational: enum_str(&req.operational_status) == "Operative",
                };
                let r = self.handler.on_change_availability(params).await?;
                Ok(Response::ChangeAvailability(decode_call(
                    json!({ "status": r.status }),
                )?))
            }
            Action::Reset(req) => {
                let params = ResetParams {
                    kind: enum_str(&req.request_type),
                    evse_id: req.evse_id.map(|v| v as i64),
                };
                let r = self.handler.on_reset(params).await?;
                Ok(Response::Reset(decode_call(json!({ "status": r.status }))?))
            }
            Action::UnlockConnector(req) => {
                let params = UnlockConnectorParams {
                    connector_id: req.connector_id as i64,
                    evse_id: Some(req.evse_id as i64),
                };
                let r = self.handler.on_unlock_connector(params).await?;
                Ok(Response::UnlockConnector(decode_call(
                    json!({ "status": r.status }),
                )?))
            }
            Action::TriggerMessage(req) => {
                let params = TriggerMessageParams {
                    requested_message: enum_str(&req.requested_message),
                    connector_id: req.evse.as_ref().map(|e| e.id as i64),
                };
                let r = self.handler.on_trigger_message(params).await?;
                Ok(Response::TriggerMessage(decode_call(
                    json!({ "status": r.status }),
                )?))
            }
            Action::ClearCache(_) => {
                let r = self.handler.on_clear_cache().await?;
                Ok(Response::ClearCache(decode_call(
                    json!({ "status": r.status }),
                )?))
            }
            Action::GetLocalListVersion(_) => {
                let r = self.handler.on_get_local_list_version().await?;
                Ok(Response::GetLocalListVersion(decode_call(
                    json!({ "versionNumber": r.version }),
                )?))
            }
            Action::CancelReservation(req) => {
                let params = CancelReservationParams {
                    reservation_id: req.reservation_id as i64,
                };
                let r = self.handler.on_cancel_reservation(params).await?;
                Ok(Response::CancelReservation(decode_call(
                    json!({ "status": r.status }),
                )?))
            }
            Action::SetChargingProfile(req) => {
                let params = SetChargingProfileParams {
                    connector_id: None,
                    evse_id: Some(req.evse_id as i64),
                    charging_profile: serde_json::to_value(&req.charging_profile)
                        .unwrap_or(Value::Null),
                };
                let r = self.handler.on_set_charging_profile(params).await?;
                Ok(Response::SetChargingProfile(decode_call(
                    json!({ "status": r.status }),
                )?))
            }
            Action::ClearChargingProfile(req) => {
                let params = ClearChargingProfileParams {
                    id: req.charging_profile_id.map(|v| v as i64),
                    criteria: serde_json::to_value(&req.charging_profile_criteria)
                        .unwrap_or(Value::Null),
                };
                let r = self.handler.on_clear_charging_profile(params).await?;
                Ok(Response::ClearChargingProfile(decode_call(
                    json!({ "status": r.status }),
                )?))
            }
            Action::GetCompositeSchedule(req) => {
                let params = GetCompositeScheduleParams {
                    connector_id: None,
                    evse_id: Some(req.evse_id as i64),
                    duration: req.duration as i64,
                };
                let r = self.handler.on_get_composite_schedule(params).await?;
                let mut resp = json!({ "status": r.status });
                if !r.schedule.is_null() {
                    resp["schedule"] = r.schedule;
                }
                Ok(Response::GetCompositeSchedule(decode_call(resp)?))
            }
            Action::ReserveNow(req) => {
                let params = ReserveNowParams {
                    reservation_id: req.id as i64,
                    connector_id: None,
                    evse_id: req.evse_id.map(|v| v as i64),
                    expiry_date: req.expiry_date_time.to_rfc3339(),
                    id_tag: req.id_token.id_token,
                };
                let r = self.handler.on_reserve_now(params).await?;
                Ok(Response::ReserveNow(decode_call(
                    json!({ "status": r.status }),
                )?))
            }
            Action::SendLocalList(req) => {
                let params = SendLocalListParams {
                    list_version: req.version_number as i64,
                    update_type: enum_str(&req.update_type),
                    local_authorization_list: serde_json::to_value(&req.local_authorization_list)
                        .unwrap_or(Value::Null),
                };
                let r = self.handler.on_send_local_list(params).await?;
                Ok(Response::SendLocalList(decode_call(
                    json!({ "status": r.status }),
                )?))
            }
            Action::UpdateFirmware(req) => {
                let params = UpdateFirmwareParams {
                    payload: serde_json::to_value(&req).unwrap_or(Value::Null),
                };
                let r = self.handler.on_update_firmware(params).await?;
                Ok(Response::UpdateFirmware(decode_call(
                    json!({ "status": r.status }),
                )?))
            }
            Action::DataTransfer(req) => {
                let params = DataTransferParams {
                    vendor_id: req.vendor_id,
                    message_id: req.message_id,
                    data: req.data,
                };
                let r = self.handler.on_data_transfer(params).await?;
                Ok(Response::DataTransfer(decode_call(
                    json!({ "status": r.status, "data": r.data }),
                )?))
            }
            Action::RequestStopTransaction(req) => {
                let params = RemoteStopParams {
                    transaction_id: req.transaction_id,
                };
                let r = self.handler.on_stop_transaction_requested(params).await?;
                Ok(Response::RequestStopTransaction(decode_call(
                    json!({ "status": r.status }),
                )?))
            }
            other => Err(unmapped(crate::action::v2_0_1::V2_0_1::action_name(&other))),
        }
    }
}
