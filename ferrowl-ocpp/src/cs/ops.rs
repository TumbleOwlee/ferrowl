//! `CsOps`: the version-portable outbound API a charging-station simulation calls to talk to the
//! CSMS. Implemented per version on [`Client`](crate::cs::Client); unsupported methods default to
//! [`Error::NotSupported`].

use std::future::Future;

use crate::action::Version;
use crate::error::Error;
use crate::semantic::types::*;

/// CS-outbound semantic operations. Methods present in only one OCPP version default to
/// `Err(Error::NotSupported)` on the other.
///
/// This trait currently exposes the wired slice (both-version basics + the transaction merge +
/// a representative v2.0.1-only method); the remaining outbound actions are additive and follow
/// the same shape.
pub trait CsOps: Send + Sync + 'static {
    fn boot_notification(
        &self,
        _params: BootNotificationParams,
    ) -> impl Future<Output = Result<BootNotificationResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    fn heartbeat(&self) -> impl Future<Output = Result<HeartbeatResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    fn authorize(
        &self,
        _params: AuthorizeParams,
    ) -> impl Future<Output = Result<AuthorizeResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// Merged: v1.6 `StartTransaction`, v2.0.1 `TransactionEvent { event_type: Started }`.
    fn start_transaction(
        &self,
        _params: StartTransactionParams,
    ) -> impl Future<Output = Result<StartTransactionResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// Merged: v1.6 `StopTransaction`, v2.0.1 `TransactionEvent { event_type: Ended }`.
    fn stop_transaction(
        &self,
        _params: StopTransactionParams,
    ) -> impl Future<Output = Result<StopTransactionResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// v2.0.1-only; defaults to `NotSupported` under v1.6.
    fn notify_event(
        &self,
        _params: NotifyEventParams,
    ) -> impl Future<Output = Result<NotifyEventResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// `StatusNotification` (both versions).
    fn status_notification(
        &self,
        _params: StatusNotificationParams,
    ) -> impl Future<Output = Result<(), Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// `MeterValues` (both versions).
    fn meter_values(
        &self,
        _params: MeterValuesParams,
    ) -> impl Future<Output = Result<(), Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// `DataTransfer` (both versions).
    fn data_transfer(
        &self,
        _params: DataTransferParams,
    ) -> impl Future<Output = Result<DataTransferResult, Error>> + Send {
        async { Err(Error::NotSupported) }
    }

    /// `FirmwareStatusNotification` (both versions).
    fn firmware_status_notification(
        &self,
        _params: FirmwareStatusNotificationParams,
    ) -> impl Future<Output = Result<(), Error>> + Send {
        async { Err(Error::NotSupported) }
    }
}

// ---------------------------------------------------------------------------------------------
// Per-version implementations. The CS sends a typed Call and reads the reply back from the
// encoded response payload. v2.0.1 transaction methods use the client's adapter-internal
// `TxState` for the `seq_no` the merged `TransactionEvent` requires (Decision 6).
// ---------------------------------------------------------------------------------------------

