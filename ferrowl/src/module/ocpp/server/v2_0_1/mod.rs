//! OCPP 2.0.1 CSMS (server) specifics: the two observed-state types, the inbound handler, and the
//! [`ServerVersion`] glue that lets the generic server view drive OCPP 2.0.1.

mod handler;
mod state;

use ferrowl_ocpp::V2_0_1;

use crate::module::ocpp::server::backend::{EventTx, RfidList, Scope};
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

    fn inbound_connector(_name: &str, request: &serde_json::Value) -> Scope {
        let evse = request["evse"]["id"]
            .as_i64()
            .or_else(|| request["evseId"].as_i64());
        let connector = request["evse"]["connectorId"]
            .as_i64()
            .or_else(|| request["connectorId"].as_i64())
            .filter(|&c| c >= 1);
        match evse {
            Some(e) if e >= 1 => Scope::evse(e, connector),
            _ => Scope::CS,
        }
    }

    fn config_action() -> &'static str {
        "GetVariables"
    }

    fn config_request(key: &str) -> serde_json::Value {
        // GetVariables needs an explicit component + variable. Accept "Component/Variable";
        // a bare key uses it for both names.
        let (component, variable) = key.split_once('/').unwrap_or((key, key));
        serde_json::json!({
            "getVariableData": [{
                "component": { "name": component },
                "variable": { "name": variable },
            }]
        })
    }

    fn parse_config(response: &serde_json::Value) -> Vec<(String, String, bool)> {
        let mut rows = Vec::new();
        let Some(results) = response["getVariableResult"].as_array() else {
            return rows;
        };
        for r in results {
            let component = r["component"]["name"].as_str().unwrap_or_default();
            let variable = r["variable"]["name"].as_str().unwrap_or_default();
            let status = r["attributeStatus"].as_str().unwrap_or("Unknown");
            let value = if status == "Accepted" {
                r["attributeValue"].as_str().unwrap_or_default().to_string()
            } else {
                format!("<{status}>")
            };
            // 2.0.1 mutability is per-attribute (not a simple bool); treat as writable.
            rows.push((format!("{component}/{variable}"), value, false));
        }
        rows
    }

    fn set_action() -> &'static str {
        "SetVariables"
    }

    fn set_request(key: &str, value: &str) -> serde_json::Value {
        let (component, variable) = key.split_once('/').unwrap_or((key, key));
        serde_json::json!({
            "setVariableData": [{
                "attributeValue": value,
                "component": { "name": component },
                "variable": { "name": variable },
            }]
        })
    }

    fn action_spec(name: &str) -> Option<crate::module::ocpp::action_dialog::ActionSpec> {
        crate::module::ocpp::spec::v2_0_1::action_spec(name)
    }

    fn json_actions() -> &'static [&'static str] {
        crate::module::ocpp::spec::v2_0_1::json_actions()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ut_inbound_connector_scope() {
        // Nested evse object carries both EVSE id and connector → distinct scopes per connector.
        let a = V2_0_1::inbound_connector(
            "TransactionEvent",
            &json!({ "evse": { "id": 1, "connectorId": 1 } }),
        );
        let b = V2_0_1::inbound_connector(
            "TransactionEvent",
            &json!({ "evse": { "id": 1, "connectorId": 2 } }),
        );
        assert_eq!(a, Scope::evse(1, Some(1)));
        assert_eq!(b, Scope::evse(1, Some(2)));
        assert_ne!(a, b);
        // Top-level evseId (no nested object), no connector.
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
