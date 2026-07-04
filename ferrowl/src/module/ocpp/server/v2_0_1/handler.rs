//! OCPP 2.0.1 inbound (CS→CSMS) handler for the CSMS role. The decision logic is shared with 2.1
//! (`v2_1::handler`) via [`v2_common`](crate::module::ocpp::server::v2_common), but the typed
//! `Action201`/`Response201` wire enums differ per version, so the two are separate (non-generic)
//! impls. Default-derived Accepted/empty replies, except BootNotification, Heartbeat, and the
//! RFID-gated Authorize / transaction-starting TransactionEvent.

use std::future::Future;

use serde_json::Value;

use ferrowl_ocpp::csms::{ConnectionId, CsmsActionHandler};
use ferrowl_ocpp::{Action201, CallError, CallErrorCode, Response201, V2_0_1, Version};

use crate::module::ocpp::server::backend::{EventTx, RfidLists, ServerEvent};
use crate::module::ocpp::server::v2_common::craft_response;

/// CSMS inbound handler for OCPP 2.0.1.
pub struct CsmsHandler {
    tx: EventTx,
    rfids: RfidLists,
}

impl CsmsHandler {
    pub fn new(tx: EventTx, rfids: RfidLists) -> Self {
        Self { tx, rfids }
    }

    fn respond(
        &self,
        name: &str,
        action: &Action201,
        request: &Value,
    ) -> Result<Response201, CallError> {
        match craft_response(name, request, &self.rfids) {
            Some(payload) => V2_0_1::decode_result(action, payload)
                .map_err(|e| CallError::new(CallErrorCode::InternalError, e.to_string())),
            None => V2_0_1::default_response(name).ok_or_else(|| {
                CallError::new(
                    CallErrorCode::NotImplemented,
                    "action not handled by the CSMS",
                )
            }),
        }
    }
}

impl CsmsActionHandler<V2_0_1> for CsmsHandler {
    fn handle_call(
        &self,
        conn: ConnectionId,
        action: Action201,
    ) -> impl Future<Output = Result<Response201, CallError>> + Send {
        let name = V2_0_1::action_name(&action).to_string();
        let request = V2_0_1::encode_action(&action).unwrap_or(Value::Null);
        let result = self.respond(&name, &action, &request);
        let response = match &result {
            Ok(resp) => V2_0_1::encode_response(resp).unwrap_or(Value::Null),
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
    use crate::module::ocpp::scope::Scope;
    use crate::module::ocpp::server::backend::RfidStore;
    use ferrowl_ocpp::{Action201 as Action, Response201 as Response, V2_0_1 as Ver, Version};
    use serde_json::json;

    fn rfids(store: RfidStore) -> RfidLists {
        std::sync::Arc::new(parking_lot::RwLock::new(store))
    }

    fn authorize(id_token: &str) -> Action {
        Ver::decode_call(
            "Authorize",
            json!({ "idToken": { "idToken": id_token, "type": "ISO14443" } }),
        )
        .unwrap()
    }

    fn status(resp: &Response) -> String {
        Ver::encode_response(resp).unwrap()["idTokenInfo"]["status"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn ut_authorize_gated_by_rfid_list() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let store = RfidStore {
            cs: vec!["GOOD".to_string()],
            ..Default::default()
        };
        let handler = CsmsHandler::new(tx, rfids(store));
        let r_good = handler
            .handle_call(ConnectionId(1), authorize("GOOD"))
            .await
            .unwrap();
        let r_bad = handler
            .handle_call(ConnectionId(1), authorize("NOPE"))
            .await
            .unwrap();
        assert_eq!(status(&r_good), "Accepted");
        assert_eq!(status(&r_bad), "Invalid");
    }

    #[tokio::test]
    async fn ut_empty_rfid_list_accepts_all() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let handler = CsmsHandler::new(tx, rfids(RfidStore::default()));
        let resp = handler
            .handle_call(ConnectionId(1), authorize("ANYTHING"))
            .await
            .unwrap();
        assert_eq!(status(&resp), "Accepted");
    }

    #[tokio::test]
    async fn ut_transaction_event_gated_per_evse() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        // EVSE1 allows ETAG only on evse 1; the inherited CS tag is allowed anywhere.
        let mut store = RfidStore {
            cs: vec!["CS".to_string()],
            ..Default::default()
        };
        store.add(Scope::evse(1, None), "ETAG".to_string());
        let handler = CsmsHandler::new(tx, rfids(store));
        let started = |evse: i64, tag: &str| {
            Ver::decode_call(
                "TransactionEvent",
                json!({
                    "eventType": "Started",
                    "timestamp": "2030-01-01T00:00:00Z",
                    "triggerReason": "Authorized",
                    "seqNo": 0,
                    "transactionInfo": { "transactionId": "t1" },
                    "evse": { "id": evse },
                    "idToken": { "idToken": tag, "type": "ISO14443" }
                }),
            )
            .unwrap()
        };
        let r_own = handler
            .handle_call(ConnectionId(1), started(1, "ETAG"))
            .await
            .unwrap();
        let r_cs = handler
            .handle_call(ConnectionId(1), started(2, "CS"))
            .await
            .unwrap();
        let r_other = handler
            .handle_call(ConnectionId(1), started(2, "ETAG"))
            .await
            .unwrap();
        assert_eq!(status(&r_own), "Accepted");
        assert_eq!(status(&r_cs), "Accepted");
        assert_eq!(status(&r_other), "Invalid");
    }
}
