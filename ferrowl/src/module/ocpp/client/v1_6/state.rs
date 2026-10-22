//! OCPP 1.6 charging-station state, split by level. [`CsState`] holds the charging-station-wide
//! data (boot identity, the configuration-key store answering GetConfiguration / mutated by
//! ChangeConfiguration, the latest CSMS reservation, the heartbeat cadence) plus a list of
//! [`ConnectorState`]s — one per connector multiplexed over the single websocket. Each connector
//! carries its own metering (fed into MeterValues), status and transaction. Shared (behind a
//! `parking_lot::RwLock`) between the view, the inbound handler, and the Lua sim.

use crate::module::ocpp::client::backend::rfc3339_now;
pub use crate::module::ocpp::client::config::ConfigKey;

/// One connector's live state (metering, status, transaction) for OCPP 1.6.
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
    /// Charging limit from the latest `TxProfile` SetChargingProfile (in `limit_unit`), if any.
    /// Transaction-scoped: cleared when the transaction ends.
    pub limit: Option<f64>,
    pub limit_unit: String,
    /// Charging limit from the latest `TxDefaultProfile` SetChargingProfile (in `default_limit_unit`).
    pub default_limit: Option<f64>,
    pub default_limit_unit: String,
    /// Charging limit from the latest `ChargePointMaxProfile` SetChargingProfile (in `max_limit_unit`).
    pub max_limit: Option<f64>,
    pub max_limit_unit: String,
    /// idTag of the latest accepted ReserveNow targeting this connector, cleared on a matching
    /// CancelReservation. A `connectorId` of 0 reserves the charge point itself (CS-level).
    pub reserved_rfid: Option<String>,
    /// reservationId of that reservation, used to match the CancelReservation that clears it.
    pub reservation_id: Option<i64>,
}

impl ConnectorState {
    /// A fresh connector with the given id and sensible simulation defaults.
    pub fn new(connector_id: i64) -> Self {
        Self {
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
            limit: None,
            limit_unit: "A".to_string(),
            default_limit: None,
            default_limit_unit: "A".to_string(),
            max_limit: None,
            max_limit_unit: "A".to_string(),
            reserved_rfid: None,
            reservation_id: None,
        }
    }
}

/// OCPP 1.6 charging-station (CS-level) state plus its connectors.
pub struct CsState {
    pub model: String,
    pub vendor: String,
    pub firmware_version: String,
    pub serial_number: String,
    /// SIM ICCID (OC-R-104). Empty = unset; omitted from `BootNotification` when empty.
    pub iccid: String,
    /// SIM IMSI (OC-R-104). Empty = unset; omitted from `BootNotification` when empty.
    pub imsi: String,
    /// Installed meter's serial number (OC-R-104). Empty = unset; omitted from
    /// `BootNotification` when empty.
    pub meter_serial_number: String,
    /// Installed meter's type/model (OC-R-104). Empty = unset; omitted from `BootNotification`
    /// when empty.
    pub meter_type: String,
    /// idTag of the latest accepted charge-point-wide ReserveNow (`connectorId` 0), cleared on a
    /// matching CancelReservation. Per-connector reservations live on [`ConnectorState`].
    pub reserved_rfid: Option<String>,
    /// reservationId of that CS-level reservation, used to match its CancelReservation.
    pub reservation_id: Option<i64>,
    pub config: Vec<ConfigKey>,
    /// Heartbeat cadence (seconds) the CSMS returned in its last BootNotification response. `None`
    /// until a BootNotification round-trips; the view falls back to a built-in default.
    pub heartbeat_interval_secs: Option<u64>,
    /// Connectors multiplexed over the single websocket. Always at least one.
    pub connectors: Vec<ConnectorState>,
}

