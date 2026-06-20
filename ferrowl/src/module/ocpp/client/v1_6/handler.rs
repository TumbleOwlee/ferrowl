//! OCPP 1.6 inbound (CSMS→CS) handler, answered from [`CsState`]. GetConfiguration is built from
//! the config store, ChangeConfiguration writes it, Reset mutates state; every other inbound Call
//! is default-accepted (see `UNHANDLED.md`). Each inbound Call and our reply are recorded.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

use ferrowl_ocpp::cs::CsActionHandler;
use ferrowl_ocpp::v1_6::messages::change_configuration::ChangeConfigurationResponse;
use ferrowl_ocpp::v1_6::messages::get_configuration::GetConfigurationResponse;
use ferrowl_ocpp::v1_6::messages::reset::ResetResponse;
use ferrowl_ocpp::v1_6::types::{ConfigurationStatus, KeyValue, ResetResponseStatus};
use ferrowl_ocpp::{Action16, CallError, CallErrorCode, Response16, V1_6, Version};

use crate::module::ocpp::client::backend::{Dir, Messages, OcppMessage, now_ms};
use crate::module::ocpp::client::v1_6::state::CsState;

/// Inbound handler for an OCPP 1.6 charging station, backed by shared [`CsState`].
pub struct CsStateHandler {
    online: Arc<AtomicBool>,
    messages: Messages,
    state: Arc<RwLock<CsState>>,
}

impl CsStateHandler {
    pub fn new(online: Arc<AtomicBool>, messages: Messages, state: Arc<RwLock<CsState>>) -> Self {
        Self {
            online,
            messages,
            state,
        }
    }

    /// Build the response for an inbound action from state (or default-accept), and a log context.
    fn respond(&self, action: &Action16) -> (Result<Response16, CallError>, String) {
        match action {
            Action16::GetConfiguration(req) => {
                let state = self.state.read().unwrap();
                let wanted = req.key.as_deref();
                let mut keys = Vec::new();
                let mut unknown = Vec::new();
                match wanted {
                    Some(list) => {
                        for k in list {
                            match state.config.iter().find(|c| &c.key == k) {
                                Some(c) => keys.push(key_value(c)),
                                None => unknown.push(k.clone()),
                            }
                        }
                    }
                    None => keys = state.config.iter().map(key_value).collect(),
                }
                let resp = GetConfigurationResponse {
                    configuration_key: (!keys.is_empty()).then_some(keys),
                    unknown_key: (!unknown.is_empty()).then_some(unknown),
                };
                (
                    Ok(Response16::GetConfiguration(resp)),
                    "answered from config".to_string(),
                )
            }
            Action16::ChangeConfiguration(req) => {
                let mut state = self.state.write().unwrap();
                let status = match state.config.iter_mut().find(|c| c.key == req.key) {
                    Some(c) if c.readonly => ConfigurationStatus::Rejected,
                    Some(c) => {
                        c.value = req.value.clone();
                        ConfigurationStatus::Accepted
                    }
                    None => {
                        state.config.push(super::state::ConfigKey {
                            key: req.key.clone(),
                            value: req.value.clone(),
                            readonly: false,
                        });
                        ConfigurationStatus::Accepted
                    }
                };
                (
                    Ok(Response16::ChangeConfiguration(ChangeConfigurationResponse {
                        status,
                    })),
                    format!("{} = {}", req.key, req.value),
                )
            }
            Action16::Reset(_) => {
                let mut state = self.state.write().unwrap();
                state.status = "Available".to_string();
                state.transaction_id = None;
                state.session_energy = 0.0;
                (
                    Ok(Response16::Reset(ResetResponse {
                        status: ResetResponseStatus::Accepted,
                    })),
                    "state reset".to_string(),
                )
            }
            other => {
                let name = V1_6::action_name(other);
                match V1_6::default_response(name) {
                    Some(resp) => (Ok(resp), "default-accepted".to_string()),
                    None => (
                        Err(CallError::new(
                            CallErrorCode::NotImplemented,
                            "action not handled by the charging-station simulator",
                        )),
                        "not implemented".to_string(),
                    ),
                }
            }
        }
    }
}

impl CsActionHandler<V1_6> for CsStateHandler {
    fn handle_call(
        &self,
        action: Action16,
    ) -> impl Future<Output = Result<Response16, CallError>> + Send {
        let name = V1_6::action_name(&action).to_string();
        let request = V1_6::encode_action(&action).unwrap_or(serde_json::Value::Null);
        let (result, context) = self.respond(&action);
        let reply_payload = match &result {
            Ok(resp) => V1_6::encode_response(resp).unwrap_or(serde_json::Value::Null),
            Err(_) => serde_json::Value::Null,
        };
        let ok = result.is_ok();
        let messages = self.messages.clone();
        async move {
            // Record the inbound Call, then our reply.
            messages.write().await.push(OcppMessage {
                ts: now_ms(),
                direction: Dir::In,
                name: name.clone(),
                payload: request,
                ok: None,
                context: "inbound call".to_string(),
            });
            messages.write().await.push(OcppMessage {
                ts: now_ms(),
                direction: Dir::Out,
                name,
                payload: reply_payload,
                ok: Some(ok),
                context,
            });
            result
        }
    }

    fn on_connected(&self) -> impl Future<Output = ()> + Send {
        let online = self.online.clone();
        async move {
            online.store(true, Ordering::Relaxed);
        }
    }

    fn on_disconnected(&self) -> impl Future<Output = ()> + Send {
        let online = self.online.clone();
        async move {
            online.store(false, Ordering::Relaxed);
        }
    }
}

/// Map a stored config key to the wire `KeyValue`.
fn key_value(c: &super::state::ConfigKey) -> KeyValue {
    KeyValue {
        key: c.key.clone(),
        readonly: c.readonly,
        value: Some(c.value.clone()),
    }
}
