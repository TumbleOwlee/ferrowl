//! The [`Module`]: one running Modbus endpoint with its registers, shared
//! memory, log, and optional Lua simulation.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ferrowl_codec::{Access, Address, Kind, Register};
use ferrowl_modbus::{FunctionCode, Key, Operation, SlaveKey, Transport as NetConfig};
use ferrowl_store::{CellKind as MemKind, CellType, Memory, Range};
use tokio::sync::RwLock;

use crate::app::LogRing;
use crate::config::{
    DeviceConfig, Endpoint, ModuleSpec, Role,
    device::{DEFAULT_DELAY_MS, DEFAULT_INTERVAL_MS, DEFAULT_TIMEOUT_MS, NamedValue, ReadRanges},
};
use crate::instance::Instance;
use crate::instance::config::{ClientConfig, ServerConfig};
use crate::instance::error::Error;
use crate::lua::{SimHandle, run_sim};
use crate::view::log::format_timestamp;

pub type ModuleMemory = Arc<RwLock<Memory<Key<SlaveKey>>>>;
pub type ModuleLog = Arc<RwLock<LogRing>>;
/// Shared store of virtual-register values (no Modbus address), keyed by register name. Shared
/// with the Lua sim thread so `update` scripts can drive virtual registers and the table shows them.
pub type VirtualStore = Arc<RwLock<HashMap<String, ferrowl_codec::Value>>>;
/// Optional per-module log file, shared into the log/status callbacks; swappable at runtime so
/// `:log` takes effect on already-running modules.
pub type FileSink = Arc<Mutex<Option<BufWriter<std::fs::File>>>>;

/// One module instance: a modbus client (reads an external server) or server (simulates a
/// device), plus its register set, shared memory and ring log.
pub struct Module {
    name: String,
    instance: Instance<SlaveKey>,
    registers: Vec<(String, String, Register, Vec<NamedValue>)>,
    /// Shared operations list — owned here so it can be updated in-place without rebuilding the
    /// network instance (the instance holds a clone of the same Arc).
    operations: Arc<RwLock<Vec<Operation>>>,
    memory: ModuleMemory,
    log: ModuleLog,
    file_sink: FileSink,
    /// Per-register `update` Lua snippets (register name → code), run on the sim thread.
    scripts: Vec<(String, String)>,
    /// Explicit per-function-code read ranges from the device config (empty = auto-merge).
    read_ranges: ReadRanges,
    /// Simulation cycle period, derived from the resolved `interval_ms`.
    sim_interval: Duration,
    /// The running simulation thread, if any (started in `start`, stopped in `stop`).
    sim: Option<SimHandle>,
    /// Shared values for virtual registers (no Modbus address), keyed by register name.
    virtual_values: VirtualStore,
}