impl Default for CsState {
    fn default() -> Self {
        let cfg = |key: &str, value: &str, readonly: bool| ConfigKey {
            key: key.to_string(),
            value: value.to_string(),
            readonly,
        };
        Self {
            model: "Ferrowl-EVSE".to_string(),
            vendor: "Ferrowl".to_string(),
            firmware_version: "1.0.0".to_string(),
            serial_number: "FERROWL-0001".to_string(),
            iccid: String::new(),
            imsi: String::new(),
            meter_serial_number: String::new(),
            meter_type: String::new(),
            reserved_rfid: None,
            reservation_id: None,
            heartbeat_interval_secs: None,
            // Common OCPP 1.6 standard configuration keys (§9.1) with sensible defaults and
            // spec-accurate read-only flags.
            config: vec![
                cfg("AuthorizeRemoteTxRequests", "true", false),
                cfg("ClockAlignedDataInterval", "0", false),
                cfg("ConnectionTimeOut", "60", false),
                cfg("ConnectorPhaseRotation", "NotApplicable", false),
                cfg("GetConfigurationMaxKeys", "100", true),
                cfg("HeartbeatInterval", "300", false),
                cfg("LocalAuthorizeOffline", "true", false),
                cfg("LocalPreAuthorize", "false", false),
                cfg(
                    "MeterValuesAlignedData",
                    "Energy.Active.Import.Register",
                    false,
                ),
                cfg(
                    "MeterValuesSampledData",
                    "Energy.Active.Import.Register",
                    false,
                ),
                cfg("MeterValueSampleInterval", "60", false),
                cfg("NumberOfConnectors", "1", true),
                cfg("ResetRetries", "1", false),
                cfg("StopTransactionOnEVSideDisconnect", "true", false),
                cfg("StopTransactionOnInvalidId", "true", false),
                cfg("StopTxnAlignedData", "", false),
                cfg("StopTxnSampledData", "", false),
                cfg("SupportedFeatureProfiles", "Core,SmartCharging", true),
                cfg("TransactionMessageAttempts", "3", false),
                cfg("TransactionMessageRetryInterval", "60", false),
                cfg("UnlockConnectorOnEVSideDisconnect", "true", false),
                cfg("WebSocketPingInterval", "60", false),
                cfg("LocalAuthListEnabled", "true", false),
                cfg("LocalAuthListMaxLength", "100", true),
                cfg("SendLocalListMaxLength", "100", true),
                // SmartCharging profile.
                cfg("ChargeProfileMaxStackLevel", "10", false),
                cfg(
                    "ChargingScheduleAllowedChargingRateUnit",
                    "Current,Power",
                    true,
                ),
                cfg("ChargingScheduleMaxPeriods", "10", true),
                cfg("MaxChargingProfilesInstalled", "10", true),
            ],
            connectors: vec![ConnectorState::new(1)],
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
    /// Find a connector by its id.
    pub fn connector(&self, id: i64) -> Option<&ConnectorState> {
        self.connectors.iter().find(|c| c.connector_id == id)
    }

    /// Find a connector by its id (mutable).
    pub fn connector_mut(&mut self, id: i64) -> Option<&mut ConnectorState> {
        self.connectors.iter_mut().find(|c| c.connector_id == id)
    }

    /// Add a connector with `id` if absent; returns whether it was added.
    pub fn add_connector(&mut self, id: i64) -> bool {
        if self.connectors.iter().any(|c| c.connector_id == id) {
            return false;
        }
        self.connectors.push(ConnectorState::new(id));
        self.connectors.sort_by_key(|c| c.connector_id);
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
            nv(
                "Reservation ID",
                self.reservation_id
                    .map_or_else(|| "—".to_string(), |id| id.to_string()),
            ),
            nv("ICCID", self.iccid.clone()),
            nv("IMSI", self.imsi.clone()),
            nv("Meter Serial Number", self.meter_serial_number.clone()),
            nv("Meter Type", self.meter_type.clone()),
        ]
    }

    /// Read a CS-level field by name, for the bare `C_OCPP:Get(name)` Lua binding.
    pub fn cs_get_field(&self, name: &str) -> Option<ferrowl_lua::module::ValueType> {
        use ferrowl_lua::module::ValueType as Vt;
        Some(match name {
            "Model" => Vt::String(self.model.clone()),
            "Vendor" => Vt::String(self.vendor.clone()),
            "Firmware Version" => Vt::String(self.firmware_version.clone()),
            "Serial Number" => Vt::String(self.serial_number.clone()),
            "ICCID" => Vt::String(self.iccid.clone()),
            "IMSI" => Vt::String(self.imsi.clone()),
            "Meter Serial Number" => Vt::String(self.meter_serial_number.clone()),
            "Meter Type" => Vt::String(self.meter_type.clone()),
            "Reserved RFID" => Vt::String(self.reserved_rfid.clone().unwrap_or_default()),
            "Reservation ID" => match self.reservation_id {
                Some(id) => Vt::Int(id as i128),
                None => Vt::Nil,
            },
            _ => return None,
        })
    }

    /// Write a CS-level field by name, for the bare `C_OCPP:Set(name, value)` Lua binding.
    pub fn cs_set_field(&mut self, name: &str, value: ferrowl_lua::module::ValueType) -> bool {
        use ferrowl_lua::module::ValueType as Vt;
        match (name, value) {
            ("Model", Vt::String(s)) => self.model = s,
            ("Vendor", Vt::String(s)) => self.vendor = s,
            ("Firmware Version", Vt::String(s)) => self.firmware_version = s,
            ("Serial Number", Vt::String(s)) => self.serial_number = s,
            ("ICCID", Vt::String(s)) => self.iccid = s,
            ("IMSI", Vt::String(s)) => self.imsi = s,
            ("Meter Serial Number", Vt::String(s)) => self.meter_serial_number = s,
            ("Meter Type", Vt::String(s)) => self.meter_type = s,
            _ => return false,
        }
        true
    }
}

impl ConnectorState {
    /// Rows for a connector's state table (metering + connector status/transaction).
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
                    .map_or_else(|| "—".to_string(), |l| format!("{l:.1}")),
            ),
            nv(
                "Default Charge Limit",
                &self.default_limit_unit,
                self.default_limit
                    .map_or_else(|| "—".to_string(), |l| format!("{l:.1}")),
            ),
            nv(
                "Max Charge Limit",
                &self.max_limit_unit,
                self.max_limit
                    .map_or_else(|| "—".to_string(), |l| format!("{l:.1}")),
            ),
            nv(
                "Reserved RFID",
                "",
                self.reserved_rfid
                    .clone()
                    .unwrap_or_else(|| "—".to_string()),
            ),
            nv(
                "Reservation ID",
                "",
                self.reservation_id
                    .map_or_else(|| "—".to_string(), |id| id.to_string()),
            ),
        ]
    }

    /// Read a connector field by name, for `C_OCPP:Connector(id):Get(name)`.
    pub fn get_field(&self, name: &str) -> Option<ferrowl_lua::module::ValueType> {
        use ferrowl_lua::module::ValueType as Vt;
        Some(match name {
            "Connector ID" => Vt::Int(self.connector_id as i128),
            "Used Phases" => Vt::String(self.phases.clone()),
            "Voltage" => Vt::Float(self.voltage),
            "Current L1" => Vt::Float(self.current[0]),
            "Current L2" => Vt::Float(self.current[1]),
            "Current L3" => Vt::Float(self.current[2]),
            "Power" => Vt::Float(self.power),
            "Frequency" => Vt::Float(self.frequency),
            "Total Energy" => Vt::Float(self.total_energy),
            "Session Energy" => Vt::Float(self.session_energy),
            "State of Charge" => Vt::Float(self.soc),
            "Temperature" => Vt::Float(self.temperature),
            "Status" => Vt::String(self.status.clone()),
            "RFID" => Vt::String(self.rfid.clone()),
            "Charge Limit" => match self.limit {
                Some(l) => Vt::Float(l),
                None => Vt::Nil,
            },
            "Default Charge Limit" => match self.default_limit {
                Some(l) => Vt::Float(l),
                None => Vt::Nil,
            },
            "Max Charge Limit" => match self.max_limit {
                Some(l) => Vt::Float(l),
                None => Vt::Nil,
            },
            "Reserved RFID" => Vt::String(self.reserved_rfid.clone().unwrap_or_default()),
            "Reservation ID" => match self.reservation_id {
                Some(id) => Vt::Int(id as i128),
                None => Vt::Nil,
            },
            _ => return None,
        })
    }

    /// Write a connector field by name, for `C_OCPP:Connector(id):Set(name, value)`. Numeric
    /// fields accept an int or float; string fields accept a string.
    pub fn set_field(&mut self, name: &str, value: ferrowl_lua::module::ValueType) -> bool {
        use ferrowl_lua::module::ValueType as Vt;
        let num = |v: &Vt| match v {
            Vt::Int(i) => Some(*i as f64),
            Vt::Float(f) => Some(*f),
            _ => None,
        };
        match (name, &value) {
            ("Connector ID", _) => match num(&value) {
                Some(n) => self.connector_id = n as i64,
                None => return false,
            },
            ("Voltage", _) => match num(&value) {
                Some(n) => self.voltage = n,
                None => return false,
            },
            ("Current L1", _) => match num(&value) {
                Some(n) => self.current[0] = n,
                None => return false,
            },
            ("Current L2", _) => match num(&value) {
                Some(n) => self.current[1] = n,
                None => return false,
            },
            ("Current L3", _) => match num(&value) {
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
            ("State of Charge", _) => match num(&value) {
                Some(n) => self.soc = n,
                None => return false,
            },
            ("Temperature", _) => match num(&value) {
                Some(n) => self.temperature = n,
                None => return false,
            },
            ("Total Energy", _) => match num(&value) {
                Some(n) => self.total_energy = n,
                None => return false,
            },
            ("Session Energy", _) => match num(&value) {
                Some(n) => self.session_energy = n,
                None => return false,
            },
            ("Used Phases", Vt::String(s)) => self.phases.clone_from(s),
            ("Status", Vt::String(s)) => self.status.clone_from(s),
            ("RFID", Vt::String(s)) => self.rfid.clone_from(s),
            ("Charge Limit", _) => match num(&value) {
                Some(n) => self.limit = Some(n),
                None => return false,
            },
            ("Default Charge Limit", _) => match num(&value) {
                Some(n) => self.default_limit = Some(n),
                None => return false,
            },
            ("Max Charge Limit", _) => match num(&value) {
                Some(n) => self.max_limit = Some(n),
                None => return false,
            },
            _ => return false,
        }
        true
    }

    /// Energy meter reading in Wh (StartTransaction/StopTransaction units).
    pub fn meter_wh(&self) -> i64 {
        (self.total_energy * 1000.0) as i64
    }

    /// An OCPP `meterValue` array reflecting this connector's state, for MeterValues.
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
        // OCPP 1.6 has no UnitOfMeasure for frequency (Hertz is implied), so the unit is omitted.
        sampled.push(serde_json::json!({
            "value": format!("{:.1}", self.frequency),
            "measurand": "Frequency",
        }));
        sampled.push(serde_json::json!({
            "value": format!("{:.1}", self.temperature),
            "measurand": "Temperature",
            "unit": "Celsius",
        }));
        sampled.push(serde_json::json!({
            "value": format!("{:.1}", self.soc),
            "measurand": "SoC",
            "unit": "Percent",
        }));
        serde_json::json!([{ "timestamp": rfc3339_now(), "sampledValue": sampled }])
    }
}

