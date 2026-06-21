//! OCPP 2.0.1 inbound (CSMS→CS) handler, answered from [`CsState`]. GetVariables is built from the
//! variable store, SetVariables writes it, Reset mutates state; every other inbound Call is
//! default-accepted (see `UNHANDLED.md`). Each inbound Call and our reply are recorded.

use std::future::Future;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

use ferrowl_ocpp::cs::CsActionHandler;
use ferrowl_ocpp::v2_0_1::datatypes::get_variable_result_type::GetVariableResultType;
use ferrowl_ocpp::v2_0_1::datatypes::set_variable_result_type::SetVariableResultType;
use ferrowl_ocpp::v2_0_1::enumerations::get_variable_status_enum_type::GetVariableStatusEnumType;
use ferrowl_ocpp::v2_0_1::enumerations::reset_status_enum_type::ResetStatusEnumType;
use ferrowl_ocpp::v2_0_1::enumerations::set_variable_status_enum_type::SetVariableStatusEnumType;
use ferrowl_ocpp::v2_0_1::messages::get_variables::GetVariablesResponse;
use ferrowl_ocpp::v2_0_1::messages::reset::ResetResponse;
use ferrowl_ocpp::v2_0_1::messages::set_variables::SetVariablesResponse;
use ferrowl_ocpp::{Action201, CallError, CallErrorCode, Response201, V2_0_1, Version};

use crate::module::ocpp::client::backend::{Dir, Messages, OcppMessage, push_capped};
use crate::module::ocpp::client::config::ConfigKey;
use crate::module::ocpp::client::v2_0_1::state::CsState;

/// Inbound handler for an OCPP 2.0.1 charging station, backed by shared [`CsState`].
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

    fn respond(&self, action: &Action201) -> (Result<Response201, CallError>, String) {
        match action {
            Action201::GetVariables(req) => {
                let state = self.state.read().unwrap();
                let results = req
                    .get_variable_data
                    .iter()
                    .map(|d| {
                        let found = state.config.iter().find(|c| c.key == d.variable.name);
                        GetVariableResultType {
                            attribute_status: match found {
                                Some(_) => GetVariableStatusEnumType::Accepted,
                                None => GetVariableStatusEnumType::UnknownVariable,
                            },
                            attribute_type: d.attribute_type.clone(),
                            attribute_value: found.map(|c| c.value.clone()),
                            component: d.component.clone(),
                            variable: d.variable.clone(),
                            attribute_status_info: None,
                        }
                    })
                    .collect();
                (
                    Ok(Response201::GetVariables(GetVariablesResponse {
                        get_variable_result: results,
                    })),
                    "answered from variables".to_string(),
                )
            }
            Action201::SetVariables(req) => {
                let mut state = self.state.write().unwrap();
                let results = req
                    .set_variable_data
                    .iter()
                    .map(|d| {
                        let status =
                            match state.config.iter_mut().find(|c| c.key == d.variable.name) {
                                Some(c) if c.readonly => SetVariableStatusEnumType::Rejected,
                                Some(c) => {
                                    c.value = d.attribute_value.clone();
                                    SetVariableStatusEnumType::Accepted
                                }
                                None => {
                                    state.config.push(ConfigKey {
                                        key: d.variable.name.clone(),
                                        value: d.attribute_value.clone(),
                                        readonly: false,
                                    });
                                    SetVariableStatusEnumType::Accepted
                                }
                            };
                        SetVariableResultType {
                            attribute_type: d.attribute_type.clone(),
                            attribute_status: status,
                            component: d.component.clone(),
                            variable: d.variable.clone(),
                            attribute_status_info: None,
                        }
                    })
                    .collect();
                (
                    Ok(Response201::SetVariables(SetVariablesResponse {
                        set_variable_result: results,
                    })),
                    "variables updated".to_string(),
                )
            }
            Action201::Reset(_) => {
                let mut state = self.state.write().unwrap();
                state.status = "Available".to_string();
                state.transaction_id = None;
                state.session_energy = 0.0;
                (
                    Ok(Response201::Reset(ResetResponse {
                        status: ResetStatusEnumType::Accepted,
                        status_info: None,
                    })),
                    "state reset".to_string(),
                )
            }
            Action201::SetChargingProfile(_) => {
                let json = V2_0_1::encode_action(action).unwrap_or(serde_json::Value::Null);
                let schedule = &json["chargingProfile"]["chargingSchedule"][0];
                let period = &schedule["chargingSchedulePeriod"][0];
                let context = if let Some(limit) = period["limit"].as_f64() {
                    let unit = schedule["chargingRateUnit"]
                        .as_str()
                        .unwrap_or("A")
                        .to_string();
                    let mut state = self.state.write().unwrap();
                    state.limit = Some(limit);
                    state.limit_unit = unit.clone();
                    format!("limit {limit} {unit}")
                } else {
                    "no limit in profile".to_string()
                };
                let resp = V2_0_1::default_response("SetChargingProfile")
                    .expect("SetChargingProfile is a known action");
                (Ok(resp), context)
            }
            other => {
                let name = V2_0_1::action_name(other);
                match V2_0_1::default_response(name) {
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

impl CsActionHandler<V2_0_1> for CsStateHandler {
    fn handle_call(
        &self,
        action: Action201,
    ) -> impl Future<Output = Result<Response201, CallError>> + Send {
        let name = V2_0_1::action_name(&action).to_string();
        let request = V2_0_1::encode_action(&action).unwrap_or(serde_json::Value::Null);
        let (result, context) = self.respond(&action);
        let reply_payload = match &result {
            Ok(resp) => V2_0_1::encode_response(resp).unwrap_or(serde_json::Value::Null),
            Err(_) => serde_json::Value::Null,
        };
        let ok = result.is_ok();
        let messages = self.messages.clone();
        async move {
            let mut guard = messages.write().await;
            push_capped(
                &mut guard,
                OcppMessage::new(Dir::In, name.clone(), request, None, "inbound call"),
            );
            push_capped(
                &mut guard,
                OcppMessage::new(Dir::Out, name, reply_payload, Some(ok), context),
            );
            drop(guard);
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
