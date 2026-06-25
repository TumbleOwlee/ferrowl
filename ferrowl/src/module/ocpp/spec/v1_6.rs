//! OCPP 1.6 action specs. Flat actions assemble a flat object; nested actions (SetChargingProfile,
//! SendLocalList) fold a few flat fields into the full nested request via a custom assembler. The
//! remaining actions ([`json_actions`]) use the raw JSON editor explicitly — see the completeness
//! test that asserts every dialog-reachable action is classified (no silent JSON-by-absence).

use crate::module::ocpp::action_dialog::{
    ActionSpec, Assembler, PropKind, PropSource, PropSpec, flat_object, prop,
};
use serde_json::{Value, json};

const RESET_TYPE: &[&str] = &["Soft", "Hard"];
const AVAILABILITY: &[&str] = &["Operative", "Inoperative"];
const RATE_UNIT: &[&str] = &["A", "W"];
const PROFILE_PURPOSE: &[&str] = &["ChargePointMaxProfile", "TxDefaultProfile", "TxProfile"];
const TRIGGER_MSG: &[&str] = &[
    "BootNotification",
    "DiagnosticsStatusNotification",
    "FirmwareStatusNotification",
    "Heartbeat",
    "MeterValues",
    "StatusNotification",
];
const CP_STATUS: &[&str] = &[
    "Available",
    "Preparing",
    "Charging",
    "SuspendedEVSE",
    "SuspendedEV",
    "Finishing",
    "Reserved",
    "Unavailable",
    "Faulted",
];
const FW_STATUS: &[&str] = &[
    "Downloaded",
    "DownloadFailed",
    "Downloading",
    "Idle",
    "InstallationFailed",
    "Installing",
    "Installed",
];
const DIAG_STATUS: &[&str] = &["Idle", "Uploaded", "UploadFailed", "Uploading"];
const UPDATE_TYPE: &[&str] = &["Full", "Differential"];

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
        "connectorId": prop(pairs, "connectorId").cloned().unwrap_or(json!(0)),
        "csChargingProfiles": {
            "chargingProfileId": 1,
            "stackLevel": prop(pairs, "stackLevel").cloned().unwrap_or(json!(0)),
            "chargingProfilePurpose": prop(pairs, "purpose").cloned().unwrap_or(json!("TxProfile")),
            "chargingProfileKind": "Absolute",
            "chargingSchedule": {
                "chargingRateUnit": prop(pairs, "chargingRateUnit").cloned().unwrap_or(json!("A")),
                "chargingSchedulePeriod": [period],
            },
        },
    })
}

