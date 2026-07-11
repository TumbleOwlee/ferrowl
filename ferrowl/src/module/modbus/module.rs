//! The `ModbusModule` struct: one running endpoint with its registers, shared memory, log, and
//! optional Lua simulation — construction, start/stop lifecycle, and runtime accessors.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use ferrowl_codec::{Address, Kind, Register};
use ferrowl_modbus::{Key, Operation, SlaveKey};
use ferrowl_store::{CellKind as MemKind, Memory, Range};
use parking_lot::RwLock as MemLock;
use tokio::sync::RwLock;

use crate::app::{Level, LogRing};
use crate::config::{
    DeviceConfig, Endpoint, ModuleSpec, Role,
    device::{
        DEFAULT_DELAY_MS, DEFAULT_INTERVAL_MS, DEFAULT_RECONNECT, DEFAULT_TIMEOUT_MS, NamedValue,
        ReadRanges,
    },
};
use crate::instance::Instance;
use crate::instance::error::Error;
use crate::lua::{SimHandle, run_sim};

use super::build::{
    Timing, build_instance, build_read_operations, default_value, endpoint_to_config,
    explicit_read_coverage,
};
use super::log::{FileSink, append, open_sink};

pub type ModuleMemory = Arc<MemLock<Memory<Key<SlaveKey>>>>;
pub type ModuleLog = Arc<RwLock<LogRing>>;
/// Shared store of virtual-register values (no Modbus address), keyed by register name. Shared
/// with the Lua sim thread so scripts can drive virtual registers and the table shows them.
pub type VirtualStore = Arc<RwLock<HashMap<String, ferrowl_codec::Value>>>;

/// Classifies a network/status line from the client/server instance for the log ring: outright
/// disconnects and transport errors are `Error`, degraded-but-recovering states (lost connection,
/// backoff, retried exceptions) are `Warning`, everything else (request intent, success) is `Info`.
fn network_log_level(s: &str) -> Level {
    let lower = s.to_lowercase();
    if lower.contains("disconnecting")
        || lower.contains("reconnect disabled")
        || lower.contains("timed out")
    {
        Level::Error
    } else if lower.contains("disconnected")
        || lower.contains("reconnecting")
        || lower.contains("invalid")
        || lower.contains("dropped")
        || lower.contains("failed")
    {
        Level::Warning
    } else {
        Level::Info
    }
}

/// One module instance: a modbus client (reads an external server) or server (simulates a
/// device), plus its register set, shared memory and ring log.
pub struct ModbusModule {
    name: String,
    instance: Instance<SlaveKey>,
    registers: Vec<(String, String, Register, Vec<NamedValue>)>,
    /// Shared operations list — owned here so it can be updated in-place without rebuilding the
    /// network instance (the instance holds a clone of the same Arc).
    operations: Arc<RwLock<Vec<Operation>>>,
    memory: ModuleMemory,
    log: ModuleLog,
    file_sink: FileSink,
    /// Enabled global Lua scripts (name → code), run on the sim thread.
    scripts: Vec<(String, String)>,
    /// Explicit per-function-code read ranges from the device config (empty = auto-merge).
    read_ranges: ReadRanges,
    /// Script sim cycle period, from the device config's `script_interval` — separate from
    /// `interval_ms`, which keeps driving device-polling cadence untouched. Controls the Lua
    /// sim loop (`run_sim`).
    script_interval: Duration,
    /// The running simulation thread, if any. Runs iff at least one script is enabled
    /// (see `ensure_sim`), independent of the network instance's start/stop state.
    sim: Option<SimHandle>,
    /// Shared values for virtual registers (no Modbus address), keyed by register name.
    virtual_values: VirtualStore,
}

