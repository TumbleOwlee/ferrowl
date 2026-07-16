//! OCPP 2.0.1 CSMS-side observed state, split by level. The connector target is the EVSE id;
//! metering arrives via MeterValues (numeric `sampledValue`s), status via StatusNotification's
//! `connectorStatus`, and transactions via TransactionEvent.

use ferrowl_lua::module::ValueType;
use ferrowl_ocpp::{V2_0_1, Version};

use crate::module::ocpp::client::backend::rfc3339_now;
use crate::module::ocpp::client::lua_sim::OcppFields;
use crate::module::ocpp::server::backend::Scope;
use crate::module::ocpp::server::view::EntryStateT;

fn csms_action_names() -> Vec<&'static str> {
    V2_0_1::csms_actions().iter().map(|(n, _)| *n).collect()
}

/// CS-level (non-connector) observed state.
#[derive(Default)]
pub struct CsLevelState {
    pub model: String,
    pub vendor: String,
    pub firmware_version: String,
    pub serial_number: String,
    pub last_heartbeat: String,
}

impl OcppFields for CsLevelState {
    fn actions() -> Vec<&'static str> {
        csms_action_names()
    }
    fn get_field(&self, name: &str) -> Option<ValueType> {
        Some(match name {
            "Model" => ValueType::String(self.model.clone()),
            "Vendor" => ValueType::String(self.vendor.clone()),
            "FirmwareVersion" => ValueType::String(self.firmware_version.clone()),
            "SerialNumber" => ValueType::String(self.serial_number.clone()),
            "LastHeartbeat" => ValueType::String(self.last_heartbeat.clone()),
            _ => return None,
        })
    }
    fn set_field(&mut self, name: &str, value: ValueType) -> bool {
        match (name, value) {
            ("Model", ValueType::String(s)) => self.model = s,
            ("Vendor", ValueType::String(s)) => self.vendor = s,
            ("FirmwareVersion", ValueType::String(s)) => self.firmware_version = s,
            ("SerialNumber", ValueType::String(s)) => self.serial_number = s,
            _ => return false,
        }
        true
    }
}

impl EntryStateT for CsLevelState {
    fn apply_inbound(
        &mut self,
        name: &str,
        request: &serde_json::Value,
        _response: &serde_json::Value,
    ) {
        match name {
            "BootNotification" => {
                let cs = &request["chargingStation"];
                if let Some(m) = cs["model"].as_str() {
                    self.model = m.to_string();
                }
                if let Some(v) = cs["vendorName"].as_str() {
                    self.vendor = v.to_string();
                }
                if let Some(fw) = cs["firmwareVersion"].as_str() {
                    self.firmware_version = fw.to_string();
                }
                if let Some(sn) = cs["serialNumber"].as_str() {
                    self.serial_number = sn.to_string();
                }
            }
            "Heartbeat" => self.last_heartbeat = rfc3339_now(),
            _ => {}
        }
    }

    fn derive_payload(&self, name: &str, _scope: Scope) -> Option<serde_json::Value> {
        Some(match name {
            "Reset" => serde_json::json!({ "type": "Immediate" }),
            "ClearCache" | "GetLocalListVersion" => serde_json::json!({}),
            _ => return None,
        })
    }

    fn fields(&self) -> Vec<(String, String, String)> {
        vec![
            ("Model".into(), String::new(), self.model.clone()),
            ("Vendor".into(), String::new(), self.vendor.clone()),
            (
                "FirmwareVersion".into(),
                String::new(),
                self.firmware_version.clone(),
            ),
            (
                "SerialNumber".into(),
                String::new(),
                self.serial_number.clone(),
            ),
            (
                "LastHeartbeat".into(),
                String::new(),
                self.last_heartbeat.clone(),
            ),
        ]
    }
}

/// Per-EVSE observed state.
pub struct ConnectorState {
    pub evse_id: i64,
    pub voltage: f64,
    pub current: [f64; 3],
    pub power: f64,
    pub frequency: f64,
    pub total_energy: f64,
    pub session_energy: f64,
    pub soc: f64,
    pub temperature: f64,
    pub status: String,
    pub rfid: String,
    pub transaction_id: Option<String>,
    /// Read-only mirror of the per-purpose charging limits we have transmitted-and-had-accepted via
    /// SetChargingProfile. `limit` (TxProfile) is transaction-scoped; the others persist.
    pub limit: Option<f64>,
    pub limit_unit: String,
    pub default_limit: Option<f64>,
    pub default_limit_unit: String,
    pub max_limit: Option<f64>,
    pub max_limit_unit: String,
    pub external_limit: Option<f64>,
    pub external_limit_unit: String,
}

