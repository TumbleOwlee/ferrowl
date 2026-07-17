//! OCPP 2.0.1 typed inbound-response construction — the [`TypedInbound`] impl the shared generic
//! handler ([`crate::module::ocpp::client::handler`]) calls for the responses whose body depends on
//! request/state data. Each method extracts the typed 2.0.1 request and builds the typed 2.0.1
//! response from `rust_ocpp::v2_0_1`; the store lookup/mutation comes in as a closure, so the
//! decision logic is not duplicated here (only the typed plumbing is). The 2.1 twin is
//! `v2_1/inbound.rs`.

use ferrowl_ocpp::V2_0_1;
use ferrowl_ocpp::v2_0_1::datatypes::get_variable_result::GetVariableResultType;
use ferrowl_ocpp::v2_0_1::datatypes::set_variable_result::SetVariableResultType;
use ferrowl_ocpp::v2_0_1::enumerations::charging_profile_status::ChargingProfileStatusEnumType;
use ferrowl_ocpp::v2_0_1::enumerations::get_variable_status::GetVariableStatusEnumType;
use ferrowl_ocpp::v2_0_1::enumerations::reset_status::ResetStatusEnumType;
use ferrowl_ocpp::v2_0_1::enumerations::set_variable_status::SetVariableStatusEnumType;
use ferrowl_ocpp::v2_0_1::messages::get_variables::GetVariablesResponse;
use ferrowl_ocpp::v2_0_1::messages::reset::ResetResponse;
use ferrowl_ocpp::v2_0_1::messages::set_charging_profile::SetChargingProfileResponse;
use ferrowl_ocpp::v2_0_1::messages::set_variables::SetVariablesResponse;
use ferrowl_ocpp::{Action201, Response201};

use crate::module::ocpp::client::handler::{SetOutcome, TypedInbound};

impl TypedInbound for V2_0_1 {
    fn get_variables_response(
        action: &Action201,
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Response201 {
        let Action201::GetVariables(req) = action else {
            unreachable!("dispatched by action name")
        };
        let get_variable_result = req
            .get_variable_data
            .iter()
            .map(|d| {
                let attribute_value = lookup(&d.variable.name);
                GetVariableResultType {
                    attribute_status: match attribute_value {
                        Some(_) => GetVariableStatusEnumType::Accepted,
                        None => GetVariableStatusEnumType::UnknownVariable,
                    },
                    attribute_type: d.attribute_type.clone(),
                    attribute_value,
                    component: d.component.clone(),
                    variable: d.variable.clone(),
                    attribute_status_info: None,
                    custom_data: None,
                }
            })
            .collect();
        Response201::GetVariables(Box::new(GetVariablesResponse {
            get_variable_result,
            custom_data: None,
        }))
    }

    fn set_variables_response(
        action: &Action201,
        mut apply: impl FnMut(&str, &str) -> SetOutcome,
    ) -> Response201 {
        let Action201::SetVariables(req) = action else {
            unreachable!("dispatched by action name")
        };
        let set_variable_result = req
            .set_variable_data
            .iter()
            .map(|d| SetVariableResultType {
                attribute_status: match apply(&d.variable.name, &d.attribute_value) {
                    SetOutcome::Accepted => SetVariableStatusEnumType::Accepted,
                    SetOutcome::Rejected => SetVariableStatusEnumType::Rejected,
                },
                attribute_type: d.attribute_type.clone(),
                component: d.component.clone(),
                variable: d.variable.clone(),
                attribute_status_info: None,
                custom_data: None,
            })
            .collect();
        Response201::SetVariables(Box::new(SetVariablesResponse {
            set_variable_result,
            custom_data: None,
        }))
    }

    fn reset_response() -> Response201 {
        Response201::Reset(Box::new(ResetResponse {
            status: ResetStatusEnumType::Accepted,
            status_info: None,
            custom_data: None,
        }))
    }

    fn set_charging_profile_rejected() -> Response201 {
        Response201::SetChargingProfile(Box::new(SetChargingProfileResponse {
            status: ChargingProfileStatusEnumType::Rejected,
            status_info: None,
            custom_data: None,
        }))
    }
}
