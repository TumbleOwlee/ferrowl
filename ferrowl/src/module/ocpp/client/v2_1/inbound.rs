//! OCPP 2.1 inbound (CSMS→CS) decision logic — the fully-typed [`Inbound`] impl the generic
//! [`CsStateHandler`](crate::module::ocpp::client::handler) delegates to. Identical decision logic
//! to 2.0.1 (`v2_0_1/inbound.rs`), typed over `rust_ocpp::v2_1`; the two are deliberate near-copies
//! because their request/response types are distinct, and full compile-time typing is preferred
//! over sharing the bodies through an untyped `serde_json::Value`.

use std::sync::Arc;

use parking_lot::RwLock;

use ferrowl_ocpp::v2_1::datatypes::get_variable_result::GetVariableResultType;
use ferrowl_ocpp::v2_1::datatypes::set_variable_result::SetVariableResultType;
use ferrowl_ocpp::v2_1::enumerations::charging_profile_purpose::ChargingProfilePurposeEnumType;
use ferrowl_ocpp::v2_1::enumerations::charging_profile_status::ChargingProfileStatusEnumType;
use ferrowl_ocpp::v2_1::enumerations::charging_rate_unit::ChargingRateUnitEnumType;
use ferrowl_ocpp::v2_1::enumerations::get_variable_status::GetVariableStatusEnumType;
use ferrowl_ocpp::v2_1::enumerations::operational_status::OperationalStatusEnumType;
use ferrowl_ocpp::v2_1::enumerations::reset_status::ResetStatusEnumType;
use ferrowl_ocpp::v2_1::enumerations::set_variable_status::SetVariableStatusEnumType;
use ferrowl_ocpp::v2_1::messages::get_variables::GetVariablesResponse;
use ferrowl_ocpp::v2_1::messages::reset::ResetResponse;
use ferrowl_ocpp::v2_1::messages::set_charging_profile::SetChargingProfileResponse;
use ferrowl_ocpp::v2_1::messages::set_variables::SetVariablesResponse;
use ferrowl_ocpp::{Action21, CallError, CallErrorCode, Response21, V2_1, Version};

use crate::module::ocpp::client::config::ConfigKey;
use crate::module::ocpp::client::handler::Inbound;
use crate::module::ocpp::client::v2_0_1::state::CsState;
use crate::module::ocpp::client::v2_common::clear_limit_by_purpose;
use crate::module::ocpp::lock::{with_state, with_state_mut};

/// The wire name of a charging-profile purpose, for the log context string.
fn purpose_str(p: &ChargingProfilePurposeEnumType) -> &'static str {
    match p {
        ChargingProfilePurposeEnumType::TxDefaultProfile => "TxDefaultProfile",
        ChargingProfilePurposeEnumType::ChargingStationMaxProfile => "ChargingStationMaxProfile",
        ChargingProfilePurposeEnumType::ChargingStationExternalConstraints => {
            "ChargingStationExternalConstraints"
        }
        ChargingProfilePurposeEnumType::TxProfile => "TxProfile",
        _ => "TxProfile",
    }
}