impl Default for ConnectorState {
    fn default() -> Self {
        Self {
            evse_id: 0,
            voltage: 0.0,
            current: [0.0; 3],
            power: 0.0,
            frequency: 0.0,
            total_energy: 0.0,
            session_energy: 0.0,
            soc: 0.0,
            temperature: 0.0,
            status: "Unknown".to_string(),
            rfid: String::new(),
            transaction_id: None,
            limit: None,
            limit_unit: "A".to_string(),
            default_limit: None,
            default_limit_unit: "A".to_string(),
            max_limit: None,
            max_limit_unit: "A".to_string(),
            external_limit: None,
            external_limit_unit: "A".to_string(),
        }
    }
}

impl OcppFields for ConnectorState {
    fn actions() -> Vec<&'static str> {
        csms_action_names()
    }
    fn get_field(&self, name: &str) -> Option<ValueType> {
        Some(match name {
            "EvseId" | "ConnectorId" => ValueType::Int(self.evse_id as i128),
            "Voltage" => ValueType::Float(self.voltage),
            "Current" | "CurrentL1" => ValueType::Float(self.current[0]),
            "CurrentL2" => ValueType::Float(self.current[1]),
            "CurrentL3" => ValueType::Float(self.current[2]),
            "Power" => ValueType::Float(self.power),
            "Frequency" => ValueType::Float(self.frequency),
            "TotalEnergy" => ValueType::Float(self.total_energy),
            "SessionEnergy" => ValueType::Float(self.session_energy),
            "Soc" => ValueType::Float(self.soc),
            "Temperature" => ValueType::Float(self.temperature),
            "Status" => ValueType::String(self.status.clone()),
            "Rfid" => ValueType::String(self.rfid.clone()),
            "TransactionId" => ValueType::String(self.transaction_id.clone().unwrap_or_default()),
            "ChargeLimit" => {
                let l = self.limit?;
                ValueType::Float(l)
            }
            "DefaultChargeLimit" => {
                let l = self.default_limit?;
                ValueType::Float(l)
            }
            "MaxChargeLimit" => {
                let l = self.max_limit?;
                ValueType::Float(l)
            }
            "ExternalChargeLimit" => {
                let l = self.external_limit?;
                ValueType::Float(l)
            }
            _ => return None,
        })
    }
    fn set_field(&mut self, name: &str, value: ValueType) -> bool {
        let num = |v: &ValueType| match v {
            ValueType::Int(i) => Some(*i as f64),
            ValueType::Float(f) => Some(*f),
            _ => None,
        };
        match (name, &value) {
            ("Voltage", _) => match num(&value) {
                Some(n) => self.voltage = n,
                None => return false,
            },
            ("Power", _) => match num(&value) {
                Some(n) => self.power = n,
                None => return false,
            },
            ("Frequency", _) => match num(&value) {
                Some(n) => self.frequency = n,
                None => return false,
            },
            ("Soc", _) => match num(&value) {
                Some(n) => self.soc = n,
                None => return false,
            },
            ("Temperature", _) => match num(&value) {
                Some(n) => self.temperature = n,
                None => return false,
            },
            ("Status", ValueType::String(s)) => self.status = s.clone(),
            ("Rfid", ValueType::String(s)) => self.rfid = s.clone(),
            _ => return false,
        }
        true
    }
}

impl EntryStateT for ConnectorState {
    fn apply_inbound(
        &mut self,
        name: &str,
        request: &serde_json::Value,
        _response: &serde_json::Value,
    ) {
        if let Some(e) = evse_of(request) {
            self.evse_id = e;
        }
        match name {
            "StatusNotification" => {
                if let Some(s) = request["connectorStatus"].as_str() {
                    self.status = s.to_string();
                }
            }
            "TransactionEvent" => {
                if let Some(tag) = request["idToken"]["idToken"].as_str() {
                    self.rfid = tag.to_string();
                }
                match request["eventType"].as_str() {
                    Some("Started") => {
                        self.transaction_id = request["transactionInfo"]["transactionId"]
                            .as_str()
                            .map(str::to_owned);
                        self.status = "Charging".to_string();
                    }
                    Some("Ended") => {
                        self.transaction_id = None;
                        self.limit = None;
                        self.status = "Available".to_string();
                    }
                    _ => {}
                }
            }
            "MeterValues" => apply_meter_values(self, request),
            _ => {}
        }
    }

