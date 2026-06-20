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
    pub total_energy: f64,
    pub session_energy: f64,
    pub status: String,
    pub rfid: String,
    pub model: String,
    pub vendor: String,
    pub transaction_id: Option<String>,
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
            total_energy: 0.0,
            session_energy: 0.0,
            status: "Available".to_string(),
            rfid: "DEADBEEF".to_string(),
            model: "Ferrowl-EVSE".to_string(),
            vendor: "Ferrowl".to_string(),
            transaction_id: None,
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
            nv("Total Energy", "kWh", format!("{:.2}", self.total_energy)),
            nv("Session Energy", "kWh", format!("{:.2}", self.session_energy)),
            nv("Status", "", self.status.clone()),
            nv("RFID", "", self.rfid.clone()),
            nv("Model", "", self.model.clone()),
            nv("Vendor", "", self.vendor.clone()),
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
        serde_json::json!([{ "timestamp": rfc3339_now(), "sampledValue": sampled }])
    }
}