#[cfg(test)]
mod tests {
    use super::{ConnectorState, CsState};
    use ferrowl_lua::module::ValueType as Vt;

    #[test]
    /// OC-R-061 — the CS builds a MeterValues request from a connector's live transaction state that decodes as valid.
    fn ut_meter_values_payload_decodes() {
        use ferrowl_ocpp::{V1_6, Version};
        // The full measurand/unit set (incl. Frequency/Temperature/SoC) must decode as a typed
        // OCPP 1.6 MeterValues request — guards against invalid UnitOfMeasure/Measurand variants.
        let c = ConnectorState::new(1);
        let payload = serde_json::json!({
            "connectorId": c.connector_id,
            "meterValue": c.meter_value_json(),
        });
        assert!(V1_6::decode_call("MeterValues", payload).is_ok());
    }

    #[test]
    /// OC-R-058 — each connector carries its own metering/status/transaction/limit/tag/reservation fields.
    fn ut_connector_get_set_field_roundtrip() {
        let mut c = ConnectorState::new(1);
        // Numeric set accepts int or float; string fields accept strings.
        assert!(c.set_field("Power", Vt::Float(11000.0)));
        assert!(c.set_field("Current L2", Vt::Int(16)));
        assert!(c.set_field("Status", Vt::String("Charging".into())));
        assert!(matches!(c.get_field("Power"), Some(Vt::Float(v)) if v == 11000.0));
        assert!(matches!(c.get_field("Current L1"), Some(Vt::Float(_))));
        assert!(matches!(c.get_field("Current L2"), Some(Vt::Float(v)) if v == 16.0));
        assert!(matches!(c.get_field("Status"), Some(Vt::String(ref v)) if v == "Charging"));
        assert!(!c.set_field("Nope", Vt::Int(1)));
        assert!(!c.set_field("Power", Vt::String("x".into())));
    }

