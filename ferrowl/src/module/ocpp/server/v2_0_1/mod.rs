//! OCPP 2.0.1 CSMS (server) specifics. The observed-state types live in [`state`] and are shared
//! with 2.1; the inbound handler ([`handler`]) is its own type but the [`ServerVersion`] method
//! bodies are shared as plain free functions in
//! [`v2_common`](crate::module::ocpp::server::v2_common); this `impl` wires in the 2.0.1 handler
//! type and action specs (the two seams that actually differ per version).

pub(crate) mod handler;
pub(crate) mod state;

use crate::module::ocpp::server::backend::{EventTx, RfidLists, Scope};
use crate::module::ocpp::server::v2_common as common;
use crate::module::ocpp::server::view::ServerVersion;
use ferrowl_ocpp::V2_0_1;

impl ServerVersion for V2_0_1 {
    type Cs = crate::module::ocpp::server::v2_0_1::state::CsLevelState;
    type Conn = crate::module::ocpp::server::v2_0_1::state::ConnectorState;
    type Handler = crate::module::ocpp::server::v2_0_1::handler::CsmsHandler;

    fn handler(tx: EventTx, rfids: RfidLists) -> Self::Handler {
        crate::module::ocpp::server::v2_0_1::handler::CsmsHandler::new(tx, rfids)
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
        crate::module::ocpp::spec::v2_0_1::action_spec(name)
    }

    fn json_actions() -> &'static [&'static str] {
        crate::module::ocpp::spec::v2_0_1::json_actions()
    }

    fn json_template(name: &str) -> Option<serde_json::Value> {
        crate::module::ocpp::spec::v2_0_1::json_template(name)
    }
}

#[cfg(test)]
mod tests {
    use crate::module::ocpp::server::backend::Scope;
    use crate::module::ocpp::server::view::ServerVersion;
    use ferrowl_ocpp::V2_0_1;
    use serde_json::json;

    #[test]
    /// OC-R-078 — an inbound 2.0.1 Call is tagged with the charge-point/EVSE scope it belongs to.
    fn ut_inbound_connector_scope() {
        // Connectors are bucketed by EVSE id only — a nested `connectorId` is ignored, so two
        // connectors on the same EVSE map to the same (connector-less) scope.
        let a = V2_0_1::inbound_connector(
            "TransactionEvent",
            &json!({ "evse": { "id": 1, "connectorId": 1 } }),
        );
        let b = V2_0_1::inbound_connector(
            "TransactionEvent",
            &json!({ "evse": { "id": 1, "connectorId": 2 } }),
        );
        assert_eq!(a, Scope::evse(1, None));
        assert_eq!(b, Scope::evse(1, None));
        assert_eq!(a, b);
        // Top-level evseId (no nested object).
        assert_eq!(
            V2_0_1::inbound_connector("MeterValues", &json!({ "evseId": 2 })),
            Scope::evse(2, None)
        );
        // No EVSE → CS-level.
        assert_eq!(
            V2_0_1::inbound_connector("BootNotification", &json!({})),
            Scope::CS
        );
    }

    #[test]
    /// OC-R-078 — an outbound 2.0.1 Call is tagged with the EVSE scope it targets, defaulting the EVSE id.
    fn ut_inject_scope_defaults_evse_id() {
        // A connector-scoped Lua payload gets the EVSE id when it lacks one.
        let mut p = json!({});
        V2_0_1::inject_scope(&mut p, Scope::evse(3, None));
        assert_eq!(p["evseId"], 3);
        // The `0` default an encoded request struct carries is treated as unset and overwritten.
        let mut p0 = json!({ "evseId": 0 });
        V2_0_1::inject_scope(&mut p0, Scope::evse(3, None));
        assert_eq!(p0["evseId"], 3);
        // A genuine non-zero evseId is preserved.
        let mut p2 = json!({ "evseId": 8 });
        V2_0_1::inject_scope(&mut p2, Scope::evse(3, None));
        assert_eq!(p2["evseId"], 8);
        // CS-level scope is a no-op.
        let mut p3 = json!({});
        V2_0_1::inject_scope(&mut p3, Scope::CS);
        assert!(p3.get("evseId").is_none());
    }

    #[test]
    fn ut_lua_connector_id_uses_evse() {
        // Lua addresses 2.0.1 connectors by EVSE id (connector dimension is always None).
        assert_eq!(V2_0_1::lua_connector_id(Scope::evse(2, None)), Some(2));
        assert_eq!(V2_0_1::lua_connector_id(Scope::evse(3, Some(1))), Some(3));
        // CS-level is not a connector.
        assert_eq!(V2_0_1::lua_connector_id(Scope::CS), None);
    }

    #[test]
    /// OC-R-065 — a 2.0.1 GetVariables request splits into component/variable targets to select which keys are read.
    fn ut_config_request_splits_component_variable() {
        let req = V2_0_1::config_request("OCPPCommCtrlr/HeartbeatInterval");
        assert_eq!(
            req["getVariableData"][0]["component"]["name"],
            "OCPPCommCtrlr"
        );
        assert_eq!(
            req["getVariableData"][0]["variable"]["name"],
            "HeartbeatInterval"
        );
        // A bare key uses it for both component and variable.
        let bare = V2_0_1::config_request("Foo");
        assert_eq!(bare["getVariableData"][0]["component"]["name"], "Foo");
        assert_eq!(bare["getVariableData"][0]["variable"]["name"], "Foo");
    }

    #[test]
    /// OC-R-065 — a 2.0.1 GetVariables request's key list is parsed to select which configuration keys are read.
    fn ut_parse_get_variables() {
        let resp = json!({
            "getVariableResult": [
                {
                    "attributeStatus": "Accepted",
                    "attributeValue": "30",
                    "component": { "name": "OCPPCommCtrlr" },
                    "variable": { "name": "HeartbeatInterval" },
                },
                {
                    "attributeStatus": "Rejected",
                    "component": { "name": "X" },
                    "variable": { "name": "Y" },
                },
            ]
        });
        let rows = V2_0_1::parse_config(&resp);
        assert_eq!(
            rows[0],
            ("OCPPCommCtrlr/HeartbeatInterval".into(), "30".into(), false)
        );
        assert_eq!(rows[1], ("X/Y".into(), "<Rejected>".into(), false));
    }
}
