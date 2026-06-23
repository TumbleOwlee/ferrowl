//! OCPP 2.0.1 charging-station state, split by level. [`CsState`] holds the CS-wide data (boot
//! identity, the variable store answering GetVariables / mutated by SetVariables, the latest CSMS
//! reservation, the heartbeat cadence) plus a list of [`ConnectorState`]s. Each connector carries
//! its EVSE id, 2.0.1 status enum, metering, and a locally-minted string transaction (with a seq
//! counter). Shared (behind `std::sync::RwLock`) between view, inbound handler, and Lua sim.

use crate::module::ocpp::client::backend::rfc3339_now;
pub use crate::module::ocpp::client::config::ConfigKey;

/// One connector's live state (EVSE id, metering, status, transaction) for OCPP 2.0.1.
pub struct ConnectorState {
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
    pub transaction_id: Option<String>,
    /// Set once the CSMS has acknowledged the `TransactionEvent(Started)`. Auto-MeterValues only
    /// transmit while this is true, so a failed start never leaks meter readings.
    pub tx_confirmed: bool,
    pub seq_no: i32,
    tx_counter: u64,
    /// Charging limit from the latest SetChargingProfile (in `limit_unit`), if any.
    pub limit: Option<f64>,
    pub limit_unit: String,
}

impl ConnectorState {
    /// A fresh connector on `evse_id`/`connector_id` with sensible simulation defaults.
    pub fn new(evse_id: i64, connector_id: i64) -> Self {
        Self {
            evse_id,
            connector_id,
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
            transaction_id: None,
            tx_confirmed: false,
            seq_no: 0,
            tx_counter: 0,
            limit: None,
            limit_unit: "A".to_string(),
        }
    }