    #[test]
    /// OC-R-057 — the CS maintains charge-point-wide state plus a list of connector states.
    fn ut_cs_level_field_roundtrip_and_connectors() {
        let mut s = CsState::default();
        assert!(s.cs_set_field("Model", Vt::String("M".into())));
        assert!(matches!(s.cs_get_field("Model"), Some(Vt::String(ref v)) if v == "M"));
        // OC-R-104 — the four 1.6-only meter/modem identity fields roundtrip the same way.
        assert!(s.cs_set_field("ICCID", Vt::String("8912".into())));
        assert!(s.cs_set_field("IMSI", Vt::String("2901".into())));
        assert!(s.cs_set_field("Meter Serial Number", Vt::String("MTR-1".into())));
        assert!(s.cs_set_field("Meter Type", Vt::String("MT-X".into())));
        assert!(matches!(s.cs_get_field("ICCID"), Some(Vt::String(ref v)) if v == "8912"));
        assert!(matches!(s.cs_get_field("IMSI"), Some(Vt::String(ref v)) if v == "2901"));
        assert!(
            matches!(s.cs_get_field("Meter Serial Number"), Some(Vt::String(ref v)) if v == "MTR-1")
        );
        assert!(matches!(s.cs_get_field("Meter Type"), Some(Vt::String(ref v)) if v == "MT-X"));
        // Connector fields are not CS-level; they live on ConnectorState.
        assert!(s.cs_get_field("Power").is_none());
        // One default connector; adding is idempotent by id.
        assert_eq!(s.connectors.len(), 1);
        assert!(s.add_connector(2));
        assert!(!s.add_connector(2));
        assert_eq!(s.connectors.len(), 2);
        assert!(s.connector(2).is_some());
        assert!(s.connector(9).is_none());
    }

