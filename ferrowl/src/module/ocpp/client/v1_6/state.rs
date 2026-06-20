//! OCPP 1.6 charging-station state. Holds the system parameters the CS exposes/uses: metering
//! (fed into MeterValues), connector/transaction status, boot identity, and the configuration-key
//! store that answers GetConfiguration / is mutated by ChangeConfiguration. Shared (behind a
//! `std::sync::RwLock`) between the view (sync render/edit) and the inbound handler (brief locks,
//! never held across an await).

use crate::module::ocpp::client::backend::rfc3339_now;
pub use crate::module::ocpp::client::config::ConfigKey;

/// OCPP 1.6 charging-station state.
pub struct CsState {
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
    pub transaction_id: Option<i64>,
    /// Charging limit from the latest SetChargingProfile (in `limit_unit`), if any.
    pub limit: Option<f64>,
    pub limit_unit: String,
    pub config: Vec<ConfigKey>,
}

impl Default for CsState {
    fn default() -> Self {
        let cfg = |key: &str, value: &str, readonly: bool| ConfigKey {
            key: key.to_string(),
            value: value.to_string(),
            readonly,
        };
        Self {
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
            limit: None,
            limit_unit: "A".to_string(),
            config: vec![
                cfg("HeartbeatInterval", "300", false),
                cfg("MeterValueSampleInterval", "60", false),
                cfg("NumberOfConnectors", "1", true),
                cfg("ConnectorPhaseRotation", "NotApplicable", false),
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

/// A row in the configuration-key table.
#[derive(Clone, Debug)]
pub struct ConfigRow {
    pub key: String,
    pub value: String,
    pub ro: String,
}

impl CsState {
    /// Rows for the state table (metering + connector + identity). Config keys are managed by the
    /// inbound handler, not shown here.
    pub fn rows(&self) -> Vec<NvRow> {
        let nv = |name: &str, unit: &str, value: String| NvRow {
            name: name.to_string(),
            unit: unit.to_string(),
            value,
        };
        vec![
            nv("Connector ID", "", self.connector_id.to_string()),
            nv("Used Phases", "", self.phases.clone()),
            nv("Voltage", "V", format!("{:.1}", self.voltage)),
            nv("Current L1", "A", format!("{:.1}", self.current[0])),
            nv("Current L2", "A", format!("{:.1}", self.current[1])),
            nv("Current L3", "A", format!("{:.1}", self.current[2])),
            nv("Power", "W", format!("{:.1}", self.power)),
            nv("Total Energy", "kWh", format!("{:.2}", self.total_energy)),
            nv(
                "Session Energy",
                "kWh",
                format!("{:.2}", self.session_energy),
            ),
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

    /// Rows for the configuration-key table.
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

    /// Energy meter reading in Wh (StartTransaction/StopTransaction units).
    pub fn meter_wh(&self) -> i64 {
        (self.total_energy * 1000.0) as i64
    }

    /// An OCPP `meterValue` array reflecting the current state, for MeterValues.
    pub fn meter_value_json(&self) -> serde_json::Value {
        let mut sampled = Vec::new();
        for (i, phase) in ["L1", "L2", "L3"].iter().enumerate() {
            if self.phases.split(',').any(|p| p.trim() == *phase) {
                sampled.push(serde_json::json!({
                    "value": format!("{:.1}", self.current[i]),
                    "measurand": "Current.Import",
                    "phase": phase,
                    "unit": "A",
                }));
            }
        }
        sampled.push(serde_json::json!({
            "value": format!("{:.1}", self.voltage),
            "measurand": "Voltage",
            "unit": "V",
        }));
        sampled.push(serde_json::json!({
            "value": format!("{:.1}", self.power),
            "measurand": "Power.Active.Import",
            "unit": "W",
        }));
        sampled.push(serde_json::json!({
            "value": self.meter_wh().to_string(),
            "measurand": "Energy.Active.Import.Register",
            "unit": "Wh",
        }));
        serde_json::json!([{ "timestamp": rfc3339_now(), "sampledValue": sampled }])
    }
}