impl ModbusModule {
    /// Build a module from an instance spec and its device-type config.
    pub fn new(spec: &ModuleSpec, device: &DeviceConfig) -> Self {
        let mut memory = Memory::<Key<SlaveKey>>::default();
        let mut registers: Vec<(String, String, Register, Vec<NamedValue>)> = Vec::new();
        let scripts = super::registers::collect_scripts(device);
        let mut virtual_init: HashMap<String, ferrowl_codec::Value> = HashMap::new();

        for (name, def) in &device.definitions {
            let register = def.register();
            registers.push((
                name.clone(),
                def.description.clone(),
                register.clone(),
                def.values.clone(),
            ));
            if let Address::Virtual = register.address() {
                let init = def
                    .default
                    .as_ref()
                    .map(|s| s.to_value(def.resolution))
                    .unwrap_or_else(|| default_value(&register));
                virtual_init.insert(name.clone(), init);
            }
            if let Some(range) = def.mem_range() {
                let key = Key {
                    id: SlaveKey {
                        slave_id: def.slave_id,
                        kind: def.register().kind().clone(),
                    },
                };
                let mem_kind = match def.kind() {
                    Kind::Coil | Kind::HoldingRegister => MemKind::ReadWrite(def.mem_type()),
                    Kind::DiscreteInput | Kind::InputRegister => MemKind::Read(def.mem_type()),
                };
                memory.add_ranges(key, &mem_kind, std::slice::from_ref(&range));
                if let Some(default) = &def.default
                    && let Ok(raw) = register.encode(&default.to_string())
                {
                    let write_key = Key {
                        id: SlaveKey {
                            slave_id: def.slave_id,
                            kind: def.register().kind().clone(),
                        },
                    };
                    memory.write_unchecked(write_key, &Range::new(range.start(), raw.len()), &raw);
                }
            }
        }
        // Cover gaps inside explicit read ranges (Read cells) so a batched client read can store
        // the whole request; the gap words are read but otherwise unused.
        for (key, mem_kind, range) in explicit_read_coverage(&registers, &device.read_ranges) {
            memory.add_ranges(key, &mem_kind, std::slice::from_ref(&range));
        }
        let operations = build_read_operations(&registers, &device.read_ranges);

        let memory: ModuleMemory = Arc::new(MemLock::new(memory));
        let operations = Arc::new(RwLock::new(operations));
        let log: ModuleLog = Arc::new(RwLock::new(LogRing::init()));

        let file_sink: FileSink = Arc::new(std::sync::Mutex::new(None));
        let _ = open_sink(&file_sink, device.log_file.as_deref(), &spec.name);

        let timing = Self::resolve_timing(device);
        let net_config = endpoint_to_config(&spec.endpoint, &timing);
        let instance = build_instance(spec.role, net_config, operations.clone(), memory.clone());

        let mut module = Self {
            name: spec.name.clone(),
            instance,
            registers,
            operations,
            memory,
            log,
            file_sink,
            scripts,
            read_ranges: device.read_ranges.clone(),
            script_interval: device.script_interval_duration(),
            sim: None,
            virtual_values: Arc::new(RwLock::new(virtual_init)),
        };
        module.ensure_sim();
        module
    }

