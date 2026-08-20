//! `ModbusMonitorModule` (MB-R-076, MB-R-140–145): a monitor's construction/start/stop
//! lifecycle. Unlike [`super::super::module::ModbusModule`] there is no `Instance<T>`, no
//! operations list, no virtual store, and no Lua sim surface — a monitor owns exactly one log
//! and one observed-value table, receive-only.

use std::sync::Arc;

use tokio::sync::RwLock;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;

use crate::app::LogRing;
use crate::config::device::MonitorRegisterDef;
use crate::config::{Endpoint, ModuleSpec, MonitorDeviceConfig};
use ferrowl_modbus::LogFn;
use ferrowl_modbus::ServerCommand;
use ferrowl_modbus::monitor::SharedObservedTable;

use super::super::log::{FileSink, open_sink};
use super::build::{MonitorNetConfig, MonitorTransportError, endpoint_to_monitor_config};

#[allow(dead_code)] // consumed starting s5 (ModbusMonitorModuleView)
pub type ModuleLog = Arc<RwLock<LogRing>>;

/// Errors from a monitor's own lifecycle: either the role/transport compatibility check
/// (MB-R-140) or a network error surfaced from the running receive task.
#[derive(Debug, thiserror::Error)]
#[allow(dead_code)] // forward-declared; real construction/consumption lands in s5/s8
pub enum Error {
    #[error("Network error: {0}")]
    Net(#[from] ferrowl_modbus::Error),
    #[error(transparent)]
    Transport(#[from] MonitorTransportError),
}

/// One monitor instance: a receive-only observer of an `Rtu`/`Ascii` bus, its register
/// interpretations, observed-value table, and log — no operations list, no memory store, no
/// Lua sim (MB-R-076).
// Forward-declared: real app-side construction (the 3 call sites, session role dispatch) lands
// in s8 of the modbus-bus-monitor plan; the view lands in s5. `#[allow(dead_code)]`, not a
// stub — already fully implemented and tested here.
#[allow(dead_code)]
pub struct ModbusMonitorModule {
    name: String,
    endpoint: Endpoint,
    reconnect: bool,
    interpretations: Vec<(String, MonitorRegisterDef)>,
    table: SharedObservedTable,
    log: ModuleLog,
    file_sink: FileSink,
    command_tx: Option<Sender<ServerCommand>>,
    task: Option<JoinHandle<Result<(), ferrowl_modbus::Error>>>,
}

#[allow(dead_code)] // forward-declared; see ModbusMonitorModule's note
impl ModbusMonitorModule {
    /// Build a monitor module from an instance spec and its device-type config. The
    /// endpoint/transport is validated later, at [`start`](Self::start) — construction never
    /// fails.
    pub fn new(spec: &ModuleSpec, device: &MonitorDeviceConfig) -> Self {
        let interpretations: Vec<(String, MonitorRegisterDef)> = device
            .definitions
            .iter()
            .map(|(name, def)| (name.clone(), def.clone()))
            .collect();

        let file_sink: FileSink = Arc::new(std::sync::Mutex::new(None));
        let _ = open_sink(&file_sink, device.log_file.as_deref(), &spec.name);

        Self {
            name: spec.name.clone(),
            endpoint: spec.endpoint.clone(),
            reconnect: device
                .reconnect
                .unwrap_or(crate::config::device::DEFAULT_RECONNECT),
            interpretations,
            table: Arc::new(parking_lot::RwLock::new(
                ferrowl_modbus::monitor::ObservedTable::default(),
            )),
            log: Arc::new(RwLock::new(LogRing::init())),
            file_sink,
            command_tx: None,
            task: None,
        }
    }

    pub fn table(&self) -> SharedObservedTable {
        self.table.clone()
    }

    pub fn log(&self) -> ModuleLog {
        self.log.clone()
    }

    pub fn interpretations(&self) -> &[(String, MonitorRegisterDef)] {
        &self.interpretations
    }

    /// Append a brand-new interpretation to the module's cached list.
    pub fn add_interpretation(&mut self, name: String, def: MonitorRegisterDef) {
        self.interpretations.push((name, def));
    }

    /// Remove an interpretation from the module's cached list by name (no-op if absent).
    pub fn remove_interpretation_by_name(&mut self, name: &str) {
        self.interpretations.retain(|(n, _)| n != name);
    }

    /// Start the monitor: validate the endpoint/transport (MB-R-140), spawn the receive-only
    /// task, and route its log/status lines into the ring log and (if configured) the
    /// per-module log file.
    pub async fn start<L, S>(&mut self, log: L, status: S) -> Result<(), Error>
    where
        L: LogFn + Clone,
        S: LogFn + Clone,
    {
        let net_config = endpoint_to_monitor_config(&self.endpoint, self.reconnect)?;

        let (tx, rx) = tokio::sync::mpsc::channel::<ServerCommand>(10);

        let log_ring = self.log.clone();
        let log_sink = self.file_sink.clone();
        let log_cb = move |s: String| {
            let log_ring = log_ring.clone();
            let log_sink = log_sink.clone();
            async move {
                log_ring.write().await.write(network_log_level(&s), &s);
                super::super::log::append(&log_sink, &s);
            }
        };

        let status_ring = self.log.clone();
        let status_sink = self.file_sink.clone();
        let status_cb = move |s: String| {
            let status_ring = status_ring.clone();
            let status_sink = status_sink.clone();
            async move {
                let line = format!("[status] {s}");
                status_ring
                    .write()
                    .await
                    .write(network_log_level(&line), &line);
                super::super::log::append(&status_sink, &line);
            }
        };

        let table = self.table.clone();
        let handle = match net_config {
            MonitorNetConfig::Rtu(cfg) => {
                ferrowl_modbus::rtu::MonitorBuilder::new(Arc::new(RwLock::new(cfg)), table)
                    .spawn(rx, log_cb, status_cb)
                    .await?
            }
            MonitorNetConfig::Ascii(cfg) => {
                ferrowl_modbus::ascii::MonitorBuilder::new(Arc::new(RwLock::new(cfg)), table)
                    .spawn(rx, log_cb, status_cb)
                    .await?
            }
        };

        let _ = log.invoke(format!("Monitor '{}' started", self.name)).await;
        let _ = status;
        self.command_tx = Some(tx);
        self.task = Some(handle);
        Ok(())
    }

    /// Stop the running task: send `Terminate` then, after a grace period, abort if it is still
    /// alive. Mirrors `Instance::stop`'s grace-period-then-abort shape.
    pub async fn stop(&mut self) -> Result<(), Error> {
        let sent_terminate = if let Some(tx) = &self.command_tx {
            tx.send(ServerCommand::Terminate).await.is_ok()
        } else {
            false
        };
        if sent_terminate {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
        self.command_tx = None;

        if let Some(handle) = self.task.take() {
            if handle.is_finished() {
                let _ = handle.await;
            } else {
                handle.abort();
                let _ = handle.await;
            }
        }
        Ok(())
    }
}

/// Classifies a monitor's network/status line for the log ring: reuses
/// [`super::super::module::network_log_level`]'s classification plus a monitor-specific Warning
/// branch for a discarded, malformed frame (MB-R-142) — `s2`'s decode/match state machine
/// phrases its discarded-frame log line to contain this exact substring.
#[allow(dead_code)] // consumed starting s5 (ModbusMonitorModuleView routes lines through it)
pub(crate) fn network_log_level(s: &str) -> crate::app::Level {
    if s.to_lowercase().contains("malformed frame") {
        crate::app::Level::Warning
    } else {
        super::super::module::network_log_level(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Role;

    fn spec(endpoint: Endpoint) -> ModuleSpec {
        ModuleSpec {
            name: "mon1".to_string(),
            device: "monitor.toml".to_string(),
            role: Role::Monitor,
            endpoint,
        }
    }

    fn device_with_defs() -> MonitorDeviceConfig {
        let mut definitions = std::collections::BTreeMap::new();
        definitions.insert(
            "power".to_string(),
            MonitorRegisterDef {
                slave_id: 1,
                kind: ferrowl_codec::Kind::HoldingRegister,
                address: Some(10),
                is_virtual: false,
                value_type: crate::config::device::ValueType::U16,
                endian: Default::default(),
                word_order: Default::default(),
                resolution: 1.0,
                bitmask: None,
                length: 1,
                alignment: Default::default(),
                values: vec![],
                description: String::new(),
                default: None,
            },
        );
        MonitorDeviceConfig {
            version: None,
            reconnect: Some(true),
            log_file: None,
            definitions,
        }
    }

    fn bad_rtu_endpoint() -> Endpoint {
        Endpoint::Rtu {
            path: "/dev/does-not-exist-ferrowl-monitor-test".to_string(),
            baud_rate: 9600,
            parity: None,
            data_bits: None,
            stop_bits: None,
        }
    }

    /// MB-R-076 — a monitor module seeds its interpretations from the device config's
    /// definitions and starts with an empty observed-value table, no register set.
    #[test]
    fn ut_monitor_module_new_seeds_interpretations_and_empty_table() {
        let module = ModbusMonitorModule::new(&spec(bad_rtu_endpoint()), &device_with_defs());
        assert_eq!(module.interpretations().len(), 1);
        assert_eq!(module.interpretations()[0].0, "power");
        assert!(module.table().read().unit_ids().is_empty());
    }

    /// MB-R-140 — a monitor's start() rejects a non-serial endpoint with the role/transport
    /// compatibility error, the enforcement point nothing can bypass (a hand-edited session
    /// file skips the setup dialog's own check).
    #[tokio::test]
    async fn ut_monitor_module_start_with_tcp_endpoint_fails_with_transport_error() {
        let endpoint = Endpoint::Tcp {
            ip: "127.0.0.1".to_string(),
            port: 502,
        };
        let mut module = ModbusMonitorModule::new(&spec(endpoint), &device_with_defs());
        let err = module
            .start(|_: String| async {}, |_: String| async {})
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Transport(_)));
    }

    /// MB-R-141 — `reconnect: true` against a bad serial path keeps the task retrying (still
    /// running after a short wait); `reconnect: false` ends the task promptly.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ut_monitor_module_start_stop_lifecycle() {
        let mut device = device_with_defs();
        device.reconnect = Some(true);
        let mut module = ModbusMonitorModule::new(&spec(bad_rtu_endpoint()), &device);
        module
            .start(|_: String| async {}, |_: String| async {})
            .await
            .expect("start always succeeds for a valid transport");
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        assert!(
            module.task.as_ref().is_some_and(|h| !h.is_finished()),
            "an open failure with reconnect enabled must keep retrying"
        );
        module.stop().await.expect("stop");

        let mut device = device_with_defs();
        device.reconnect = Some(false);
        let mut module = ModbusMonitorModule::new(&spec(bad_rtu_endpoint()), &device);
        module
            .start(|_: String| async {}, |_: String| async {})
            .await
            .expect("start always succeeds for a valid transport");
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while module.task.as_ref().is_some_and(|h| !h.is_finished()) {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("task should end promptly, not retry, with reconnect disabled");
    }

    /// network_log_level's monitor-specific Warning branch: a discarded malformed-frame line
    /// classifies as Warning, matching `s2`'s decode/match state machine's log wording.
    #[test]
    fn ut_network_log_level_classifies_malformed_frame_as_warning() {
        let line = "Discarding malformed frame (checksum mismatch): 01 02 03";
        assert_eq!(network_log_level(line), crate::app::Level::Warning);
    }
}
