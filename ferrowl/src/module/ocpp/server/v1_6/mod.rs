//! OCPP 1.6 CSMS (server) specifics: the two observed-state types, the inbound handler, and the
//! [`ServerVersion`] glue that lets the generic server view drive OCPP 1.6.

mod handler;
mod state;

use ferrowl_ocpp::V1_6;

use crate::module::ocpp::server::backend::{EventTx, RfidLists, Scope};
use crate::module::ocpp::server::view::ServerVersion;

use handler::CsmsHandler16;
use state::{ConnectorState, CsLevelState};

impl ServerVersion for V1_6 {
    type Cs = CsLevelState;
    type Conn = ConnectorState;
    type Handler = CsmsHandler16;

    fn handler(tx: EventTx, rfids: RfidLists) -> Self::Handler {
        CsmsHandler16::new(tx, rfids)
    }

    fn inbound_connector(_name: &str, request: &serde_json::Value) -> Scope {
        match request.get("connectorId").and_then(|v| v.as_i64()) {
            Some(c) if c >= 1 => Scope::connector(c),
            _ => Scope::CS,
        }
    }

    fn stop_tx_id(name: &str, request: &serde_json::Value) -> Option<String> {
        // StopTransaction.req carries no connectorId, only the transactionId.
        (name == "StopTransaction")
            .then(|| request["transactionId"].as_i64().map(|t| t.to_string()))
            .flatten()
    }

    fn inject_scope(payload: &mut serde_json::Value, scope: Scope) {
        if let (Some(c), Some(obj)) = (scope.connector, payload.as_object_mut()) {
            // Set the connector id when absent or still the `0` default the encoded request struct
            // carries; a genuine non-zero value (and later user overrides) win.
            let cur = obj.get("connectorId").and_then(|v| v.as_i64());
            if cur.is_none() || cur == Some(0) {
                obj.insert("connectorId".into(), serde_json::json!(c));
            }
        }
    }

    fn config_action() -> &'static str {
        "GetConfiguration"
    }

    fn config_request(key: &str) -> serde_json::Value {
        // An empty key dumps all configuration keys; a non-empty key fetches just that one.
        if key.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::json!({ "key": [key] })
        }
    }

    fn parse_config(response: &serde_json::Value) -> Vec<(String, String, bool)> {
        let mut rows = Vec::new();
        if let Some(keys) = response["configurationKey"].as_array() {
            for k in keys {
                let Some(name) = k["key"].as_str() else {
                    continue;
                };
                let value = k["value"].as_str().unwrap_or_default();
                let readonly = k["readonly"].as_bool().unwrap_or(false);
                rows.push((name.to_string(), value.to_string(), readonly));
            }
        }
        // Keys the CS rejected as unknown are surfaced too (writable by default).
        if let Some(unknown) = response["unknownKey"].as_array() {
            for k in unknown.iter().filter_map(|k| k.as_str()) {
                rows.push((k.to_string(), "<unknown>".to_string(), false));
            }
        }
        rows
    }

    fn set_action() -> &'static str {
        "ChangeConfiguration"
    }

    fn set_request(key: &str, value: &str) -> serde_json::Value {
        serde_json::json!({ "key": key, "value": value })
    }

    fn action_spec(name: &str) -> Option<crate::module::ocpp::action_dialog::ActionSpec> {
        crate::module::ocpp::spec::v1_6::action_spec(name)
    }

    fn json_actions() -> &'static [&'static str] {
        crate::module::ocpp::spec::v1_6::json_actions()
    }

    fn json_template(name: &str) -> Option<serde_json::Value> {
        crate::module::ocpp::spec::v1_6::json_template(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ut_inbound_connector_scope() {
        assert_eq!(
            V1_6::inbound_connector("StatusNotification", &json!({ "connectorId": 2 })),
            Scope::connector(2)
        );
        // connectorId 0 (or absent) is the CS as a whole.
        assert_eq!(
            V1_6::inbound_connector("StatusNotification", &json!({ "connectorId": 0 })),
            Scope::CS
        );
        assert_eq!(
            V1_6::inbound_connector("BootNotification", &json!({})),
            Scope::CS
        );
    }

    #[test]
    fn ut_inject_scope_defaults_connector_id() {
        // A connector-scoped Lua payload gets the connector id when it lacks one.
        let mut p = json!({});
        V1_6::inject_scope(&mut p, Scope::connector(2));
        assert_eq!(p["connectorId"], 2);
        // The `0` default an encoded request struct carries is treated as unset and overwritten.
        let mut p0 = json!({ "connectorId": 0 });
        V1_6::inject_scope(&mut p0, Scope::connector(2));
        assert_eq!(p0["connectorId"], 2);
        // A genuine non-zero connectorId is preserved.
        let mut p2 = json!({ "connectorId": 9 });
        V1_6::inject_scope(&mut p2, Scope::connector(2));
        assert_eq!(p2["connectorId"], 9);
        // CS-level scope is a no-op.
        let mut p3 = json!({});
        V1_6::inject_scope(&mut p3, Scope::CS);
        assert!(p3.get("connectorId").is_none());
    }

    #[test]
    fn ut_config_request_all_vs_single() {
        assert_eq!(V1_6::config_request(""), json!({}));
        assert_eq!(
            V1_6::config_request("HeartbeatInterval"),
            json!({ "key": ["HeartbeatInterval"] })
        );
    }

    #[test]
    fn ut_parse_get_configuration() {
        let resp = json!({
            "configurationKey": [
                { "key": "HeartbeatInterval", "value": "30", "readonly": false },
                { "key": "MeterValueSampleInterval", "value": "60", "readonly": true },
            ],
            "unknownKey": ["Foo"],
        });
        let rows = V1_6::parse_config(&resp);
        assert_eq!(rows[0], ("HeartbeatInterval".into(), "30".into(), false));
        assert_eq!(
            rows[1],
            ("MeterValueSampleInterval".into(), "60".into(), true)
        );
        assert_eq!(rows[2], ("Foo".into(), "<unknown>".into(), false));
    }
}
