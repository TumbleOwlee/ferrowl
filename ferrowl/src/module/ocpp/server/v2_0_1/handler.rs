//! OCPP 2.0.1 inbound (CS→CSMS) handler for the CSMS role. The handler body is shared with 2.1 via
//! [`define_csms_handler!`](crate::module::ocpp::server::v2_common::define_csms_handler) and is
//! instantiated here for `V2_0_1`. Default-derived Accepted/empty replies, except BootNotification,
//! Heartbeat, and the RFID-gated Authorize / transaction-starting TransactionEvent.

use crate::module::ocpp::server::v2_common::define_csms_handler;

define_csms_handler!(V2_0_1, Action201, Response201);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::ocpp::scope::Scope;
    use crate::module::ocpp::server::backend::{RfidLists, RfidStore};
    use ferrowl_ocpp::csms::ConnectionId;
    use ferrowl_ocpp::{Action201 as Action, Response201 as Response, V2_0_1 as Ver, Version};
    use serde_json::json;

    fn rfids(store: RfidStore) -> RfidLists {
        std::sync::Arc::new(std::sync::RwLock::new(store))
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