/// Fold a single auth entry into a `SendLocalListRequest` (omit the list when no idTag is given).
fn assemble_send_local_list(pairs: &[(&'static str, Value)]) -> Value {
    let mut req = json!({
        "listVersion": prop(pairs, "listVersion").cloned().unwrap_or(json!(1)),
        "updateType": prop(pairs, "updateType").cloned().unwrap_or(json!("Full")),
    });
    if let Some(id) = prop(pairs, "idTag") {
        req["localAuthorizationList"] = json!([{ "idTag": id }]);
    }
    req
}

/// Dialog-reachable actions that intentionally use the raw JSON editor (no typed form yet).
pub fn json_actions() -> &'static [&'static str] {
    // GetConfiguration takes a key list; it is sent directly (empty = all) and never opens a form,
    // but is listed here so the completeness test accounts for it.
    &["GetConfiguration"]
}

/// The flat action spec for `name`, or `None` (JSON editor) for complex/unsupported actions.
pub fn action_spec(name: &str) -> Option<ActionSpec> {
    use PropKind::*;
    use PropSource::*;
    Some(match name {
        // --- CSMS-originated (server sends) ---
        "Reset" => spec(&[PropSpec {
            name: "type",
            kind: Enum(RESET_TYPE),
            source: Constant("Soft"),
            optional: false,
        }]),
        "ClearCache" | "GetLocalListVersion" => spec(&[]),
        "ChangeConfiguration" => spec(&[
            PropSpec {
                name: "key",
                kind: Text,
                source: Empty,
                optional: false,
            },
            PropSpec {
                name: "value",
                kind: Text,
                source: Empty,
                optional: false,
            },
        ]),
        "ChangeAvailability" => spec(&[
            PropSpec {
                name: "connectorId",
                kind: Number,
                source: StateField("ConnectorId"),
                optional: false,
            },
            PropSpec {
                name: "type",
                kind: Enum(AVAILABILITY),
                source: Constant("Operative"),
                optional: false,
            },
        ]),
        "UnlockConnector" => spec(&[PropSpec {
            name: "connectorId",
            kind: Number,
            source: StateField("ConnectorId"),
            optional: false,
        }]),
        "TriggerMessage" => spec(&[
            PropSpec {
                name: "requestedMessage",
                kind: Enum(TRIGGER_MSG),
                source: Constant("StatusNotification"),
                optional: false,
            },
            PropSpec {
                name: "connectorId",
                kind: Number,
                source: StateField("ConnectorId"),
                optional: true,
            },
        ]),
        "RemoteStartTransaction" => spec(&[
            PropSpec {
                name: "connectorId",
                kind: Number,
                source: StateField("ConnectorId"),
                optional: true,
            },
            PropSpec {
                name: "idTag",
                kind: Text,
                source: StateField("Rfid"),
                optional: false,
            },
        ]),
        "RemoteStopTransaction" => spec(&[PropSpec {
            name: "transactionId",
            kind: Number,
            source: StateField("TransactionId"),
            optional: false,
        }]),
        "ReserveNow" => spec(&[
            PropSpec {
                name: "connectorId",
                kind: Number,
                source: StateField("ConnectorId"),
                optional: false,
            },
            PropSpec {
                name: "expiryDate",
                kind: Timestamp,
                source: Now,
                optional: false,
            },
            PropSpec {
                name: "idTag",
                kind: Text,
                source: StateField("Rfid"),
                optional: false,
            },
            PropSpec {
                name: "reservationId",
                kind: Number,
                source: Constant("1"),
                optional: false,
            },
        ]),
        "CancelReservation" => spec(&[PropSpec {
            name: "reservationId",
            kind: Number,
            source: Constant("1"),
            optional: false,
        }]),
        "GetDiagnostics" => spec(&[
            PropSpec {
                name: "location",
                kind: Text,
                source: Empty,
                optional: false,
            },
            PropSpec {
                name: "retries",
                kind: Number,
                source: Empty,
                optional: true,
            },
            PropSpec {
                name: "retryInterval",
                kind: Number,
                source: Empty,
                optional: true,
            },
        ]),
        "ClearChargingProfile" => spec(&[
            PropSpec {
                name: "id",
                kind: Number,
                source: Empty,
                optional: true,
            },
            PropSpec {
                name: "connectorId",
                kind: Number,
                source: StateField("ConnectorId"),
                optional: true,
            },
            PropSpec {
                name: "chargingProfilePurpose",
                kind: Enum(PROFILE_PURPOSE),
                source: Empty,
                optional: true,
            },
            PropSpec {
                name: "stackLevel",
                kind: Number,
                source: Empty,
                optional: true,
            },
        ]),
        "GetCompositeSchedule" => spec(&[
            PropSpec {
                name: "connectorId",
                kind: Number,
                source: StateField("ConnectorId"),
                optional: false,
            },
            PropSpec {
                name: "duration",
                kind: Number,
                source: Constant("86400"),
                optional: false,
            },
            PropSpec {
                name: "chargingRateUnit",
                kind: Enum(RATE_UNIT),
                source: Empty,
                optional: true,
            },
        ]),
        "UpdateFirmware" => spec(&[
            PropSpec {
                name: "location",
                kind: Text,
                source: Empty,
                optional: false,
            },
            PropSpec {
                name: "retrieveDate",
                kind: Timestamp,
                source: Now,
                optional: false,
            },
            PropSpec {
                name: "retries",
                kind: Number,
                source: Empty,
                optional: true,
            },
        ]),
        "SetChargingProfile" => nested(
            &[
                PropSpec {
                    name: "connectorId",
                    kind: Number,
                    source: StateField("ConnectorId"),
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
        "SendLocalList" => nested(
            &[
                PropSpec {
                    name: "listVersion",
                    kind: Number,
                    source: Constant("1"),
                    optional: false,
                },
                PropSpec {
                    name: "updateType",
                    kind: Enum(UPDATE_TYPE),
                    source: Constant("Full"),
                    optional: false,
                },
                PropSpec {
                    name: "idTag",
                    kind: Text,
                    source: StateField("Rfid"),
                    optional: true,
                },
            ],
            assemble_send_local_list,
        ),
        // --- CS-originated (client sends; non-state-driven) ---
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
        "FirmwareStatusNotification" => spec(&[PropSpec {
            name: "status",
            kind: Enum(FW_STATUS),
            source: Constant("Idle"),
            optional: false,
        }]),
        "DiagnosticsStatusNotification" => spec(&[PropSpec {
            name: "status",
            kind: Enum(DIAG_STATUS),
            source: Constant("Idle"),
            optional: false,
        }]),
        "StatusNotification" => spec(&[
            PropSpec {
                name: "connectorId",
                kind: Number,
                source: StateField("ConnectorId"),
                optional: false,
            },
            PropSpec {
                name: "errorCode",
                kind: Text,
                source: Constant("NoError"),
                optional: false,
            },
            PropSpec {
                name: "status",
                kind: Enum(CP_STATUS),
                source: StateField("Status"),
                optional: false,
            },
        ]),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{action_spec, json_actions};
    use ferrowl_ocpp::{V1_6, Version};

    /// CS actions a charging station builds from state (sent without a dialog); excluded from
    /// dialog completeness. Mirrors the client view's `STATE_DRIVEN`.
    const STATE_DRIVEN: &[&str] = &[
        "Authorize",
        "BootNotification",
        "Heartbeat",
        "MeterValues",
        "StatusNotification",
        "StartTransaction",
        "StopTransaction",
    ];

    #[test]
    fn ut_flat_and_nested_actions_have_specs() {
        assert!(action_spec("Reset").is_some());
        assert!(action_spec("ChangeAvailability").is_some());
        assert!(action_spec("DataTransfer").is_some());
        assert!(action_spec("SetChargingProfile").is_some());
        assert!(action_spec("SendLocalList").is_some());
    }

    /// Every action a dialog can open is classified: a typed spec or an explicit JSON action,
    /// disjoint and complete (no silent JSON-by-absence).
    #[test]
    fn ut_every_dialog_action_is_classified() {
        let mut reachable: Vec<&str> = V1_6::csms_actions().iter().map(|(n, _)| *n).collect();
        reachable.extend(
            V1_6::cs_actions()
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
