//! OCPP 2.0.1 charging-station state. Like the 1.6 state but with 2.0.1 shapes: EVSE id, connector
//! status enum (Available/Occupied/…), a string transaction id minted locally with a seq counter,
//! and a *variable* store (name/value/readonly) that answers GetVariables / is mutated by
//! SetVariables. Shared (behind `std::sync::RwLock`) between view and inbound handler.

use crate::module::ocpp::client::backend::rfc3339_now;
pub use crate::module::ocpp::client::config::ConfigKey;

/// OCPP 2.0.1 charging-station state.
pub struct CsState {
    pub evse_id: i64,
    pub connector_id: i64,
    pub phases: String,
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
    pub model: String,
    pub vendor: String,
    pub firmware_version: String,
    pub serial_number: String,
    pub transaction_id: Option<String>,
    /// Set once the CSMS has acknowledged the `TransactionEvent(Started)`. Auto-MeterValues only
    /// transmit while this is true, so a failed start never leaks meter readings.
    pub tx_confirmed: bool,
    pub seq_no: i32,
    tx_counter: u64,
    /// Charging limit from the latest SetChargingProfile (in `limit_unit`), if any.
    pub limit: Option<f64>,
    pub limit_unit: String,
    /// Variable store (component-agnostic name/value), answers GetVariables.
    pub config: Vec<ConfigKey>,
}

impl Default for CsState {
    fn default() -> Self {
        let var = |key: &str, value: &str, readonly: bool| ConfigKey {
            key: key.to_string(),
            value: value.to_string(),
            readonly,
        };
        Self {
            evse_id: 1,
            connector_id: 1,
            phases: "L1,L2,L3".to_string(),
            voltage: 230.0,
            current: [0.0; 3],
            power: 0.0,
            frequency: 50.0,
            total_energy: 0.0,
            session_energy: 0.0,
            soc: 0.0,
            temperature: 25.0,
            status: "Available".to_string(),
            rfid: "DEADBEEF".to_string(),
            model: "Ferrowl-EVSE".to_string(),
            vendor: "Ferrowl".to_string(),
            firmware_version: "1.0.0".to_string(),
            serial_number: "FERROWL-0001".to_string(),
            transaction_id: None,
            tx_confirmed: false,
            seq_no: 0,
            tx_counter: 0,
            limit: None,
            limit_unit: "A".to_string(),
            config: vec![
                var("HeartbeatInterval", "300", false),
                var("AuthCtrlr.Enabled", "true", false),
                var("EVSE.AvailabilityState", "Available", true),
            ],
        }
    }
}

/// A row in the left "system state" table.
#[derive(Clone, Debug)]
pub struct NvRow {
    pub name: String,
    pub unit: String,
    pub value: String,
}

/// A row in the variable table.
#[derive(Clone, Debug)]
pub struct ConfigRow {
    pub key: String,
    pub value: String,
    pub ro: String,
}

impl CsState {
    /// Mint a fresh transaction id and reset the sequence counter.
    pub fn start_tx(&mut self) -> String {
        let id = format!("ferrowl-tx-{}", self.tx_counter);
        self.tx_counter += 1;
        self.seq_no = 0;
        self.transaction_id = Some(id.clone());
        self.tx_confirmed = false;
        id
    }

    /// Next sequence number for the running transaction.
    pub fn next_seq(&mut self) -> i32 {
        let n = self.seq_no;
        self.seq_no += 1;
        n
    }

    pub fn rows(&self) -> Vec<NvRow> {
        let nv = |name: &str, unit: &str, value: String| NvRow {
            name: name.to_string(),
            unit: unit.to_string(),
            value,
        };
        vec![
            nv("EVSE ID", "", self.evse_id.to_string()),
            nv("Connector ID", "", self.connector_id.to_string()),
            nv("Used Phases", "", self.phases.clone()),
            nv("Voltage", "V", format!("{:.1}", self.voltage)),
            nv("Current L1", "A", format!("{:.1}", self.current[0])),
            nv("Current L2", "A", format!("{:.1}", self.current[1])),
            nv("Current L3", "A", format!("{:.1}", self.current[2])),
            nv("Power", "W", format!("{:.1}", self.power)),
            nv("Frequency", "Hz", format!("{:.1}", self.frequency)),
            nv("Total Energy", "kWh", format!("{:.2}", self.total_energy)),
            nv(
                "Session Energy",
                "kWh",
                format!("{:.2}", self.session_energy),
            ),
            nv("State of Charge", "%", format!("{:.1}", self.soc)),
            nv("Temperature", "°C", format!("{:.1}", self.temperature)),
            nv("Status", "", self.status.clone()),
            nv("RFID", "", self.rfid.clone()),
            nv("Model", "", self.model.clone()),
            nv("Vendor", "", self.vendor.clone()),
            nv("Firmware Version", "", self.firmware_version.clone()),
            nv("Serial Number", "", self.serial_number.clone()),
            nv(
                "Charge Limit",
                &self.limit_unit,
                self.limit
                    .map(|l| format!("{l:.1}"))
                    .unwrap_or_else(|| "—".to_string()),
            ),
        ]
    }

