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
            "ChargeLimit" => match self.limit {
                Some(l) => ValueType::Float(l),
                None => return None,
            },
            "DefaultChargeLimit" => match self.default_limit {
                Some(l) => ValueType::Float(l),
                None => return None,
            },
            "MaxChargeLimit" => match self.max_limit {
                Some(l) => ValueType::Float(l),
                None => return None,
            },
            "ExternalChargeLimit" => match self.external_limit {
                Some(l) => ValueType::Float(l),
                None => return None,
            },
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
