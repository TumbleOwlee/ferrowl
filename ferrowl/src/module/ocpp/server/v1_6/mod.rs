//! OCPP 1.6 CSMS (server) specifics: the two observed-state types, the inbound handler, and the
//! [`ServerVersion`] glue that lets the generic server view drive OCPP 1.6.

mod handler;
mod state;

use ferrowl_ocpp::V1_6;

use crate::module::ocpp::server::backend::EventTx;
use crate::module::ocpp::server::view::ServerVersion;

use handler::CsmsHandler16;
use state::{ConnectorState, CsLevelState};

impl ServerVersion for V1_6 {
    type Cs = CsLevelState;
    type Conn = ConnectorState;
    type Handler = CsmsHandler16;

    fn handler(tx: EventTx) -> Self::Handler {
        CsmsHandler16::new(tx)
    }

    fn inbound_connector(_name: &str, request: &serde_json::Value) -> Option<i64> {
        request
            .get("connectorId")
            .and_then(|v| v.as_i64())
            .filter(|&c| c >= 1)
    }
}
