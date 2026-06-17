//! `SemanticAdapter`: wraps a version-agnostic [`CsmsHandler`] and implements the low-level
//! [`CsmsActionHandler`] for each OCPP version, translating CS-initiated wire actions to/from the
//! neutral semantic types.

use std::marker::PhantomData;
use std::sync::Arc;

use serde_json::{Value, json};

use super::action_handler::CsmsActionHandler;
use super::handler::CsmsHandler;
use super::registry::ConnectionId;
use crate::action::Version;
use crate::error::CallError;
use crate::ocppj::CallErrorCode;
use crate::semantic::types::*;
use crate::semantic::{decode_call, enum_str};

/// Adapts a [`CsmsHandler`] into a per-version [`CsmsActionHandler`]. Construct with
/// [`SemanticAdapter::new`] and pass to [`ServerBuilder::spawn`](crate::csms::ServerBuilder::spawn).
pub struct SemanticAdapter<V, H> {
    handler: Arc<H>,
    _v: PhantomData<fn() -> V>,
}

impl<V, H: CsmsHandler> SemanticAdapter<V, H> {
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
        format!("no CSMS semantic mapping for {}", action.as_ref()),
    )
}

#[cfg(feature = "v1_6")]
impl<H: CsmsHandler> CsmsActionHandler<crate::action::v1_6::V1_6>
    for SemanticAdapter<crate::action::v1_6::V1_6, H>
{
    async fn handle_call(
        &self,
        conn: ConnectionId,
        action: crate::action::v1_6::Action,
    ) -> Result<crate::action::v1_6::Response, CallError> {
        use crate::action::v1_6::{Action, Response};

        match action {
            Action::BootNotification(req) => {
                let params = BootNotificationParams {
                    model: req.charge_point_model,
                    vendor: req.charge_point_vendor,
                };
                let r = self.handler.on_boot_notification(conn, params).await?;
                Ok(Response::BootNotification(decode_call(json!({
                    "currentTime": r.current_time,
                    "interval": r.interval,
                    "status": r.status,
                }))?))
            }
            Action::Heartbeat(_) => {
                let r = self.handler.on_heartbeat(conn).await?;
                Ok(Response::Heartbeat(decode_call(
                    json!({ "currentTime": r.current_time }),
                )?))
            }
            Action::Authorize(req) => {
                let r = self
                    .handler
                    .on_authorize(conn, AuthorizeParams { id_tag: req.id_tag })
                    .await?;
                Ok(Response::Authorize(decode_call(
                    json!({ "idTagInfo": { "status": r.status } }),
                )?))
            }
            Action::StartTransaction(req) => {
                let params = StartTransactionParams {
                    connector_id: req.connector_id as i64,
                    id_tag: req.id_tag,
                    meter_start: req.meter_start as i64,
                    timestamp: req.timestamp.to_rfc3339(),
                };
                let r = self.handler.on_start_transaction(conn, params).await?;
                let transaction_id: i64 = r.transaction_id.parse().unwrap_or_default();
                Ok(Response::StartTransaction(decode_call(json!({
                    "idTagInfo": { "status": r.status },
                    "transactionId": transaction_id,
                }))?))
            }
            Action::StopTransaction(req) => {
                let params = StopTransactionParams {
                    transaction_id: req.transaction_id.to_string(),
                    meter_stop: req.meter_stop as i64,
                    timestamp: req.timestamp.to_rfc3339(),
                    id_tag: req.id_tag,
                };
                let r = self.handler.on_stop_transaction(conn, params).await?;
                let body = match r.status {
                    Some(status) => json!({ "idTagInfo": { "status": status } }),
                    None => json!({}),
                };
                Ok(Response::StopTransaction(decode_call(body)?))
            }
            Action::StatusNotification(req) => {
                let params = StatusNotificationParams {
                    connector_id: req.connector_id as i64,
                    status: enum_str(&req.status),
                    error_code: Some(enum_str(&req.error_code)),
                    evse_id: None,
                    timestamp: req.timestamp.map(|t| t.to_rfc3339()),
                };
                self.handler.on_status_notification(conn, params).await?;
                Ok(Response::StatusNotification(decode_call(json!({}))?))
            }
            Action::MeterValues(req) => {
                let params = MeterValuesParams {
                    connector_id: req.connector_id as i64,
                    meter_value: serde_json::to_value(&req.meter_value).unwrap_or(Value::Null),
                };
                self.handler.on_meter_values(conn, params).await?;
                Ok(Response::MeterValues(decode_call(json!({}))?))
            }
            Action::FirmwareStatusNotification(req) => {
                let params = FirmwareStatusNotificationParams {
                    status: enum_str(&req.status),
                };
                self.handler
                    .on_firmware_status_notification(conn, params)
                    .await?;
                Ok(Response::FirmwareStatusNotification(decode_call(
                    json!({}),
                )?))
            }
            Action::DataTransfer(req) => {
                let params = DataTransferParams {
                    vendor_id: req.vendor_string,
                    message_id: req.message_id,
                    data: req.data,
                };
                let r = self.handler.on_data_transfer(conn, params).await?;
                Ok(Response::DataTransfer(decode_call(
                    json!({ "status": r.status, "data": r.data }),
                )?))
            }
            other => Err(unmapped(crate::action::v1_6::V1_6::action_name(&other))),
        }
    }
}

