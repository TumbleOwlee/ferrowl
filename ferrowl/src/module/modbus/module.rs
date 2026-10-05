//! The `ModbusModule` struct: one running endpoint with its registers, shared memory, log, and
//! optional Lua simulation — construction, start/stop lifecycle, and runtime accessors.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use ferrowl_codec::{Address, Kind, Register};
use ferrowl_modbus::{Key, Operation, SlaveKey, UnitId};
use ferrowl_store::{CellKind, Memory, Range};
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
use crate::lua::{SharedRegisters, SimHandle, run_script_once, run_sim};

use super::build::{
    Timing, build_instance, build_read_operations, declare_or_reject_msg, default_value,
    endpoint_serial_path, endpoint_to_config, explicit_read_coverage, gap_cell_subject,
};
use super::log::{FileSink, append, open_sink};
use super::serial_paths::SerialPathRegistry;

pub type ModuleMemory = Arc<MemLock<Memory<Key<SlaveKey>>>>;
pub type ModuleLog = Arc<RwLock<LogRing>>;
/// Shared store of virtual-register values (no Modbus address), keyed by register name. Shared
/// with the Lua sim thread so scripts can drive virtual registers and the table shows them.
pub type VirtualStore = Arc<RwLock<HashMap<String, ferrowl_codec::Value>>>;

/// Classifies a network/status line from the client/server instance for the log ring: outright
/// disconnects and transport errors are `Error`, degraded-but-recovering states (lost connection,
/// backoff, retried exceptions) are `Warning`, everything else (request intent, success) is `Info`.
pub(crate) fn network_log_level(s: &str) -> Level {
    let lower = s.to_lowercase();
    if lower.contains("disconnecting")
        || lower.contains("reconnect disabled")
        || lower.contains("timed out")
        || lower.contains("tls handshake")
    {
        Level::Error
    } else if lower.contains("disconnected")
        || lower.contains("reconnecting")
        || lower.contains("invalid")
        || lower.contains("dropped")
        || lower.contains("failed")
        || lower.contains("already in use")
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
    /// Live name to `Register` view of `registers` that every Lua bridge reads (SC-R-067).
    lua_registers: SharedRegisters,
    /// Shared operations list — owned here so it can be updated in-place without rebuilding the
    /// network instance (the instance holds a clone of the same Arc).
    operations: Arc<RwLock<Vec<Operation>>>,
    memory: ModuleMemory,
    log: ModuleLog,
    /// Dedicated ring for Lua sim output (`C_Log:*`/`print()`) and sim lifecycle messages,
    /// separate from `log`'s connection/status/traffic lines.
    script_log: ModuleLog,
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
    /// Cached self-signed TLS material (MB-R-138), reused across `reconfigure()` calls as long
    /// as the resolved TLS source stays self-signed. Created once here, per module instance —
    /// never re-created inside `reconfigure`, which is what makes `:restart`/`:reload`/a config
    /// edit that keeps the source self-signed reuse cached material, while a genuinely fresh
    /// module instance (new tab, device-type switch) starts with an empty cache.
    self_signed_cache: ferrowl_modbus::tcp::SelfSignedCache,
    /// MB-R-150 — the session-wide registry this instance's path-conflict check consults.
    /// Defaults to a private, unshared registry (never conflicts with anything) until
    /// `set_serial_paths` attaches the real session registry.
    serial_paths: SerialPathRegistry,
    /// This instance's own `~`-expanded Rtu/Ascii serial path, or `None` for every other
    /// transport. Recomputed on every `new()`/`reconfigure()`.
    own_serial_path: Option<String>,
    /// The current instance's path-conflict checker cell, or `None` for a non-serial transport.
    /// Re-fetched (and re-attached to `serial_paths`) on every `new()`/`reconfigure()`, since
    /// each rebuilds `self.instance` with a fresh, unattached cell.
    path_conflict_cell: Option<ferrowl_modbus::PathConflictCell>,
}