    /// Mint a fresh transaction id and reset the sequence counter.
    pub fn start_tx(&mut self) -> String {
        let id = format!("ferrowl-tx-{}-{}", self.connector_id, self.tx_counter);
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
            nv(
                "Charge Limit",
                &self.limit_unit,
                self.limit
                    .map(|l| format!("{l:.1}"))
                    .unwrap_or_else(|| "—".to_string()),
            ),
        ]
    }

    /// Read a connector field by name, for `C_OCPP:Connector(id):Get(name)`.
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
            "Soc" | "StateOfCharge" => Vt::Float(self.soc),
            "Temperature" => Vt::Float(self.temperature),
            "Status" => Vt::String(self.status.clone()),
            "Rfid" => Vt::String(self.rfid.clone()),
            "ChargeLimit" => match self.limit {
                Some(l) => Vt::Float(l),
                None => Vt::Nil,
            },
            _ => return None,
        })
    }

    /// Write a connector field by name, for `C_OCPP:Connector(id):Set(name, value)`.
    pub fn set_field(&mut self, name: &str, value: ferrowl_lua::module::ValueType) -> bool {
        use ferrowl_lua::module::ValueType as Vt;
        let num = |v: &Vt| match v {
            Vt::Int(i) => Some(*i as f64),
            Vt::Float(f) => Some(*f),
            _ => None,
        };
        match (name, &value) {
            ("EvseId", _) => match num(&value) {
                Some(n) => self.evse_id = n as i64,
                None => return false,
            },
            ("ConnectorId", _) => match num(&value) {
                Some(n) => self.connector_id = n as i64,
                None => return false,
            },
            ("Voltage", _) => match num(&value) {
                Some(n) => self.voltage = n,
                None => return false,
            },
            ("Current" | "CurrentL1", _) => match num(&value) {
                Some(n) => self.current[0] = n,
                None => return false,
            },
            ("CurrentL2", _) => match num(&value) {
                Some(n) => self.current[1] = n,
                None => return false,
            },
            ("CurrentL3", _) => match num(&value) {
                Some(n) => self.current[2] = n,
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
            ("Soc" | "StateOfCharge", _) => match num(&value) {
                Some(n) => self.soc = n,
                None => return false,
            },
            ("Temperature", _) => match num(&value) {
                Some(n) => self.temperature = n,
                None => return false,
            },
            ("TotalEnergy", _) => match num(&value) {
                Some(n) => self.total_energy = n,
                None => return false,
            },
            ("SessionEnergy", _) => match num(&value) {
                Some(n) => self.session_energy = n,
                None => return false,
            },
            ("Phases", Vt::String(s)) => self.phases = s.clone(),
            ("Status", Vt::String(s)) => self.status = s.clone(),
            ("Rfid", Vt::String(s)) => self.rfid = s.clone(),
            ("ChargeLimit", _) => match num(&value) {
                Some(n) => self.limit = Some(n),
                None => return false,
            },
            _ => return false,
        }
        true
    }

    /// Energy meter reading in Wh.
    pub fn meter_wh(&self) -> i64 {
        (self.total_energy * 1000.0) as i64
    }

    /// An OCPP 2.0.1 `meterValue` array reflecting this connector's state, for MeterValues.
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

/// OCPP 2.0.1 charging-station (CS-level) state plus its connectors.
pub struct CsState {
    pub model: String,
    pub vendor: String,
    pub firmware_version: String,
    pub serial_number: String,
    /// idToken of the latest accepted ReserveNow from the CSMS, cleared on CancelReservation.
    pub reserved_rfid: Option<String>,
    /// Variable store (component-agnostic name/value), answers GetVariables.
    pub config: Vec<ConfigKey>,
    /// Heartbeat cadence (seconds) the CSMS returned in its last BootNotification response.
    pub heartbeat_interval_secs: Option<u64>,
    pub connectors: Vec<ConnectorState>,
}

impl Default for CsState {
    fn default() -> Self {
        let var = |key: &str, value: &str, readonly: bool| ConfigKey {
            key: key.to_string(),
            value: value.to_string(),
            readonly,
        };
        Self {
            model: "Ferrowl-EVSE".to_string(),
            vendor: "Ferrowl".to_string(),
            firmware_version: "1.0.0".to_string(),
            serial_number: "FERROWL-0001".to_string(),
            reserved_rfid: None,
            config: vec![
                var("HeartbeatInterval", "300", false),
                var("AuthCtrlr.Enabled", "true", false),
                var("EVSE.AvailabilityState", "Available", true),
            ],
            heartbeat_interval_secs: None,
            connectors: vec![ConnectorState::new(1, 1)],
        }
    }
}

/// A row in a state table (rendered by the generic client view).
#[derive(Clone, Debug)]
pub struct NvRow {
    pub name: String,
    pub unit: String,
    pub value: String,
}

impl CsState {
    /// Find a connector by its connector id.
    pub fn connector(&self, id: i64) -> Option<&ConnectorState> {
        self.connectors.iter().find(|c| c.connector_id == id)
    }

    /// Find a connector by its connector id (mutable).
    pub fn connector_mut(&mut self, id: i64) -> Option<&mut ConnectorState> {
        self.connectors.iter_mut().find(|c| c.connector_id == id)
    }

    /// Find the connector on an EVSE. In 2.0.1 entries are addressed by EVSE id (one connector per
    /// EVSE in the simulator), so this is the primary lookup for scope resolution.
    pub fn connector_by_evse(&self, evse: i64) -> Option<&ConnectorState> {
        self.connectors.iter().find(|c| c.evse_id == evse)
    }

    /// Find the connector on an EVSE (mutable).
    pub fn connector_mut_by_evse(&mut self, evse: i64) -> Option<&mut ConnectorState> {
        self.connectors.iter_mut().find(|c| c.evse_id == evse)
    }

    /// Add a connector `(evse, connector)` if its connector id is absent; returns whether added.
    pub fn add_connector(&mut self, evse: i64, connector: i64) -> bool {
        if self.connectors.iter().any(|c| c.connector_id == connector) {
            return false;
        }
        self.connectors.push(ConnectorState::new(evse, connector));
        self.connectors.sort_by_key(|c| (c.evse_id, c.connector_id));
        true
    }

    /// Rows for the CS-level state table (boot identity + the latest reservation).
    pub fn cs_rows(&self) -> Vec<NvRow> {
        let nv = |name: &str, value: String| NvRow {
            name: name.to_string(),
            unit: String::new(),
            value,
        };
        vec![
            nv("Model", self.model.clone()),
            nv("Vendor", self.vendor.clone()),
            nv("Firmware Version", self.firmware_version.clone()),
            nv("Serial Number", self.serial_number.clone()),
            nv(
                "Reserved RFID",
                self.reserved_rfid
                    .clone()
                    .unwrap_or_else(|| "—".to_string()),
            ),
        ]
    }

    /// Read a CS-level field by name, for the bare `C_OCPP:Get(name)` Lua binding.
    pub fn cs_get_field(&self, name: &str) -> Option<ferrowl_lua::module::ValueType> {
        use ferrowl_lua::module::ValueType as Vt;
        Some(match name {
            "Model" => Vt::String(self.model.clone()),
            "Vendor" => Vt::String(self.vendor.clone()),
            "FirmwareVersion" => Vt::String(self.firmware_version.clone()),
            "SerialNumber" => Vt::String(self.serial_number.clone()),
            "ReservedRfid" => Vt::String(self.reserved_rfid.clone().unwrap_or_default()),
            _ => return None,
        })
    }

    /// Write a CS-level field by name, for the bare `C_OCPP:Set(name, value)` Lua binding.
    pub fn cs_set_field(&mut self, name: &str, value: ferrowl_lua::module::ValueType) -> bool {
        use ferrowl_lua::module::ValueType as Vt;
        match (name, value) {
            ("Model", Vt::String(s)) => self.model = s,
            ("Vendor", Vt::String(s)) => self.vendor = s,
            ("FirmwareVersion", Vt::String(s)) => self.firmware_version = s,
            ("SerialNumber", Vt::String(s)) => self.serial_number = s,
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{ConnectorState, CsState};
    use ferrowl_lua::module::ValueType as Vt;

    #[test]
    fn ut_meter_values_payload_decodes() {
        use ferrowl_ocpp::{V2_0_1, Version};
        let c = ConnectorState::new(1, 1);
        let payload = serde_json::json!({
            "evseId": c.evse_id,
            "meterValue": c.meter_value_json(),
        });
        assert!(V2_0_1::decode_call("MeterValues", payload).is_ok());
    }

    #[test]
    fn ut_connector_get_set_field_evse_id() {
        let mut c = ConnectorState::new(1, 1);
        assert!(c.set_field("EvseId", Vt::Int(2)));
        assert!(matches!(c.get_field("EvseId"), Some(Vt::Int(2))));
        assert!(c.set_field("Voltage", Vt::Float(400.0)));
        assert!(matches!(c.get_field("Voltage"), Some(Vt::Float(v)) if v == 400.0));
        assert!(!c.set_field("Unknown", Vt::Bool(true)));
    }

    #[test]
    fn ut_cs_level_and_connectors() {
        let mut s = CsState::default();
        assert!(s.cs_set_field("Model", Vt::String("M".into())));
        assert!(matches!(s.cs_get_field("Model"), Some(Vt::String(ref v)) if v == "M"));
        assert!(s.cs_get_field("Power").is_none());
        assert!(s.add_connector(1, 2));
        assert!(!s.add_connector(1, 2));
        assert!(s.connector(2).is_some());
    }
}
