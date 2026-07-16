//! OCPP 2.1 action specs. 2.1 is a strict superset of 2.0.1 (the 64 shared actions carry the same
//! required-field shape — 2.1's additions to shared payload types, e.g. `ChargingSchedulePeriodType`
//! discharge/setpoint fields, are all optional — so [`action_spec`]/[`json_actions`] delegate to
//! [`super::v2_0_1`] for any name not in the 2.1-only delta below.
//!
//! Of the 26 2.1-only actions, most (DER control get/clear/set, tariff-id queries, priority
//! charging, settlement, web-payment) are flat: their only required fields are scalars, and every
//! nested/repeated-list field they carry is optional and can be left absent without breaking
//! decode. `RequestBatterySwap` and `NotifyAllowedEnergyTransfer` need one folded sub-object/list
//! (same `nested` pattern as 2.0.1's `RequestStartTransaction`/`SendLocalList`). The remaining
//! actions require a nested object or repeated list with no optional escape hatch (battery data,
//! certificate-chain-status requests, DER curve reports, tariffs, periodic-event-stream params,
//! dynamic-schedule updates) and stay on the raw JSON editor.

use crate::module::ocpp::action_dialog::{
    ActionSpec, Assembler, PropKind, PropSource, PropSpec, flat_object, prop,
};
use serde_json::{Value, json};
use std::sync::OnceLock;

