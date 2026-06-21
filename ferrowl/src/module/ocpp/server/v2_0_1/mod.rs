//! OCPP 2.0.1 CSMS (server) specifics: the two observed-state types, the inbound handler, and the
//! [`ServerVersion`] glue that lets the generic server view drive OCPP 2.0.1.

mod handler;
mod state;

use ferrowl_ocpp::V2_0_1;

use crate::module::ocpp::server::backend::{EventTx, RfidList};
use crate::module::ocpp::server::view::ServerVersion;

use handler::CsmsHandler201;
use state::{ConnectorState, CsLevelState};

impl ServerVersion for V2_0_1 {
    type Cs = CsLevelState;
    type Conn = ConnectorState;
    type Handler = CsmsHandler201;

    fn handler(tx: EventTx, rfids: RfidList) -> Self::Handler {
        CsmsHandler201::new(tx, rfids)
    }

    fn inbound_connector(_name: &str, request: &serde_json::Value) -> Option<i64> {
        request["evse"]["id"]
            .as_i64()
            .or_else(|| request["evseId"].as_i64())
            .or_else(|| request["connectorId"].as_i64())
            .filter(|&c| c >= 1)
    }
}
