//! OCPP 1.6 CSMS-side observed state, split by level: a CS-level record (model/vendor/firmware,
//! fed by BootNotification/Heartbeat/FirmwareStatusNotification) and a per-connector record
//! (metering/status/transaction, fed by StatusNotification/MeterValues/transactions). Both expose
//! their fields to Lua via `C_OCPP:Get`/`Set` and can derive simple outbound CSMS→CS payloads.

use ferrowl_lua::module::ValueType;
use ferrowl_ocpp::{V1_6, Version};

use crate::module::ocpp::client::backend::rfc3339_now;
use crate::module::ocpp::client::lua_sim::OcppFields;
use crate::module::ocpp::server::view::EntryStateT;

/// The CSMS action names exposed to Lua as `C_OCPP:<Action>` for OCPP 1.6.
fn csms_action_names() -> Vec<&'static str> {
    V1_6::csms_actions().iter().map(|(n, _)| *n).collect()
}

/// CS-level (non-connector) observed state.
#[derive(Default)]
pub struct CsLevelState {
    pub model: String,
    pub vendor: String,
    pub firmware_version: String,
    pub serial_number: String,
    pub iccid: String,
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
            "Iccid" => ValueType::String(self.iccid.clone()),
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
                if let Some(m) = request["chargePointModel"].as_str() {
                    self.model = m.to_string();
                }
                if let Some(v) = request["chargePointVendor"].as_str() {
                    self.vendor = v.to_string();
                }
                if let Some(fw) = request["firmwareVersion"].as_str() {
                    self.firmware_version = fw.to_string();
                }
                if let Some(sn) = request["chargePointSerialNumber"].as_str() {
                    self.serial_number = sn.to_string();
                }
                if let Some(iccid) = request["iccid"].as_str() {
                    self.iccid = iccid.to_string();
                }
            }
            "Heartbeat" => self.last_heartbeat = rfc3339_now(),
            "FirmwareStatusNotification" => {
                if let Some(s) = request["status"].as_str() {
                    self.firmware_version = format!("{} ({s})", self.firmware_version);
                }
            }
            _ => {}
        }
    }

    fn derive_payload(&self, name: &str, _connector_id: Option<i64>) -> Option<serde_json::Value> {
        Some(match name {
            "Reset" => serde_json::json!({ "type": "Soft" }),
            "ClearCache" | "GetLocalListVersion" | "GetConfiguration" => serde_json::json!({}),
            _ => return None,
        })
    }

    fn fields(&self) -> Vec<(String, String)> {
        vec![
            ("Model".into(), self.model.clone()),
            ("Vendor".into(), self.vendor.clone()),
            ("FirmwareVersion".into(), self.firmware_version.clone()),
            ("SerialNumber".into(), self.serial_number.clone()),
            ("Iccid".into(), self.iccid.clone()),
            ("LastHeartbeat".into(), self.last_heartbeat.clone()),
        ]
    }
}

/// Per-connector observed state.
pub struct ConnectorState {
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
    pub transaction_id: Option<i64>,
}

