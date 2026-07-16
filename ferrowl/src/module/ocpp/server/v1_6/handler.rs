//! OCPP 1.6 inbound (CS→CSMS) handler for the CSMS role. Answers every CS-originated Call with a
//! `Default`-derived Accepted/empty response, except the few whose default is unusable:
//! BootNotification (Accepted + currentTime + interval), Heartbeat (currentTime), and
//! StartTransaction (a freshly minted transactionId). Authorize and StartTransaction additionally
//! gate the id tag against the RFID accept-lists: Authorize (no connector) against the CS list
//! unioned with every connector list, StartTransaction against its connector's list ∪ the CS list.
//! Every Call and its reply are forwarded to the view as a [`ServerEvent::Inbound`] for logging and
//! observed-state updates.

use std::future::Future;
use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::{Value, json};

use ferrowl_ocpp::csms::{ConnectionId, CsmsActionHandler};
use ferrowl_ocpp::{Action16, CallError, CallErrorCode, Response16, V1_6, Version};

use crate::module::ocpp::client::backend::rfc3339_now;
use crate::module::ocpp::scope::Scope;
use crate::module::ocpp::server::backend::{
    EventTx, RfidLists, ServerEvent, cs_authorized, scope_authorized,
};

/// CSMS inbound handler for OCPP 1.6.
pub struct CsmsHandler16 {
    tx: EventTx,
    next_txid: AtomicI64,
    rfids: RfidLists,
}

impl CsmsHandler16 {
    pub fn new(tx: EventTx, rfids: RfidLists) -> Self {
        Self {
            tx,
            next_txid: AtomicI64::new(1),
            rfids,
        }
    }