impl Module {
    /// Build a module from an instance spec and its device-type config.
    pub fn new(spec: &ModuleSpec, device: &DeviceConfig) -> Self {
        let mut memory = Memory::<Key<SlaveKey>>::default();
        let mut registers: Vec<(String, String, Register, Vec<NamedValue>)> = Vec::new();
        let mut scripts: Vec<(String, String)> = Vec::new();
        let mut virtual_init: HashMap<String, ferrowl_codec::Value> = HashMap::new();

        for (name, def) in &device.definitions {
            let register = def.register();
            registers.push((
                name.clone(),
                def.description.clone(),
                register.clone(),
                def.values.clone(),
            ));
            if let Some(code) = &def.update
                && !code.trim().is_empty()
            {
                scripts.push((name.clone(), code.clone()));
            }
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
                    memory.write_unchecked(write_key, &Range::new(range.start, raw.len()), &raw);
                }
            }
        }
        // Cover gaps inside explicit read ranges (Read cells) so a batched client read can store
        // the whole request; the gap words are read but otherwise unused.
        for (key, mem_kind, range) in explicit_read_coverage(&registers, &device.read_ranges) {
            memory.add_ranges(key, &mem_kind, std::slice::from_ref(&range));
        }
        let operations = build_read_operations(&registers, &device.read_ranges);

        let memory: ModuleMemory = Arc::new(RwLock::new(memory));
        let operations = Arc::new(RwLock::new(operations));
        let log: ModuleLog = Arc::new(RwLock::new(LogRing::init()));

        let file_sink: FileSink = Arc::new(Mutex::new(None));
        open_sink(&file_sink, device.log_file.as_deref(), &spec.name);

        let timing = Self::resolve_timing(spec, device);
        let net_config = endpoint_to_config(&spec.endpoint, &timing);
        let instance = build_instance(spec.role, net_config, operations.clone(), memory.clone());

        Self {
            name: spec.name.clone(),
            instance,
            registers,
            operations,
            memory,
            log,
            file_sink,
            scripts,
            read_ranges: device.read_ranges.clone(),
            sim_interval: Duration::from_millis(timing.interval_ms.max(1000) as u64),
            sim: None,
            virtual_values: Arc::new(RwLock::new(virtual_init)),
        }
    }

    /// Resolve effective timing for an instance: a `ModuleSpec` override wins, else the device
    /// config value, else the built-in default.
    pub fn resolve_timing(spec: &ModuleSpec, device: &DeviceConfig) -> Timing {
        Timing {
            timeout_ms: spec
                .timeout_ms
                .or(device.timeout_ms)
                .unwrap_or(DEFAULT_TIMEOUT_MS),
            delay_ms: spec
                .delay_ms
                .or(device.delay_ms)
                .unwrap_or(DEFAULT_DELAY_MS),
            interval_ms: spec
                .interval_ms
                .or(device.interval_ms)
                .unwrap_or(DEFAULT_INTERVAL_MS),
        }
    }

    /// (Re)point this module's log file at `base` (None disables file logging). The filename is
    /// `<stem>.<tab-name>.<ext>` next to `base`. Takes effect on already-running modules.
    pub fn set_log_base(&self, base: Option<&str>) {
        open_sink(&self.file_sink, base, &self.name);
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
    /// configured) the per-module log file. Also (re)starts the Lua simulation thread.
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
                        log.write().await.write(&s);
                        append(&log_sink, &s);
                    }
                },
                move |s: String| {
                    let status = status.clone();
                    let status_sink = status_sink.clone();
                    async move {
                        let line = format!("[status] {s}");
                        status.write().await.write(&line);
                        append(&status_sink, &line);
                    }
                },
            )
            .await;
        self.start_sim();
        result
    }

    pub async fn stop(&mut self) -> Result<(), Error> {
        self.stop_sim();
        self.instance.stop().await
    }

    /// Spawn the Lua simulation thread (no-op if there are no `update` scripts). Any previously
    /// running thread is stopped first so this is safe to call on restart.
    fn start_sim(&mut self) {
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
            self.sim_interval,
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

    /// Start the Lua simulation thread (`:lua start`). No-op when there are no `update` scripts.
    pub fn start_lua(&mut self) {
        self.start_sim();
    }

    /// Stop the Lua simulation thread (`:lua stop`).
    pub fn stop_lua(&mut self) {
        self.stop_sim();
    }

    /// Whether the Lua simulation thread is currently running.
    pub fn lua_running(&self) -> bool {
        self.sim.is_some()
    }

    /// Replace the module's script list and restart the simulation thread so the new scripts take
    /// effect. Any previously running sim thread is stopped first.
    pub fn reload_scripts(&mut self, scripts: Vec<(String, String)>) {
        self.scripts = scripts;
        if self.sim.take().is_some() {
            self.start_sim();
        }
    }

    /// Send a write command to the underlying client (errors for servers / when stopped).
    pub async fn send_command(&self, command: ferrowl_modbus::Command) -> Result<(), Error> {
        self.instance.send_command(command).await
    }

    /// Rebuild the underlying instance for a new endpoint/role (e.g. switching client↔server),
    /// reusing the existing memory + registers. Stops the current instance first; the caller is
    /// expected to `start()` afterwards. This keeps the instance in sync with the spec so writes
    /// dispatch correctly.
    pub async fn reconfigure(
        &mut self,
        endpoint: &Endpoint,
        role: Role,
        timing: Timing,
        read_ranges: ReadRanges,
    ) -> Result<(), Error> {
        // Best-effort stop of any running instance and its simulation thread; the caller is
        // expected to `start()` afterwards, which restarts the sim.
        self.stop_sim();
        let _ = self.instance.stop().await;

        // Adopt new explicit read ranges: cover their gaps in memory, then rebuild operations.
        self.read_ranges = read_ranges;
        for (key, mem_kind, range) in explicit_read_coverage(&self.registers, &self.read_ranges) {
            self.memory
                .write()
                .await
                .add_ranges(key, &mem_kind, std::slice::from_ref(&range));
        }
        self.rebuild_operations().await;
        self.sim_interval = Duration::from_millis(timing.interval_ms.max(1000) as u64);
        let net_config = endpoint_to_config(endpoint, &timing);
        self.instance = build_instance(
            role,
            net_config,
            self.operations.clone(),
            self.memory.clone(),
        );
        Ok(())
    }

    pub fn is_instance_active(&self) -> bool {
        self.instance.active()
    }
}