#[cfg(feature = "v1_6")]
impl CsOps for super::Client<crate::action::v1_6::V1_6> {
    async fn boot_notification(
        &self,
        params: BootNotificationParams,
    ) -> Result<BootNotificationResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let action = Action::BootNotification(crate::semantic::decode_out(serde_json::json!({
            "chargePointModel": params.model,
            "chargePointVendor": params.vendor,
        }))?);
        let v = V1_6::encode_response(&self.call(action).await?)?;
        Ok(BootNotificationResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
            current_time: v["currentTime"].as_str().unwrap_or_default().to_owned(),
            interval: v["interval"].as_i64().unwrap_or_default(),
        })
    }

    async fn heartbeat(&self) -> Result<HeartbeatResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let action = Action::Heartbeat(crate::semantic::decode_out(serde_json::json!({}))?);
        let v = V1_6::encode_response(&self.call(action).await?)?;
        Ok(HeartbeatResult {
            current_time: v["currentTime"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn authorize(&self, params: AuthorizeParams) -> Result<AuthorizeResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let action = Action::Authorize(crate::semantic::decode_out(serde_json::json!({
            "idTag": params.id_tag,
        }))?);
        let v = V1_6::encode_response(&self.call(action).await?)?;
        Ok(AuthorizeResult {
            status: v["idTagInfo"]["status"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        })
    }

    async fn start_transaction(
        &self,
        params: StartTransactionParams,
    ) -> Result<StartTransactionResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let action = Action::StartTransaction(crate::semantic::decode_out(serde_json::json!({
            "connectorId": params.connector_id,
            "idTag": params.id_tag,
            "meterStart": params.meter_start,
            "timestamp": params.timestamp,
        }))?);
        let v = V1_6::encode_response(&self.call(action).await?)?;
        Ok(StartTransactionResult {
            transaction_id: v["transactionId"].as_i64().unwrap_or_default().to_string(),
            status: v["idTagInfo"]["status"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        })
    }

    async fn stop_transaction(
        &self,
        params: StopTransactionParams,
    ) -> Result<StopTransactionResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let transaction_id: i64 = params.transaction_id.parse().unwrap_or_default();
        let action = Action::StopTransaction(crate::semantic::decode_out(serde_json::json!({
            "transactionId": transaction_id,
            "meterStop": params.meter_stop,
            "timestamp": params.timestamp,
            "idTag": params.id_tag,
        }))?);
        let v = V1_6::encode_response(&self.call(action).await?)?;
        Ok(StopTransactionResult {
            status: v["idTagInfo"]["status"].as_str().map(str::to_owned),
        })
    }

    async fn status_notification(&self, params: StatusNotificationParams) -> Result<(), Error> {
        use crate::action::v1_6::Action;
        let action = Action::StatusNotification(crate::semantic::decode_out(serde_json::json!({
            "connectorId": params.connector_id,
            "errorCode": params.error_code.unwrap_or_else(|| "NoError".to_owned()),
            "status": params.status,
            "timestamp": params.timestamp,
        }))?);
        self.call(action).await?;
        Ok(())
    }

    async fn meter_values(&self, params: MeterValuesParams) -> Result<(), Error> {
        use crate::action::v1_6::Action;
        let action = Action::MeterValues(crate::semantic::decode_out(serde_json::json!({
            "connectorId": params.connector_id,
            "meterValue": params.meter_value,
        }))?);
        self.call(action).await?;
        Ok(())
    }

    async fn data_transfer(&self, params: DataTransferParams) -> Result<DataTransferResult, Error> {
        use crate::action::v1_6::{Action, V1_6};
        let action = Action::DataTransfer(crate::semantic::decode_out(serde_json::json!({
            "vendorId": params.vendor_id,
            "messageId": params.message_id,
            "data": params.data,
        }))?);
        let v = V1_6::encode_response(&self.call(action).await?)?;
        Ok(DataTransferResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
            data: v["data"].as_str().map(str::to_owned),
        })
    }

    async fn firmware_status_notification(
        &self,
        params: FirmwareStatusNotificationParams,
    ) -> Result<(), Error> {
        use crate::action::v1_6::Action;
        let action = Action::FirmwareStatusNotification(crate::semantic::decode_out(
            serde_json::json!({ "status": params.status }),
        )?);
        self.call(action).await?;
        Ok(())
    }
}

