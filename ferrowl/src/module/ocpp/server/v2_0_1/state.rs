//! OCPP 2.0.1 CSMS-side observed state, split by level. The connector target is the EVSE id;
//! metering arrives via MeterValues (numeric `sampledValue`s), status via StatusNotification's
//! `connectorStatus`, and transactions via TransactionEvent.

use ferrowl_lua::module::ValueType;
use ferrowl_ocpp::{V2_0_1, Version};

use crate::module::ocpp::client::backend::rfc3339_now;
use crate::module::ocpp::client::lua_sim::OcppFields;
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
    fn apply_inbound(&mut self, name: &str, request: &serde_json::Value) {
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

    fn derive_payload(&self, name: &str, _connector_id: Option<i64>) -> Option<serde_json::Value> {
        Some(match name {
            "Reset" => serde_json::json!({ "type": "Immediate" }),
            "ClearCache" | "GetLocalListVersion" => serde_json::json!({}),
            _ => return None,
        })
    }

    fn fields(&self) -> Vec<(String, String)> {
        vec![
            ("Model".into(), self.model.clone()),
            ("Vendor".into(), self.vendor.clone()),
            ("FirmwareVersion".into(), self.firmware_version.clone()),
            ("SerialNumber".into(), self.serial_number.clone()),
            ("LastHeartbeat".into(), self.last_heartbeat.clone()),
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
    fn apply_inbound(&mut self, name: &str, request: &serde_json::Value) {
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
                        self.status = "Available".to_string();
                    }
                    _ => {}
                }
            }
            "MeterValues" => apply_meter_values(self, request),
            _ => {}
        }
    }

    fn derive_payload(&self, name: &str, connector_id: Option<i64>) -> Option<serde_json::Value> {
        let evse = connector_id.unwrap_or(self.evse_id);
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

    fn fields(&self) -> Vec<(String, String)> {
        vec![
            ("EvseId".into(), self.evse_id.to_string()),
            ("Status".into(), self.status.clone()),
            ("Rfid".into(), self.rfid.clone()),
            (
                "TransactionId".into(),
                self.transaction_id.clone().unwrap_or_default(),
            ),
        ]
    }

    fn metering(&self) -> Vec<(String, String)> {
        vec![
            ("Voltage".into(), format!("{:.1}", self.voltage)),
            ("CurrentL1".into(), format!("{:.1}", self.current[0])),
            ("CurrentL2".into(), format!("{:.1}", self.current[1])),
            ("CurrentL3".into(), format!("{:.1}", self.current[2])),
            ("Power".into(), format!("{:.1}", self.power)),
            ("Frequency".into(), format!("{:.2}", self.frequency)),
            ("TotalEnergy".into(), format!("{:.3}", self.total_energy)),
            (
                "SessionEnergy".into(),
                format!("{:.3}", self.session_energy),
            ),
            ("Soc".into(), format!("{:.1}", self.soc)),
            ("Temperature".into(), format!("{:.1}", self.temperature)),
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