/// Initial display value for a register: its format decoded from all-zero words (e.g. "0").
/// Used to seed virtual registers so the table isn't blank before a script or `:set` runs.
pub(crate) fn default_value(register: &Register) -> ferrowl_codec::Value {
    let zeros = vec![0u16; register.format().width()];
    register
        .decode(&zeros)
        .unwrap_or(ferrowl_codec::Value::Ascii(String::new()))
}

/// Parse user input (`:set`, Lua `write`) into a typed [`Value`](ferrowl_codec::Value),
/// attaching the register's display resolution (1.0 when the format has none).
pub(crate) fn str_to_value(s: &str, register: &Register) -> ferrowl_codec::Value {
    let res = register.format().resolution().map(|r| r.0).unwrap_or(1.0);
    crate::config::device::Scalar::from_input(s).to_value(res)
}

fn function_code(register: &Register) -> FunctionCode {
    use ferrowl_codec::Kind;
    match register.kind() {
        Kind::Coil => FunctionCode::ReadCoils,
        Kind::DiscreteInput => FunctionCode::ReadDiscreteInputs,
        Kind::HoldingRegister => FunctionCode::ReadHoldingRegisters,
        Kind::InputRegister => FunctionCode::ReadInputRegisters,
    }
}

/// Modbus per-request limits: 2000 bits for coils/discrete inputs, 125 words for registers.
const MAX_COILS_PER_READ: usize = 2000;
const MAX_REGISTERS_PER_READ: usize = 125;

fn read_limit(fc: FunctionCode) -> usize {
    match fc {
        FunctionCode::ReadCoils | FunctionCode::ReadDiscreteInputs => MAX_COILS_PER_READ,
        _ => MAX_REGISTERS_PER_READ,
    }
}

/// Stable grouping/sort key for the four readable function codes (others are not read).
fn fn_code_key(fc: FunctionCode) -> u8 {
    match fc {
        FunctionCode::ReadCoils => 1,
        FunctionCode::ReadDiscreteInputs => 2,
        FunctionCode::ReadHoldingRegisters => 3,
        FunctionCode::ReadInputRegisters => 4,
        _ => 0,
    }
}

/// Readable register spans grouped by `(slave, function-code key)`, each value carrying the
/// function code and a list of `(start, end)` spans. Used for both operation and memory planning.
type ReadableSpanGroups =
    std::collections::BTreeMap<(u8, u8), (FunctionCode, Kind, Vec<(usize, usize)>)>;

fn group_readable_spans(
    registers: &[(String, String, Register, Vec<NamedValue>)],
    include_write_only: bool,
) -> ReadableSpanGroups {
    let mut groups = std::collections::BTreeMap::new();
    for (_, _, register, _) in registers {
        if let Address::Fixed(addr) = register.address() {
            if !include_write_only && *register.access() == Access::WriteOnly {
                continue;
            }
            let fc = function_code(register);
            let start = *addr as usize;
            groups
                .entry((*register.slave_id(), fn_code_key(fc)))
                .or_insert_with(|| (fc, register.kind().clone(), Vec::new()))
                .2
                .push((start, start + register.format().width()));
        }
    }
    groups
}

