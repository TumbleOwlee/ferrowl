//! OCPP 2.0.1 inbound (CS→CSMS) handler for the CSMS role. Default-derived Accepted/empty replies,
//! except BootNotification (Accepted + currentTime + interval) and Heartbeat (currentTime).
//! Authorize and a transaction-starting TransactionEvent (one carrying an idToken) gate the tag
//! against the RFID accept-lists: Authorize (no EVSE) against the CS list unioned with every
//! connector list, TransactionEvent against its EVSE's list ∪ the CS list. Every Call + reply is
//! forwarded to the view.

use std::future::Future;

use serde_json::{Value, json};

use ferrowl_ocpp::csms::{ConnectionId, CsmsActionHandler};
use ferrowl_ocpp::{Action201, CallError, CallErrorCode, Response201, V2_0_1, Version};

use crate::module::ocpp::client::backend::rfc3339_now;
use crate::module::ocpp::scope::Scope;
use crate::module::ocpp::server::backend::{
    EventTx, RfidLists, ServerEvent, cs_authorized, scope_authorized,
};

/// CSMS inbound handler for OCPP 2.0.1.
pub struct CsmsHandler201 {
    tx: EventTx,
    rfids: RfidLists,
}

impl CsmsHandler201 {
    pub fn new(tx: EventTx, rfids: RfidLists) -> Self {
        Self { tx, rfids }
    }

    /// `"Accepted"` / `"Invalid"` for a CS-wide check (Authorize carries no EVSE).
    fn cs_status(&self, id_token: &str) -> &'static str {
        if cs_authorized(&self.rfids, id_token) {
            "Accepted"
        } else {
            "Invalid"
        }
    }

    /// `"Accepted"` / `"Invalid"` for a tag on a specific EVSE (its list ∪ the CS list).
    fn evse_status(&self, evse_id: i64, id_token: &str) -> &'static str {
        if scope_authorized(&self.rfids, Scope::evse(evse_id, None), id_token) {
            "Accepted"
        } else {
            "Invalid"
        }
    }

    fn respond(
        &self,
        name: &str,
        action: &Action201,
        request: &Value,
    ) -> Result<Response201, CallError> {
        let crafted: Option<Value> = match name {
            "BootNotification" => Some(json!({
                "currentTime": rfc3339_now(),
                "interval": 300,
                "status": "Accepted",
            })),
            "Heartbeat" => Some(json!({ "currentTime": rfc3339_now() })),
            // Authorize carries no EVSE, so it is checked against the CS list unioned with every
            // connector list.
            "Authorize" => {
                let tag = request["idToken"]["idToken"].as_str().unwrap_or_default();
                Some(json!({ "idTokenInfo": { "status": self.cs_status(tag) } }))
            }
            // A TransactionEvent carrying an idToken (a transaction start) names an EVSE, so it is
            // gated by that EVSE's list ∪ the CS list; the reply echoes the decision.
            "TransactionEvent" if request["idToken"].is_object() => {
                let tag = request["idToken"]["idToken"].as_str().unwrap_or_default();
                let evse = request["evse"]["id"].as_i64().unwrap_or_default();
                Some(json!({ "idTokenInfo": { "status": self.evse_status(evse, tag) } }))
            }
            _ => None,
        };
        match crafted {
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

impl CsmsActionHandler<V2_0_1> for CsmsHandler201 {
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
    use crate::module::ocpp::server::backend::{RfidLists, RfidStore};
    use ferrowl_ocpp::csms::ConnectionId;

    fn rfids(store: RfidStore) -> RfidLists {
        std::sync::Arc::new(std::sync::RwLock::new(store))
    }

    fn authorize(id_token: &str) -> Action201 {
        V2_0_1::decode_call(
            "Authorize",
            json!({ "idToken": { "idToken": id_token, "type": "ISO14443" } }),
        )
        .unwrap()
    }

    fn status(resp: &Response201) -> String {
        V2_0_1::encode_response(resp).unwrap()["idTokenInfo"]["status"]
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
        let handler = CsmsHandler201::new(tx, rfids(store));
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
        let handler = CsmsHandler201::new(tx, rfids(RfidStore::default()));
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
        let handler = CsmsHandler201::new(tx, rfids(store));
        let started = |evse: i64, tag: &str| {
            V2_0_1::decode_call(
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