#[cfg(feature = "v2_0_1")]
impl<H: CsmsHandler> CsmsActionHandler<crate::action::v2_0_1::V2_0_1>
    for SemanticAdapter<crate::action::v2_0_1::V2_0_1, H>
{
    async fn handle_call(
        &self,
        conn: ConnectionId,
        action: crate::action::v2_0_1::Action,
    ) -> Result<crate::action::v2_0_1::Response, CallError> {
        use crate::action::v2_0_1::{Action, Response};
        use rust_ocpp::v2_0_1::enumerations::transaction_event_enum_type::TransactionEventEnumType;

        match action {
            Action::BootNotification(req) => {
                let params = BootNotificationParams {
                    model: req.charging_station.model,
                    vendor: req.charging_station.vendor_name,
                };
                let r = self.handler.on_boot_notification(conn, params).await?;
                Ok(Response::BootNotification(decode_call(json!({
                    "currentTime": r.current_time,
                    "interval": r.interval,
                    "status": r.status,
                }))?))
            }
            Action::Heartbeat(_) => {
                let r = self.handler.on_heartbeat(conn).await?;
                Ok(Response::Heartbeat(decode_call(
                    json!({ "currentTime": r.current_time }),
                )?))
            }
            Action::Authorize(req) => {
                let r = self
                    .handler
                    .on_authorize(
                        conn,
                        AuthorizeParams {
                            id_tag: req.id_token.id_token,
                        },
                    )
                    .await?;
                Ok(Response::Authorize(decode_call(
                    json!({ "idTokenInfo": { "status": r.status } }),
                )?))
            }
            Action::TransactionEvent(req) => {
                let transaction_id = req.transaction_info.transaction_id.clone();
                let id_tag = req.id_token.as_ref().map(|t| t.id_token.clone());
                match req.event_type {
                    TransactionEventEnumType::Started => {
                        let params = StartTransactionParams {
                            connector_id: req
                                .evse
                                .as_ref()
                                .map(|e| e.id as i64)
                                .unwrap_or_default(),
                            id_tag: id_tag.unwrap_or_default(),
                            meter_start: 0,
                            timestamp: req.timestamp.to_rfc3339(),
                        };
                        let r = self.handler.on_start_transaction(conn, params).await?;
                        Ok(Response::TransactionEvent(decode_call(
                            json!({ "idTokenInfo": { "status": r.status } }),
                        )?))
                    }
                    TransactionEventEnumType::Ended => {
                        let params = StopTransactionParams {
                            transaction_id,
                            meter_stop: 0,
                            timestamp: req.timestamp.to_rfc3339(),
                            id_tag,
                        };
                        let r = self.handler.on_stop_transaction(conn, params).await?;
                        let body = match r.status {
                            Some(status) => json!({ "idTokenInfo": { "status": status } }),
                            None => json!({}),
                        };
                        Ok(Response::TransactionEvent(decode_call(body)?))
                    }
                    TransactionEventEnumType::Updated => {
                        // Metering updates are accepted but not surfaced as a distinct semantic
                        // method yet.
                        Ok(Response::TransactionEvent(decode_call(json!({}))?))
                    }
                }
            }
            Action::NotifyEvent(req) => {
                let params = NotifyEventParams {
                    generated_at: req.generated_at.to_rfc3339(),
                    seq_no: req.seq_no as i64,
                    event_data: serde_json::to_value(&req.event_data).unwrap_or_default(),
                };
                let _ = self.handler.on_notify_event(conn, params).await?;
                Ok(Response::NotifyEvent(decode_call(json!({}))?))
            }
            Action::StatusNotification(req) => {
                let params = StatusNotificationParams {
                    connector_id: req.connector_id as i64,
                    status: enum_str(&req.connector_status),
                    error_code: None,
                    evse_id: Some(req.evse_id as i64),
                    timestamp: Some(req.timestamp.to_rfc3339()),
                };
                self.handler.on_status_notification(conn, params).await?;
                Ok(Response::StatusNotification(decode_call(json!({}))?))
            }
            Action::MeterValues(req) => {
                let params = MeterValuesParams {
                    connector_id: req.evse_id as i64,
                    meter_value: serde_json::to_value(&req.meter_value).unwrap_or(Value::Null),
                };
                self.handler.on_meter_values(conn, params).await?;
                Ok(Response::MeterValues(decode_call(json!({}))?))
            }
            Action::FirmwareStatusNotification(req) => {
                let params = FirmwareStatusNotificationParams {
                    status: enum_str(&req.status),
                };
                self.handler
                    .on_firmware_status_notification(conn, params)
                    .await?;
                Ok(Response::FirmwareStatusNotification(decode_call(
                    json!({}),
                )?))
            }
            Action::DataTransfer(req) => {
                let params = DataTransferParams {
                    vendor_id: req.vendor_id,
                    message_id: req.message_id,
                    data: req.data,
                };
                let r = self.handler.on_data_transfer(conn, params).await?;
                Ok(Response::DataTransfer(decode_call(
                    json!({ "status": r.status, "data": r.data }),
                )?))
            }
            other => Err(unmapped(crate::action::v2_0_1::V2_0_1::action_name(&other))),
        }
    }
}