/// Build batched read operations. For each `(slave, function code)`: if the device config defines
/// explicit ranges for that code they are used verbatim (gaps included), otherwise contiguous
/// registers are auto-merged. Either way batches are split so no request exceeds the Modbus
/// per-request limit (125 words / 2000 bits).
fn build_read_operations(
    registers: &[(String, String, Register, Vec<NamedValue>)],
    read_ranges: &ReadRanges,
) -> Vec<Operation> {
    let mut ops = Vec::new();
    for ((slave, _), (fc, kind, mut spans)) in group_readable_spans(registers, false) {
        let limit = read_limit(fc);
        spans.sort_unstable();

        // Collect sorted register boundaries before spans is consumed, so the emit loop can
        // snap split points back to a register boundary and never cut a register in half.
        let reg_starts: Vec<usize> = spans.iter().map(|&(s, _)| s).collect();
        let reg_ends: Vec<usize> = spans.iter().map(|&(_, e)| e).collect();

        let explicit = read_ranges.ranges_for(kind);
        let batches = if explicit.is_empty() {
            // No explicit ranges: Do not merge registers at all.
            spans
        } else {
            // Each explicit range groups the registers that fall inside it into a single read,
            // bridging the gaps *between* those registers but trimmed to their actual extent
            // (leading/trailing space inside the range is not read). Registers outside every
            // explicit range are auto-merged into their own requests.
            let mut windows: Vec<(usize, usize)> =
                explicit.iter().map(|r| (r.start, r.end)).collect();
            windows.sort_unstable();
            let windows = merge_spans(&windows);

            let mut bounds: Vec<Option<(usize, usize)>> = vec![None; windows.len()];
            let mut uncovered: Vec<(usize, usize)> = Vec::new();
            for &(s, e) in &spans {
                match windows.iter().position(|&(ws, we)| s < we && e > ws) {
                    Some(i) => {
                        let b = bounds[i].get_or_insert((s, e));
                        b.0 = b.0.min(s);
                        b.1 = b.1.max(e);
                    }
                    None => uncovered.push((s, e)),
                }
            }
            let mut batches: Vec<(usize, usize)> = bounds.into_iter().flatten().collect();
            uncovered.sort_unstable();
            batches.extend(&uncovered);
            batches.sort_unstable();
            batches
        };

        // Emit each batch, splitting so no request exceeds the protocol limit.
        // If the naive cut point falls inside a register, snap back to that register's start
        // so a register is never read in half (e.g. a U128 spanning 120-128 must not split at 125).
        // Cuts that land in gaps between registers are left as-is.
        for (start, end) in batches {
            let mut s = start;
            while s < end {
                let naive_e = (s + limit).min(end);
                let e = if naive_e < end {
                    // Find the last register whose start < naive_e.
                    let idx = reg_starts.partition_point(|&rs| rs < naive_e);
                    if idx > 0 && reg_ends[idx - 1] > naive_e {
                        // naive_e bisects register [reg_starts[idx-1], reg_ends[idx-1]); snap back.
                        reg_starts[idx - 1]
                    } else {
                        naive_e
                    }
                } else {
                    naive_e
                };
                ops.push(Operation {
                    slave_id: slave,
                    fn_code: fc,
                    range: Range::new(s, e - s),
                });
                s = e;
            }
        }
    }
    ops
}

/// For every function code with explicit read ranges, the gap cells (inside those ranges but not
/// backed by a register) that must be added to memory as `Read` so a batched read can be stored.
fn explicit_read_coverage(
    registers: &[(String, String, Register, Vec<NamedValue>)],
    read_ranges: &ReadRanges,
) -> Vec<(Key<SlaveKey>, MemKind, Range)> {
    let mut out = Vec::new();
    for ((slave, _), (_, kind, mut spans)) in group_readable_spans(registers, true) {
        let explicit = read_ranges.ranges_for(kind.clone());
        if explicit.is_empty() {
            continue;
        }
        spans.sort_unstable();
        let covered = merge_spans(&spans);
        let mem_type = match kind {
            Kind::Coil | Kind::DiscreteInput => CellType::Coil,
            Kind::HoldingRegister | Kind::InputRegister => CellType::Register,
        };
        let key = Key {
            id: SlaveKey {
                slave_id: slave,
                kind: kind.clone(),
            },
        };
        for r in &explicit {
            for gap in subtract_spans(r.start, r.end, &covered) {
                out.push((key.clone(), MemKind::Read(mem_type), gap));
            }
        }
    }
    out
}