    fn apply_outbound(
        &mut self,
        name: &str,
        request: &serde_json::Value,
        response: &serde_json::Value,
    ) {
        // Mirror a SetChargingProfile the station accepted into the matching per-purpose limit.
        if name != "SetChargingProfile" || response["status"].as_str() != Some("Accepted") {
            return;
        }
        let profile = &request["chargingProfile"];
        let schedule = &profile["chargingSchedule"][0];
        let period = &schedule["chargingSchedulePeriod"][0];
        let Some(limit) = period["limit"].as_f64() else {
            return;
        };
        let unit = schedule["chargingRateUnit"]
            .as_str()
            .unwrap_or("A")
            .to_string();
        match profile["chargingProfilePurpose"].as_str() {
            Some("TxDefaultProfile") => {
                self.default_limit = Some(limit);
                self.default_limit_unit = unit;
            }
            Some("ChargingStationMaxProfile") => {
                self.max_limit = Some(limit);
                self.max_limit_unit = unit;
            }
            Some("ChargingStationExternalConstraints") => {
                self.external_limit = Some(limit);
                self.external_limit_unit = unit;
            }
            _ => {
                self.limit = Some(limit);
                self.limit_unit = unit;
            }
        }
    }

    fn derive_payload(&self, name: &str, scope: Scope) -> Option<serde_json::Value> {
        let evse = scope.evse.unwrap_or(self.evse_id);
        Some(match name {
            "RequestStartTransaction" => serde_json::json!({
                "evseId": evse,
                "remoteStartId": 1,
                "idToken": { "idToken": self.idtag(), "type": "Central" },
            }),
            "RequestStopTransaction" => {
                serde_json::json!({ "transactionId": self.transaction_id.clone()? })
            }
            "UnlockConnector" => serde_json::json!({ "evseId": evse, "connectorId": 1 }),
            _ => return None,
        })
    }

    fn fields(&self) -> Vec<(String, String, String)> {
        vec![
            ("EvseId".into(), String::new(), self.evse_id.to_string()),
            ("Status".into(), String::new(), self.status.clone()),
            ("Rfid".into(), String::new(), self.rfid.clone()),
            (
                "TransactionId".into(),
                String::new(),
                self.transaction_id.clone().unwrap_or_default(),
            ),
            limit_field("ChargeLimit", self.limit, &self.limit_unit),
            limit_field(
                "DefaultChargeLimit",
                self.default_limit,
                &self.default_limit_unit,
            ),
            limit_field("MaxChargeLimit", self.max_limit, &self.max_limit_unit),
            limit_field(
                "ExternalChargeLimit",
                self.external_limit,
                &self.external_limit_unit,
            ),
        ]
    }

    fn metering(&self) -> Vec<(String, String, String)> {
        vec![
            ("Voltage".into(), "V".into(), format!("{:.1}", self.voltage)),
            (
                "CurrentL1".into(),
                "A".into(),
                format!("{:.1}", self.current[0]),
            ),
            (
                "CurrentL2".into(),
                "A".into(),
                format!("{:.1}", self.current[1]),
            ),
            (
                "CurrentL3".into(),
                "A".into(),
                format!("{:.1}", self.current[2]),
            ),
            ("Power".into(), "W".into(), format!("{:.1}", self.power)),
            (
                "Frequency".into(),
                "Hz".into(),
                format!("{:.2}", self.frequency),
            ),
            (
                "TotalEnergy".into(),
                "kWh".into(),
                format!("{:.3}", self.total_energy),
            ),
            (
                "SessionEnergy".into(),
                "kWh".into(),
                format!("{:.3}", self.session_energy),
            ),
            ("Soc".into(), "%".into(), format!("{:.1}", self.soc)),
            (
                "Temperature".into(),
                "°C".into(),
                format!("{:.1}", self.temperature),
            ),
        ]
    }
}

impl ConnectorState {
    fn idtag(&self) -> String {
        if self.rfid.is_empty() {
            "DEADBEEF".to_string()
        } else {
            self.rfid.clone()
        }
    }
}