    /// Resolve effective timing for an instance from the device config, falling back to the
    /// built-in defaults. Timing is no longer a per-instance (session) override.
    pub fn resolve_timing(device: &DeviceConfig) -> Timing {
        Timing {
            timeout_ms: device.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS),
            delay_ms: device.delay_ms.unwrap_or(DEFAULT_DELAY_MS),
            interval_ms: device.interval_ms.unwrap_or(DEFAULT_INTERVAL_MS),
            reconnect: device.reconnect.unwrap_or(DEFAULT_RECONNECT),
        }
    }

    /// (Re)point this module's log file at `base` (None disables file logging). The filename is
    /// `<stem>.<tab-name>.<ext>` next to `base`. Takes effect on already-running modules.
    /// Returns an error if the file can't be opened.
    pub fn set_log_base(&self, base: Option<&str>) -> Result<(), std::io::Error> {
        open_sink(&self.file_sink, base, &self.name)
    }

    pub fn memory(&self) -> ModuleMemory {
        self.memory.clone()
    }

    pub fn log(&self) -> ModuleLog {
        self.log.clone()
    }

    pub fn registers(&self) -> &[(String, String, Register, Vec<NamedValue>)] {
        &self.registers
    }

    /// Store a value for a virtual register (replaces any previous value).
    pub async fn set_virtual_value(&self, name: &str, val: ferrowl_codec::Value) {
        self.virtual_values
            .write()
            .await
            .insert(name.to_string(), val);
    }

    /// Shared handle to the virtual-register store (snapshot it for display, or share with the sim).
    pub fn virtual_store(&self) -> VirtualStore {
        self.virtual_values.clone()
    }

    /// Append a brand-new register to the module's cached register list.
    pub fn add_register(
        &mut self,
        name: String,
        description: String,
        register: Register,
        named_values: Vec<NamedValue>,
    ) {
        self.registers
            .push((name, description, register, named_values));
    }

    /// Remove a register from the module's cached register list by name (no-op if absent).
    pub fn remove_register_by_name(&mut self, name: &str) {
        self.registers.retain(|(n, _, _, _)| n != name);
    }

    /// Replace one register's cached metadata (name, description, register, named values).
    pub fn update_register(
        &mut self,
        idx: usize,
        name: String,
        description: String,
        register: Register,
        named_values: Vec<NamedValue>,
    ) {
        if let Some(slot) = self.registers.get_mut(idx) {
            *slot = (name, description, register, named_values);
        }
    }

    /// Rebuild the shared operations list from the current register cache. The network instance
    /// sees the change immediately because it holds a clone of the same Arc.
    pub async fn rebuild_operations(&self) {
        *self.operations.write().await = build_read_operations(&self.registers, &self.read_ranges);
    }

    /// Start the underlying client/server, routing its log + status into the ring log and (if
    /// configured) the per-module log file. The Lua simulation thread is independent of network
    /// start/stop — it runs whenever there are enabled scripts, regardless.
    pub async fn start(&mut self) -> Result<(), Error> {
        let log = self.log.clone();
        let log_sink = self.file_sink.clone();
        let status = self.log.clone();
        let status_sink = self.file_sink.clone();
        self.instance
            .start(
                move |s: String| {
                    let log = log.clone();
                    let log_sink = log_sink.clone();
                    async move {
                        log.write().await.write(network_log_level(&s), &s);
                        append(&log_sink, &s);
                    }
                },
                move |s: String| {
                    let status = status.clone();
                    let status_sink = status_sink.clone();
                    async move {
                        let line = format!("[status] {s}");
                        status.write().await.write(network_log_level(&line), &line);
                        append(&status_sink, &line);
                    }
                },
            )
            .await
    }

    pub async fn stop(&mut self) -> Result<(), Error> {
        self.instance.stop().await
    }

    /// (Re)start the simulation thread from a fresh register snapshot if there is at least one
    /// enabled script; stop it otherwise. Any previously running thread is stopped first, so this
    /// is safe to call whenever the enabled-script set may have changed (construction, script
    /// edits) — it is the single source of truth for whether the sim runs.
    fn ensure_sim(&mut self) {
        self.stop_sim();
        let registers: HashMap<String, Register> = self
            .registers
            .iter()
            .map(|(name, _, register, _)| (name.clone(), register.clone()))
            .collect();
        self.sim = run_sim(
            self.memory.clone(),
            self.virtual_values.clone(),
            registers,
            self.scripts.clone(),
            self.script_interval,
            self.log.clone(),
            self.file_sink.clone(),
        );
    }

    /// Stop and join the simulation thread if one is running.
    fn stop_sim(&mut self) {
        if let Some(mut sim) = self.sim.take() {
            sim.stop();
        }
    }

    /// Whether the Lua simulation thread is currently running. Test-only: no production caller
    /// remains once the manual `:lua start|stop|status` command was removed (Stage 1b).
    #[cfg(test)]
    pub(crate) fn lua_running(&self) -> bool {
        self.sim.is_some()
    }

    /// Replace the module's script list and restart the simulation thread so the new scripts take
    /// effect (fresh Lua state — stopped if none remain enabled).
    pub fn reload_scripts(&mut self, scripts: Vec<(String, String)>) {
        self.scripts = scripts;
        self.ensure_sim();
    }

    /// Send a write command to the underlying client (errors for servers / when stopped).
    pub async fn send_command(&self, command: ferrowl_modbus::Command) -> Result<(), Error> {
        self.instance.send_command(command).await
    }

    /// Rebuild the underlying instance for a new endpoint/role (e.g. switching client↔server),
    /// reusing the existing memory + registers. Stops the current instance first; the caller is
    /// expected to `start()` afterwards. This keeps the instance in sync with the spec so writes
    /// dispatch correctly. The simulation thread is left running (it's decoupled from the network
    /// instance) but is restarted at the end so a changed sim interval takes effect.
    pub async fn reconfigure(
        &mut self,
        endpoint: &Endpoint,
        role: Role,
        timing: Timing,
        read_ranges: ReadRanges,
    ) -> Result<(), Error> {
        // Best-effort stop of any running instance; the caller is expected to `start()` afterwards.
        let _ = self.instance.stop().await;

        // Adopt new explicit read ranges: cover their gaps in memory, then rebuild operations.
        self.read_ranges = read_ranges;
        for (key, mem_kind, range) in explicit_read_coverage(&self.registers, &self.read_ranges) {
            self.memory
                .write()
                .add_ranges(key, &mem_kind, std::slice::from_ref(&range));
        }
        self.rebuild_operations().await;
        let net_config = endpoint_to_config(endpoint, &timing);
        self.instance = build_instance(
            role,
            net_config,
            self.operations.clone(),
            self.memory.clone(),
        );
        self.ensure_sim();
        Ok(())
    }

    pub fn is_instance_active(&self) -> bool {
        self.instance.active()
    }
}

