//! OCPP 2.0.1 inbound (CS→CSMS) handler for the CSMS role. Default-derived Accepted/empty replies,
//! except BootNotification (Accepted + currentTime + interval) and Heartbeat (currentTime).
//! TransactionEvent uses the default response. Every Call + reply is forwarded to the view.

use std::future::Future;

use serde_json::{Value, json};

use ferrowl_ocpp::csms::{ConnectionId, CsmsActionHandler};
use ferrowl_ocpp::{Action201, CallError, CallErrorCode, Response201, V2_0_1, Version};

use crate::module::ocpp::client::backend::rfc3339_now;
use crate::module::ocpp::server::backend::{EventTx, ServerEvent};

/// CSMS inbound handler for OCPP 2.0.1.
pub struct CsmsHandler201 {
    tx: EventTx,
}

impl CsmsHandler201 {
    pub fn new(tx: EventTx) -> Self {
        Self { tx }
    }

    fn respond(&self, name: &str, action: &Action201) -> Result<Response201, CallError> {
        let crafted: Option<Value> = match name {
            "BootNotification" => Some(json!({
                "currentTime": rfc3339_now(),
                "interval": 300,
                "status": "Accepted",
            })),
            "Heartbeat" => Some(json!({ "currentTime": rfc3339_now() })),
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
        let result = self.respond(&name, &action);
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