/// A `(field, unit, value)` state row for an optional per-purpose charging limit; the value is `—`
/// when no limit has been mirrored.
fn limit_field(name: &str, limit: Option<f64>, unit: &str) -> (String, String, String) {
    (
        name.into(),
        unit.into(),
        limit
            .map(|l| format!("{l:.1}"))
            .unwrap_or_else(|| "—".to_string()),
    )
}

/// The EVSE id an OCPP 2.0.1 request targets, from `evse.id`, top-level `evseId`, or `connectorId`.
fn evse_of(request: &serde_json::Value) -> Option<i64> {
    request["evse"]["id"]
        .as_i64()
        .or_else(|| request["evseId"].as_i64())
        .or_else(|| request["connectorId"].as_i64())
}

/// Fold an OCPP 2.0.1 MeterValues request's numeric `sampledValue`s into the connector's metering.
fn apply_meter_values(state: &mut ConnectorState, request: &serde_json::Value) {
    let Some(meter_values) = request["meterValue"].as_array() else {
        return;
    };
    for mv in meter_values {
        let Some(samples) = mv["sampledValue"].as_array() else {
            continue;
        };
        for s in samples {
            let value = s["value"]
                .as_f64()
                .or_else(|| s["value"].as_str().and_then(|v| v.parse().ok()))
                .unwrap_or(0.0);
            match s["measurand"]
                .as_str()
                .unwrap_or("Energy.Active.Import.Register")
            {
                "Voltage" => state.voltage = value,
                "Power.Active.Import" => state.power = value,
                "Frequency" => state.frequency = value,
                "Temperature" => state.temperature = value,
                "SoC" => state.soc = value,
                "Energy.Active.Import.Register" => state.total_energy = value / 1000.0,
                "Energy.Active.Import.Interval" => state.session_energy = value / 1000.0,
                "Current.Import" => {
                    let idx = match s["phase"].as_str() {
                        Some("L2") => 1,
                        Some("L3") => 2,
                        _ => 0,
                    };
                    state.current[idx] = value;
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// OC-R-077 — the 2.0.1 CSMS observes a station's EVSEs from inbound MeterValues traffic and tracks their state.
    fn ut_connector_meter_values_update_state() {
        let mut s = ConnectorState::default();
        let req = serde_json::json!({
            "evseId": 2,
            "meterValue": [{ "timestamp": "t", "sampledValue": [
                { "value": 230.0, "measurand": "Voltage", "unit": "V" },
                { "value": 16.0, "measurand": "Current.Import", "phase": "L1", "unit": "A" },
                { "value": 11000, "measurand": "Power.Active.Import", "unit": "W" },
                { "value": 5000, "measurand": "Energy.Active.Import.Register", "unit": "Wh" },
            ]}]
        });
        s.apply_inbound("MeterValues", &req, &serde_json::Value::Null);
        assert_eq!(s.evse_id, 2);
        assert_eq!(s.voltage, 230.0);
        assert_eq!(s.current[0], 16.0);
        assert_eq!(s.power, 11000.0);
        assert_eq!(s.total_energy, 5.0); // Wh → kWh
    }

    #[test]
    /// OC-R-077 — the 2.0.1 CSMS derives an EVSE's observed state, tracked per connection rather than pre-configured.
    fn ut_connector_derive_payload() {
        let mut s = ConnectorState::default();
        s.apply_inbound(
            "TransactionEvent",
            &serde_json::json!({
                "evseId": 1,
                "eventType": "Started",
                "idToken": { "idToken": "ABC" },
                "transactionInfo": { "transactionId": "42" },
            }),
            &serde_json::Value::Null,
        );
        let p = s
            .derive_payload("RequestStartTransaction", Scope::evse(1, None))
            .unwrap();
        assert_eq!(p["evseId"], 1);
        assert_eq!(p["idToken"]["idToken"], "ABC");
        // The transactionId minted via TransactionEvent(Started) is recorded, so RequestStop can derive.
        assert_eq!(s.transaction_id, Some("42".to_string()));
        assert_eq!(
            s.derive_payload("RequestStopTransaction", Scope::evse(1, None))
                .unwrap()["transactionId"],
            "42"
        );
        // Complex action → JSON editor fallback.
        assert!(
            s.derive_payload("ReserveNow", Scope::evse(1, None))
                .is_none()
        );
    }

    #[test]
    /// OC-R-067 — an accepted charging profile is mirrored into the targeted EVSE under the field matching its purpose.
    fn ut_apply_outbound_mirrors_accepted_profile_by_purpose() {
        let mut s = ConnectorState::default();
        let profile = |purpose: &str, limit: f64| {
            serde_json::json!({
                "evseId": 1,
                "chargingProfile": {
                    "chargingProfilePurpose": purpose,
                    "chargingSchedule": [{
                        "chargingRateUnit": "A",
                        "chargingSchedulePeriod": [{ "startPeriod": 0, "limit": limit }],
                    }],
                },
            })
        };
        let accepted = serde_json::json!({ "status": "Accepted" });
        s.apply_outbound(
            "SetChargingProfile",
            &profile("TxDefaultProfile", 10.0),
            &accepted,
        );
        s.apply_outbound(
            "SetChargingProfile",
            &profile("ChargingStationMaxProfile", 32.0),
            &accepted,
        );
        s.apply_outbound(
            "SetChargingProfile",
            &profile("ChargingStationExternalConstraints", 20.0),
            &accepted,
        );
        assert_eq!(s.default_limit, Some(10.0));
        assert_eq!(s.max_limit, Some(32.0));
        assert_eq!(s.external_limit, Some(20.0));
        assert_eq!(s.limit, None);
        // A rejected response is not mirrored.
        s.apply_outbound(
            "SetChargingProfile",
            &profile("TxProfile", 16.0),
            &serde_json::json!({ "status": "Rejected" }),
        );
        assert_eq!(s.limit, None);
    }

    #[test]
    /// OC-R-072 — ending a transaction clears only the transaction-scoped limit; the default and maximum limits persist.
    fn ut_limit_fields_are_readonly_and_stop_clears_only_tx() {
        let mut s = ConnectorState::default();
        // The mirror fields reject writes via the Lua/edit path.
        assert!(!s.set_field("ChargeLimit", ValueType::Float(16.0)));
        assert!(!s.set_field("DefaultChargeLimit", ValueType::Float(10.0)));
        assert_eq!(s.limit, None);
        // A stop (TransactionEvent Ended) clears only the transaction-scoped limit.
        s.limit = Some(16.0);
        s.default_limit = Some(10.0);
        s.transaction_id = Some("7".to_string());
        s.apply_inbound(
            "TransactionEvent",
            &serde_json::json!({ "eventType": "Ended" }),
            &serde_json::Value::Null,
        );
        assert_eq!(s.transaction_id, None);
        assert_eq!(s.limit, None);
        assert_eq!(s.default_limit, Some(10.0));
    }

    #[test]
    /// OC-R-077 — the 2.0.1 CSMS observes and derives charge-point-level state from inbound traffic.
    fn ut_cs_level_boot_and_derive() {
        let mut s = CsLevelState::default();
        s.apply_inbound(
            "BootNotification",
            &serde_json::json!({ "chargingStation": { "model": "M", "vendorName": "V" } }),
            &serde_json::Value::Null,
        );
        assert_eq!(s.model, "M");
        assert_eq!(s.vendor, "V");
        assert_eq!(
            s.derive_payload("Reset", Scope::CS).unwrap()["type"],
            "Immediate"
        );
        assert!(s.derive_payload("UnlockConnector", Scope::CS).is_none());
    }

    #[test]
    fn ut_connector_get_field_covers_all() {
        let mut s = ConnectorState {
            evse_id: 2,
            voltage: 230.0,
            current: [16.0, 15.0, 14.0],
            status: "Charging".to_string(),
            transaction_id: Some("tx-1".to_string()),
            ..Default::default()
        };
        assert!(matches!(s.get_field("EvseId"), Some(ValueType::Int(2))));
        assert!(matches!(s.get_field("Voltage"), Some(ValueType::Float(v)) if v == 230.0));
        assert!(matches!(s.get_field("CurrentL3"), Some(ValueType::Float(v)) if v == 14.0));
        assert!(matches!(s.get_field("Status"), Some(ValueType::String(ref v)) if v == "Charging"));
        assert!(matches!(s.get_field("TransactionId"), Some(ValueType::String(ref v)) if v == "tx-1"));
        assert!(s.get_field("ExternalChargeLimit").is_none());
        s.external_limit = Some(20.0);
        assert!(matches!(s.get_field("ExternalChargeLimit"), Some(ValueType::Float(v)) if v == 20.0));
        assert!(s.get_field("Nope").is_none());
    }

    #[test]
    fn ut_connector_set_field_coerces_and_rejects() {
        let mut s = ConnectorState::default();
        assert!(s.set_field("Voltage", ValueType::Int(230)));
        assert_eq!(s.voltage, 230.0);
        assert!(s.set_field("Status", ValueType::String("Faulted".into())));
        assert!(s.set_field("Rfid", ValueType::String("ABC".into())));
        assert!(!s.set_field("Voltage", ValueType::String("x".into())));
        assert!(!s.set_field("Unknown", ValueType::Int(1)));
    }

    #[test]
    fn ut_connector_metering_and_fields_rows() {
        let mut s = ConnectorState {
            voltage: 230.0,
            ..Default::default()
        };
        assert_eq!(s.metering().len(), 10);
        assert!(s.fields().iter().any(|(n, _, v)| n == "ChargeLimit" && v == "—"));
        s.max_limit = Some(32.0);
        assert!(s.fields().iter().any(|(n, _, v)| n == "MaxChargeLimit" && v == "32.0"));
    }

    /// OC-R-077 — EVSE-scoped remote actions derive their payload from observed state.
    #[test]
    fn ut_connector_derive_payloads() {
        let mut s = ConnectorState {
            evse_id: 3,
            ..Default::default()
        };
        let start = s.derive_payload("RequestStartTransaction", Scope::evse(3, None)).unwrap();
        assert_eq!(start["evseId"], 3);
        assert_eq!(start["idToken"]["idToken"], "DEADBEEF"); // empty RFID → default tag
        assert_eq!(
            s.derive_payload("UnlockConnector", Scope::evse(3, None)).unwrap()["evseId"],
            3
        );
        // No transaction yet → RequestStop derives no payload.
        assert!(s.derive_payload("RequestStopTransaction", Scope::evse(3, None)).is_none());
        s.transaction_id = Some("tx-9".to_string());
        assert_eq!(
            s.derive_payload("RequestStopTransaction", Scope::evse(3, None)).unwrap()["transactionId"],
            "tx-9"
        );
    }

    /// OC-R-077 — a TransactionEvent drives the observed transaction and status.
    #[test]
    fn ut_connector_transaction_event_lifecycle() {
        let mut s = ConnectorState::default();
        s.apply_inbound(
            "TransactionEvent",
            &serde_json::json!({
                "evseId": 1,
                "eventType": "Started",
                "idToken": { "idToken": "RF" },
                "transactionInfo": { "transactionId": "tx-7" },
            }),
            &serde_json::Value::Null,
        );
        assert_eq!(s.rfid, "RF");
        assert_eq!(s.transaction_id.as_deref(), Some("tx-7"));
        assert_eq!(s.status, "Charging");
        s.apply_inbound(
            "TransactionEvent",
            &serde_json::json!({ "eventType": "Ended" }),
            &serde_json::Value::Null,
        );
        assert_eq!(s.transaction_id, None);
        assert_eq!(s.status, "Available");
    }

    #[test]
    fn ut_evse_of_reads_all_shapes() {
        assert_eq!(evse_of(&serde_json::json!({ "evse": { "id": 5 } })), Some(5));
        assert_eq!(evse_of(&serde_json::json!({ "evseId": 6 })), Some(6));
        assert_eq!(evse_of(&serde_json::json!({ "connectorId": 7 })), Some(7));
        assert_eq!(evse_of(&serde_json::json!({})), None);
    }

    #[test]
    fn ut_cs_level_get_set_and_heartbeat() {
        let mut s = CsLevelState::default();
        assert!(s.set_field("Model", ValueType::String("M".into())));
        assert!(!s.set_field("LastHeartbeat", ValueType::String("x".into()))); // read-only
        assert!(!s.set_field("Model", ValueType::Int(1))); // wrong type
        assert!(matches!(s.get_field("Model"), Some(ValueType::String(ref v)) if v == "M"));
        assert!(s.get_field("Unknown").is_none());
        assert_eq!(s.fields().len(), 5);
        assert!(!CsLevelState::actions().is_empty());
        s.apply_inbound("Heartbeat", &serde_json::json!({}), &serde_json::Value::Null);
        assert!(!s.last_heartbeat.is_empty());
        assert_eq!(s.derive_payload("ClearCache", Scope::CS).unwrap(), serde_json::json!({}));
    }
}