    #[test]
    /// SC-R-085 — every label the CS and connector state tables show resolves through get.
    fn ut_every_view_label_resolves_through_get() {
        let mut s = CsState::default();
        for row in s.cs_rows() {
            assert!(s.cs_get_field(&row.name).is_some(), "CS label {}", row.name);
        }
        let c = s.connector_mut(1).unwrap();
        c.limit = Some(1.0);
        for row in c.rows() {
            assert!(
                c.get_field(&row.name).is_some(),
                "connector label {}",
                row.name
            );
        }
    }

    #[test]
    /// SC-E-046 — the former compact field names are unknown: get is `None`, set is `false`.
    fn ut_former_field_names_are_unknown() {
        let mut s = CsState::default();
        for name in [
            "FirmwareVersion",
            "SessionEnergy",
            "Current",
            "Soc",
            "ConnectorId",
        ] {
            assert!(s.cs_get_field(name).is_none(), "cs get {name}");
            assert!(!s.cs_set_field(name, Vt::Int(1)), "cs set {name}");
            let c = s.connector_mut(1).unwrap();
            assert!(c.get_field(name).is_none(), "conn get {name}");
            assert!(!c.set_field(name, Vt::Int(1)), "conn set {name}");
        }
    }

    #[test]
    /// SC-E-047 — label matching is exact: case and surrounding whitespace matter.
    fn ut_view_label_match_is_case_sensitive() {
        let s = CsState::default();
        let c = s.connector(1).unwrap();
        assert!(c.get_field("Session Energy").is_some());
        for name in [
            "session energy",
            "Session energy",
            "SessionEnergy ",
            "Session Energy ",
        ] {
            assert!(c.get_field(name).is_none(), "{name}");
        }
    }
}
