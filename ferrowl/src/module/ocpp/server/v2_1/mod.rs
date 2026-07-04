//! OCPP 2.1 CSMS (server) binding. 2.1 reuses the 2.0.1 CSMS behaviour, so it shares the observed
//! state types ([`crate::module::ocpp::server::v2_0_1::state`]) and the [`ServerVersion`] method
//! bodies (shared as plain free functions in
//! [`v2_common`](crate::module::ocpp::server::v2_common)); this `impl` wires in the 2.1 handler
//! type and action specs. Action specs come from [`crate::module::ocpp::spec::v2_1`], which
//! classifies the 2.1-only actions and delegates the 64 shared actions to `spec::v2_0_1`.

pub(crate) mod handler;

use crate::module::ocpp::server::backend::{EventTx, RfidLists, Scope};
use crate::module::ocpp::server::v2_common as common;
use crate::module::ocpp::server::view::ServerVersion;
use ferrowl_ocpp::V2_1;

impl ServerVersion for V2_1 {
    type Cs = crate::module::ocpp::server::v2_0_1::state::CsLevelState;
    type Conn = crate::module::ocpp::server::v2_0_1::state::ConnectorState;
    type Handler = crate::module::ocpp::server::v2_1::handler::CsmsHandler;

    fn handler(tx: EventTx, rfids: RfidLists) -> Self::Handler {
        crate::module::ocpp::server::v2_1::handler::CsmsHandler::new(tx, rfids)
    }

    fn inbound_connector(name: &str, request: &serde_json::Value) -> Scope {
        common::inbound_connector(name, request)
    }

    fn stop_tx_id(name: &str, request: &serde_json::Value) -> Option<String> {
        common::stop_tx_id(name, request)
    }

    fn inject_scope(payload: &mut serde_json::Value, scope: Scope) {
        common::inject_scope(payload, scope)
    }

    fn lua_connector_id(scope: Scope) -> Option<i64> {
        common::lua_connector_id(scope)
    }

    fn config_has_component() -> bool {
        common::config_has_component()
    }

    fn config_action() -> &'static str {
        common::config_action()
    }

    fn config_request(key: &str) -> serde_json::Value {
        common::config_request(key)
    }

    fn parse_config(response: &serde_json::Value) -> Vec<(String, String, bool)> {
        common::parse_config(response)
    }

    fn set_action() -> &'static str {
        common::set_action()
    }

    fn set_request(key: &str, value: &str) -> serde_json::Value {
        common::set_request(key, value)
    }

    fn action_spec(name: &str) -> Option<crate::module::ocpp::action_dialog::ActionSpec> {
        crate::module::ocpp::spec::v2_1::action_spec(name)
    }

    fn json_actions() -> &'static [&'static str] {
        crate::module::ocpp::spec::v2_1::json_actions()
    }

    fn json_template(name: &str) -> Option<serde_json::Value> {
        crate::module::ocpp::spec::v2_1::json_template(name)
    }
}