    /// `"Accepted"` / `"Invalid"` for a CS-wide check (Authorize carries no connector).
    fn cs_status(&self, id_tag: &str) -> &'static str {
        if cs_authorized(&self.rfids, id_tag) {
            "Accepted"
        } else {
            "Invalid"
        }
    }

    /// `"Accepted"` / `"Invalid"` for a tag on a specific connector (its list ∪ the CS list).
    fn connector_status(&self, connector_id: i64, id_tag: &str) -> &'static str {
        if scope_authorized(&self.rfids, Scope::connector(connector_id), id_tag) {
            "Accepted"
        } else {
            "Invalid"
        }
    }

    /// Build the typed response for an inbound action. `request` is the encoded Call payload.
    fn respond(
        &self,
        name: &str,
        action: &Action16,
        request: &Value,
    ) -> Result<Response16, CallError> {
        let tag = || request["idTag"].as_str().unwrap_or_default();
        let crafted: Option<Value> = match name {
            "BootNotification" => Some(json!({
                "status": "Accepted",
                "currentTime": rfc3339_now(),
                "interval": 300,
            })),
            "Heartbeat" => Some(json!({ "currentTime": rfc3339_now() })),
            // Authorize carries no connector, so it is checked against the CS list unioned with
            // every connector list.
            "Authorize" => Some(json!({ "idTagInfo": { "status": self.cs_status(tag()) } })),
            // StartTransaction names a connector, so it is gated by that connector's list ∪ the CS.
            "StartTransaction" => {
                let id = self.next_txid.fetch_add(1, Ordering::Relaxed);
                let connector = request["connectorId"].as_i64().unwrap_or_default();
                let status = self.connector_status(connector, tag());
                Some(json!({ "transactionId": id, "idTagInfo": { "status": status } }))
            }
            _ => None,
        };
        match crafted {
            Some(payload) => V1_6::decode_result(action, payload)
                .map_err(|e| CallError::new(CallErrorCode::InternalError, e.to_string())),
            None => V1_6::default_response(name).ok_or_else(|| {
                CallError::new(
                    CallErrorCode::NotImplemented,
                    "action not handled by the CSMS",
                )
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
        let result = self.respond(&name, &action, &request);
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

    use crate::module::ocpp::server::backend::{RfidLists, RfidStore};

    fn empty_rfids() -> RfidLists {
        std::sync::Arc::new(parking_lot::RwLock::new(RfidStore::default()))
    }

    fn rfids(store: RfidStore) -> RfidLists {
        std::sync::Arc::new(parking_lot::RwLock::new(store))
    }

    #[tokio::test]
    /// OC-R-073 — the CSMS answers BootNotification with a crafted response carrying the current time and a heartbeat interval.
    async fn ut_boot_response_and_inbound_event() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let handler = CsmsHandler16::new(tx, empty_rfids());
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
    /// OC-R-073 — the CSMS answers a transaction start with a freshly minted, unique transaction id plus an accept/reject status.
    async fn ut_start_transaction_mints_unique_txids() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let handler = CsmsHandler16::new(tx, empty_rfids());
        let txid = |resp: &Response16| {
            V1_6::encode_response(resp).unwrap()["transactionId"]
                .as_i64()
                .unwrap()
        };
        let r1 = handler
            .handle_call(
                ConnectionId(1),
                V1_6::default_action("StartTransaction").unwrap(),
            )
            .await
            .unwrap();
        let r2 = handler
            .handle_call(
                ConnectionId(1),
                V1_6::default_action("StartTransaction").unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(txid(&r1), txid(&r2));
    }

    #[tokio::test]
    /// OC-R-076 — an authorization (naming no connector) is checked against the charge-point-wide list unioned with every connector list.
    async fn ut_authorize_gated_by_rfid_list() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let store = RfidStore {
            cs: vec!["GOOD".to_string()],
            ..Default::default()
        };
        let handler = CsmsHandler16::new(tx, rfids(store));
        let status = |resp: &Response16| {
            V1_6::encode_response(resp).unwrap()["idTagInfo"]["status"]
                .as_str()
                .unwrap()
                .to_string()
        };
        let good = V1_6::decode_call("Authorize", json!({ "idTag": "GOOD" })).unwrap();
        let bad = V1_6::decode_call("Authorize", json!({ "idTag": "NOPE" })).unwrap();
        let r_good = handler.handle_call(ConnectionId(1), good).await.unwrap();
        let r_bad = handler.handle_call(ConnectionId(1), bad).await.unwrap();
        assert_eq!(status(&r_good), "Accepted");
        assert_eq!(status(&r_bad), "Invalid");
    }

    #[tokio::test]
    /// OC-R-076 — a transaction start (naming a connector) is checked against that connector's effective set only.
    async fn ut_start_transaction_gated_per_connector() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        // CONN1 is allowed only on connector 1; the inherited CS tag is allowed anywhere.
        let mut store = RfidStore {
            cs: vec!["CS".to_string()],
            ..Default::default()
        };
        store.add(Scope::connector(1), "CONN1".to_string());
        let handler = CsmsHandler16::new(tx, rfids(store));
        let status = |resp: &Response16| {
            V1_6::encode_response(resp).unwrap()["idTagInfo"]["status"]
                .as_str()
                .unwrap()
                .to_string()
        };
        let start = |connector: i64, tag: &str| {
            V1_6::decode_call(
                "StartTransaction",
                json!({ "connectorId": connector, "idTag": tag,
                        "meterStart": 0, "timestamp": "2030-01-01T00:00:00Z" }),
            )
            .unwrap()
        };
        // connector tag on its own connector, and the inherited CS tag, are accepted;
        let r_own = handler
            .handle_call(ConnectionId(1), start(1, "CONN1"))
            .await
            .unwrap();
        let r_cs = handler
            .handle_call(ConnectionId(1), start(2, "CS"))
            .await
            .unwrap();
        // the connector-1 tag is rejected on connector 2.
        let r_other = handler
            .handle_call(ConnectionId(1), start(2, "CONN1"))
            .await
            .unwrap();
        assert_eq!(status(&r_own), "Accepted");
        assert_eq!(status(&r_cs), "Accepted");
        assert_eq!(status(&r_other), "Invalid");
    }

    #[tokio::test]
    /// OC-R-075 — an empty effective accept-set accepts every tag.
    async fn ut_empty_rfid_list_accepts_all() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let handler = CsmsHandler16::new(tx, empty_rfids());
        let action = V1_6::decode_call("Authorize", json!({ "idTag": "ANYTHING" })).unwrap();
        let resp = handler.handle_call(ConnectionId(1), action).await.unwrap();
        assert_eq!(
            V1_6::encode_response(&resp).unwrap()["idTagInfo"]["status"],
            "Accepted"
        );
    }
}