    pub fn config_rows(&self) -> Vec<ConfigRow> {
        self.config
            .iter()
            .map(|c| ConfigRow {
                key: c.key.clone(),
                value: c.value.clone(),
                ro: if c.readonly { "yes" } else { "no" }.to_string(),
            })
            .collect()
    }

    /// Read a state field by name, for `C_OCPP:Get(name)`. Like the 1.6 mapping plus `EvseId`.
    pub fn get_field(&self, name: &str) -> Option<ferrowl_lua::module::ValueType> {
        use ferrowl_lua::module::ValueType as Vt;
        Some(match name {
            "EvseId" => Vt::Int(self.evse_id as i128),
            "ConnectorId" => Vt::Int(self.connector_id as i128),
            "Phases" => Vt::String(self.phases.clone()),
            "Voltage" => Vt::Float(self.voltage),
            "Current" | "CurrentL1" => Vt::Float(self.current[0]),
            "CurrentL2" => Vt::Float(self.current[1]),
            "CurrentL3" => Vt::Float(self.current[2]),
            "Power" => Vt::Float(self.power),
            "Frequency" => Vt::Float(self.frequency),
            "TotalEnergy" => Vt::Float(self.total_energy),
            "SessionEnergy" => Vt::Float(self.session_energy),
            "Soc" => Vt::Float(self.soc),
            "Temperature" => Vt::Float(self.temperature),
            "Status" => Vt::String(self.status.clone()),
            "Rfid" => Vt::String(self.rfid.clone()),
            "Model" => Vt::String(self.model.clone()),
            "Vendor" => Vt::String(self.vendor.clone()),
            "FirmwareVersion" => Vt::String(self.firmware_version.clone()),
            "SerialNumber" => Vt::String(self.serial_number.clone()),
            _ => return None,
        })
    }

    /// Write a state field by name, for `C_OCPP:Set(name, value)`. Numeric fields accept an int or
    /// float; string fields accept a string. Returns false for an unknown name or a type mismatch.
    pub fn set_field(&mut self, name: &str, value: ferrowl_lua::module::ValueType) -> bool {
        use ferrowl_lua::module::ValueType as Vt;
        let num = |v: &Vt| match v {
            Vt::Int(i) => Some(*i as f64),
            Vt::Float(f) => Some(*f),
            _ => None,
        };
        match (name, &value) {
            ("EvseId", _) => match num(&value) {
                Some(n) => {
                    self.evse_id = n as i64;
                    true
                }
                None => false,
            },
            ("ConnectorId", _) => match num(&value) {
                Some(n) => {
                    self.connector_id = n as i64;
                    true
                }
                None => false,
            },
            ("Voltage", _) => match num(&value) {
                Some(n) => {
                    self.voltage = n;
                    true
                }
                None => false,
            },
            ("Current" | "CurrentL1", _) => match num(&value) {
                Some(n) => {
                    self.current[0] = n;
                    true
                }
                None => false,
            },
            ("CurrentL2", _) => match num(&value) {
                Some(n) => {
                    self.current[1] = n;
                    true
                }
                None => false,
            },
            ("CurrentL3", _) => match num(&value) {
                Some(n) => {
                    self.current[2] = n;
                    true
                }
                None => false,
            },
            ("Power", _) => match num(&value) {
                Some(n) => {
                    self.power = n;
                    true
                }
                None => false,
            },
            ("Frequency", _) => match num(&value) {
                Some(n) => {
                    self.frequency = n;
                    true
                }
                None => false,
            },
            ("Soc", _) => match num(&value) {
                Some(n) => {
                    self.soc = n;
                    true
                }
                None => false,
            },
            ("Temperature", _) => match num(&value) {
                Some(n) => {
                    self.temperature = n;
                    true
                }
                None => false,
            },
            ("TotalEnergy", _) => match num(&value) {
                Some(n) => {
                    self.total_energy = n;
                    true
                }
                None => false,
            },
            ("SessionEnergy", _) => match num(&value) {
                Some(n) => {
                    self.session_energy = n;
                    true
                }
                None => false,
            },
            ("Phases", Vt::String(s)) => {
                self.phases = s.clone();
                true
            }
            ("Status", Vt::String(s)) => {
                self.status = s.clone();
                true
            }
            ("Rfid", Vt::String(s)) => {
                self.rfid = s.clone();
                true
            }
            ("Model", Vt::String(s)) => {
                self.model = s.clone();
                true
            }
            ("Vendor", Vt::String(s)) => {
                self.vendor = s.clone();
                true
            }
            ("FirmwareVersion", Vt::String(s)) => {
                self.firmware_version = s.clone();
                true
            }
            ("SerialNumber", Vt::String(s)) => {
                self.serial_number = s.clone();
                true
            }
            _ => false,
        }
    }