#[cfg(feature = "v2_0_1")]
impl CsOps for super::Client<crate::action::v2_0_1::V2_0_1> {
    async fn boot_notification(
        &self,
        params: BootNotificationParams,
    ) -> Result<BootNotificationResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let action = Action::BootNotification(crate::semantic::decode_out(serde_json::json!({
            "reason": "PowerUp",
            "chargingStation": { "model": params.model, "vendorName": params.vendor },
        }))?);
        let v = V2_0_1::encode_response(&self.call(action).await?)?;
        Ok(BootNotificationResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
            current_time: v["currentTime"].as_str().unwrap_or_default().to_owned(),
            interval: v["interval"].as_i64().unwrap_or_default(),
        })
    }

    async fn heartbeat(&self) -> Result<HeartbeatResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let action = Action::Heartbeat(crate::semantic::decode_out(serde_json::json!({}))?);
        let v = V2_0_1::encode_response(&self.call(action).await?)?;
        Ok(HeartbeatResult {
            current_time: v["currentTime"].as_str().unwrap_or_default().to_owned(),
        })
    }

    async fn authorize(&self, params: AuthorizeParams) -> Result<AuthorizeResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let action = Action::Authorize(crate::semantic::decode_out(serde_json::json!({
            "idToken": { "idToken": params.id_tag, "type": "Central" },
        }))?);
        let v = V2_0_1::encode_response(&self.call(action).await?)?;
        Ok(AuthorizeResult {
            status: v["idTokenInfo"]["status"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        })
    }

    async fn start_transaction(
        &self,
        params: StartTransactionParams,
    ) -> Result<StartTransactionResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let (transaction_id, seq_no) = self.tx_state.start_transaction();
        let action = Action::TransactionEvent(crate::semantic::decode_out(serde_json::json!({
            "eventType": "Started",
            "timestamp": params.timestamp,
            "triggerReason": "Authorized",
            "seqNo": seq_no,
            "transactionInfo": { "transactionId": transaction_id },
            "idToken": { "idToken": params.id_tag, "type": "Central" },
            "evse": { "id": params.connector_id },
        }))?);
        let v = V2_0_1::encode_response(&self.call(action).await?)?;
        Ok(StartTransactionResult {
            transaction_id,
            status: v["idTokenInfo"]["status"]
                .as_str()
                .unwrap_or("Accepted")
                .to_owned(),
        })
    }

    async fn stop_transaction(
        &self,
        params: StopTransactionParams,
    ) -> Result<StopTransactionResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let seq_no = self.tx_state.next_seq(&params.transaction_id);
        let mut event = serde_json::json!({
            "eventType": "Ended",
            "timestamp": params.timestamp,
            "triggerReason": "StopAuthorized",
            "seqNo": seq_no,
            "transactionInfo": { "transactionId": params.transaction_id },
        });
        if let Some(id_tag) = &params.id_tag {
            event["idToken"] = serde_json::json!({ "idToken": id_tag, "type": "Central" });
        }
        let action = Action::TransactionEvent(crate::semantic::decode_out(event)?);
        let v = V2_0_1::encode_response(&self.call(action).await?)?;
        Ok(StopTransactionResult {
            status: v["idTokenInfo"]["status"].as_str().map(str::to_owned),
        })
    }

    async fn notify_event(&self, params: NotifyEventParams) -> Result<NotifyEventResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let action = Action::NotifyEvent(crate::semantic::decode_out(serde_json::json!({
            "generatedAt": params.generated_at,
            "seqNo": params.seq_no,
            "eventData": params.event_data,
        }))?);
        let _ = V2_0_1::encode_response(&self.call(action).await?)?;
        Ok(NotifyEventResult {})
    }

    async fn status_notification(&self, params: StatusNotificationParams) -> Result<(), Error> {
        use crate::action::v2_0_1::Action;
        let action = Action::StatusNotification(crate::semantic::decode_out(serde_json::json!({
            "timestamp": params.timestamp.unwrap_or_else(|| "1970-01-01T00:00:00Z".to_owned()),
            "connectorStatus": params.status,
            "evseId": params.evse_id.unwrap_or_default(),
            "connectorId": params.connector_id,
        }))?);
        self.call(action).await?;
        Ok(())
    }

    async fn meter_values(&self, params: MeterValuesParams) -> Result<(), Error> {
        use crate::action::v2_0_1::Action;
        let action = Action::MeterValues(crate::semantic::decode_out(serde_json::json!({
            "evseId": params.connector_id,
            "meterValue": params.meter_value,
        }))?);
        self.call(action).await?;
        Ok(())
    }

    async fn data_transfer(&self, params: DataTransferParams) -> Result<DataTransferResult, Error> {
        use crate::action::v2_0_1::{Action, V2_0_1};
        let action = Action::DataTransfer(crate::semantic::decode_out(serde_json::json!({
            "vendorId": params.vendor_id,
            "messageId": params.message_id,
            "data": params.data,
        }))?);
        let v = V2_0_1::encode_response(&self.call(action).await?)?;
        Ok(DataTransferResult {
            status: v["status"].as_str().unwrap_or_default().to_owned(),
            data: v["data"].as_str().map(str::to_owned),
        })
    }

    async fn firmware_status_notification(
        &self,
        params: FirmwareStatusNotificationParams,
    ) -> Result<(), Error> {
        use crate::action::v2_0_1::Action;
        let action = Action::FirmwareStatusNotification(crate::semantic::decode_out(
            serde_json::json!({ "status": params.status }),
        )?);
        self.call(action).await?;
        Ok(())
    }
}