const DER_CONTROL_TYPE: &[&str] = &[
    "EnterService",
    "FreqDroop",
    "FreqWatt",
    "FixedPFAbsorb",
    "FixedPFInject",
    "FixedVar",
    "Gradients",
    "HFMustTrip",
    "HFMayTrip",
    "HVMustTrip",
    "HVMomCess",
    "HVMayTrip",
    "LimitMaxDischarge",
    "LFMustTrip",
    "LVMustTrip",
    "LVMomCess",
    "LVMayTrip",
    "PowerMonitoringMustTrip",
    "VoltVar",
    "VoltWatt",
    "WattPF",
    "WattVar",
];
const GRID_EVENT_FAULT: &[&str] = &[
    "CurrentImbalance",
    "LocalEmergency",
    "LowInputPower",
    "OverCurrent",
    "OverFrequency",
    "OverVoltage",
    "PhaseRotation",
    "RemoteEmergency",
    "UnderFrequency",
    "UnderVoltage",
    "VoltageImbalance",
];
const PAYMENT_STATUS: &[&str] = &["Settled", "Canceled", "Rejected", "Failed"];
const ENERGY_TRANSFER_MODE: &[&str] = &[
    "AC_single_phase",
    "AC_two_phase",
    "AC_three_phase",
    "DC",
    "AC_BPT",
    "AC_BPT_DER",
    "AC_DER",
    "DC_BPT",
    "DC_ACDP",
    "DC_ACDP_BPT",
    "WPT",
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

/// `{ idToken: {idToken, type}, requestId }`. 2.1's `IdTokenType.type` is a free string (not a
/// closed enum, unlike 2.0.1's `IdTokenEnumType`), so the type row is free text here.
fn assemble_request_battery_swap(pairs: &[(&'static str, Value)]) -> Value {
    json!({
        "idToken": {
            "idToken": prop(pairs, "idToken").cloned().unwrap_or(json!("")),
            "type": prop(pairs, "idTokenType").cloned().unwrap_or(json!("ISO14443")),
        },
        "requestId": prop(pairs, "requestId").cloned().unwrap_or(json!(1)),
    })
}

/// `{ allowedEnergyTransfer: [<mode>], transactionId }` — a single-element mode list.
fn assemble_notify_allowed_energy_transfer(pairs: &[(&'static str, Value)]) -> Value {
    json!({
        "allowedEnergyTransfer": [prop(pairs, "allowedEnergyTransfer").cloned().unwrap_or(json!("DC"))],
        "transactionId": prop(pairs, "transactionId").cloned().unwrap_or(json!("")),
    })
}

/// Dialog-reachable 2.1-only actions that stay on the raw JSON editor: a required nested object or
/// repeated list with no optional field to drop. Kept explicit so the completeness test forces a
/// decision for new actions.
pub fn json_actions() -> &'static [&'static str] {
    const NEW: &[&str] = &[
        // CS-originated.
        "BatterySwap", // battery_data: Vec<BatteryDataType> (required, min 1)
        "GetCertificateChainStatus", // certificate_status_requests: Vec<...> (required, min 1)
        "OpenPeriodicEventStream", // constant_stream_data: ConstantStreamDataType (required)
        "ReportDERControl", // multiple optional Vec<DER curve/setting types>, deeply nested
        // CSMS-originated.
        "AdjustPeriodicEventStream", // params: PeriodicEventStreamParamsType (required)
        "ChangeTransactionTariff",   // tariff: TariffType (required)
        "SetDefaultTariff",          // tariff: TariffType (required)
        "UpdateDynamicSchedule",     // schedule_update: ChargingScheduleUpdateType (required)
    ];
    // 2.0.1's JSON-only set carries over unchanged for the 64 shared actions; combined once and
    // leaked so repeated calls don't reallocate.
    static COMBINED: OnceLock<Vec<&'static str>> = OnceLock::new();
    COMBINED.get_or_init(|| {
        super::v2_0_1::json_actions()
            .iter()
            .copied()
            .chain(NEW.iter().copied())
            .collect()
    })
}

/// A decode-valid example payload for a [`json_actions`] entry (see
/// [`super::v2_0_1::json_template`]). 2.1-only actions are spelled out below; the shared
/// JSON-only actions reuse 2.0.1's templates (2.1 payloads are supersets — additions are
/// optional, so the 2.0.1 shapes still decode).
pub fn json_template(name: &str) -> Option<Value> {
    Some(match name {
        "BatterySwap" => json!({
            "eventType": "BatteryIn",
            "requestId": 1,
            "idToken": { "idToken": "TAG-1", "type": "ISO14443" },
            "batteryData": [{
                "evseId": 1,
                "serialNumber": "BAT-001",
                "soC": 80,
                "soH": 95,
            }],
        }),
        "GetCertificateChainStatus" => json!({
            "certificateStatusRequests": [{
                "source": "OCSP",
                "urls": ["https://ocsp.example.com"],
                "certificateHashData": {
                    "hashAlgorithm": "SHA256",
                    "issuerNameHash": "a1b2c3",
                    "issuerKeyHash": "d4e5f6",
                    "serialNumber": "1234",
                },
            }],
        }),
        "OpenPeriodicEventStream" => json!({
            "constantStreamData": {
                "id": 1,
                "variableMonitoringId": 1,
                "params": { "interval": 60, "values": 10 },
            },
        }),
        "ReportDERControl" => json!({
            "requestId": 1,
            "curve": [{
                "id": "curve-1",
                "curveType": "FreqWatt",
                "isDefault": false,
                "isSuperseded": false,
                "curve": {
                    "priority": 0,
                    "yUnit": "PctMaxW",
                    "curveData": [{ "x": 50, "y": 100 }],
                },
            }],
        }),
        "AdjustPeriodicEventStream" => json!({
            "id": 1,
            "params": { "interval": 60, "values": 10 },
        }),
        "ChangeTransactionTariff" => json!({
            "transactionId": "tx-1",
            "tariff": { "tariffId": "tariff-1", "currency": "EUR" },
        }),
        "SetDefaultTariff" => json!({
            "evseId": 1,
            "tariff": { "tariffId": "tariff-1", "currency": "EUR" },
        }),
        "UpdateDynamicSchedule" => json!({
            "chargingProfileId": 1,
            "scheduleUpdate": { "limit": 16 },
        }),
        // Shared with 2.0.1, but 2.1's VariableMonitoringType requires eventNotificationType.
        "NotifyMonitoringReport" => json!({
            "requestId": 1,
            "seqNo": 0,
            "generatedAt": "2026-01-01T00:00:00Z",
            "monitor": [{
                "component": { "name": "EVSE", "evse": { "id": 1 } },
                "variable": { "name": "Power.Active.Import" },
                "variableMonitoring": [{
                    "id": 1,
                    "severity": 5,
                    "transaction": false,
                    "type": "UpperThreshold",
                    "value": 11000,
                    "eventNotificationType": "PreconfiguredMonitor",
                }],
            }],
        }),
        _ => return super::v2_0_1::json_template(name),
    })
}

/// The action spec for `name`. 2.1-only actions are classified below; everything else (the 64
/// shared actions) delegates to 2.0.1's specs unchanged.
pub fn action_spec(name: &str) -> Option<ActionSpec> {
    use PropKind::*;
    use PropSource::*;
    Some(match name {
        // --- Flat: CS-originated ---
        "NotifyDERAlarm" => spec(&[
            PropSpec {
                name: "controlType",
                kind: Enum(DER_CONTROL_TYPE),
                source: Constant("EnterService"),
                optional: false,
            },
            PropSpec {
                name: "timestamp",
                kind: Timestamp,
                source: Now,
                optional: false,
            },
            PropSpec {
                name: "alarmEnded",
                kind: Bool,
                source: Empty,
                optional: true,
            },
            PropSpec {
                name: "gridEventFault",
                kind: Enum(GRID_EVENT_FAULT),
                source: Empty,
                optional: true,
            },
            PropSpec {
                name: "extraInfo",
                kind: Text,
                source: Empty,
                optional: true,
            },
        ]),
        "NotifyDERStartStop" => spec(&[
            PropSpec {
                name: "controlId",
                kind: Text,
                source: Empty,
                optional: false,
            },
            PropSpec {
                name: "started",
                kind: Bool,
                source: Constant("true"),
                optional: false,
            },
            PropSpec {
                name: "timestamp",
                kind: Timestamp,
                source: Now,
                optional: false,
            },
        ]),
        "NotifyPriorityCharging" => spec(&[
            PropSpec {
                name: "activated",
                kind: Bool,
                source: Constant("true"),
                optional: false,
            },
            PropSpec {
                name: "transactionId",
                kind: Text,
                source: StateField("TransactionId"),
                optional: false,
            },
        ]),
        "NotifySettlement" => spec(&[
            PropSpec {
                name: "pspRef",
                kind: Text,
                source: Empty,
                optional: false,
            },
            PropSpec {
                name: "settlementAmount",
                kind: Number,
                source: Constant("0"),
                optional: false,
            },
            PropSpec {
                name: "settlementTime",
                kind: Timestamp,
                source: Now,
                optional: false,
            },
            PropSpec {
                name: "status",
                kind: Enum(PAYMENT_STATUS),
                source: Constant("Settled"),
                optional: false,
            },
            PropSpec {
                name: "receiptId",
                kind: Text,
                source: Empty,
                optional: true,
            },
            PropSpec {
                name: "receiptUrl",
                kind: Text,
                source: Empty,
                optional: true,
            },
            PropSpec {
                name: "statusInfo",
                kind: Text,
                source: Empty,
                optional: true,
            },
            PropSpec {
                name: "transactionId",
                kind: Text,
                source: StateField("TransactionId"),
                optional: true,
            },
            PropSpec {
                name: "vatNumber",
                kind: Text,
                source: Empty,
                optional: true,
            },
        ]),
        "NotifyWebPaymentStarted" => spec(&[
            PropSpec {
                name: "evseId",
                kind: Number,
                source: StateField("EvseId"),
                optional: false,
            },
            PropSpec {
                name: "timeout",
                kind: Number,
                source: Constant("60"),
                optional: false,
            },
        ]),
        "ClosePeriodicEventStream" => spec(&[PropSpec {
            name: "id",
            kind: Number,
            source: Constant("1"),
            optional: false,
        }]),
        "PullDynamicScheduleUpdate" => spec(&[PropSpec {
            name: "chargingProfileId",
            kind: Number,
            source: Constant("1"),
            optional: false,
        }]),
        "VatNumberValidation" => spec(&[
            PropSpec {
                name: "vatNumber",
                kind: Text,
                source: Empty,
                optional: false,
            },
            PropSpec {
                name: "evseId",
                kind: Number,
                source: StateField("EvseId"),
                optional: true,
            },
        ]),
        // --- Flat: CSMS-originated ---
        "AFRRSignal" => spec(&[
            PropSpec {
                name: "signal",
                kind: Number,
                source: Constant("0"),
                optional: false,
            },
            PropSpec {
                name: "timestamp",
                kind: Timestamp,
                source: Now,
                optional: false,
            },
        ]),
        "ClearDERControl" => spec(&[
            PropSpec {
                name: "isDefault",
                kind: Bool,
                source: Constant("false"),
                optional: false,
            },
            PropSpec {
                name: "controlId",
                kind: Text,
                source: Empty,
                optional: true,
            },
            PropSpec {
                name: "controlType",
                kind: Enum(DER_CONTROL_TYPE),
                source: Empty,
                optional: true,
            },
        ]),
        "GetDERControl" => spec(&[
            PropSpec {
                name: "requestId",
                kind: Number,
                source: Constant("1"),
                optional: false,
            },
            PropSpec {
                name: "controlId",
                kind: Text,
                source: Empty,
                optional: true,
            },
            PropSpec {
                name: "controlType",
                kind: Enum(DER_CONTROL_TYPE),
                source: Empty,
                optional: true,
            },
            PropSpec {
                name: "isDefault",
                kind: Bool,
                source: Empty,
                optional: true,
            },
        ]),
        "GetPeriodicEventStream" => spec(&[]),
        "GetTariffs" => spec(&[PropSpec {
            name: "evseId",
            kind: Number,
            source: StateField("EvseId"),
            optional: false,
        }]),
        "ClearTariffs" => spec(&[PropSpec {
            name: "evseId",
            kind: Number,
            source: StateField("EvseId"),
            optional: true,
        }]),
        "SetDERControl" => spec(&[
            PropSpec {
                name: "controlId",
                kind: Text,
                source: Empty,
                optional: false,
            },
            PropSpec {
                name: "controlType",
                kind: Enum(DER_CONTROL_TYPE),
                source: Constant("EnterService"),
                optional: false,
            },
            PropSpec {
                name: "isDefault",
                kind: Bool,
                source: Constant("false"),
                optional: false,
            },
        ]),
        "UsePriorityCharging" => spec(&[
            PropSpec {
                name: "activate",
                kind: Bool,
                source: Constant("true"),
                optional: false,
            },
            PropSpec {
                name: "transactionId",
                kind: Text,
                source: StateField("TransactionId"),
                optional: false,
            },
        ]),
        // --- Nested ---
        "RequestBatterySwap" => nested(
            &[
                PropSpec {
                    name: "idToken",
                    kind: Text,
                    source: StateField("Rfid"),
                    optional: false,
                },
                PropSpec {
                    name: "idTokenType",
                    kind: Text,
                    source: Constant("ISO14443"),
                    optional: false,
                },
                PropSpec {
                    name: "requestId",
                    kind: Number,
                    source: Constant("1"),
                    optional: false,
                },
            ],
            assemble_request_battery_swap,
        ),
        "NotifyAllowedEnergyTransfer" => nested(
            &[
                PropSpec {
                    name: "allowedEnergyTransfer",
                    kind: Enum(ENERGY_TRANSFER_MODE),
                    source: Constant("DC"),
                    optional: false,
                },
                PropSpec {
                    name: "transactionId",
                    kind: Text,
                    source: StateField("TransactionId"),
                    optional: false,
                },
            ],
            assemble_notify_allowed_energy_transfer,
        ),
        // --- Delegate: the 64 shared actions reuse 2.0.1's specs unchanged (see module doc). ---
        _ => return super::v2_0_1::action_spec(name),
    })
}

#[cfg(test)]
mod tests {
    use super::{action_spec, json_actions, json_template};
    use crate::module::ocpp::action_dialog::ActionDialog;
    use ferrowl_ocpp::{V2_1, Version};

    /// Every JSON-only action (2.1-only and inherited) ships a template that decodes and
    /// validates against the 2.1 types — this also proves the reused 2.0.1 templates still fit.
    #[test]
    /// OC-R-091 — every 2.1 raw-JSON action (own and inherited) ships a template that decodes and validates against the 2.1 types.
    fn ut_json_templates_cover_all_json_actions_and_decode() {
        for name in json_actions() {
            let template =
                json_template(name).unwrap_or_else(|| panic!("{name} has no JSON template"));
            let action = V2_1::decode_call(name, template)
                .unwrap_or_else(|e| panic!("{name} template does not decode: {e}"));
            V2_1::validate(&action)
                .unwrap_or_else(|e| panic!("{name} template fails validation: {e}"));
        }
    }

    /// CS actions a charging station builds from state (sent without a dialog); mirrors the client
    /// view's `STATE_DRIVEN` (same set as 2.0.1 — 2.1 adds no new state-driven action).
    const STATE_DRIVEN: &[&str] = &[
        "Authorize",
        "BootNotification",
        "Heartbeat",
        "MeterValues",
        "StatusNotification",
    ];

    fn reachable() -> Vec<&'static str> {
        let mut names: Vec<&str> = V2_1::csms_actions().iter().map(|(n, _)| *n).collect();
        names.extend(
            V2_1::cs_actions()
                .iter()
                .copied()
                .filter(|n| !STATE_DRIVEN.contains(n)),
        );
        names
    }

    #[test]
    /// OC-R-090 — the new 2.1 actions are classified typed vs raw-JSON by whether their required fields are nested/repeated.
    fn ut_new_2_1_actions_have_specs_or_are_json() {
        // Flat.
        assert!(action_spec("NotifyDERAlarm").is_some());
        assert!(action_spec("ClearDERControl").is_some());
        assert!(action_spec("SetDERControl").is_some());
        assert!(action_spec("GetTariffs").is_some());
        // Nested.
        assert!(action_spec("RequestBatterySwap").is_some());
        assert!(action_spec("NotifyAllowedEnergyTransfer").is_some());
        // JSON-only (required nested/repeated payloads).
        assert!(action_spec("BatterySwap").is_none());
        assert!(action_spec("SetDefaultTariff").is_none());
        assert!(json_actions().contains(&"BatterySwap"));
        assert!(json_actions().contains(&"SetDefaultTariff"));
    }

    #[test]
    /// OC-R-089 — a 2.1 action shared with 2.0.1 resolves to the same typed/raw-JSON classification through the version seam.
    fn ut_shared_actions_delegate_to_v2_0_1() {
        // A shared action classified by 2.0.1 must resolve identically through 2.1's seam.
        assert!(action_spec("Reset").is_some());
        assert!(action_spec("SetChargingProfile").is_some());
        assert!(action_spec("TransactionEvent").is_none());
        assert!(json_actions().contains(&"TransactionEvent"));
    }

    /// Every dialog-reachable 2.1 action is exactly one of: a typed spec or an explicit JSON action.
    #[test]
    /// OC-R-089 — every dialog-reachable 2.1 action is exactly one of typed or raw-JSON.
    fn ut_every_dialog_action_is_classified() {
        for name in reachable() {
            let has_spec = action_spec(name).is_some();
            let is_json = json_actions().contains(&name);
            assert!(
                has_spec ^ is_json,
                "{name} must be exactly one of spec/json"
            );
        }
    }

    /// Every typed action's default-prefilled dialog assembles a payload that decodes into the real
    /// rust-ocpp 2.1 request type.
    #[test]
    /// OC-R-094 — every typed 2.1 action's default-prefilled dialog assembles a payload that decodes against the 2.1 request type.
    fn ut_default_payloads_decode_for_every_spec() {
        for name in reachable() {
            let Some(spec) = action_spec(name) else {
                continue;
            };
            let mut dialog = ActionDialog::filled_for_test(name.to_string(), &spec);
            let payload = dialog.payload_for_test();
            assert!(
                V2_1::decode_call(name, payload.clone()).is_ok(),
                "{name} default payload did not decode: {payload}"
            );
        }
    }
}