impl Inbound for V2_1 {
    fn respond(
        state: &Arc<RwLock<CsState>>,
        action: &Action21,
    ) -> (Result<Response21, CallError>, String) {
        match action {
            Action21::GetVariables(req) => with_state(state, |s| {
                let get_variable_result = req
                    .get_variable_data
                    .iter()
                    .map(|d| {
                        let found = s.config.iter().find(|c| c.key == d.variable.name);
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
                            custom_data: None,
                        }
                    })
                    .collect();
                (
                    Ok(Response21::GetVariables(Box::new(GetVariablesResponse {
                        get_variable_result,
                        custom_data: None,
                    }))),
                    "answered from variables".to_string(),
                )
            }),
            Action21::SetVariables(req) => with_state_mut(state, |s| {
                let set_variable_result = req
                    .set_variable_data
                    .iter()
                    .map(|d| {
                        let attribute_status =
                            match s.config.iter_mut().find(|c| c.key == d.variable.name) {
                                Some(c) if c.readonly => SetVariableStatusEnumType::Rejected,
                                Some(c) => {
                                    c.value = d.attribute_value.clone();
                                    SetVariableStatusEnumType::Accepted
                                }
                                None => {
                                    s.config.push(ConfigKey {
                                        key: d.variable.name.clone(),
                                        value: d.attribute_value.clone(),
                                        readonly: false,
                                    });
                                    SetVariableStatusEnumType::Accepted
                                }
                            };
                        SetVariableResultType {
                            attribute_status,
                            attribute_type: d.attribute_type.clone(),
                            component: d.component.clone(),
                            variable: d.variable.clone(),
                            attribute_status_info: None,
                            custom_data: None,
                        }
                    })
                    .collect();
                (
                    Ok(Response21::SetVariables(Box::new(SetVariablesResponse {
                        set_variable_result,
                        custom_data: None,
                    }))),
                    "variables updated".to_string(),
                )
            }),
            Action21::Reset(_) => with_state_mut(state, |s| {
                for c in &mut s.connectors {
                    c.status = "Available".to_string();
                    c.transaction_id = None;
                    c.session_energy = 0.0;
                }
                (
                    Ok(Response21::Reset(Box::new(ResetResponse {
                        status: ResetStatusEnumType::Accepted,
                        status_info: None,
                        custom_data: None,
                    }))),
                    "state reset".to_string(),
                )
            }),
            Action21::SetChargingProfile(req) => {
                let profile = &req.charging_profile;
                let purpose = &profile.charging_profile_purpose;
                let stack = profile.stack_level as i64;
                let evse = req.evse_id as i64;
                with_state_mut(state, |s| {
                    // Reject profiles whose stack level exceeds ChargeProfileMaxStackLevel (when
                    // that key is configured with a numeric value); otherwise accept (no ceiling).
                    let max_stack = s
                        .config
                        .iter()
                        .find(|c| c.key == "ChargeProfileMaxStackLevel")
                        .and_then(|c| c.value.parse::<i64>().ok());
                    if let Some(max) = max_stack
                        && stack > max
                    {
                        let resp =
                            Response21::SetChargingProfile(Box::new(SetChargingProfileResponse {
                                status: ChargingProfileStatusEnumType::Rejected,
                                status_info: None,
                                custom_data: None,
                            }));
                        (
                            Ok(resp),
                            format!("rejected: stackLevel {stack} > max {max}"),
                        )
                    } else {
                        // Apply the limit to the connector on the targeted EVSE (fall back to the
                        // first), routed by charging-profile purpose into the matching field. In
                        // 2.1 the period limit is optional (phases split it into l2/l3); an absent
                        // limit applies nothing, as in 2.0.1 when the JSON limit was missing.
                        let applied = profile.charging_schedule.first().and_then(|sc| {
                            sc.charging_schedule_period
                                .first()
                                .and_then(|p| p.limit.map(|l| (sc, l)))
                        });
                        let context = if let Some((schedule, limit_dec)) = applied {
                            let limit: f64 = limit_dec.to_string().parse().unwrap_or(0.0);
                            let unit = match &schedule.charging_rate_unit {
                                ChargingRateUnitEnumType::A => "A",
                                ChargingRateUnitEnumType::W => "W",
                            }
                            .to_string();
                            let idx = s
                                .connectors
                                .iter()
                                .position(|c| c.evse_id == evse)
                                .or((!s.connectors.is_empty()).then_some(0));
                            if let Some(i) = idx {
                                let c = &mut s.connectors[i];
                                match purpose {
                                    ChargingProfilePurposeEnumType::TxDefaultProfile => {
                                        c.default_limit = Some(limit);
                                        c.default_limit_unit = unit.clone();
                                    }
                                    ChargingProfilePurposeEnumType::ChargingStationMaxProfile => {
                                        c.max_limit = Some(limit);
                                        c.max_limit_unit = unit.clone();
                                    }
                                    ChargingProfilePurposeEnumType::ChargingStationExternalConstraints => {
                                        c.external_limit = Some(limit);
                                        c.external_limit_unit = unit.clone();
                                    }
                                    _ => {
                                        c.limit = Some(limit);
                                        c.limit_unit = unit.clone();
                                    }
                                }
                            }
                            format!("{} limit {limit} {unit}", purpose_str(purpose))
                        } else {
                            "no limit in profile".to_string()
                        };
                        let resp = V2_1::default_response("SetChargingProfile")
                            .expect("SetChargingProfile is a known action");
                        (Ok(resp), context)
                    }
                })
            }
            Action21::ReserveNow(req) => {
                let tag = req.id_token.id_token.clone();
                let id = Some(req.id as i64);
                with_state_mut(state, |s| {
                    // An evseId-less (or evseId 0) ReserveNow reserves the station itself
                    // (CS-level); otherwise it targets the connector on that EVSE (entries are
                    // addressed by EVSE id).
                    let context = match req.evse_id.filter(|&e| e != 0) {
                        Some(e) => match s.connector_mut_by_evse(e as i64) {
                            Some(c) => {
                                c.reserved_rfid = Some(tag.clone());
                                c.reservation_id = id;
                                format!("reserved evse {e} for {tag}")
                            }
                            None => format!("unknown evse {e}"),
                        },
                        None => {
                            s.reserved_rfid = Some(tag.clone());
                            s.reservation_id = id;
                            format!("reserved CS for {tag}")
                        }
                    };
                    let resp =
                        V2_1::default_response("ReserveNow").expect("ReserveNow is a known action");
                    (Ok(resp), context)
                })
            }
            Action21::CancelReservation(req) => with_state_mut(state, |s| {
                // Clear whichever level holds the matching reservationId.
                let rid = req.reservation_id as i64;
                let context = if s.reservation_id == Some(rid) {
                    s.reserved_rfid = None;
                    s.reservation_id = None;
                    format!("cancelled CS reservation {rid}")
                } else if let Some(c) = s
                    .connectors
                    .iter_mut()
                    .find(|c| c.reservation_id == Some(rid))
                {
                    c.reserved_rfid = None;
                    c.reservation_id = None;
                    format!("cancelled evse {} reservation {rid}", c.evse_id)
                } else {
                    "no matching reservation".to_string()
                };
                let resp = V2_1::default_response("CancelReservation")
                    .expect("CancelReservation is a known action");
                (Ok(resp), context)
            }),
            Action21::ChangeAvailability(req) => {
                let status = match req.operational_status {
                    OperationalStatusEnumType::Inoperative => "Unavailable",
                    _ => "Available",
                };
                with_state_mut(state, |s| {
                    // An evseId-less (or evseId 0) ChangeAvailability targets the whole station.
                    let context = match req.evse.as_ref().map(|e| e.id as i64).filter(|&e| e != 0) {
                        Some(e) => {
                            if let Some(c) = s.connector_mut_by_evse(e) {
                                c.status = status.to_string();
                            }
                            format!("evse {e} -> {status}")
                        }
                        None => {
                            for c in &mut s.connectors {
                                c.status = status.to_string();
                            }
                            format!("all -> {status}")
                        }
                    };
                    let resp = V2_1::default_response("ChangeAvailability")
                        .expect("ChangeAvailability is a known action");
                    (Ok(resp), context)
                })
            }
            Action21::RequestStartTransaction(req) => with_state_mut(state, |s| {
                // Optional evseId; fall back to the first connector. Mint a transaction and charge.
                let idx = req
                    .evse_id
                    .filter(|&e| e != 0)
                    .and_then(|e| s.connectors.iter().position(|c| c.evse_id == e as i64))
                    .or((!s.connectors.is_empty()).then_some(0));
                let context = match idx {
                    Some(i) => {
                        let tx = s.connectors[i].start_tx();
                        s.connectors[i].status = "Charging".to_string();
                        format!("started tx {tx} on evse {}", s.connectors[i].evse_id)
                    }
                    None => "no connector to start".to_string(),
                };
                let resp = V2_1::default_response("RequestStartTransaction")
                    .expect("RequestStartTransaction is a known action");
                (Ok(resp), context)
            }),
            Action21::RequestStopTransaction(req) => {
                let tx = req.transaction_id.clone();
                with_state_mut(state, |s| {
                    let context = match s
                        .connectors
                        .iter_mut()
                        .find(|c| c.transaction_id.as_deref() == Some(tx.as_str()))
                    {
                        Some(c) => {
                            c.transaction_id = None;
                            c.limit = None;
                            c.status = "Available".to_string();
                            format!("stopped tx {tx} on evse {}", c.evse_id)
                        }
                        None => format!("no active tx {tx}"),
                    };
                    let resp = V2_1::default_response("RequestStopTransaction")
                        .expect("RequestStopTransaction is a known action");
                    (Ok(resp), context)
                })
            }
            Action21::ClearChargingProfile(req) => {
                let criteria = req.charging_profile_criteria.as_ref();
                let purpose = criteria
                    .and_then(|c| c.charging_profile_purpose.as_ref())
                    .map(|p| purpose_str(p).to_string());
                let evse = criteria.and_then(|c| c.evse_id).filter(|&e| e != 0);
                with_state_mut(state, |s| {
                    // evseId lives in the criteria; absent (or 0) clears every connector. The
                    // purpose criterion (when given) selects which per-purpose limit is erased;
                    // absent clears all.
                    match evse {
                        Some(e) => {
                            if let Some(c) = s.connector_mut_by_evse(e as i64) {
                                clear_limit_by_purpose(c, purpose.as_deref());
                            }
                        }
                        None => {
                            for c in &mut s.connectors {
                                clear_limit_by_purpose(c, purpose.as_deref());
                            }
                        }
                    }
                    let resp = V2_1::default_response("ClearChargingProfile")
                        .expect("ClearChargingProfile is a known action");
                    (Ok(resp), "charging profile cleared".to_string())
                })
            }
            Action21::UnlockConnector(req) => with_state_mut(state, |s| {
                let context = match Some(req.evse_id as i64).filter(|&e| e != 0) {
                    Some(e) => {
                        if let Some(c) = s.connector_mut_by_evse(e) {
                            c.status = "Available".to_string();
                        }
                        format!("evse {e} unlocked")
                    }
                    None => "no evse to unlock".to_string(),
                };
                let resp = V2_1::default_response("UnlockConnector")
                    .expect("UnlockConnector is a known action");
                (Ok(resp), context)
            }),
            other => {
                let name = V2_1::action_name(other);
                match V2_1::default_response(name) {
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
