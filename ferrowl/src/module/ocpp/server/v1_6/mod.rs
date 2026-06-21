//! OCPP 1.6 CSMS (server) specifics: the two observed-state types, the inbound handler, and the
//! [`ServerVersion`] glue that lets the generic server view drive OCPP 1.6.

mod handler;
mod state;

use ferrowl_ocpp::V1_6;

use crate::module::ocpp::server::backend::{EventTx, RfidList};
use crate::module::ocpp::server::view::ServerVersion;

use handler::CsmsHandler16;
use state::{ConnectorState, CsLevelState};

impl ServerVersion for V1_6 {
    type Cs = CsLevelState;
    type Conn = ConnectorState;
    type Handler = CsmsHandler16;

    fn handler(tx: EventTx, rfids: RfidList) -> Self::Handler {
        CsmsHandler16::new(tx, rfids)
    }

    fn inbound_connector(_name: &str, request: &serde_json::Value) -> Option<i64> {
        request
            .get("connectorId")
            .and_then(|v| v.as_i64())
            .filter(|&c| c >= 1)
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

    fn parse_config(response: &serde_json::Value) -> Vec<(String, String)> {
        let mut rows = Vec::new();
        if let Some(keys) = response["configurationKey"].as_array() {
            for k in keys {
                let Some(name) = k["key"].as_str() else {
                    continue;
                };
                let value = k["value"].as_str().unwrap_or_default();
                let ro = if k["readonly"].as_bool().unwrap_or(false) {
                    " (ro)"
                } else {
                    ""
                };
                rows.push((name.to_string(), format!("{value}{ro}")));
            }
        }
        // Keys the CS rejected as unknown are surfaced too.
        if let Some(unknown) = response["unknownKey"].as_array() {
            for k in unknown.iter().filter_map(|k| k.as_str()) {
                rows.push((k.to_string(), "<unknown>".to_string()));
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        assert_eq!(rows[0], ("HeartbeatInterval".into(), "30".into()));
        assert_eq!(
            rows[1],
            ("MeterValueSampleInterval".into(), "60 (ro)".into())
        );
        assert_eq!(rows[2], ("Foo".into(), "<unknown>".into()));
    }
}
