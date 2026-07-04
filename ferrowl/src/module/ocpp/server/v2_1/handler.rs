//! OCPP 2.1 inbound (CS→CSMS) handler for the CSMS role: the same decision logic as 2.0.1's
//! handler (`v2_0_1::handler`), shared via
//! [`v2_common`](crate::module::ocpp::server::v2_common), but typed over `Action21`/`Response21`,
//! so the two are separate (non-generic) impls.

use std::future::Future;

use serde_json::Value;

use ferrowl_ocpp::csms::{ConnectionId, CsmsActionHandler};
use ferrowl_ocpp::{Action21, CallError, CallErrorCode, Response21, V2_1, Version};

use crate::module::ocpp::server::backend::{EventTx, RfidLists, ServerEvent};
use crate::module::ocpp::server::v2_common::craft_response;

/// CSMS inbound handler for OCPP 2.1.
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
        action: &Action21,
        request: &Value,
    ) -> Result<Response21, CallError> {
        match craft_response(name, request, &self.rfids) {
            Some(payload) => V2_1::decode_result(action, payload)
                .map_err(|e| CallError::new(CallErrorCode::InternalError, e.to_string())),
            None => V2_1::default_response(name).ok_or_else(|| {
                CallError::new(
                    CallErrorCode::NotImplemented,
                    "action not handled by the CSMS",
                )
            }),
        }
    }
}

impl CsmsActionHandler<V2_1> for CsmsHandler {
    fn handle_call(
        &self,
        conn: ConnectionId,
        action: Action21,
    ) -> impl Future<Output = Result<Response21, CallError>> + Send {
        let name = V2_1::action_name(&action).to_string();
        let request = V2_1::encode_action(&action).unwrap_or(Value::Null);
        let result = self.respond(&name, &action, &request);
        let response = match &result {
            Ok(resp) => V2_1::encode_response(resp).unwrap_or(Value::Null),
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
