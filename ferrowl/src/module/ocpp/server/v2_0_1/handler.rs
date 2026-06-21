//! OCPP 2.0.1 inbound (CS→CSMS) handler for the CSMS role. Default-derived Accepted/empty replies,
//! except BootNotification (Accepted + currentTime + interval) and Heartbeat (currentTime).
//! TransactionEvent uses the default response. Every Call + reply is forwarded to the view.

use std::future::Future;

use serde_json::{Value, json};

use ferrowl_ocpp::csms::{ConnectionId, CsmsActionHandler};
use ferrowl_ocpp::{Action201, CallError, CallErrorCode, Response201, V2_0_1, Version};

use crate::module::ocpp::client::backend::rfc3339_now;
use crate::module::ocpp::server::backend::{EventTx, RfidList, ServerEvent, rfid_accepted};

/// CSMS inbound handler for OCPP 2.0.1.
pub struct CsmsHandler201 {
    tx: EventTx,
    rfids: RfidList,
}

impl CsmsHandler201 {
    pub fn new(tx: EventTx, rfids: RfidList) -> Self {
        Self { tx, rfids }
    }

    /// `"Accepted"` / `"Invalid"` for an id token, per the configured accept-list.
    fn id_token_status(&self, id_token: &str) -> &'static str {
        if rfid_accepted(&self.rfids, id_token) {
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
            "Authorize" => {
                let tag = request["idToken"]["idToken"].as_str().unwrap_or_default();
                Some(json!({ "idTokenInfo": { "status": self.id_token_status(tag) } }))
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
    use ferrowl_ocpp::csms::ConnectionId;

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
        let rfids = std::sync::Arc::new(std::sync::RwLock::new(vec!["GOOD".to_string()]));
        let handler = CsmsHandler201::new(tx, rfids);
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
        let rfids = std::sync::Arc::new(std::sync::RwLock::new(Vec::new()));
        let handler = CsmsHandler201::new(tx, rfids);
        let resp = handler
            .handle_call(ConnectionId(1), authorize("ANYTHING"))
            .await
            .unwrap();
        assert_eq!(status(&resp), "Accepted");
    }
}