#[cfg(test)]
mod tests {
    use ferrowl_codec::Kind;

    #[test]
    fn ut_resolve_timing_fallback() {
        use super::ModbusModule;
        use crate::config::DeviceConfig;
        use crate::config::device::{
            DEFAULT_DELAY_MS, DEFAULT_INTERVAL_MS, DEFAULT_RECONNECT, DEFAULT_TIMEOUT_MS,
        };

        let mut device = DeviceConfig::default();

        // No device values: built-in defaults.
        let timing = ModbusModule::resolve_timing(&device);
        assert_eq!(timing.timeout_ms, DEFAULT_TIMEOUT_MS);
        assert_eq!(timing.delay_ms, DEFAULT_DELAY_MS);
        assert_eq!(timing.interval_ms, DEFAULT_INTERVAL_MS);
        assert_eq!(timing.reconnect, DEFAULT_RECONNECT);

        // Device values beat the defaults.
        device.timeout_ms = Some(2000);
        device.delay_ms = Some(500);
        device.reconnect = Some(false);
        let timing = ModbusModule::resolve_timing(&device);
        assert_eq!(timing.timeout_ms, 2000);
        assert_eq!(timing.delay_ms, 500);
        assert!(!timing.reconnect);
        assert_eq!(timing.interval_ms, DEFAULT_INTERVAL_MS);
    }

    fn device_with_defs() -> crate::config::DeviceConfig {
        use crate::config::DeviceConfig;
        use crate::config::device::{
            AccessCfg, AlignmentCfg, EndianCfg, NamedValue, ReadRanges, RegisterDef, Scalar,
            ValueType,
        };
        use std::collections::BTreeMap;

        let base = |address: Option<u16>, is_virtual: bool, update, default| RegisterDef {
            slave_id: 1,
            kind: Kind::HoldingRegister,
            address,
            is_virtual,
            access: AccessCfg::ReadWrite,
            value_type: ValueType::U16,
            endian: EndianCfg::Big,
            resolution: 1.0,
            bitmask: None,
            length: 1,
            alignment: AlignmentCfg::Left,
            values: vec![NamedValue {
                name: "a".into(),
                value: Scalar::Int(1),
            }],
            update,
            description: "desc".into(),
            default,
        };

        let mut definitions = BTreeMap::new();
        // Fixed register with a default value (exercises encode + write_unchecked) and a script.
        definitions.insert(
            "hold".into(),
            base(Some(0), false, Some("x = 1".into()), Some(Scalar::Int(7))),
        );
        // Virtual register without a default (exercises default_value).
        definitions.insert("virt".into(), base(None, true, None, None));

        DeviceConfig {
            version: None,
            timeout_ms: Some(1000),
            delay_ms: None,
            interval_ms: Some(500),
            reconnect: None,
            log_file: Some(
                std::env::temp_dir()
                    .join("ferrowl_module_test.log")
                    .to_string_lossy()
                    .into_owned(),
            ),
            read_ranges: ReadRanges {
                holding: Some("0-10".into()),
                ..Default::default()
            },
            definitions,
            scripts: Vec::new(),
            script_interval: 1.0,
        }
    }