/// Merge a sorted list of `(start, end)` spans into non-overlapping spans.
fn merge_spans(spans: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for &(s, e) in spans {
        match out.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => out.push((s, e)),
        }
    }
    out
}

/// Sub-intervals of `[start, end)` not covered by the (sorted, merged) `covered` spans.
fn subtract_spans(start: usize, end: usize, covered: &[(usize, usize)]) -> Vec<Range> {
    let mut gaps = Vec::new();
    let mut cur = start;
    for &(cs, ce) in covered {
        if ce <= cur || cs >= end {
            continue;
        }
        if cs > cur {
            gaps.push(Range::new(cur, cs - cur));
        }
        cur = cur.max(ce);
        if cur >= end {
            break;
        }
    }
    if cur < end {
        gaps.push(Range::new(cur, end - cur));
    }
    gaps
}

/// Open (append) the per-module log file for `base`, or clear the sink when `base` is `None` or
/// the file can't be opened.
fn open_sink(sink: &FileSink, base: Option<&str>, name: &str) {
    let writer = base.and_then(|base| {
        let path = module_log_path(base, name);
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
            .map(BufWriter::new)
    });
    if let Ok(mut guard) = sink.lock() {
        *guard = writer;
    }
}

/// `<stem>.<sanitized-name>.<ext>` (or `<base>.<name>` without an extension), next to `base`.
fn module_log_path(base: &str, name: &str) -> PathBuf {
    let sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let path = Path::new(base);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("ferrowl");
    let filename = match path.extension().and_then(|s| s.to_str()) {
        Some(ext) => format!("{stem}.{sanitized}.{ext}"),
        None => format!("{stem}.{sanitized}"),
    };
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(filename),
        _ => PathBuf::from(filename),
    }
}

/// Append a timestamped line to the file sink (if any), flushing so it's durable.
pub(crate) fn append(sink: &FileSink, line: &str) {
    if let Ok(mut guard) = sink.lock()
        && let Some(writer) = guard.as_mut()
    {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let ts = format_timestamp(ms);
        let _ = writeln!(writer, "[{ts}] {line}");
        let _ = writer.flush();
    }
}

/// Resolved per-instance timing (ms). Built by [`Module::resolve_timing`].
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub timeout_ms: usize,
    pub delay_ms: usize,
    pub interval_ms: usize,
}

fn endpoint_to_config(endpoint: &Endpoint, timing: &Timing) -> NetConfig {
    match endpoint {
        Endpoint::Tcp { ip, port } => NetConfig::Tcp(ferrowl_modbus::tcp::Config {
            ip: ip.clone(),
            port: *port,
            timeout_ms: timing.timeout_ms,
            delay_ms: timing.delay_ms,
            interval_ms: timing.interval_ms,
        }),
        Endpoint::Rtu {
            path,
            baud_rate,
            parity,
            data_bits,
            stop_bits,
        } => NetConfig::Rtu(ferrowl_modbus::rtu::Config {
            path: path.clone(),
            baud_rate: *baud_rate,
            slave: 0,
            parity: parity.clone(),
            data_bits: *data_bits,
            stop_bits: *stop_bits,
            timeout_ms: timing.timeout_ms,
            delay_ms: timing.delay_ms,
            interval_ms: timing.interval_ms,
        }),
    }
}