    /// Energy meter reading in Wh.
    pub fn meter_wh(&self) -> i64 {
        (self.total_energy * 1000.0) as i64
    }

    /// An OCPP 2.0.1 `meterValue` array reflecting the current state, for MeterValues. The 2.0.1
    /// `sampledValue.value` is numeric and the unit is nested under `unitOfMeasure`.
    pub fn meter_value_json(&self) -> serde_json::Value {
        let mut sampled = Vec::new();
        for (i, phase) in ["L1", "L2", "L3"].iter().enumerate() {
            if self.phases.split(',').any(|p| p.trim() == *phase) {
                sampled.push(serde_json::json!({
                    "value": self.current[i],
                    "measurand": "Current.Import",
                    "phase": phase,
                    "unitOfMeasure": { "unit": "A" },
                }));
            }
        }
        sampled.push(serde_json::json!({
            "value": self.voltage,
            "measurand": "Voltage",
            "unitOfMeasure": { "unit": "V" },
        }));
        sampled.push(serde_json::json!({
            "value": self.power,
            "measurand": "Power.Active.Import",
            "unitOfMeasure": { "unit": "W" },
        }));
        sampled.push(serde_json::json!({
            "value": self.meter_wh(),
            "measurand": "Energy.Active.Import.Register",
            "unitOfMeasure": { "unit": "Wh" },
        }));
        sampled.push(serde_json::json!({
            "value": self.frequency,
            "measurand": "Frequency",
            "unitOfMeasure": { "unit": "Hz" },
        }));
        // OCPP 2.0.1 has no Temperature measurand, so it is not reported here (only Frequency/SoC).
        sampled.push(serde_json::json!({
            "value": self.soc,
            "measurand": "SoC",
            "unitOfMeasure": { "unit": "Percent" },
        }));
        serde_json::json!([{ "timestamp": rfc3339_now(), "sampledValue": sampled }])
    }
}

#[cfg(test)]
mod tests {
    use super::CsState;
    use ferrowl_lua::module::ValueType as Vt;

    #[test]
    fn ut_meter_values_payload_decodes() {
        use ferrowl_ocpp::{V2_0_1, Version};
        let s = CsState::default();
        let payload = serde_json::json!({
            "evseId": s.evse_id,
            "meterValue": s.meter_value_json(),
        });
        assert!(V2_0_1::decode_call("MeterValues", payload).is_ok());
    }

    #[test]
    fn ut_get_set_field_evse_id() {
        let mut s = CsState::default();
        // EvseId is the 2.0.1-only field.
        assert!(s.set_field("EvseId", Vt::Int(2)));
        assert!(matches!(s.get_field("EvseId"), Some(Vt::Int(2))));
        assert!(s.set_field("Voltage", Vt::Float(400.0)));
        assert!(matches!(s.get_field("Voltage"), Some(Vt::Float(v)) if v == 400.0));
        assert!(!s.set_field("Unknown", Vt::Bool(true)));
    }
}