    #[test]
    fn ut_module_new_tcp_server_and_sync_accessors() {
        use super::ModbusModule;
        use crate::config::{Endpoint, ModuleSpec, Role};

        let device = device_with_defs();
        let spec = ModuleSpec {
            name: "evse 1".into(),
            device: String::new(),
            role: Role::Server,
            endpoint: Endpoint::Tcp {
                ip: "127.0.0.1".into(),
                port: 5020,
            },
        };

        let mut module = ModbusModule::new(&spec, &device);
        assert_eq!(module.registers().len(), 2);
        let _ = module.memory();
        let _ = module.log();
        let _ = module.virtual_store();
        assert!(!module.lua_running());
        assert!(!module.is_instance_active());

        // Register-cache mutation helpers.
        let reg = module.registers()[0].2.clone();
        module.add_register("new".into(), "d".into(), reg.clone(), vec![]);
        assert_eq!(module.registers().len(), 3);
        module.update_register(0, "renamed".into(), "d".into(), reg.clone(), vec![]);
        module.update_register(99, "oob".into(), "d".into(), reg, vec![]); // out-of-bounds no-op
        module.remove_register_by_name("new");
        assert_eq!(module.registers().len(), 2);

        // Log-base reconfiguration: clear, then attempt a path that fails.
        assert!(module.set_log_base(None).is_ok());
        assert!(module.set_log_base(Some("/no/such/ferrowl/dir/base.log")).is_err());
    }

    #[test]
    fn ut_module_new_rtu_client() {
        use super::ModbusModule;
        use crate::config::{Endpoint, ModuleSpec, Role};

        let device = device_with_defs();
        let spec = ModuleSpec {
            name: "meter".into(),
            device: String::new(),
            role: Role::Client,
            endpoint: Endpoint::Rtu {
                path: "/dev/ttyUSB0".into(),
                baud_rate: 9600,
                parity: Some("none".into()),
                data_bits: Some(8),
                stop_bits: Some(1),
            },
        };

        let module = ModbusModule::new(&spec, &device);
        assert_eq!(module.registers().len(), 2);
        assert!(!module.is_instance_active());
    }

    // --- Sim lifecycle: decoupled from network start/stop, driven only by enabled scripts. ---

    /// One fixed U16 "marker" register (address 0) plus `scripts` (global Lua scripts, as loaded
    /// from a device config's `scripts` list).
    fn device_with_script(
        scripts: Vec<crate::config::script::ScriptDef>,
    ) -> crate::config::DeviceConfig {
        use crate::config::DeviceConfig;
        use crate::config::device::{
            AccessCfg, AlignmentCfg, EndianCfg, NamedValue, ReadRanges, RegisterDef, Scalar,
            ValueType,
        };
        use std::collections::BTreeMap;

        let mut definitions = BTreeMap::new();
        definitions.insert(
            "marker".to_string(),
            RegisterDef {
                slave_id: 1,
                kind: Kind::HoldingRegister,
                address: Some(0),
                is_virtual: false,
                access: AccessCfg::ReadWrite,
                value_type: ValueType::U16,
                endian: EndianCfg::Big,
                resolution: 1.0,
                bitmask: None,
                length: 1,
                alignment: AlignmentCfg::Left,
                values: vec![NamedValue {
                    name: "a".into(),
                    value: Scalar::Int(1),
                }],
                update: None,
                description: "desc".into(),
                default: Some(Scalar::Int(0)),
            },
        );

        DeviceConfig {
            version: None,
            timeout_ms: Some(1000),
            delay_ms: None,
            interval_ms: Some(50),
            reconnect: None,
            log_file: None,
            read_ranges: ReadRanges {
                holding: Some("0-10".into()),
                ..Default::default()
            },
            definitions,
            scripts,
            // Fast sim cycle so tests don't have to wait long for a tick.
            script_interval: 0.05,
        }
    }

    fn script(code: &str, enabled: bool) -> crate::config::script::ScriptDef {
        crate::config::script::ScriptDef {
            name: "sim".to_string(),
            code: code.to_string(),
            enabled,
        }
    }