fn build_instance(
    role: Role,
    config: NetConfig,
    operations: Arc<RwLock<Vec<Operation>>>,
    memory: ModuleMemory,
) -> Instance<SlaveKey> {
    match (role, config) {
        (Role::Client, NetConfig::Tcp(cfg)) => Instance::with_tcp_client(ClientConfig {
            config: Arc::new(RwLock::new(cfg)),
            operations,
            memory,
        }),
        (Role::Server, NetConfig::Tcp(cfg)) => Instance::with_tcp_server(ServerConfig {
            config: Arc::new(RwLock::new(cfg)),
            memory,
        }),
        (Role::Client, NetConfig::Rtu(cfg)) => Instance::with_rtu_client(ClientConfig {
            config: Arc::new(RwLock::new(cfg)),
            operations,
            memory,
        }),
        (Role::Server, NetConfig::Rtu(cfg)) => Instance::with_rtu_server(ServerConfig {
            config: Arc::new(RwLock::new(cfg)),
            memory,
        }),
    }
}

#[cfg(test)]
mod tests {
    use ferrowl_codec::format::{BitField, Endian, Resolution};
    use ferrowl_codec::{Access, Address, Format, Kind, RegisterBuilder};
    use ferrowl_modbus::{Key, SlaveKey};
    use ferrowl_store::{CellKind as MemKind, CellType, Memory, Range};

    #[test]
    fn ut_module_log_path() {
        use super::module_log_path;
        assert_eq!(
            module_log_path("ferrowl.log", "evse-1"),
            std::path::PathBuf::from("ferrowl.evse-1.log")
        );
        assert_eq!(
            module_log_path("logs/run.log", "evse 1"),
            std::path::PathBuf::from("logs/run.evse_1.log")
        );
        assert_eq!(
            module_log_path("out", "m"),
            std::path::PathBuf::from("out.m")
        );
    }

    #[test]
    fn ut_resolve_timing_fallback() {
        use super::Module;
        use crate::config::device::{DEFAULT_DELAY_MS, DEFAULT_INTERVAL_MS, DEFAULT_TIMEOUT_MS};
        use crate::config::{DeviceConfig, Endpoint, ModuleSpec, Role};

        let mut spec = ModuleSpec {
            name: "m".into(),
            device: String::new(),
            role: Role::Server,
            endpoint: Endpoint::Tcp {
                ip: "127.0.0.1".into(),
                port: 502,
            },
            timeout_ms: None,
            delay_ms: None,
            interval_ms: None,
        };
        let mut device = DeviceConfig::default();

        // No spec/device values: built-in defaults.
        let timing = Module::resolve_timing(&spec, &device);
        assert_eq!(timing.timeout_ms, DEFAULT_TIMEOUT_MS);
        assert_eq!(timing.delay_ms, DEFAULT_DELAY_MS);
        assert_eq!(timing.interval_ms, DEFAULT_INTERVAL_MS);

        // Device values beat the defaults.
        device.timeout_ms = Some(2000);
        device.delay_ms = Some(500);
        let timing = Module::resolve_timing(&spec, &device);
        assert_eq!(timing.timeout_ms, 2000);
        assert_eq!(timing.delay_ms, 500);
        assert_eq!(timing.interval_ms, DEFAULT_INTERVAL_MS);

        // Spec overrides beat the device values.
        spec.timeout_ms = Some(100);
        let timing = Module::resolve_timing(&spec, &device);
        assert_eq!(timing.timeout_ms, 100);
        assert_eq!(timing.delay_ms, 500);
    }

    fn entry(
        slave: u8,
        kind: Kind,
        addr: u16,
        format: Format,
        access: Access,
    ) -> (
        String,
        String,
        ferrowl_codec::Register,
        Vec<crate::config::device::NamedValue>,
    ) {
        let register = RegisterBuilder::default()
            .slave_id(slave)
            .access(access)
            .kind(kind)
            .address(Address::Fixed(addr))
            .format(format)
            .build()
            .unwrap();
        (String::new(), String::new(), register, vec![])
    }

    fn u16reg(
        slave: u8,
        kind: Kind,
        addr: u16,
        access: Access,
    ) -> (
        String,
        String,
        ferrowl_codec::Register,
        Vec<crate::config::device::NamedValue>,
    ) {
        entry(
            slave,
            kind,
            addr,
            Format::U16((Endian::Big, Resolution(1.0), BitField::default())),
            access,
        )
    }

