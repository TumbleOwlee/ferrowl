//! OCPP 2.0.1 action specs. Flat actions assemble a flat object; SetChargingProfile and NotifyEvent
//! fold a few flat fields into the full nested request via a custom assembler. Everything else
//! ([`json_actions`]) uses the raw JSON editor explicitly — the completeness test asserts every
//! dialog-reachable action is classified (no silent JSON-by-absence). The `evse` object on
//! Reset/ChangeAvailability/TriggerMessage is omitted (optional); use JSON for EVSE targeting.

use crate::module::ocpp::action_dialog::{
    ActionSpec, Assembler, PropKind, PropSource, PropSpec, flat_object, prop,
};
use serde_json::{Value, json};

const RESET_TYPE: &[&str] = &["Immediate", "OnIdle"];
const OPERATIONAL: &[&str] = &["Operative", "Inoperative"];
const TRIGGER_MSG: &[&str] = &[
    "BootNotification",
    "Heartbeat",
    "MeterValues",
    "StatusNotification",
    "FirmwareStatusNotification",
];
const PROFILE_PURPOSE: &[&str] = &[
    "ChargingStationExternalConstraints",
    "ChargingStationMaxProfile",
    "TxDefaultProfile",
    "TxProfile",
];
const RATE_UNIT: &[&str] = &["A", "W"];
const EVENT_TRIGGER: &[&str] = &["Alerting", "Delta", "Periodic"];
const EVENT_NOTIFICATION: &[&str] = &[
    "HardWiredNotification",
    "HardWiredMonitor",
    "PreconfiguredMonitor",
    "CustomMonitor",
];

const fn spec(props: &'static [PropSpec]) -> ActionSpec {
    ActionSpec {
        props,
        assemble: flat_object,
        complex: false,
    }
}

/// A spec for a nested action: flat editable `props` folded into the full request by `assemble`.
const fn nested(props: &'static [PropSpec], assemble: Assembler) -> ActionSpec {
    ActionSpec {
        props,
        assemble,
        complex: true,
    }
}

