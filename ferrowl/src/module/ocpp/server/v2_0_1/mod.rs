//! OCPP 2.0.1 CSMS (server) specifics. The observed-state types live in [`state`] and are shared
//! with 2.1; the inbound handler ([`handler`]) and the [`ServerVersion`] glue are shared via the
//! macros in [`crate::module::ocpp::server::v2_common`] and instantiated here for `V2_0_1`.

pub(crate) mod handler;
pub(crate) mod state;

use crate::module::ocpp::server::v2_common::define_server_version;

define_server_version!(V2_0_1, v2_0_1, v2_0_1);

#[cfg(test)]
mod tests {
    use crate::module::ocpp::server::backend::Scope;
    use crate::module::ocpp::server::view::ServerVersion;
    use ferrowl_ocpp::V2_0_1;
    use serde_json::json;

    #[test]
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