    #[test]
    fn ut_str_to_value_uses_register_resolution() {
        use super::str_to_value;
        use ferrowl_codec::Value;

        let reg = entry(
            1,
            Kind::HoldingRegister,
            0,
            Format::U16((Endian::Big, Resolution(0.5), BitField::default())),
            Access::ReadWrite,
        )
        .2;

        // Integer and float inputs carry the register's resolution.
        match str_to_value("10", &reg) {
            Value::I64((v, r)) => {
                assert_eq!(v, 10);
                assert_eq!(r.0, 0.5);
            }
            other => panic!("expected I64, got {other:?}"),
        }
        match str_to_value("1.5", &reg) {
            Value::F64((v, r)) => {
                assert_eq!(v, 1.5);
                assert_eq!(r.0, 0.5);
            }
            other => panic!("expected F64, got {other:?}"),
        }
        // Non-numeric input falls through to ASCII.
        match str_to_value("idle", &reg) {
            Value::Ascii(s) => assert_eq!(s, "idle"),
            other => panic!("expected Ascii, got {other:?}"),
        }
    }

    #[test]
    fn ut_build_read_operations_batches() {
        use super::build_read_operations;
        use crate::config::device::ReadRanges;
        use ferrowl_modbus::FunctionCode;
        let mut read_ranges = ReadRanges {
            holding: Some("0-2".to_string()),
            ..Default::default()
        };

        // Contiguous holding registers 0,1,2 merge into one request because of read ranges;
        // a 4th at 5 stays separate (gap is never read). A write-only register is excluded entirely.
        let regs = vec![
            u16reg(1, Kind::HoldingRegister, 0, Access::ReadWrite),
            u16reg(1, Kind::HoldingRegister, 1, Access::ReadOnly),
            u16reg(1, Kind::HoldingRegister, 2, Access::ReadWrite),
            u16reg(1, Kind::HoldingRegister, 5, Access::ReadOnly),
            u16reg(1, Kind::HoldingRegister, 3, Access::WriteOnly),
        ];
        let ops = build_read_operations(&regs, &read_ranges);
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].fn_code, FunctionCode::ReadHoldingRegisters);
        assert_eq!((ops[0].range.start, ops[0].range.end), (0, 3));
        assert_eq!((ops[1].range.start, ops[1].range.end), (5, 6));

        // Different function codes never merge even at the same address.
        let regs = vec![
            u16reg(1, Kind::Coil, 0, Access::ReadOnly),
            u16reg(1, Kind::HoldingRegister, 0, Access::ReadOnly),
        ];
        let ops = build_read_operations(&regs, &read_ranges);
        assert_eq!(ops.len(), 2);

        read_ranges.holding = Some("0-150".to_string());
        // Contiguous span exceeding the 125-register limit splits. 16 contiguous U128 (8 words
        // each) cover [0,128): 15 fit in [0,120), the 16th opens a new request at [120,128).
        let regs: Vec<_> = (0..16)
            .map(|i| {
                entry(
                    1,
                    Kind::HoldingRegister,
                    i * 8,
                    Format::U128((Endian::Big, Resolution(1.0), BitField::default())),
                    Access::ReadOnly,
                )
            })
            .collect();
        let ops = build_read_operations(&regs, &read_ranges);
        assert_eq!(ops.len(), 2);
        assert_eq!((ops[0].range.start, ops[0].range.end), (0, 120));
        assert_eq!((ops[1].range.start, ops[1].range.end), (120, 128));
    }

    #[test]
    fn ut_explicit_read_ranges() {
        use super::build_read_operations;
        use crate::config::device::ReadRanges;

        // Registers at 20-25 and 30-35 inside range "0-100": one read trimmed to the registers'
        // extent (20-35), bridging the gap between them but not the empty 0-20 / 35-100.
        let regs = vec![
            entry(
                1,
                Kind::HoldingRegister,
                20,
                Format::U128((Endian::Big, Resolution(1.0), BitField::default())), // width 8 -> 20..28
                Access::ReadWrite,
            ),
            entry(
                1,
                Kind::HoldingRegister,
                30,
                Format::U16((Endian::Big, Resolution(1.0), BitField::default())), // 30..31
                Access::ReadWrite,
            ),
        ];
        let ranges = ReadRanges {
            holding: Some("0-100".to_string()),
            ..Default::default()
        };
        let ops = build_read_operations(&regs, &ranges);
        assert_eq!(ops.len(), 1);
        assert_eq!((ops[0].range.start, ops[0].range.end), (20, 31));

        // A bridged bounding span exceeding the limit is split into limit-sized requests.
        let regs = vec![
            u16reg(1, Kind::HoldingRegister, 0, Access::ReadWrite),
            u16reg(1, Kind::HoldingRegister, 200, Access::ReadWrite),
        ];
        let wide = ReadRanges {
            holding: Some("0-300".to_string()),
            ..Default::default()
        };
        let ops = build_read_operations(&regs, &wide);
        assert_eq!(ops.len(), 2);
        assert_eq!((ops[0].range.start, ops[0].range.end), (0, 125));
        assert_eq!((ops[1].range.start, ops[1].range.end), (125, 201));

        // A register outside every explicit range is still read, in its own request.
        let regs = vec![
            u16reg(1, Kind::HoldingRegister, 0, Access::ReadWrite),
            u16reg(1, Kind::HoldingRegister, 2, Access::ReadWrite),
            u16reg(1, Kind::HoldingRegister, 50, Access::ReadWrite),
        ];
        let small = ReadRanges {
            holding: Some("0-3".to_string()),
            ..Default::default()
        };
        let ops = build_read_operations(&regs, &small);
        let mut got: Vec<_> = ops.iter().map(|o| (o.range.start, o.range.end)).collect();
        got.sort_unstable();
        // Registers 0 and 2 bridge to [0,3); register 50 reads alone.
        assert_eq!(got, vec![(0, 3), (50, 51)]);
    }

    // Replicates the server `:set`/edit write path + the table decode read path.
    #[test]
    fn ut_server_value_write_roundtrip() {
        let mut memory: Memory<Key<SlaveKey>> = Memory::default();
        let key = Key {
            id: SlaveKey {
                slave_id: 1u8,
                kind: Kind::HoldingRegister,
            },
        };
        memory.add_ranges(
            key.clone(),
            &MemKind::ReadWrite(CellType::Register),
            &[Range::new(0, 1)],
        );

        let register = RegisterBuilder::default()
            .slave_id(1u8)
            .access(Access::ReadWrite)
            .kind(Kind::HoldingRegister)
            .address(Address::Fixed(0))
            .format(Format::U16((
                Endian::Big,
                Resolution(1.0),
                BitField::default(),
            )))
            .build()
            .unwrap();

        let raw = register.encode("50").unwrap();
        assert!(
            memory
                .write(
                    key.clone(),
                    &CellType::Register,
                    &Range::new(0, raw.len()),
                    &raw
                )
                .is_ok(),
            "write should succeed for a Combined register cell"
        );

        let read = memory
            .read(
                key,
                &CellType::Register,
                &Range::new(0, register.format().width()),
            )
            .expect("read should succeed");
        assert_eq!(read, vec![50]);
        assert_eq!(format!("{}", register.decode(&read).unwrap()), "50");
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
            read_code: 4,
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
        }
    }

    #[test]
    fn ut_module_new_tcp_server_and_sync_accessors() {
        use super::Module;
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
            timeout_ms: None,
            delay_ms: None,
            interval_ms: None,
        };

        let mut module = Module::new(&spec, &device);
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

        // Log-base reconfiguration: clear, then point at a fresh (unwritable) path.
        module.set_log_base(None);
        module.set_log_base(Some("/no/such/ferrowl/dir/base.log"));
    }

    #[test]
    fn ut_module_new_rtu_client() {
        use super::Module;
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
            timeout_ms: Some(750),
            delay_ms: Some(10),
            interval_ms: Some(2000),
        };

        let module = Module::new(&spec, &device);
        assert_eq!(module.registers().len(), 2);
        assert!(!module.is_instance_active());
    }
}
