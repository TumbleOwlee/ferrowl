//! Shared OCPP 2.x CSMS (server) bindings, generated for both 2.0.1 and 2.1.
//!
//! Like the client side, 2.1 reuses the 2.0.1 CSMS behaviour. The inbound handler builds replies as
//! JSON (decoded into the version's typed `Response` via `decode_result`/`default_response`), and
//! the `ServerVersion` glue is pure JSON/string logic, so both live here once and are instantiated
//! per version over the shared [`crate::module::ocpp::server::v2_0_1::state`] types. Macros take
//! plain idents and build full paths from them (`:ident` may precede `::`, `:path` may not).

/// Emit a version's `CsmsHandler` (struct + inbound `CsmsActionHandler` impl) for marker
/// `ferrowl_ocpp::$marker`, typed over the version's `$Action`/`$Response` wire enums.
macro_rules! define_csms_handler {
    ($marker:ident, $Action:ident, $Response:ident) => {
use std::future::Future;

use serde_json::{Value, json};

use ferrowl_ocpp::csms::{ConnectionId, CsmsActionHandler};
use ferrowl_ocpp::{CallError, CallErrorCode, Version};

use crate::module::ocpp::client::backend::rfc3339_now;
use crate::module::ocpp::scope::Scope;
use crate::module::ocpp::server::backend::{
    EventTx, RfidLists, ServerEvent, cs_authorized, scope_authorized,
};

/// CSMS inbound handler for OCPP 2.0.1.
pub struct CsmsHandler {
    tx: EventTx,
    rfids: RfidLists,
}

impl CsmsHandler {
    pub fn new(tx: EventTx, rfids: RfidLists) -> Self {
        Self { tx, rfids }
    }

    /// `"Accepted"` / `"Invalid"` for a CS-wide check (Authorize carries no EVSE).
    fn cs_status(&self, id_token: &str) -> &'static str {
        if cs_authorized(&self.rfids, id_token) {
            "Accepted"
        } else {
            "Invalid"
        }
    }

    /// `"Accepted"` / `"Invalid"` for a tag on a specific EVSE (its list ∪ the CS list).
    fn evse_status(&self, evse_id: i64, id_token: &str) -> &'static str {
        if scope_authorized(&self.rfids, Scope::evse(evse_id, None), id_token) {
            "Accepted"
        } else {
            "Invalid"
        }
    }

    fn respond(
        &self,
        name: &str,
        action: &ferrowl_ocpp::$Action,
        request: &Value,
    ) -> Result<ferrowl_ocpp::$Response, CallError> {
        let crafted: Option<Value> = match name {
            "BootNotification" => Some(json!({
                "currentTime": rfc3339_now(),
                "interval": 300,
                "status": "Accepted",
            })),
            "Heartbeat" => Some(json!({ "currentTime": rfc3339_now() })),
            // Authorize carries no EVSE, so it is checked against the CS list unioned with every
            // connector list.
            "Authorize" => {
                let tag = request["idToken"]["idToken"].as_str().unwrap_or_default();
                Some(json!({ "idTokenInfo": { "status": self.cs_status(tag) } }))
            }
            // A TransactionEvent carrying an idToken (a transaction start) names an EVSE, so it is
            // gated by that EVSE's list ∪ the CS list; the reply echoes the decision.
            "TransactionEvent" if request["idToken"].is_object() => {
                let tag = request["idToken"]["idToken"].as_str().unwrap_or_default();
                let evse = request["evse"]["id"].as_i64().unwrap_or_default();
                Some(json!({ "idTokenInfo": { "status": self.evse_status(evse, tag) } }))
            }
            _ => None,
        };
        match crafted {
            Some(payload) => ferrowl_ocpp::$marker::decode_result(action, payload)
                .map_err(|e| CallError::new(CallErrorCode::InternalError, e.to_string())),
            None => ferrowl_ocpp::$marker::default_response(name).ok_or_else(|| {
                CallError::new(
                    CallErrorCode::NotImplemented,
                    "action not handled by the CSMS",
                )
            }),
        }
    }
}