/// Fold flat fields into a `SetChargingProfileRequest` with a single-period absolute schedule.
fn assemble_set_charging_profile(pairs: &[(&'static str, Value)]) -> Value {
    let mut period = json!({
        "startPeriod": 0,
        "limit": prop(pairs, "limit").cloned().unwrap_or(Value::Null),
    });
    if let Some(n) = prop(pairs, "numberPhases") {
        period["numberPhases"] = n.clone();
    }
    json!({
        "evseId": prop(pairs, "evseId").cloned().unwrap_or(json!(0)),
        "chargingProfile": {
            "id": 1,
            "stackLevel": prop(pairs, "stackLevel").cloned().unwrap_or(json!(0)),
            "chargingProfilePurpose": prop(pairs, "purpose").cloned().unwrap_or(json!("TxProfile")),
            "chargingProfileKind": "Absolute",
            "chargingSchedule": [{
                "id": 1,
                "chargingRateUnit": prop(pairs, "chargingRateUnit").cloned().unwrap_or(json!("A")),
                "chargingSchedulePeriod": [period],
            }],
        },
    })
}

/// Fold flat fields into a `NotifyEventRequest` carrying a single `eventData` entry.
fn assemble_notify_event(pairs: &[(&'static str, Value)]) -> Value {
    let ts = prop(pairs, "generatedAt").cloned().unwrap_or(json!(""));
    json!({
        "generatedAt": ts,
        "seqNo": prop(pairs, "seqNo").cloned().unwrap_or(json!(0)),
        "eventData": [{
            "eventId": prop(pairs, "eventId").cloned().unwrap_or(json!(1)),
            "timestamp": ts,
            "trigger": prop(pairs, "trigger").cloned().unwrap_or(json!("Alerting")),
            "actualValue": prop(pairs, "actualValue").cloned().unwrap_or(json!("")),
            "eventNotificationType": prop(pairs, "eventNotificationType")
                .cloned()
                .unwrap_or(json!("HardWiredNotification")),
            "component": { "name": prop(pairs, "componentName").cloned().unwrap_or(json!("")) },
            "variable": { "name": prop(pairs, "variableName").cloned().unwrap_or(json!("")) },
        }],
    })
}

/// Dialog-reachable actions that intentionally use the raw JSON editor (nested/list payloads with
/// no typed form yet). Kept explicit so the completeness test forces a decision for new actions.
pub fn json_actions() -> &'static [&'static str] {
    &[
        // CSMS-originated.
        "CancelReservation",
        "CertificateSigned",
        "ClearChargingProfile",
        "ClearDisplayMessage",
        "ClearVariableMonitoring",
        "CostUpdated",
        "CustomerInformation",
        "DeleteCertificate",
        "GetBaseReport",
        "GetChargingProfiles",
        "GetCompositeSchedule",
        "GetDisplayMessages",
        "GetInstalledCertificateIds",
        "GetLog",
        "GetMonitoringReport",
        "GetReport",
        "GetTransactionStatus",
        "GetVariables",
        "InstallCertificate",
        "PublishFirmware",
        "RequestStartTransaction",
        "ReserveNow",
        "SendLocalList",
        "SetDisplayMessage",
        "SetMonitoringBase",
        "SetMonitoringLevel",
        "SetNetworkProfile",
        "SetVariableMonitoring",
        "SetVariables",
        "UnpublishFirmware",
        "UpdateFirmware",
        // CS-originated.
        "ClearedChargingLimit",
        "FirmwareStatusNotification",
        "Get15118EVCertificate",
        "GetCertificateStatus",
        "LogStatusNotification",
        "NotifyChargingLimit",
        "NotifyCustomerInformation",
        "NotifyDisplayMessages",
        "NotifyEVChargingNeeds",
        "NotifyEVChargingSchedule",
        "NotifyMonitoringReport",
        "NotifyReport",
        "PublishFirmwareStatusNotification",
        "ReportChargingProfiles",
        "ReservationStatusUpdate",
        "SecurityEventNotification",
        "SignCertificate",
        "TransactionEvent",
    ]
}

/// The action spec for `name`, or `None` for actions handled by the raw JSON editor
/// ([`json_actions`]).
pub fn action_spec(name: &str) -> Option<ActionSpec> {
    use PropKind::*;
    use PropSource::*;
    Some(match name {
        "Reset" => spec(&[PropSpec {
            name: "type",
            kind: Enum(RESET_TYPE),
            source: Constant("Immediate"),
            optional: false,
        }]),
        "ClearCache" | "GetLocalListVersion" => spec(&[]),
        "ChangeAvailability" => spec(&[PropSpec {
            name: "operationalStatus",
            kind: Enum(OPERATIONAL),
            source: Constant("Operative"),
            optional: false,
        }]),
        "TriggerMessage" => spec(&[PropSpec {
            name: "requestedMessage",
            kind: Enum(TRIGGER_MSG),
            source: Constant("StatusNotification"),
            optional: false,
        }]),
        "UnlockConnector" => spec(&[
            PropSpec {
                name: "evseId",
                kind: Number,
                source: StateField("EvseId"),
                optional: false,
            },
            PropSpec {
                name: "connectorId",
                kind: Number,
                source: Constant("1"),
                optional: false,
            },
        ]),
        "RequestStopTransaction" => spec(&[PropSpec {
            name: "transactionId",
            kind: Text,
            source: StateField("TransactionId"),
            optional: false,
        }]),
        "DataTransfer" => spec(&[
            PropSpec {
                name: "vendorId",
                kind: Text,
                source: Empty,
                optional: false,
            },
            PropSpec {
                name: "messageId",
                kind: Text,
                source: Empty,
                optional: true,
            },
            PropSpec {
                name: "data",
                kind: Text,
                source: Empty,
                optional: true,
            },
        ]),
        "SetChargingProfile" => nested(
            &[
                PropSpec {
                    name: "evseId",
                    kind: Number,
                    source: StateField("EvseId"),
                    optional: false,
                },
                PropSpec {
                    name: "limit",
                    kind: Number,
                    source: Constant("16"),
                    optional: false,
                },
                PropSpec {
                    name: "numberPhases",
                    kind: Number,
                    source: Empty,
                    optional: true,
                },
                PropSpec {
                    name: "purpose",
                    kind: Enum(PROFILE_PURPOSE),
                    source: Constant("TxProfile"),
                    optional: false,
                },
                PropSpec {
                    name: "stackLevel",
                    kind: Number,
                    source: Constant("0"),
                    optional: false,
                },
                PropSpec {
                    name: "chargingRateUnit",
                    kind: Enum(RATE_UNIT),
                    source: Constant("A"),
                    optional: false,
                },
            ],
            assemble_set_charging_profile,
        ),
        "NotifyEvent" => nested(
            &[
                PropSpec {
                    name: "generatedAt",
                    kind: Timestamp,
                    source: Now,
                    optional: false,
                },
                PropSpec {
                    name: "seqNo",
                    kind: Number,
                    source: Constant("0"),
                    optional: false,
                },
                PropSpec {
                    name: "eventId",
                    kind: Number,
                    source: Constant("1"),
                    optional: false,
                },
                PropSpec {
                    name: "trigger",
                    kind: Enum(EVENT_TRIGGER),
                    source: Constant("Alerting"),
                    optional: false,
                },
                PropSpec {
                    name: "eventNotificationType",
                    kind: Enum(EVENT_NOTIFICATION),
                    source: Constant("HardWiredNotification"),
                    optional: false,
                },
                PropSpec {
                    name: "actualValue",
                    kind: Text,
                    source: Empty,
                    optional: false,
                },
                PropSpec {
                    name: "componentName",
                    kind: Text,
                    source: Empty,
                    optional: false,
                },
                PropSpec {
                    name: "variableName",
                    kind: Text,
                    source: Empty,
                    optional: false,
                },
            ],
            assemble_notify_event,
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{action_spec, json_actions};
    use ferrowl_ocpp::{V2_0_1, Version};

    /// CS actions a charging station builds from state (sent without a dialog); mirrors the client
    /// view's `STATE_DRIVEN`.
    const STATE_DRIVEN: &[&str] = &[
        "Authorize",
        "BootNotification",
        "Heartbeat",
        "MeterValues",
        "StatusNotification",
    ];

    #[test]
    fn ut_flat_and_nested_actions_have_specs() {
        assert!(action_spec("Reset").is_some());
        assert!(action_spec("UnlockConnector").is_some());
        assert!(action_spec("SetChargingProfile").is_some());
        assert!(action_spec("NotifyEvent").is_some());
        // Nested idToken / variable actions stay on the JSON editor.
        assert!(action_spec("RequestStartTransaction").is_none());
        assert!(action_spec("SetVariables").is_none());
    }

    /// Every dialog-reachable action is exactly one of: a typed spec or an explicit JSON action.
    #[test]
    fn ut_every_dialog_action_is_classified() {
        let mut reachable: Vec<&str> = V2_0_1::csms_actions().iter().map(|(n, _)| *n).collect();
        reachable.extend(
            V2_0_1::cs_actions()
                .iter()
                .copied()
                .filter(|n| !STATE_DRIVEN.contains(n)),
        );
        for name in reachable {
            let has_spec = action_spec(name).is_some();
            let is_json = json_actions().contains(&name);
            assert!(
                has_spec ^ is_json,
                "{name} must be exactly one of spec/json"
            );
        }
    }
}
