//! OCPP 1.6 inbound (CS→CSMS) handler for the CSMS role. Answers every CS-originated Call with a
//! `Default`-derived Accepted/empty response, except the few whose default is unusable:
//! BootNotification (Accepted + currentTime + interval), Heartbeat (currentTime), and
//! StartTransaction (a freshly minted transactionId). Every Call and its reply are forwarded to the
//! view as a [`ServerEvent::Inbound`] for logging and observed-state updates.

use std::future::Future;
use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::{Value, json};

use ferrowl_ocpp::csms::{ConnectionId, CsmsActionHandler};
use ferrowl_ocpp::{Action16, CallError, CallErrorCode, Response16, V1_6, Version};

use crate::module::ocpp::client::backend::rfc3339_now;
use crate::module::ocpp::server::backend::{EventTx, ServerEvent};

/// CSMS inbound handler for OCPP 1.6.
pub struct CsmsHandler16 {
    tx: EventTx,
    next_txid: AtomicI64,
}

impl CsmsHandler16 {
    pub fn new(tx: EventTx) -> Self {
        Self {
            tx,
            next_txid: AtomicI64::new(1),
        }
    }

    /// Build the typed response for an inbound action.
    fn respond(&self, name: &str, action: &Action16) -> Result<Response16, CallError> {
        let crafted: Option<Value> = match name {
            "BootNotification" => Some(json!({
                "status": "Accepted",
                "currentTime": rfc3339_now(),
                "interval": 300,
            })),
            "Heartbeat" => Some(json!({ "currentTime": rfc3339_now() })),
            "StartTransaction" => {
                let id = self.next_txid.fetch_add(1, Ordering::Relaxed);
                Some(json!({ "transactionId": id, "idTagInfo": { "status": "Accepted" } }))
            }
            _ => None,
        };
        match crafted {
            Some(payload) => V1_6::decode_result(action, payload)
                .map_err(|e| CallError::new(CallErrorCode::InternalError, e.to_string())),
            None => V1_6::default_response(name).ok_or_else(|| {
                CallError::new(CallErrorCode::NotImplemented, "action not handled by the CSMS")
            }),
        }
    }
}

impl CsmsActionHandler<V1_6> for CsmsHandler16 {
    fn handle_call(
        &self,
        conn: ConnectionId,
        action: Action16,
    ) -> impl Future<Output = Result<Response16, CallError>> + Send {
        let name = V1_6::action_name(&action).to_string();
        let request = V1_6::encode_action(&action).unwrap_or(Value::Null);
        let result = self.respond(&name, &action);
        let response = match &result {
            Ok(resp) => V1_6::encode_response(resp).unwrap_or(Value::Null),
            Err(_) => Value::Null,
        };
        let _ = self.tx.send(ServerEvent::Inbound {
            conn,
            name,
            request,
            response,
        });
        async move { result }
    }

    fn on_connected(&self, conn: ConnectionId) -> impl Future<Output = ()> + Send {
        let _ = self.tx.send(ServerEvent::Connected { conn });
        async {}
    }

    fn on_disconnected(&self, conn: ConnectionId) -> impl Future<Output = ()> + Send {
        let _ = self.tx.send(ServerEvent::Disconnected { conn });
        async {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrowl_ocpp::csms::ConnectionId;

    #[tokio::test]
    async fn ut_boot_response_and_inbound_event() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let handler = CsmsHandler16::new(tx);
        let action = V1_6::default_action("BootNotification").unwrap();
        let resp = handler.handle_call(ConnectionId(1), action).await.unwrap();
        let json = V1_6::encode_response(&resp).unwrap();
        assert_eq!(json["status"], "Accepted");
        assert_eq!(json["interval"], 300);
        // The Call + reply are forwarded to the view.
        match rx.try_recv().unwrap() {
            ServerEvent::Inbound { name, .. } => assert_eq!(name, "BootNotification"),
            other => panic!("expected Inbound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn ut_start_transaction_mints_unique_txids() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let handler = CsmsHandler16::new(tx);
        let txid = |resp: &Response16| V1_6::encode_response(resp).unwrap()["transactionId"].as_i64().unwrap();
        let r1 = handler
            .handle_call(ConnectionId(1), V1_6::default_action("StartTransaction").unwrap())
            .await
            .unwrap();
        let r2 = handler
            .handle_call(ConnectionId(1), V1_6::default_action("StartTransaction").unwrap())
            .await
            .unwrap();
        assert_ne!(txid(&r1), txid(&r2));
    }
}