impl CsmsActionHandler<ferrowl_ocpp::$marker> for CsmsHandler {
    fn handle_call(
        &self,
        conn: ConnectionId,
        action: ferrowl_ocpp::$Action,
    ) -> impl Future<Output = Result<ferrowl_ocpp::$Response, CallError>> + Send {
        let name = ferrowl_ocpp::$marker::action_name(&action).to_string();
        let request = ferrowl_ocpp::$marker::encode_action(&action).unwrap_or(Value::Null);
        let result = self.respond(&name, &action, &request);
        let response = match &result {
            Ok(resp) => ferrowl_ocpp::$marker::encode_response(resp).unwrap_or(Value::Null),
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
    };
}

pub(crate) use define_csms_handler;

/// Emit the `ServerVersion` impl for marker `ferrowl_ocpp::$marker`, wiring the shared observed
/// state to the CSMS handler in `server::$ver::handler` and action specs in `spec::$specver`.
macro_rules! define_server_version {
    ($marker:ident, $ver:ident, $specver:ident) => {
        use crate::module::ocpp::server::backend::{EventTx, RfidLists, Scope};
        use crate::module::ocpp::server::view::ServerVersion;

impl ServerVersion for ferrowl_ocpp::$marker {
    type Cs = crate::module::ocpp::server::v2_0_1::state::CsLevelState;
    type Conn = crate::module::ocpp::server::v2_0_1::state::ConnectorState;
    type Handler = crate::module::ocpp::server::$ver::handler::CsmsHandler;

    fn handler(tx: EventTx, rfids: RfidLists) -> Self::Handler {
        crate::module::ocpp::server::$ver::handler::CsmsHandler::new(tx, rfids)
    }

    fn inbound_connector(_name: &str, request: &serde_json::Value) -> Scope {
        // 2.0.1 connectors are listed and addressed by EVSE id only; a nested/top-level
        // `connectorId` is ignored for bucketing (connector kept `None`).
        let evse = request["evse"]["id"]
            .as_i64()
            .or_else(|| request["evseId"].as_i64());
        match evse {
            Some(e) if e >= 1 => Scope::evse(e, None),
            _ => Scope::CS,
        }
    }

    fn stop_tx_id(name: &str, request: &serde_json::Value) -> Option<String> {
        // A TransactionEvent(Ended) may omit `evse`, in which case it buckets to CS scope; route it
        // to the connector holding this transactionId instead.
        (name == "TransactionEvent" && request["eventType"].as_str() == Some("Ended"))
            .then(|| {
                request["transactionInfo"]["transactionId"]
                    .as_str()
                    .map(str::to_owned)
            })
            .flatten()
    }

    fn inject_scope(payload: &mut serde_json::Value, scope: Scope) {
        if let (Some(e), Some(obj)) = (scope.evse, payload.as_object_mut()) {
            // Set the EVSE id when absent or still the `0` default the encoded request struct
            // carries; a genuine non-zero value (and later user overrides) win.
            let cur = obj.get("evseId").and_then(|v| v.as_i64());
            if cur.is_none() || cur == Some(0) {
                obj.insert("evseId".into(), serde_json::json!(e));
            }
        }
    }

    fn lua_connector_id(scope: Scope) -> Option<i64> {
        // 2.0.1 connectors are addressed by EVSE id (the connector dimension is always `None`).
        scope.evse
    }

    fn config_has_component() -> bool {
        // GetVariables/SetVariables keys are `Component/Variable`; show a Component column.
        true
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
        crate::module::ocpp::spec::$specver::action_spec(name)
    }

    fn json_actions() -> &'static [&'static str] {
        crate::module::ocpp::spec::$specver::json_actions()
    }
}
    };
}

pub(crate) use define_server_version;