impl ModbusModule {
    /// Build a module from an instance spec and its device-type config.
    pub fn new(spec: &ModuleSpec, device: &DeviceConfig) -> Self {
        let mut memory = Memory::<Key<SlaveKey>>::default();
        let mut log = LogRing::init();
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
                    .map_or_else(|| default_value(&register), |s| s.to_value(def.resolution));
                virtual_init.insert(name.clone(), init);
            }
            if let Some(range) = def.mem_range() {
                let key = Key {
                    id: SlaveKey {
                        slave_id: UnitId(def.slave_id),
                        kind: def.register().kind().clone(),
                    },
                };
                let mem_kind = match def.kind() {
                    Kind::Coil | Kind::HoldingRegister => CellKind::read_write(def.mem_type()),
                    Kind::DiscreteInput | Kind::InputRegister => CellKind::read(def.mem_type()),
                };
                if let Err(msg) = declare_or_reject_msg(
                    &mut memory,
                    key,
                    &mem_kind,
                    &range,
                    &format!("register '{name}'"),
                ) {
                    log.write(Level::Warning, &msg);
                }
                if let Some(default) = &def.default
                    && let Ok(raw) = register.encode(&default.to_string())
                {
                    let write_key = Key {
                        id: SlaveKey {
                            slave_id: UnitId(def.slave_id),
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
            let subject = gap_cell_subject(&key);
            if let Err(msg) = declare_or_reject_msg(&mut memory, key, &mem_kind, &range, &subject) {
                log.write(Level::Warning, &msg);
            }
        }
        let operations = build_read_operations(&registers, &device.read_ranges);

        let memory: ModuleMemory = Arc::new(MemLock::new(memory));
        let operations = Arc::new(RwLock::new(operations));
        let log: ModuleLog = Arc::new(RwLock::new(log));
        let script_log: ModuleLog = Arc::new(RwLock::new(LogRing::init()));

        let file_sink: FileSink = Arc::new(std::sync::Mutex::new(None));
        let _ = open_sink(&file_sink, device.log_file.as_deref(), &spec.name);

        let timing = Self::resolve_timing(device);
        let self_signed_cache = ferrowl_modbus::tcp::new_self_signed_cache();
        let net_config = endpoint_to_config(&spec.endpoint, &timing, device.tls.clone());
        let instance = build_instance(
            spec.role.client_or_server(),
            net_config,
            operations.clone(),
            memory.clone(),
            self_signed_cache.clone(),
        );

        let lua_registers: SharedRegisters = Arc::new(MemLock::new(
            registers
                .iter()
                .map(|(name, _, register, _)| (name.clone(), register.clone()))
                .collect(),
        ));
        let mut module = Self {
            name: spec.name.clone(),
            instance,
            registers,
            lua_registers,
            operations,
            memory,
            log,
            script_log,
            file_sink,
            scripts,
            read_ranges: device.read_ranges.clone(),
            script_interval: device.script_interval_duration(),
            sim: None,
            virtual_values: Arc::new(RwLock::new(virtual_init)),
            self_signed_cache,
            serial_paths: SerialPathRegistry::default(),
            own_serial_path: None,
            path_conflict_cell: None,
        };
        module.own_serial_path = endpoint_serial_path(&spec.endpoint);
        module.attach_path_conflict();
        module.ensure_sim();
        module
    }

    /// MB-R-150 — (re)attach the current `serial_paths` registry to this instance's
    /// path-conflict cell, if it has one (Rtu/Ascii only). Called at the end of `new()` and
    /// `reconfigure()` (both of which build a fresh `self.instance`, and so a fresh, unattached
    /// cell), and from `set_serial_paths()` (when the owning session's registry itself is
    /// attached or swapped).
    fn attach_path_conflict(&mut self) {
        self.path_conflict_cell = self.instance.path_conflict_cell();
        if let Some(cell) = &self.path_conflict_cell {
            let registry = self.serial_paths.clone();
            let name = self.name.clone();
            cell.set(Arc::new(move |path: &str| registry.conflict(&name, path)));
        }
    }

    /// MB-R-150 — attach this session's live Rtu/Ascii path-conflict registry. A module that
    /// never receives this call keeps the default, unshared registry (never conflicts with
    /// anything) — the pre-feature behavior, still correct for standalone/test use.
    // `#[allow(dead_code)]`: implemented and tested above; App does not call this via
    // `rebuild_registry` yet.
    #[allow(dead_code)]
    pub fn set_serial_paths(&mut self, registry: SerialPathRegistry) {
        self.serial_paths = registry;
        self.attach_path_conflict();
    }

    /// Resolve effective timing for an instance from the device config, falling back to the
    /// built-in defaults.
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

    /// The dedicated log fed by the script sim's `print()`/`C_Log:*` output, distinct from `log`
    /// (connection/status/traffic lines) — shown at the bottom of the scripts dialog.
    pub fn script_log(&self) -> ModuleLog {
        self.script_log.clone()
    }

    pub fn lua_registers(&self) -> SharedRegisters {
        self.lua_registers.clone()
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
        self.lua_registers
            .write()
            .insert(name.clone(), register.clone());
        self.registers
            .push((name, description, register, named_values));
    }

    /// Remove a register from the module's cached register list by name (no-op if absent).
    pub fn remove_register_by_name(&mut self, name: &str) {
        self.lua_registers.write().remove(name);
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
            let mut map = self.lua_registers.write();
            map.remove(&slot.0);
            map.insert(name.clone(), register.clone());
            drop(map);
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
        let result = self
            .instance
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
            .await;
        // MB-R-150 — claim this instance's serial path (Rtu/Ascii only; a no-op via
        // `own_serial_path` being `None` for every other transport) so the registry can report
        // it as a conflict to any other instance that shares it.
        if result.is_ok()
            && let Some(path) = &self.own_serial_path
        {
            self.serial_paths.claim(&self.name, path);
        }
        result
    }

    /// Test-only: the view's deferred-dispatch stop leg (`handle_command`/`refresh`) uses
    /// `request_stop`/`poll_stop` instead; kept as the blocking convenience its own extensive
    /// test suite still exercises directly for setup/cleanup.
    #[cfg(test)]
    pub async fn stop(&mut self) -> Result<(), Error> {
        let result = self.instance.stop().await;
        // MB-R-150 — release unconditionally, even on an error other than `NotRunning`: `stop()`
        // still aborts the task via its grace-then-abort fallback, so leaving a stale claim
        // behind would be the more harmful failure mode ("recovers…once the conflicting
        // instance stops").
        self.serial_paths.release(&self.name);
        result
    }

    /// UI-R-314 — sends the underlying instance a graceful terminate and returns immediately;
    /// see `Instance::request_stop`.
    pub async fn request_stop(&mut self) -> Result<(), Error> {
        let result = self.instance.request_stop().await;
        // MB-R-150 — release only when the failure means the instance was already `Idle` (no
        // task, so no claim of ours is still live to protect): `active()` stays `true` while a
        // stop is already in flight (`Stopping`), and that in-flight task still holds the port,
        // so releasing here would let another instance claim it out from under it.
        if result.is_err() && !self.instance.active() {
            self.serial_paths.release(&self.name);
        }
        result
    }

    /// UI-R-315, MB-R-150 — polls a stop requested via `request_stop`; releases this instance's
    /// serial path claim once the underlying instance reports an outcome, exactly as `stop()`
    /// does today.
    pub async fn poll_stop(&mut self) -> Option<Result<(), Error>> {
        let result = self.instance.poll_stop().await?;
        self.serial_paths.release(&self.name);
        Some(result)
    }

    /// (Re)start the simulation thread (registers resolve live, SC-R-067) if there is at least one
    /// enabled script; stop it otherwise. Any previously running thread is stopped first, so this
    /// is safe to call whenever the enabled-script set may have changed (construction, script
    /// edits) — it is the single source of truth for whether the sim runs.
    fn ensure_sim(&mut self) {
        self.stop_sim();
        self.sim = run_sim(
            self.memory.clone(),
            self.virtual_values.clone(),
            self.lua_registers.clone(),
            self.scripts.clone(),
            self.script_interval,
            self.script_log.clone(),
            self.file_sink.clone(),
        );
    }

    /// Stop and join the simulation thread if one is running.
    fn stop_sim(&mut self) {
        if let Some(mut sim) = self.sim.take() {
            sim.stop();
        }
    }

    /// Whether the Lua simulation thread is currently running. Test-only: production code never
    /// asks — `ensure_sim` alone decides whether the sim runs.
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

    /// Update the script sim cycle period. Takes effect on the next `reload_scripts` restart (does
    /// not itself restart a running sim), mirroring how a scripts edit is what actually triggers
    /// the restart.
    pub fn set_script_interval(&mut self, interval: Duration) {
        self.script_interval = interval;
    }

    /// Execute one script once against this module's registers, outside the sim (SC-R-035). Does
    /// not touch the sim thread: the script need not be in `self.scripts` and need not be enabled,
    /// which is what lets the script dialog run an unsaved, disabled script on demand.
    pub fn run_script_once(&self, name: String, code: String) {
        run_script_once(
            self.memory.clone(),
            self.virtual_values.clone(),
            self.lua_registers.clone(),
            name,
            code,
            self.script_log.clone(),
            self.file_sink.clone(),
        );
    }

    /// Send a write command to the underlying client (errors for servers / when stopped).
    pub async fn send_command(&self, command: ferrowl_modbus::Command) -> Result<(), Error> {
        self.instance.send_command(command).await
    }

    /// Rebuild the underlying instance for a new endpoint/role (e.g. switching client↔server),
    /// reusing the existing memory + registers. The caller must have already settled its own
    /// deferred stop of any previously running instance before calling; the caller is expected to
    /// `start()` afterwards. This keeps the instance in sync with the spec so writes dispatch
    /// correctly. The simulation thread is decoupled from the network instance and keeps running
    /// untouched (SC-R-081).
    pub async fn reconfigure(
        &mut self,
        endpoint: &Endpoint,
        role: Role,
        timing: Timing,
        read_ranges: ReadRanges,
        tls: ferrowl_modbus::tcp::ModbusTlsConfig,
    ) -> Result<(), Error> {
        // Adopt new explicit read ranges: cover their gaps in memory, then rebuild operations.
        self.read_ranges = read_ranges;
        for (key, mem_kind, range) in explicit_read_coverage(&self.registers, &self.read_ranges) {
            let subject = gap_cell_subject(&key);
            let rejected = {
                let mut mem = self.memory.write();
                declare_or_reject_msg(&mut mem, key, &mem_kind, &range, &subject).err()
            };
            if let Some(msg) = rejected {
                self.log.write().await.write(Level::Warning, &msg);
            }
        }
        self.rebuild_operations().await;
        let net_config = endpoint_to_config(endpoint, &timing, tls);
        self.instance = build_instance(
            role.client_or_server(),
            net_config,
            self.operations.clone(),
            self.memory.clone(),
            self.self_signed_cache.clone(),
        );
        self.own_serial_path = endpoint_serial_path(endpoint);
        self.attach_path_conflict();
        Ok(())
    }

    /// MB-R-137/153 — superseded as the view-facing signal by `connection_status()` (which
    /// distinguishes running-but-not-connected from not-running), but kept as the plain
    /// task-alive check its own existing tests still assert on directly.
    #[allow(dead_code)]
    pub fn is_instance_active(&self) -> bool {
        self.instance.active()
    }

    /// The address a server-role instance is actually bound to right now — `None` for a client
    /// instance, a pure-serial (Rtu/Ascii) server, a never-started instance, or one currently
    /// backing off from a failed bind (MB-R-130). Lets the view display the real OS-assigned
    /// port when the configured port was `0` (mirrors OCPP's own CSMS status line, OC-R-083).
    pub fn bound_addr(&self) -> Option<std::net::SocketAddr> {
        self.instance.bound_addr()
    }

    /// MB-R-137/153 — the tri-state connection status shown by this module's status bar.
    pub fn connection_status(&self) -> crate::view::status_bar::ConnStatus {
        self.instance.connection_status()
    }
}

#[cfg(test)]
mod tests {
    use ferrowl_codec::Kind;
    use ferrowl_modbus::UnitId;
    use ferrowl_test_support::{
        TempDirGuard, reserve_temp_dir, wait_until, wait_until_async, wait_until_blocking,
    };

    #[test]
    /// MB-R-087 — effective timing uses the device config's values when set, otherwise the built-in defaults.
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

    #[test]
    /// MB-R-178 (server logging half) — a TLS handshake failure line classifies as
    /// `Level::Error`, matching the peer/failure-detail format `on_tls_handshake_failed`
    /// logs (`"TLS handshake with {peer} failed: {detail}."`).
    fn ut_network_log_level_classifies_tls_handshake_failure_as_error() {
        use super::network_log_level;
        use crate::app::Level;

        let line = "TLS handshake with 127.0.0.1:5502 failed: bad certificate.";
        assert_eq!(network_log_level(line), Level::Error);
    }

    #[test]
    /// MB-R-150 — a serial-path-conflict log line classifies as `Level::Warning`, the same
    /// degraded-but-recovering bucket as "reconnecting"/"disconnected", not the Info default.
    fn ut_network_log_level_classifies_path_conflict_as_warning() {
        use super::network_log_level;
        use crate::app::Level;

        let line = "Serial path '/dev/ttyUSB0' is already in use by module 'PLC Sim' in this \
                     session; skipping open.";
        assert_eq!(network_log_level(line), Level::Warning);
    }

    fn device_with_defs() -> (crate::config::DeviceConfig, TempDirGuard) {
        use crate::config::DeviceConfig;
        use crate::config::device::{
            AccessCfg, AlignmentCfg, EndianCfg, NamedValue, ReadRanges, RegisterDef, Scalar,
            ValueType, WordOrderCfg,
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
            word_order: WordOrderCfg::default(),
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

        let dir = reserve_temp_dir("ferrowl_modbus_module");
        let log_file = dir.join("test.log").to_string_lossy().into_owned();

        let device = DeviceConfig {
            version: None,
            timeout_ms: Some(1000),
            delay_ms: None,
            interval_ms: Some(500),
            reconnect: None,
            tls: Default::default(),
            log_file: Some(log_file),
            read_ranges: ReadRanges {
                holding: Some("0-10".into()),
                ..Default::default()
            },
            definitions,
            scripts: Vec::new(),
            script_interval: 1.0,
        };
        (device, dir)
    }

    #[test]
    /// MB-R-088 — a TCP server module's register-cache edits (add/rename/remove) rebuild its register set.
    fn ut_module_new_tcp_server_and_sync_accessors() {
        use super::ModbusModule;
        use crate::config::{Endpoint, ModuleSpec, Role};

        let (device, _dir) = device_with_defs();
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
        assert!(
            module
                .set_log_base(Some("/no/such/ferrowl/dir/base.log"))
                .is_err()
        );
    }

    #[tokio::test]
    /// MB-R-210 — a module instance owns one shared register store and one log: two accessor
    /// calls return handles to the same store/log (not fresh copies), and the store `memory()`
    /// hands back is literally what the running network instance serves from — a raw write
    /// through `memory()` is visible to a real client's read over the wire.
    async fn ut_module_owns_one_shared_store_and_log() {
        use super::ModbusModule;
        use ferrowl_modbus::{Address, Key, SlaveKey};
        use ferrowl_store::Range;

        let (device, _dir) = device_with_defs();
        let mut module = ModbusModule::new(&test_spec("mb210", 0), &device);
        assert!(std::sync::Arc::ptr_eq(&module.memory(), &module.memory()));
        assert!(std::sync::Arc::ptr_eq(&module.log(), &module.log()));

        module.start().await.expect("start");
        let addr = wait_until(
            "listener bind",
            tokio::time::Duration::from_millis(20),
            tokio::time::Duration::from_secs(10),
            || module.bound_addr(),
        )
        .await;

        module.memory().write().write_unchecked(
            Key {
                id: SlaveKey {
                    slave_id: UnitId(1),
                    kind: Kind::HoldingRegister,
                },
            },
            &Range::new(0, 1),
            &[99u16],
        );

        let mut client =
            rust_modbus::Client::<_, rust_modbus::Tcp>::new(rust_modbus::FrameTransport::new(
                tokio::net::TcpStream::connect(addr).await.expect("connect"),
            ));
        let registers = client
            .read_holding_registers(UnitId(1), Address(0), rust_modbus::Quantity(1))
            .await
            .expect("a real client must read the value written through memory()");
        assert_eq!(
            registers[0],
            rust_modbus::RegisterValue(99),
            "the network instance must serve from the exact store memory() returns"
        );

        module.stop().await.expect("stop");
    }

    #[tokio::test]
    /// MB-R-130 (bound_addr companion) — a TCP server module's `bound_addr()` is `None` before
    /// `start()`, `Some(<real addr>)` once the listener actually binds (even with the configured
    /// `port: 0`), and `None` again after `stop()` — the same ready-signal lifecycle
    /// `ferrowl-modbus`'s `ServerBuilder::spawn` and `Instance::bound_addr` already prove,
    /// threaded one layer further through the module the view reads from.
    async fn ut_module_bound_addr_reflects_listener_state() {
        use super::ModbusModule;
        use crate::config::{Endpoint, ModuleSpec, Role};

        let (device, _dir) = device_with_defs();
        let spec = ModuleSpec {
            name: "srv".into(),
            device: String::new(),
            role: Role::Server,
            endpoint: Endpoint::Tcp {
                ip: "127.0.0.1".into(),
                port: 0,
            },
        };
        let mut module = ModbusModule::new(&spec, &device);
        assert!(module.bound_addr().is_none());

        module.start().await.expect("start");

        let addr = wait_until(
            "listener bind",
            tokio::time::Duration::from_millis(20),
            tokio::time::Duration::from_secs(10),
            || module.bound_addr(),
        )
        .await;
        assert_ne!(addr.port(), 0, "the OS must have assigned a real port");

        module.stop().await.expect("stop");
        assert!(
            module.bound_addr().is_none(),
            "bound_addr must clear once the module stops"
        );
    }

    #[test]
    /// MB-R-079 — a register definition's `default` value is encoded and written into the store at construction (bypassing cell access checks).
    fn ut_default_value_written_into_store_at_construction() {
        use super::ModbusModule;
        use crate::config::{Endpoint, ModuleSpec, Role};
        use ferrowl_modbus::{Key, SlaveKey};
        use ferrowl_store::Range;

        // `device_with_defs` seeds the fixed holding register "hold" at address 0 with default 7.
        let (device, _dir) = device_with_defs();
        let spec = ModuleSpec {
            name: "srv".into(),
            device: String::new(),
            role: Role::Server,
            endpoint: Endpoint::Tcp {
                ip: "127.0.0.1".into(),
                port: 5020,
            },
        };
        let module = ModbusModule::new(&spec, &device);
        let key = Key {
            id: SlaveKey {
                slave_id: UnitId(1),
                kind: Kind::HoldingRegister,
            },
        };
        let stored = module
            .memory()
            .read()
            .read_unchecked(key, &Range::new(0, 1));
        assert_eq!(stored, Some(vec![7]));
    }

    #[tokio::test]
    /// MB-R-089 — reconfiguring a module's endpoint/role rebuilds the instance against the same store and preserves the stored register values.
    async fn ut_reconfigure_preserves_stored_values() {
        use super::ModbusModule;
        use crate::config::device::ReadRanges;
        use crate::config::{Endpoint, ModuleSpec, Role};
        use ferrowl_modbus::{Key, SlaveKey};
        use ferrowl_store::Range;

        let (device, _dir) = device_with_defs();
        let spec = ModuleSpec {
            name: "srv".into(),
            device: String::new(),
            role: Role::Server,
            endpoint: Endpoint::Tcp {
                ip: "127.0.0.1".into(),
                port: 5020,
            },
        };
        let mut module = ModbusModule::new(&spec, &device);
        let key = Key {
            id: SlaveKey {
                slave_id: UnitId(1),
                kind: Kind::HoldingRegister,
            },
        };
        // The store carries the default (7) at address 0.
        assert_eq!(
            module
                .memory()
                .read()
                .read_unchecked(key.clone(), &Range::new(0, 1)),
            Some(vec![7])
        );

        // Switch role (server → client) and endpoint; the stored value must survive.
        let timing = ModbusModule::resolve_timing(&device);
        module
            .reconfigure(
                &Endpoint::Tcp {
                    ip: "127.0.0.1".into(),
                    port: 5099,
                },
                Role::Client,
                timing,
                ReadRanges::default(),
                device.tls.clone(),
            )
            .await
            .expect("reconfigure");

        assert_eq!(
            module
                .memory()
                .read()
                .read_unchecked(key, &Range::new(0, 1)),
            Some(vec![7])
        );
    }

    #[test]
    /// An RTU client module instance builds its register set from the device config.
    fn ut_module_new_rtu_client() {
        use super::ModbusModule;
        use crate::config::{Endpoint, ModuleSpec, Role};

        let (device, _dir) = device_with_defs();
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

    fn rtu_spec(name: &str, path: &str) -> crate::config::ModuleSpec {
        use crate::config::{Endpoint, ModuleSpec, Role};
        ModuleSpec {
            name: name.to_string(),
            device: String::new(),
            role: Role::Client,
            endpoint: Endpoint::Rtu {
                path: path.to_string(),
                baud_rate: 9600,
                parity: None,
                data_bits: None,
                stop_bits: None,
            },
        }
    }

    #[tokio::test]
    /// MB-R-150 — starting a module claims its serial path in the attached registry (visible to
    /// another instance sharing the registry as a conflict); stopping releases it.
    async fn ut_module_start_claims_serial_path_stop_releases() {
        use super::ModbusModule;
        use crate::module::modbus::SerialPathRegistry;

        let (device, _dir) = device_with_defs();
        let path = "/nonexistent/mb-r-150-a";
        let mut module_a = ModbusModule::new(&rtu_spec("A", path), &device);
        let registry = SerialPathRegistry::new();
        module_a.set_serial_paths(registry.clone());

        let _ = module_a.start().await;
        assert_eq!(registry.conflict("B", path), Some("A".to_string()));

        module_a.stop().await.expect("stop");
        assert_eq!(registry.conflict("B", path), None);
    }

    #[tokio::test]
    /// MB-R-150 — a repeat `request_stop()` while already stopping must not release the serial
    /// claim: the task still holds the port until `poll_stop()` reaps it, so releasing it here
    /// would let another instance claim the same path out from under the still-running task.
    async fn ut_module_request_stop_while_stopping_does_not_release_claim() {
        use super::ModbusModule;
        use crate::instance::error::{Error, InstanceError};
        use crate::module::modbus::SerialPathRegistry;

        let (device, _dir) = device_with_defs();
        let path = "/nonexistent/mb-r-150-stopping";
        let mut module_a = ModbusModule::new(&rtu_spec("A", path), &device);
        let registry = SerialPathRegistry::new();
        module_a.set_serial_paths(registry.clone());

        let _ = module_a.start().await;
        assert_eq!(registry.conflict("B", path), Some("A".to_string()));

        module_a.request_stop().await.expect("first request_stop");
        let err = module_a.request_stop().await.unwrap_err();
        assert!(matches!(err, Error::Instance(InstanceError::NotRunning)));
        assert_eq!(
            registry.conflict("B", path),
            Some("A".to_string()),
            "claim must survive a repeat request_stop while still stopping"
        );

        let _ = wait_until_async(
            "stop settles",
            tokio::time::Duration::from_millis(5),
            tokio::time::Duration::from_secs(10),
            async || module_a.poll_stop().await,
        )
        .await;
        assert_eq!(registry.conflict("B", path), None);
    }

    #[tokio::test]
    /// MB-R-150 — `reconfigure()` reattaches the (possibly freshly-serial) instance to the
    /// already-set registry, so a start after reconfiguring still sees a conflict against another
    /// instance sharing the same path.
    async fn ut_module_reconfigure_reattaches_path_conflict() {
        use super::ModbusModule;
        use crate::config::device::ReadRanges;
        use crate::config::{Endpoint, ModuleSpec, Role};
        use crate::module::modbus::SerialPathRegistry;

        let (device, _dir) = device_with_defs();
        let path = "/nonexistent/mb-r-150-b";

        // Module B already claims the path directly in the shared registry.
        let registry = SerialPathRegistry::new();
        registry.claim("B", path);

        // Module A starts as a Tcp client (no serial path), then reconfigures onto the same Rtu
        // path B already claims.
        let spec = ModuleSpec {
            name: "A".into(),
            device: String::new(),
            role: Role::Client,
            endpoint: Endpoint::Tcp {
                ip: "127.0.0.1".into(),
                port: 5020,
            },
        };
        let mut module_a = ModbusModule::new(&spec, &device);
        module_a.set_serial_paths(registry.clone());

        let timing = ModbusModule::resolve_timing(&device);
        module_a
            .reconfigure(
                &Endpoint::Rtu {
                    path: path.to_string(),
                    baud_rate: 9600,
                    parity: None,
                    data_bits: None,
                    stop_bits: None,
                },
                Role::Client,
                timing,
                ReadRanges::default(),
                Default::default(),
            )
            .await
            .expect("reconfigure");

        let _ = module_a.start().await;
        assert_eq!(registry.conflict("A", path), Some("B".to_string()));
    }

    #[tokio::test]
    /// MB-R-150 — a module that never receives `set_serial_paths` keeps its own private, default
    /// registry: its claim never lands in any other (e.g. a session-wide) registry.
    async fn ut_module_new_without_set_serial_paths_never_conflicts() {
        use super::ModbusModule;
        use crate::module::modbus::SerialPathRegistry;

        let (device, _dir) = device_with_defs();
        let path = "/nonexistent/mb-r-150-c";
        let mut module_a = ModbusModule::new(&rtu_spec("A", path), &device);

        let _ = module_a.start().await;

        let unrelated_registry = SerialPathRegistry::new();
        assert_eq!(unrelated_registry.conflict("B", path), None);
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
            ValueType, WordOrderCfg,
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
                word_order: WordOrderCfg::default(),
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
            tls: Default::default(),
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
                        slave_id: UnitId(1),
                        kind: Kind::HoldingRegister,
                    },
                },
                &CellType::Register,
                &Range::new(0, 1),
            )
            .unwrap_or_default();
        raw.first().copied().unwrap_or(0)
    }

    #[test]
    /// SC-R-026 — a Modbus sim runs from construction on an enabled script, with no network start.
    fn ut_sim_starts_at_construction_without_network_start() {
        use super::ModbusModule;

        let device = device_with_script(vec![script(r#"C_Register:Set("marker", 7)"#, true)]);
        let module = ModbusModule::new(&test_spec("sim1", 15201), &device);

        // No `start()` call anywhere — the sim runs solely because a script is enabled.
        assert!(module.lua_running());
        wait_until_blocking(
            "marker reaches 7",
            std::time::Duration::from_millis(10),
            std::time::Duration::from_secs(10),
            || (read_marker(&module) == 7).then_some(()),
        );
    }

    #[test]
    /// SC-R-011 — with no enabled script (empty or disabled), no sim thread is spawned.
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
    /// SC-R-024 — reloading with no enabled script stops the running sim and leaves it stopped.
    fn ut_reload_scripts_all_disabled_stops_sim() {
        use super::ModbusModule;

        let device = device_with_script(vec![script(r#"C_Register:Set("marker", 7)"#, true)]);
        let mut module = ModbusModule::new(&test_spec("sim3", 15204), &device);
        assert!(module.lua_running());

        module.reload_scripts(vec![]); // no enabled scripts left
        assert!(!module.lua_running());
    }

    #[test]
    /// SC-R-024 — toggling a script on via reload starts a fresh sim thread.
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
        wait_until_blocking(
            "marker reaches 7",
            std::time::Duration::from_millis(10),
            std::time::Duration::from_secs(10),
            || (read_marker(&module) == 7).then_some(()),
        );
    }

    #[test]
    /// SC-R-024 — editing a script restarts the sim on a fresh Lua context so the new code takes over.
    fn ut_reload_scripts_changed_code_takes_effect() {
        use super::ModbusModule;

        let device = device_with_script(vec![script(r#"C_Register:Set("marker", 1)"#, true)]);
        let mut module = ModbusModule::new(&test_spec("sim5", 15206), &device);
        wait_until_blocking(
            "marker reaches 1",
            std::time::Duration::from_millis(10),
            std::time::Duration::from_secs(10),
            || (read_marker(&module) == 1).then_some(()),
        );

        // Fresh Lua state on restart: new code takes over immediately (proves a restart, not the
        // old thread still running the old script).
        module.reload_scripts(vec![(
            "sim".to_string(),
            r#"C_Register:Set("marker", 2)"#.to_string(),
        )]);
        wait_until_blocking(
            "marker reaches 2",
            std::time::Duration::from_millis(10),
            std::time::Duration::from_secs(10),
            || (read_marker(&module) == 2).then_some(()),
        );
    }

    #[tokio::test]
    /// SC-R-026 — starting or stopping the network instance leaves the sim thread running.
    async fn ut_network_stop_leaves_sim_running() {
        use super::ModbusModule;

        let device = device_with_script(vec![script(r#"C_Register:Set("marker", 7)"#, true)]);
        // Port 0: this is the one sim test that actually calls `start()`, so a fixed port
        // would fail whenever something else occupies it.
        let mut module = ModbusModule::new(&test_spec("sim6", 0), &device);
        assert!(module.lua_running());

        module.start().await.expect("start");
        assert!(module.lua_running());
        module.stop().await.expect("stop");
        assert!(module.lua_running());
    }

    // Confirms the log-ring split: Lua `print()`/`C_Log` output lands only in
    // `script_log`, never in the module's general `log` (connection/status/traffic lines).
    #[test]
    /// SC-R-031 — Modbus sim print/C_Log output goes to the module's script log, not its connection log.
    fn ut_lua_output_lands_in_script_log_not_general_log() {
        use super::ModbusModule;

        let device = device_with_script(vec![script(
            r#"print("hello"); C_Log:Info("info-line")"#,
            true,
        )]);
        let module = ModbusModule::new(&test_spec("sim7", 15208), &device);

        let script_lines = |module: &ModbusModule| -> Vec<String> {
            module
                .script_log
                .blocking_read()
                .peek_n(crate::app::LOG_SIZE)
                .into_iter()
                .map(|(_, _, l)| l)
                .collect()
        };
        wait_until_blocking(
            "Lua print/C_Log output should reach script_log",
            std::time::Duration::from_millis(10),
            std::time::Duration::from_secs(10),
            || {
                let lines = script_lines(&module);
                (lines.iter().any(|l| l == "hello") && lines.iter().any(|l| l == "info-line"))
                    .then_some(())
            },
        );

        let general_lines: Vec<String> = module
            .log
            .blocking_read()
            .peek_n(crate::app::LOG_SIZE)
            .into_iter()
            .map(|(_, _, l)| l)
            .collect();
        assert!(
            !general_lines
                .iter()
                .any(|l| l == "hello" || l == "info-line"),
            "Lua output must not leak into the general log: {general_lines:?}"
        );
    }
    fn test_register(addr: u16, u32_format: bool) -> ferrowl_codec::Register {
        use ferrowl_codec::format::{BitField, Endian, Resolution, WordOrder};
        use ferrowl_codec::{Access, Format, RegisterBuilder};
        let format = if u32_format {
            Format::u32(
                Endian::Big,
                WordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            )
        } else {
            Format::u16(
                Endian::Big,
                WordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            )
        };
        RegisterBuilder::default()
            .slave_id(UnitId(1))
            .access(Access::ReadWrite)
            .kind(Kind::HoldingRegister)
            .address(ferrowl_codec::Address::Fixed(addr))
            .format(format)
            .build()
            .unwrap()
    }

    fn read_holding(module: &super::ModbusModule, addr: u16, len: u16) -> Vec<u16> {
        use ferrowl_modbus::{Key, SlaveKey};
        use ferrowl_store::{CellType, Range};
        module
            .memory()
            .read()
            .read(
                Key {
                    id: SlaveKey {
                        slave_id: UnitId(1),
                        kind: Kind::HoldingRegister,
                    },
                },
                &CellType::Register,
                &Range::new(addr as usize, len as usize),
            )
            .unwrap_or_default()
    }

    const COUNTER: &str = r#"n = (n or 0) + 1; C_Register:Set("marker", n)"#;

    /// Globals-intact observation: wait for the counter to reach 3, run `edit`, then require
    /// that the counter never drops and keeps rising for ~300 ms.
    fn assert_counter_survives(
        module: &mut super::ModbusModule,
        edit: impl FnOnce(&mut super::ModbusModule),
    ) {
        wait_until_blocking(
            "counter reaches 3",
            std::time::Duration::from_millis(10),
            std::time::Duration::from_secs(10),
            || (read_marker(module) >= 3).then_some(()),
        );
        let before = read_marker(module);
        assert!(before >= 3, "counter never reached 3");
        edit(module);
        let mut max = before;
        for _ in 0..30 {
            let v = read_marker(module);
            assert!(
                v >= before,
                "counter dropped to {v} (before {before}): sim restarted"
            );
            max = max.max(v);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(max > before, "counter stopped advancing");
        assert!(module.lua_running());
    }

    #[test]
    /// SC-R-068 — a register added at runtime is reachable by a sim already running.
    fn ut_sim_reaches_register_added_at_runtime() {
        use super::ModbusModule;
        let device = device_with_script(vec![script(
            r#"C_Register:Set("marker", 1)
if C_Register:Has("extra") then C_Register:Set("extra", 9) end"#,
            true,
        )]);
        let mut module = ModbusModule::new(&test_spec("sim_add", 0), &device);
        assert!(module.lua_running());
        wait_until_blocking(
            "sim never ran before the add",
            std::time::Duration::from_millis(10),
            std::time::Duration::from_secs(10),
            || (read_marker(&module) == 1).then_some(()),
        );

        module.add_register(
            "extra".into(),
            String::new(),
            test_register(1, false),
            vec![],
        );
        wait_until_blocking(
            "added register never written by the running sim",
            std::time::Duration::from_millis(10),
            std::time::Duration::from_secs(10),
            || (read_holding(&module, 1, 1) == [9]).then_some(()),
        );
        assert!(module.lua_running());
    }

    #[test]
    /// SC-R-069, SC-E-041 — an edited register is reached by its new name, address and format;
    /// the old name is unknown.
    fn ut_sim_follows_register_edit_to_new_name_address_format() {
        use super::ModbusModule;
        let device = device_with_script(vec![script(
            r#"if C_Register:Has("wide") then C_Register:Set("wide", 70000) end
if C_Register:Has("marker") then C_Register:Set("marker", 3) end"#,
            true,
        )]);
        let mut module = ModbusModule::new(&test_spec("sim_edit", 0), &device);
        wait_until_blocking(
            "marker reaches 3",
            std::time::Duration::from_millis(10),
            std::time::Duration::from_secs(10),
            || (read_marker(&module) == 3).then_some(()),
        );

        module.memory().write().write_unchecked(
            ferrowl_modbus::Key {
                id: ferrowl_modbus::SlaveKey {
                    slave_id: UnitId(1),
                    kind: Kind::HoldingRegister,
                },
            },
            &ferrowl_store::Range::new(0, 1),
            &[0],
        );
        module.update_register(0, "wide".into(), "d".into(), test_register(2, true), vec![]);
        wait_until_blocking(
            "post-edit name/address/format not honoured",
            std::time::Duration::from_millis(10),
            std::time::Duration::from_secs(10),
            || (read_holding(&module, 2, 2) == [1, 4464]).then_some(()),
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert_eq!(read_marker(&module), 0, "old name must be unknown");
        assert!(module.lua_running());
    }

    #[test]
    /// SC-R-070 — register add, edit and delete never restart the sim nor reset its globals.
    fn ut_sim_keeps_globals_across_register_edits() {
        use super::ModbusModule;
        let device = device_with_script(vec![script(COUNTER, true)]);
        let mut module = ModbusModule::new(&test_spec("sim_glob", 0), &device);
        assert_counter_survives(&mut module, |m| {
            m.add_register("x".into(), String::new(), test_register(5, false), vec![]);
            let reg = m.registers()[0].2.clone();
            m.update_register(0, "marker".into(), "desc".into(), reg, vec![]);
            m.remove_register_by_name("x");
        });
    }

    #[test]
    /// SC-E-042 — a deleted register errors in the sim log while the sim and its other scripts
    /// keep running.
    fn ut_sim_keeps_running_after_register_delete() {
        use super::ModbusModule;
        let def = |name: &str, code: &str| crate::config::script::ScriptDef {
            name: name.to_string(),
            code: code.to_string(),
            enabled: true,
        };
        let mut device = device_with_script(vec![
            def("a", r#"C_Register:Get("gone")"#),
            def("b", COUNTER),
        ]);
        let mut gone = device.definitions["marker"].clone();
        gone.address = Some(1);
        device.definitions.insert("gone".into(), gone);
        let mut module = ModbusModule::new(&test_spec("sim_del", 0), &device);
        wait_until_blocking(
            "sim never ran before the delete",
            std::time::Duration::from_millis(10),
            std::time::Duration::from_secs(10),
            || (read_marker(&module) == 1).then_some(()),
        );

        module.remove_register_by_name("gone");
        wait_until_blocking(
            "unknown register error not logged",
            std::time::Duration::from_millis(10),
            std::time::Duration::from_secs(10),
            || {
                let lines: Vec<String> = module
                    .script_log()
                    .blocking_read()
                    .peek_n(crate::app::LOG_SIZE)
                    .into_iter()
                    .map(|(_, _, l)| l)
                    .collect();
                lines
                    .iter()
                    .any(|l| l.contains("[sim]") && l.contains("unknown register 'gone'"))
                    .then_some(())
            },
        );
        let a = read_marker(&module);
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(read_marker(&module) > a, "other script stopped");
        assert!(module.lua_running());
    }

    #[test]
    /// SC-E-043 — a value held in a Lua global is not refreshed by a register edit.
    fn ut_global_value_not_refreshed_by_edit() {
        use super::ModbusModule;
        let device = device_with_script(vec![script(
            r#"if held == nil then held = C_Register:Get("marker") end; C_Register:Set("copy", held + 100)"#,
            true,
        )]);
        let mut module = ModbusModule::new(&test_spec("sim_held", 0), &device);
        module.add_register(
            "copy".into(),
            String::new(),
            test_register(1, false),
            vec![],
        );
        wait_until_blocking(
            "holding 1 reads [100]",
            std::time::Duration::from_millis(10),
            std::time::Duration::from_secs(10),
            || (read_holding(&module, 1, 1) == [100]).then_some(()),
        );
        module.memory().write().write_unchecked(
            ferrowl_modbus::Key {
                id: ferrowl_modbus::SlaveKey {
                    slave_id: UnitId(1),
                    kind: Kind::HoldingRegister,
                },
            },
            &ferrowl_store::Range::new(0, 1),
            &[42],
        );
        let reg = module.registers()[0].2.clone();
        module.update_register(0, "marker".into(), "desc".into(), reg, vec![]);
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert_eq!(
            read_holding(&module, 1, 1),
            [100],
            "held global was refreshed"
        );
    }

    #[tokio::test]
    /// SC-R-081, SC-E-032 — reconfiguring endpoint/role keeps the sim running with its globals intact; reconfigure is not a stop trigger.
    async fn ut_reconfigure_keeps_sim_and_globals() {
        use super::ModbusModule;
        use crate::config::{Endpoint, Role};
        let device = device_with_script(vec![script(COUNTER, true)]);
        let mut module = ModbusModule::new(&test_spec("sim_reconf", 0), &device);
        wait_until(
            "counter reaches 3",
            std::time::Duration::from_millis(10),
            std::time::Duration::from_secs(10),
            || (read_marker(&module) >= 3).then_some(()),
        )
        .await;
        let before = read_marker(&module);
        assert!(before >= 3);
        module
            .reconfigure(
                &Endpoint::Tcp {
                    ip: "127.0.0.1".into(),
                    port: 0,
                },
                Role::Client,
                ModbusModule::resolve_timing(&device),
                device.read_ranges.clone(),
                device.tls.clone(),
            )
            .await
            .expect("reconfigure");
        let mut max = before;
        for _ in 0..30 {
            let v = read_marker(&module);
            assert!(
                v >= before,
                "counter dropped to {v} (before {before}): sim restarted"
            );
            max = max.max(v);
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(max > before);
        assert!(module.lua_running());
    }
}