impl Default for ConnectorState {
    fn default() -> Self {
        Self {
            connector_id: 0,
            phases: "L1,L2,L3".to_string(),
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
            "ConnectorId" => ValueType::Int(self.connector_id as i128),
            "Phases" => ValueType::String(self.phases.clone()),
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
        if let Some(c) = request["connectorId"].as_i64() {
            self.connector_id = c;
        }
        match name {
            "StatusNotification" => {
                if let Some(s) = request["status"].as_str() {
                    self.status = s.to_string();
                }
            }
            "StartTransaction" => {
                if let Some(tag) = request["idTag"].as_str() {
                    self.rfid = tag.to_string();
                }
                self.status = "Charging".to_string();
            }
            "StopTransaction" => {
                self.transaction_id = None;
                self.status = "Available".to_string();
            }
            "MeterValues" => apply_meter_values(self, request),
            _ => {}
        }
    }

    fn derive_payload(&self, name: &str, connector_id: Option<i64>) -> Option<serde_json::Value> {
        let cid = connector_id.unwrap_or(self.connector_id);
        Some(match name {
            "RemoteStartTransaction" => {
                serde_json::json!({ "connectorId": cid, "idTag": self.idtag() })
            }
            "RemoteStopTransaction" => {
                serde_json::json!({ "transactionId": self.transaction_id? })
            }
            "UnlockConnector" => serde_json::json!({ "connectorId": cid }),
            "ChangeAvailability" => serde_json::json!({ "connectorId": cid, "type": "Operative" }),
            _ => return None,
        })
    }

    fn fields(&self) -> Vec<(String, String)> {
        vec![
            ("ConnectorId".into(), self.connector_id.to_string()),
            ("Status".into(), self.status.clone()),
            ("Rfid".into(), self.rfid.clone()),
            (
                "TransactionId".into(),
                self.transaction_id
                    .map(|t| t.to_string())
                    .unwrap_or_default(),
            ),
            ("Phases".into(), self.phases.clone()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_connector_meter_values_update_state() {
        let mut s = ConnectorState::default();
        let req = serde_json::json!({
            "connectorId": 2,
            "meterValue": [{ "timestamp": "t", "sampledValue": [
                { "value": "230.0", "measurand": "Voltage", "unit": "V" },
                { "value": "16.0", "measurand": "Current.Import", "phase": "L1", "unit": "A" },
                { "value": "11000", "measurand": "Power.Active.Import", "unit": "W" },
                { "value": "5000", "measurand": "Energy.Active.Import.Register", "unit": "Wh" },
            ]}]
        });
        s.apply_inbound("MeterValues", &req);
        assert_eq!(s.connector_id, 2);
        assert_eq!(s.voltage, 230.0);
        assert_eq!(s.current[0], 16.0);
        assert_eq!(s.power, 11000.0);
        assert_eq!(s.total_energy, 5.0); // Wh → kWh
    }

    #[test]
    fn ut_connector_derive_payload() {
        let mut s = ConnectorState::default();
        s.apply_inbound(
            "StartTransaction",
            &serde_json::json!({ "connectorId": 1, "idTag": "ABC" }),
        );
        let p = s.derive_payload("RemoteStartTransaction", Some(1)).unwrap();
        assert_eq!(p["connectorId"], 1);
        assert_eq!(p["idTag"], "ABC");
        // Without an observed transaction, RemoteStop can't be derived → JSON editor.
        assert!(s.derive_payload("RemoteStopTransaction", Some(1)).is_none());
        // Complex action → JSON editor fallback.
        assert!(s.derive_payload("ReserveNow", Some(1)).is_none());
    }

    #[test]
    fn ut_cs_level_boot_and_derive() {
        let mut s = CsLevelState::default();
        s.apply_inbound(
            "BootNotification",
            &serde_json::json!({ "chargePointModel": "M", "chargePointVendor": "V" }),
        );
        assert_eq!(s.model, "M");
        assert_eq!(s.vendor, "V");
        assert_eq!(s.derive_payload("Reset", None).unwrap()["type"], "Soft");
        assert!(s.derive_payload("UnlockConnector", None).is_none());
    }
}

/// Fold an OCPP 1.6 MeterValues request's `sampledValue`s into the connector's metering fields.
fn apply_meter_values(state: &mut ConnectorState, request: &serde_json::Value) {
    let Some(meter_values) = request["meterValue"].as_array() else {
        return;
    };
    for mv in meter_values {
        let Some(samples) = mv["sampledValue"].as_array() else {
            continue;
        };
        for s in samples {
            let value: f64 = s["value"]
                .as_str()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0);
            let measurand = s["measurand"]
                .as_str()
                .unwrap_or("Energy.Active.Import.Register");
            match measurand {
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