    fn test_spec(name: &str, port: u16) -> crate::config::ModuleSpec {
        use crate::config::{Endpoint, ModuleSpec, Role};
        ModuleSpec {
            name: name.to_string(),
            device: String::new(),
            role: Role::Server,
            endpoint: Endpoint::Tcp {
                ip: "127.0.0.1".into(),
                port,
            },
        }
    }

    /// Read the "marker" register (holding, addr 0, U16) from `module`'s memory.
    fn read_marker(module: &super::ModbusModule) -> u16 {
        use ferrowl_modbus::{Key, SlaveKey};
        use ferrowl_store::{CellType, Range};

        let raw = module
            .memory()
            .read()
            .read(
                Key {
                    id: SlaveKey {
                        slave_id: 1,
                        kind: Kind::HoldingRegister,
                    },
                },
                &CellType::Register,
                &Range::new(0, 1),
            )
            .unwrap_or_default();
        raw.first().copied().unwrap_or(0)
    }

    /// Poll `read_marker` for up to ~2s (well beyond the test device's 50ms sim interval) until it
    /// equals `want`, to bound the wait for the sim thread's next cycle.
    fn wait_for_marker(module: &super::ModbusModule, want: u16) -> bool {
        for _ in 0..200 {
            if read_marker(module) == want {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn ut_sim_starts_at_construction_without_network_start() {
        use super::ModbusModule;

        let device = device_with_script(vec![script(r#"C_Register:Set("marker", 7)"#, true)]);
        let module = ModbusModule::new(&test_spec("sim1", 15201), &device);

        // No `start()` call anywhere — the sim runs solely because a script is enabled.
        assert!(module.lua_running());
        assert!(wait_for_marker(&module, 7));
    }

    #[test]
    fn ut_sim_not_started_when_no_enabled_scripts() {
        use super::ModbusModule;

        let device = device_with_script(vec![]);
        let module = ModbusModule::new(&test_spec("sim2", 15202), &device);
        assert!(!module.lua_running());

        let device = device_with_script(vec![script(r#"C_Register:Set("marker", 7)"#, false)]);
        let module = ModbusModule::new(&test_spec("sim2b", 15203), &device);
        assert!(!module.lua_running());
    }

    #[test]
    fn ut_reload_scripts_all_disabled_stops_sim() {
        use super::ModbusModule;

        let device = device_with_script(vec![script(r#"C_Register:Set("marker", 7)"#, true)]);
        let mut module = ModbusModule::new(&test_spec("sim3", 15204), &device);
        assert!(module.lua_running());

        module.reload_scripts(vec![]); // no enabled scripts left
        assert!(!module.lua_running());
    }

    #[test]
    fn ut_reload_scripts_toggle_on_starts_sim() {
        use super::ModbusModule;

        let device = device_with_script(vec![script(r#"C_Register:Set("marker", 7)"#, false)]);
        let mut module = ModbusModule::new(&test_spec("sim4", 15205), &device);
        assert!(!module.lua_running());

        module.reload_scripts(vec![(
            "sim".to_string(),
            r#"C_Register:Set("marker", 7)"#.to_string(),
        )]);
        assert!(module.lua_running());
        assert!(wait_for_marker(&module, 7));
    }

    #[test]
    fn ut_reload_scripts_changed_code_takes_effect() {
        use super::ModbusModule;

        let device = device_with_script(vec![script(r#"C_Register:Set("marker", 1)"#, true)]);
        let mut module = ModbusModule::new(&test_spec("sim5", 15206), &device);
        assert!(wait_for_marker(&module, 1));

        // Fresh Lua state on restart: new code takes over immediately (proves a restart, not the
        // old thread still running the old script).
        module.reload_scripts(vec![(
            "sim".to_string(),
            r#"C_Register:Set("marker", 2)"#.to_string(),
        )]);
        assert!(wait_for_marker(&module, 2));
    }

    #[tokio::test]
    async fn ut_network_stop_leaves_sim_running() {
        use super::ModbusModule;

        let device = device_with_script(vec![script(r#"C_Register:Set("marker", 7)"#, true)]);
        let mut module = ModbusModule::new(&test_spec("sim6", 15207), &device);
        assert!(module.lua_running());

        module.start().await.expect("start");
        assert!(module.lua_running());
        module.stop().await.expect("stop");
        assert!(module.lua_running());
    }
}
