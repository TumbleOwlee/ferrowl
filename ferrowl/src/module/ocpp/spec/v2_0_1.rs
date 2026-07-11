//! OCPP 2.0.1 action specs. Flat actions assemble a flat object; the rest fold a few flat editable
//! fields into the full nested request via a custom assembler (single sub-object or single-element
//! list — the same pattern as 1.6's SetChargingProfile/SendLocalList). Only the deeply-nested or
//! repeated-list payloads ([`json_actions`]) stay on the raw JSON editor. The completeness test
//! asserts every dialog-reachable action is classified (no silent JSON-by-absence).

use crate::module::ocpp::action_dialog::{
    ActionSpec, Assembler, PropKind, PropSource, PropSpec, flat_object, prop,
};
use serde_json::{Map, Value, json};

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
const REPORT_BASE: &[&str] = &[
    "ConfigurationInventory",
    "FullInventory",
    "SummaryInventory",
];
const INSTALL_CERT_USE: &[&str] = &[
    "V2GRootCertificate",
    "MORootCertificate",
    "CSMSRootCertificate",
    "ManufacturerRootCertificate",
];
const GET_CERT_ID_USE: &[&str] = &[
    "V2GRootCertificate",
    "MORootCertificate",
    "CSMSRootCertificate",
    "V2GCertificateChain",
    "ManufacturerRootCertificate",
];
const MONITORING_BASE: &[&str] = &["All", "FactoryDefault", "HardWiredOnly"];
const CERT_SIGNING_USE: &[&str] = &["ChargingStationCertificate", "V2GCertificate"];
const CHARGING_LIMIT_SOURCE: &[&str] = &["EMS", "Other", "SO", "CSO"];
const FW_STATUS: &[&str] = &[
    "Downloaded",
    "DownloadFailed",
    "Downloading",
    "DownloadScheduled",
    "DownloadPaused",
    "Idle",
    "InstallationFailed",
    "Installing",
    "Installed",
    "InstallRebooting",
    "InstallScheduled",
    "InstallVerificationFailed",
    "InvalidSignature",
    "SignatureVerified",
];
const UPLOAD_LOG_STATUS: &[&str] = &[
    "BadMessage",
    "Idle",
    "NotSupportedOperation",
    "PermissionDenied",
    "Uploaded",
    "UploadFailure",
    "Uploading",
    "AcceptedCanceled",
];
const PUB_FW_STATUS: &[&str] = &[
    "Idle",
    "DownloadScheduled",
    "Downloading",
    "Downloaded",
    "Published",
    "DownloadFailed",
    "DownloadPaused",
    "InvalidChecksum",
    "ChecksumVerified",
    "PublishFailed",
];
const CERT_ACTION: &[&str] = &["Install", "Update"];
const RES_UPDATE_STATUS: &[&str] = &["Expired", "Removed"];
const HASH_ALGO: &[&str] = &["SHA256", "SHA384", "SHA512"];
const LOG_TYPE: &[&str] = &["DiagnosticsLog", "SecurityLog"];
const ID_TOKEN_TYPE: &[&str] = &[
    "Central",
    "EMAID",
    "ISO14443",
    "ISO15693",
    "KeyCode",
    "Local",
    "MacAddress",
    "NoAuthorization",
];
const UPDATE_TYPE: &[&str] = &["Differential", "Full"];
const MSG_PRIORITY: &[&str] = &["AlwaysFront", "InFront", "NormalCycle"];
const MSG_STATE: &[&str] = &["Charging", "Faulted", "Idle", "Unavailable"];
const MSG_FORMAT: &[&str] = &["ASCII", "HTML", "URI", "UTF8"];
const COMPONENT_CRITERION: &[&str] = &["Active", "Available", "Enabled", "Problem"];
const MONITORING_CRITERION: &[&str] = &[
    "ThresholdMonitoring",
    "DeltaMonitoring",
    "PeriodicMonitoring",
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

/// Collect the present `keys` (verbatim wire names) from `pairs` into a JSON object, skipping any
/// that are absent (optional-empty). Used to build optional sub-objects from flat rows.
fn collect(pairs: &[(&'static str, Value)], keys: &[&str]) -> Map<String, Value> {
    let mut m = Map::new();
    for k in keys {
        if let Some(v) = prop(pairs, k) {
            m.insert((*k).to_string(), v.clone());
        }
    }
    m
}

/// An `IdTokenType` object from a value row (`id_key`) and a type row (`type_key`).
fn id_token(pairs: &[(&'static str, Value)], id_key: &str, type_key: &str) -> Value {
    json!({
        "idToken": prop(pairs, id_key).cloned().unwrap_or(json!("")),
        "type": prop(pairs, type_key).cloned().unwrap_or(json!("ISO14443")),
    })
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

/// `{ id: [<id>] }` — a single-element monitor id list.
fn assemble_clear_variable_monitoring(pairs: &[(&'static str, Value)]) -> Value {
    json!({ "id": [prop(pairs, "id").cloned().unwrap_or(json!(1))] })
}

/// `{ certificateType?: [<type>] }` — optional single-element certificate-type list.
fn assemble_get_installed_certificate_ids(pairs: &[(&'static str, Value)]) -> Value {
    let mut m = Map::new();
    if let Some(t) = prop(pairs, "certificateType") {
        m.insert("certificateType".into(), json!([t]));
    }
    Value::Object(m)
}

/// `{ status, requestId?, location?: [<location>] }`.
fn assemble_publish_firmware_status(pairs: &[(&'static str, Value)]) -> Value {
    let mut m = collect(pairs, &["status", "requestId"]);
    if let Some(loc) = prop(pairs, "location") {
        m.insert("location".into(), json!([loc]));
    }
    Value::Object(m)
}

/// `{ chargingProfileId?, chargingProfileCriteria?: {evseId?, chargingProfilePurpose?, stackLevel?} }`.
fn assemble_clear_charging_profile(pairs: &[(&'static str, Value)]) -> Value {
    let mut m = collect(pairs, &["chargingProfileId"]);
    let criteria = collect(pairs, &["evseId", "chargingProfilePurpose", "stackLevel"]);
    if !criteria.is_empty() {
        m.insert("chargingProfileCriteria".into(), Value::Object(criteria));
    }
    Value::Object(m)
}

/// `{ certificateHashData: {hashAlgorithm, issuerNameHash, issuerKeyHash, serialNumber} }`.
fn assemble_delete_certificate(pairs: &[(&'static str, Value)]) -> Value {
    json!({
        "certificateHashData": {
            "hashAlgorithm": prop(pairs, "hashAlgorithm").cloned().unwrap_or(json!("SHA256")),
            "issuerNameHash": prop(pairs, "issuerNameHash").cloned().unwrap_or(json!("")),
            "issuerKeyHash": prop(pairs, "issuerKeyHash").cloned().unwrap_or(json!("")),
            "serialNumber": prop(pairs, "serialNumber").cloned().unwrap_or(json!("")),
        },
    })
}

/// `{ ocspRequestData: {hashAlgorithm, issuerNameHash, issuerKeyHash, serialNumber, responderURL} }`.
fn assemble_get_certificate_status(pairs: &[(&'static str, Value)]) -> Value {
    json!({
        "ocspRequestData": {
            "hashAlgorithm": prop(pairs, "hashAlgorithm").cloned().unwrap_or(json!("SHA256")),
            "issuerNameHash": prop(pairs, "issuerNameHash").cloned().unwrap_or(json!("")),
            "issuerKeyHash": prop(pairs, "issuerKeyHash").cloned().unwrap_or(json!("")),
            "serialNumber": prop(pairs, "serialNumber").cloned().unwrap_or(json!("")),
            "responderURL": prop(pairs, "responderURL").cloned().unwrap_or(json!("")),
        },
    })
}

/// `{ logType, requestId, retries?, retryInterval?, log: {remoteLocation} }`.
fn assemble_get_log(pairs: &[(&'static str, Value)]) -> Value {
    let mut m = collect(pairs, &["logType", "requestId", "retries", "retryInterval"]);
    m.insert(
        "log".into(),
        json!({ "remoteLocation": prop(pairs, "remoteLocation").cloned().unwrap_or(json!("")) }),
    );
    Value::Object(m)
}

/// `{ getVariableData: [{component: {name}, variable: {name}}] }` — a single requested variable.
fn assemble_get_variables(pairs: &[(&'static str, Value)]) -> Value {
    json!({
        "getVariableData": [{
            "component": { "name": prop(pairs, "componentName").cloned().unwrap_or(json!("")) },
            "variable": { "name": prop(pairs, "variableName").cloned().unwrap_or(json!("")) },
        }],
    })
}

/// `{ setVariableData: [{attributeValue, component: {name}, variable: {name}}] }` — one variable.
fn assemble_set_variables(pairs: &[(&'static str, Value)]) -> Value {
    json!({
        "setVariableData": [{
            "attributeValue": prop(pairs, "attributeValue").cloned().unwrap_or(json!("")),
            "component": { "name": prop(pairs, "componentName").cloned().unwrap_or(json!("")) },
            "variable": { "name": prop(pairs, "variableName").cloned().unwrap_or(json!("")) },
        }],
    })
}

/// `{ remoteStartId, idToken: {idToken, type}, evseId? }`.
fn assemble_request_start_transaction(pairs: &[(&'static str, Value)]) -> Value {
    let mut m = collect(pairs, &["evseId"]);
    m.insert(
        "remoteStartId".into(),
        prop(pairs, "remoteStartId").cloned().unwrap_or(json!(1)),
    );
    m.insert("idToken".into(), id_token(pairs, "idToken", "idTokenType"));
    Value::Object(m)
}

/// `{ id, expiryDateTime, idToken: {idToken, type}, evseId? }`.
fn assemble_reserve_now(pairs: &[(&'static str, Value)]) -> Value {
    let mut m = collect(pairs, &["id", "expiryDateTime", "evseId"]);
    m.insert("idToken".into(), id_token(pairs, "idToken", "idTokenType"));
    Value::Object(m)
}

/// `{ versionNumber, updateType, localAuthorizationList?: [{idToken: {idToken, type}}] }`.
fn assemble_send_local_list(pairs: &[(&'static str, Value)]) -> Value {
    let mut m = collect(pairs, &["versionNumber", "updateType"]);
    if prop(pairs, "idToken").is_some() {
        m.insert(
            "localAuthorizationList".into(),
            json!([{ "idToken": id_token(pairs, "idToken", "idTokenType") }]),
        );
    }
    Value::Object(m)
}

/// `{ requestId, retries?, retryInterval?, firmware: {location, retrieveDateTime} }`.
fn assemble_update_firmware(pairs: &[(&'static str, Value)]) -> Value {
    let mut m = collect(pairs, &["requestId", "retries", "retryInterval"]);
    m.insert(
        "firmware".into(),
        json!({
            "location": prop(pairs, "location").cloned().unwrap_or(json!("")),
            "retrieveDateTime": prop(pairs, "retrieveDateTime").cloned().unwrap_or(json!("")),
        }),
    );
    Value::Object(m)
}

/// `{ message: {id, priority, state?, message: {format, content}} }`.
fn assemble_set_display_message(pairs: &[(&'static str, Value)]) -> Value {
    let mut msg = collect(pairs, &["id", "priority", "state"]);
    msg.insert(
        "message".into(),
        json!({
            "format": prop(pairs, "format").cloned().unwrap_or(json!("UTF8")),
            "content": prop(pairs, "content").cloned().unwrap_or(json!("")),
        }),
    );
    json!({ "message": Value::Object(msg) })
}

/// `{ requestId, evseId?, chargingProfile: {chargingProfilePurpose?, stackLevel?} }`.
fn assemble_get_charging_profiles(pairs: &[(&'static str, Value)]) -> Value {
    let mut m = collect(pairs, &["requestId", "evseId"]);
    let criteria = collect(pairs, &["chargingProfilePurpose", "stackLevel"]);
    m.insert("chargingProfile".into(), Value::Object(criteria));
    Value::Object(m)
}

/// `{ requestId, componentCriteria?: [<criterion>] }`.
fn assemble_get_report(pairs: &[(&'static str, Value)]) -> Value {
    let mut m = collect(pairs, &["requestId"]);
    if let Some(c) = prop(pairs, "componentCriterion") {
        m.insert("componentCriteria".into(), json!([c]));
    }
    Value::Object(m)
}

/// `{ requestId, monitoringCriteria?: [<criterion>] }`.
fn assemble_get_monitoring_report(pairs: &[(&'static str, Value)]) -> Value {
    let mut m = collect(pairs, &["requestId"]);
    if let Some(c) = prop(pairs, "monitoringCriterion") {
        m.insert("monitoringCriteria".into(), json!([c]));
    }
    Value::Object(m)
}

/// `{ evseId?, chargingLimit: {chargingLimitSource} }`.
fn assemble_notify_charging_limit(pairs: &[(&'static str, Value)]) -> Value {
    let mut m = collect(pairs, &["evseId"]);
    m.insert(
        "chargingLimit".into(),
        json!({
            "chargingLimitSource": prop(pairs, "chargingLimitSource").cloned().unwrap_or(json!("EMS")),
        }),
    );
    Value::Object(m)
}

/// `{ requestId, id?: [<id>], priority?, state? }`.
fn assemble_get_display_messages(pairs: &[(&'static str, Value)]) -> Value {
    let mut m = collect(pairs, &["requestId", "priority", "state"]);
    if let Some(id) = prop(pairs, "id") {
        m.insert("id".into(), json!([id]));
    }
    Value::Object(m)
}

/// `{ requestId, messageInfo: [{id, priority, message: {format, content}}] }` — one message.
fn assemble_notify_display_messages(pairs: &[(&'static str, Value)]) -> Value {
    json!({
        "requestId": prop(pairs, "requestId").cloned().unwrap_or(json!(1)),
        "messageInfo": [{
            "id": prop(pairs, "id").cloned().unwrap_or(json!(1)),
            "priority": prop(pairs, "priority").cloned().unwrap_or(json!("NormalCycle")),
            "message": {
                "format": prop(pairs, "format").cloned().unwrap_or(json!("UTF8")),
                "content": prop(pairs, "content").cloned().unwrap_or(json!("")),
            },
        }],
    })
}

/// Dialog-reachable actions that intentionally stay on the raw JSON editor: deeply-nested or
/// repeated-list payloads a flat table can't express without dropping data. Kept explicit so the
/// completeness test forces a decision for new actions.
pub fn json_actions() -> &'static [&'static str] {
    &[
        // CSMS-originated.
        "SetNetworkProfile", // full NetworkConnectionProfile (APN/VPN sub-objects)
        "SetVariableMonitoring", // list of monitors, each with component/variable
        // CS-originated.
        "NotifyEVChargingNeeds",
        "NotifyEVChargingSchedule",
        "NotifyMonitoringReport", // monitor: [MonitoringDataType]
        "NotifyReport",           // reportData: [ReportDataType]
        "ReportChargingProfiles", // chargingProfile: [ChargingProfileType]
        "TransactionEvent",       // large; primarily state-driven
    ]
}

/// A decode-valid example payload for a [`json_actions`] entry, prefilling the raw JSON editor.
/// Unlike the serde-`Default` skeleton (which omits every optional and renders required lists as
/// `[]`), these spell out one representative element per list so the shape is editable in place.
pub fn json_template(name: &str) -> Option<Value> {
    const TS: &str = "2026-01-01T00:00:00Z";
    Some(match name {
        "SetNetworkProfile" => json!({
            "configurationSlot": 1,
            "connectionData": {
                "ocppVersion": "OCPP20",
                "ocppTransport": "JSON",
                "ocppCsmsUrl": "wss://csms.example.com/ocpp",
                "messageTimeout": 30,
                "securityProfile": 1,
                "ocppInterface": "Wired0",
            },
        }),
        "SetVariableMonitoring" => json!({
            "setMonitoringData": [{
                "component": { "name": "EVSE", "evse": { "id": 1 } },
                "variable": { "name": "Power.Active.Import" },
                "type": "UpperThreshold",
                "severity": 5,
                "value": 11000,
            }],
        }),
        "NotifyEVChargingNeeds" => json!({
            "evseId": 1,
            "chargingNeeds": {
                "requestedEnergyTransfer": "AC_three_phase",
                "acChargingParameters": {
                    "energyAmount": 20000,
                    "evMinCurrent": 6,
                    "evMaxCurrent": 32,
                    "evMaxVoltage": 400,
                },
            },
        }),
        "NotifyEVChargingSchedule" => json!({
            "evseId": 1,
            "timeBase": TS,
            "chargingSchedule": {
                "id": 1,
                "chargingRateUnit": "A",
                "chargingSchedulePeriod": [{ "startPeriod": 0, "limit": 16 }],
            },
        }),
        "NotifyMonitoringReport" => json!({
            "requestId": 1,
            "seqNo": 0,
            "generatedAt": TS,
            "monitor": [{
                "component": { "name": "EVSE", "evse": { "id": 1 } },
                "variable": { "name": "Power.Active.Import" },
                "variableMonitoring": [{
                    "id": 1,
                    "severity": 5,
                    "transaction": false,
                    "type": "UpperThreshold",
                    "value": 11000,
                }],
            }],
        }),
        "NotifyReport" => json!({
            "requestId": 1,
            "seqNo": 0,
            "generatedAt": TS,
            "reportData": [{
                "component": { "name": "ChargingStation" },
                "variable": { "name": "Model" },
                "variableAttribute": [{
                    "type": "Actual",
                    "value": "Example",
                    "mutability": "ReadOnly",
                }],
            }],
        }),
        "ReportChargingProfiles" => json!({
            "requestId": 1,
            "evseId": 1,
            "chargingLimitSource": "CSO",
            "chargingProfile": [{
                "id": 1,
                "stackLevel": 0,
                "chargingProfilePurpose": "TxDefaultProfile",
                "chargingProfileKind": "Absolute",
                "chargingSchedule": [{
                    "id": 1,
                    "chargingRateUnit": "A",
                    "chargingSchedulePeriod": [{ "startPeriod": 0, "limit": 16 }],
                }],
            }],
        }),
        "TransactionEvent" => json!({
            "eventType": "Started",
            "timestamp": TS,
            "triggerReason": "Authorized",
            "seqNo": 0,
            "transactionInfo": { "transactionId": "tx-1", "chargingState": "Charging" },
            "evse": { "id": 1, "connectorId": 1 },
            "idToken": { "idToken": "TAG-1", "type": "ISO14443" },
        }),
        _ => return None,
    })
}

/// The action spec for `name`, or `None` for actions handled by the raw JSON editor
/// ([`json_actions`]).
pub fn action_spec(name: &str) -> Option<ActionSpec> {
    use PropKind::*;
    use PropSource::*;
    Some(match name {
        // --- Flat ---
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
        "CancelReservation" => spec(&[PropSpec {
            name: "reservationId",
            kind: Number,
            source: Constant("1"),
            optional: false,
        }]),
        "ClearDisplayMessage" => spec(&[PropSpec {
            name: "id",
            kind: Number,
            source: Constant("1"),
            optional: false,
        }]),
        "CostUpdated" => spec(&[
            PropSpec {
                name: "totalCost",
                kind: Number,
                source: Constant("0"),
                optional: false,
            },
            PropSpec {
                name: "transactionId",
                kind: Text,
                source: StateField("TransactionId"),
                optional: false,
            },
        ]),
        "GetBaseReport" => spec(&[
            PropSpec {
                name: "requestId",
                kind: Number,
                source: Constant("1"),
                optional: false,
            },
            PropSpec {
                name: "reportBase",
                kind: Enum(REPORT_BASE),
                source: Constant("FullInventory"),
                optional: false,
            },
        ]),
        "GetCompositeSchedule" => spec(&[
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
            PropSpec {
                name: "evseId",
                kind: Number,
                source: StateField("EvseId"),
                optional: false,
            },
        ]),
        "GetTransactionStatus" => spec(&[PropSpec {
            name: "transactionId",
            kind: Text,
            source: StateField("TransactionId"),
            optional: true,
        }]),
        "InstallCertificate" => spec(&[
            PropSpec {
                name: "certificateType",
                kind: Enum(INSTALL_CERT_USE),
                source: Constant("CSMSRootCertificate"),
                optional: false,
            },
            PropSpec {
                name: "certificate",
                kind: Text,
                source: Empty,
                optional: false,
            },
        ]),
        "PublishFirmware" => spec(&[
            PropSpec {
                name: "location",
                kind: Text,
                source: Empty,
                optional: false,
            },
            PropSpec {
                name: "checksum",
                kind: Text,
                source: Empty,
                optional: false,
            },
            PropSpec {
                name: "requestId",
                kind: Number,
                source: Constant("1"),
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
        "SetMonitoringBase" => spec(&[PropSpec {
            name: "monitoringBase",
            kind: Enum(MONITORING_BASE),
            source: Constant("All"),
            optional: false,
        }]),
        "SetMonitoringLevel" => spec(&[PropSpec {
            name: "severity",
            kind: Number,
            source: Constant("5"),
            optional: false,
        }]),
        "UnpublishFirmware" => spec(&[PropSpec {
            name: "checksum",
            kind: Text,
            source: Empty,
            optional: false,
        }]),
        "CertificateSigned" => spec(&[
            PropSpec {
                name: "certificateChain",
                kind: Text,
                source: Empty,
                optional: false,
            },
            PropSpec {
                name: "certificateType",
                kind: Enum(CERT_SIGNING_USE),
                source: Empty,
                optional: true,
            },
        ]),
        "SignCertificate" => spec(&[
            PropSpec {
                name: "csr",
                kind: Text,
                source: Empty,
                optional: false,
            },
            PropSpec {
                name: "certificateType",
                kind: Enum(CERT_SIGNING_USE),
                source: Empty,
                optional: true,
            },
        ]),
        "ClearedChargingLimit" => spec(&[
            PropSpec {
                name: "chargingLimitSource",
                kind: Enum(CHARGING_LIMIT_SOURCE),
                source: Constant("EMS"),
                optional: false,
            },
            PropSpec {
                name: "evseId",
                kind: Number,
                source: StateField("EvseId"),
                optional: true,
            },
        ]),
        "FirmwareStatusNotification" => spec(&[
            PropSpec {
                name: "status",
                kind: Enum(FW_STATUS),
                source: Constant("Idle"),
                optional: false,
            },
            PropSpec {
                name: "requestId",
                kind: Number,
                source: Empty,
                optional: true,
            },
        ]),
        "LogStatusNotification" => spec(&[
            PropSpec {
                name: "status",
                kind: Enum(UPLOAD_LOG_STATUS),
                source: Constant("Idle"),
                optional: false,
            },
            PropSpec {
                name: "requestId",
                kind: Number,
                source: Empty,
                optional: true,
            },
        ]),
        "Get15118EVCertificate" => spec(&[
            PropSpec {
                name: "iso15118SchemaVersion",
                kind: Text,
                source: Empty,
                optional: false,
            },
            PropSpec {
                name: "action",
                kind: Enum(CERT_ACTION),
                source: Constant("Install"),
                optional: false,
            },
            PropSpec {
                name: "exiRequest",
                kind: Text,
                source: Empty,
                optional: false,
            },
        ]),
        "NotifyCustomerInformation" => spec(&[
            PropSpec {
                name: "data",
                kind: Text,
                source: Empty,
                optional: false,
            },
            PropSpec {
                name: "seqNo",
                kind: Number,
                source: Constant("0"),
                optional: false,
            },
            PropSpec {
                name: "generatedAt",
                kind: Timestamp,
                source: Now,
                optional: false,
            },
            PropSpec {
                name: "requestId",
                kind: Number,
                source: Constant("1"),
                optional: false,
            },
        ]),
        "ReservationStatusUpdate" => spec(&[
            PropSpec {
                name: "reservationId",
                kind: Number,
                source: Constant("1"),
                optional: false,
            },
            PropSpec {
                name: "reservationUpdateStatus",
                kind: Enum(RES_UPDATE_STATUS),
                source: Constant("Expired"),
                optional: false,
            },
        ]),
        "SecurityEventNotification" => spec(&[
            PropSpec {
                name: "type",
                kind: Text,
                source: Empty,
                optional: false,
            },
            PropSpec {
                name: "timestamp",
                kind: Timestamp,
                source: Now,
                optional: false,
            },
            PropSpec {
                name: "techInfo",
                kind: Text,
                source: Empty,
                optional: true,
            },
        ]),
        "CustomerInformation" => spec(&[
            PropSpec {
                name: "requestId",
                kind: Number,
                source: Constant("1"),
                optional: false,
            },
            PropSpec {
                name: "report",
                kind: Bool,
                source: Constant("true"),
                optional: false,
            },
            PropSpec {
                name: "clear",
                kind: Bool,
                source: Constant("false"),
                optional: false,
            },
            PropSpec {
                name: "customerIdentifier",
                kind: Text,
                source: Empty,
                optional: true,
            },
        ]),
        // --- Nested ---
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
        "ClearVariableMonitoring" => nested(
            &[PropSpec {
                name: "id",
                kind: Number,
                source: Constant("1"),
                optional: false,
            }],
            assemble_clear_variable_monitoring,
        ),
        "GetInstalledCertificateIds" => nested(
            &[PropSpec {
                name: "certificateType",
                kind: Enum(GET_CERT_ID_USE),
                source: Empty,
                optional: true,
            }],
            assemble_get_installed_certificate_ids,
        ),
        "PublishFirmwareStatusNotification" => nested(
            &[
                PropSpec {
                    name: "status",
                    kind: Enum(PUB_FW_STATUS),
                    source: Constant("Idle"),
                    optional: false,
                },
                PropSpec {
                    name: "location",
                    kind: Text,
                    source: Empty,
                    optional: true,
                },
                PropSpec {
                    name: "requestId",
                    kind: Number,
                    source: Empty,
                    optional: true,
                },
            ],
            assemble_publish_firmware_status,
        ),
        "ClearChargingProfile" => nested(
            &[
                PropSpec {
                    name: "chargingProfileId",
                    kind: Number,
                    source: Empty,
                    optional: true,
                },
                PropSpec {
                    name: "evseId",
                    kind: Number,
                    source: StateField("EvseId"),
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
            ],
            assemble_clear_charging_profile,
        ),
        "DeleteCertificate" => nested(
            &[
                PropSpec {
                    name: "hashAlgorithm",
                    kind: Enum(HASH_ALGO),
                    source: Constant("SHA256"),
                    optional: false,
                },
                PropSpec {
                    name: "issuerNameHash",
                    kind: Text,
                    source: Empty,
                    optional: false,
                },
                PropSpec {
                    name: "issuerKeyHash",
                    kind: Text,
                    source: Empty,
                    optional: false,
                },
                PropSpec {
                    name: "serialNumber",
                    kind: Text,
                    source: Empty,
                    optional: false,
                },
            ],
            assemble_delete_certificate,
        ),
        "GetCertificateStatus" => nested(
            &[
                PropSpec {
                    name: "hashAlgorithm",
                    kind: Enum(HASH_ALGO),
                    source: Constant("SHA256"),
                    optional: false,
                },
                PropSpec {
                    name: "issuerNameHash",
                    kind: Text,
                    source: Empty,
                    optional: false,
                },
                PropSpec {
                    name: "issuerKeyHash",
                    kind: Text,
                    source: Empty,
                    optional: false,
                },
                PropSpec {
                    name: "serialNumber",
                    kind: Text,
                    source: Empty,
                    optional: false,
                },
                PropSpec {
                    name: "responderURL",
                    kind: Text,
                    source: Empty,
                    optional: false,
                },
            ],
            assemble_get_certificate_status,
        ),
        "GetLog" => nested(
            &[
                PropSpec {
                    name: "logType",
                    kind: Enum(LOG_TYPE),
                    source: Constant("DiagnosticsLog"),
                    optional: false,
                },
                PropSpec {
                    name: "requestId",
                    kind: Number,
                    source: Constant("1"),
                    optional: false,
                },
                PropSpec {
                    name: "remoteLocation",
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
            ],
            assemble_get_log,
        ),
        "GetVariables" => nested(
            &[
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
            assemble_get_variables,
        ),
        "SetVariables" => nested(
            &[
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
                PropSpec {
                    name: "attributeValue",
                    kind: Text,
                    source: Empty,
                    optional: false,
                },
            ],
            assemble_set_variables,
        ),
        "RequestStartTransaction" => nested(
            &[
                PropSpec {
                    name: "idToken",
                    kind: Text,
                    source: StateField("Rfid"),
                    optional: false,
                },
                PropSpec {
                    name: "idTokenType",
                    kind: Enum(ID_TOKEN_TYPE),
                    source: Constant("ISO14443"),
                    optional: false,
                },
                PropSpec {
                    name: "remoteStartId",
                    kind: Number,
                    source: Constant("1"),
                    optional: false,
                },
                PropSpec {
                    name: "evseId",
                    kind: Number,
                    source: StateField("EvseId"),
                    optional: true,
                },
            ],
            assemble_request_start_transaction,
        ),
        "ReserveNow" => nested(
            &[
                PropSpec {
                    name: "id",
                    kind: Number,
                    source: Constant("1"),
                    optional: false,
                },
                PropSpec {
                    name: "expiryDateTime",
                    kind: Timestamp,
                    source: Now,
                    optional: false,
                },
                PropSpec {
                    name: "idToken",
                    kind: Text,
                    source: StateField("Rfid"),
                    optional: false,
                },
                PropSpec {
                    name: "idTokenType",
                    kind: Enum(ID_TOKEN_TYPE),
                    source: Constant("ISO14443"),
                    optional: false,
                },
                PropSpec {
                    name: "evseId",
                    kind: Number,
                    source: StateField("EvseId"),
                    optional: true,
                },
            ],
            assemble_reserve_now,
        ),
        "SendLocalList" => nested(
            &[
                PropSpec {
                    name: "versionNumber",
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
                    name: "idToken",
                    kind: Text,
                    source: StateField("Rfid"),
                    optional: true,
                },
                PropSpec {
                    name: "idTokenType",
                    kind: Enum(ID_TOKEN_TYPE),
                    source: Constant("ISO14443"),
                    optional: false,
                },
            ],
            assemble_send_local_list,
        ),
        "UpdateFirmware" => nested(
            &[
                PropSpec {
                    name: "requestId",
                    kind: Number,
                    source: Constant("1"),
                    optional: false,
                },
                PropSpec {
                    name: "location",
                    kind: Text,
                    source: Empty,
                    optional: false,
                },
                PropSpec {
                    name: "retrieveDateTime",
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
                PropSpec {
                    name: "retryInterval",
                    kind: Number,
                    source: Empty,
                    optional: true,
                },
            ],
            assemble_update_firmware,
        ),
        "SetDisplayMessage" => nested(
            &[
                PropSpec {
                    name: "id",
                    kind: Number,
                    source: Constant("1"),
                    optional: false,
                },
                PropSpec {
                    name: "priority",
                    kind: Enum(MSG_PRIORITY),
                    source: Constant("NormalCycle"),
                    optional: false,
                },
                PropSpec {
                    name: "format",
                    kind: Enum(MSG_FORMAT),
                    source: Constant("UTF8"),
                    optional: false,
                },
                PropSpec {
                    name: "content",
                    kind: Text,
                    source: Empty,
                    optional: false,
                },
                PropSpec {
                    name: "state",
                    kind: Enum(MSG_STATE),
                    source: Empty,
                    optional: true,
                },
            ],
            assemble_set_display_message,
        ),
        "GetChargingProfiles" => nested(
            &[
                PropSpec {
                    name: "requestId",
                    kind: Number,
                    source: Constant("1"),
                    optional: false,
                },
                PropSpec {
                    name: "evseId",
                    kind: Number,
                    source: StateField("EvseId"),
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
            ],
            assemble_get_charging_profiles,
        ),
        "GetReport" => nested(
            &[
                PropSpec {
                    name: "requestId",
                    kind: Number,
                    source: Constant("1"),
                    optional: false,
                },
                PropSpec {
                    name: "componentCriterion",
                    kind: Enum(COMPONENT_CRITERION),
                    source: Empty,
                    optional: true,
                },
            ],
            assemble_get_report,
        ),
        "GetMonitoringReport" => nested(
            &[
                PropSpec {
                    name: "requestId",
                    kind: Number,
                    source: Constant("1"),
                    optional: false,
                },
                PropSpec {
                    name: "monitoringCriterion",
                    kind: Enum(MONITORING_CRITERION),
                    source: Empty,
                    optional: true,
                },
            ],
            assemble_get_monitoring_report,
        ),
        "GetDisplayMessages" => nested(
            &[
                PropSpec {
                    name: "requestId",
                    kind: Number,
                    source: Constant("1"),
                    optional: false,
                },
                PropSpec {
                    name: "id",
                    kind: Number,
                    source: Empty,
                    optional: true,
                },
                PropSpec {
                    name: "priority",
                    kind: Enum(MSG_PRIORITY),
                    source: Empty,
                    optional: true,
                },
                PropSpec {
                    name: "state",
                    kind: Enum(MSG_STATE),
                    source: Empty,
                    optional: true,
                },
            ],
            assemble_get_display_messages,
        ),
        "NotifyChargingLimit" => nested(
            &[
                PropSpec {
                    name: "evseId",
                    kind: Number,
                    source: StateField("EvseId"),
                    optional: true,
                },
                PropSpec {
                    name: "chargingLimitSource",
                    kind: Enum(CHARGING_LIMIT_SOURCE),
                    source: Constant("EMS"),
                    optional: false,
                },
            ],
            assemble_notify_charging_limit,
        ),
        "NotifyDisplayMessages" => nested(
            &[
                PropSpec {
                    name: "requestId",
                    kind: Number,
                    source: Constant("1"),
                    optional: false,
                },
                PropSpec {
                    name: "id",
                    kind: Number,
                    source: Constant("1"),
                    optional: false,
                },
                PropSpec {
                    name: "priority",
                    kind: Enum(MSG_PRIORITY),
                    source: Constant("NormalCycle"),
                    optional: false,
                },
                PropSpec {
                    name: "format",
                    kind: Enum(MSG_FORMAT),
                    source: Constant("UTF8"),
                    optional: false,
                },
                PropSpec {
                    name: "content",
                    kind: Text,
                    source: Empty,
                    optional: false,
                },
            ],
            assemble_notify_display_messages,
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{action_spec, json_actions, json_template};
    use crate::module::ocpp::action_dialog::ActionDialog;
    use ferrowl_ocpp::{V2_0_1, Version};

    /// Every JSON-only action ships a handcrafted template that decodes and validates.
    #[test]
    fn ut_json_templates_cover_all_json_actions_and_decode() {
        for name in json_actions() {
            let template =
                json_template(name).unwrap_or_else(|| panic!("{name} has no JSON template"));
            let action = V2_0_1::decode_call(name, template)
                .unwrap_or_else(|e| panic!("{name} template does not decode: {e}"));
            V2_0_1::validate(&action)
                .unwrap_or_else(|e| panic!("{name} template fails validation: {e}"));
        }
    }

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
        // Formerly JSON-only, now typed.
        assert!(action_spec("RequestStartTransaction").is_some());
        assert!(action_spec("SetVariables").is_some());
        assert!(action_spec("CancelReservation").is_some());
        // Deeply-nested payloads remain on the JSON editor.
        assert!(action_spec("TransactionEvent").is_none());
        assert!(action_spec("SetNetworkProfile").is_none());
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

    /// Every typed action's default-prefilled dialog assembles a payload that decodes into the
    /// real rust-ocpp request type (required fields present, enum values + types valid). This is
    /// the guardrail against a wrong wire name / enum / nesting in a spec or assembler.
    #[test]
    fn ut_default_payloads_decode_for_every_spec() {
        let mut reachable: Vec<&str> = V2_0_1::csms_actions().iter().map(|(n, _)| *n).collect();
        reachable.extend(
            V2_0_1::cs_actions()
                .iter()
                .copied()
                .filter(|n| !STATE_DRIVEN.contains(n)),
        );
        for name in reachable {
            let Some(spec) = action_spec(name) else {
                continue;
            };
            // Fields whose default source is Empty but are required must be filled to decode; give
            // every required-text field a placeholder so the structural check is meaningful.
            let mut dialog = ActionDialog::filled_for_test(name.to_string(), &spec);
            let payload = dialog.payload_for_test();
            assert!(
                V2_0_1::decode_call(name, payload.clone()).is_ok(),
                "{name} default payload did not decode: {payload}"
            );
        }
    }
}
