use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ferrowl_mem::{Kind as MemKind, Memory, Range};
use ferrowl_net::{Config as NetConfig, FunctionCode, Key, Operation};
use ferrowl_reg::{Address, Register};
use tokio::sync::RwLock;

use crate::app::LogRing;
use crate::config::{AppConfig, DeviceConfig, Endpoint, ModuleSpec, Role, device::{AccessCfg, NamedValue}};
use crate::instance::Instance;
use crate::instance::config::{ClientConfig, ServerConfig};
use crate::instance::error::Error;
use crate::lua::{SimHandle, run_sim};

/// Memory/key id type (the modbus "device" id `T`). A single instance uses one id; memory
/// is keyed by `Key { id, slave_id }`.
type Id = u8;

pub type ModuleMemory = Arc<RwLock<Memory<Key<Id>>>>;
pub type ModuleLog = Arc<RwLock<LogRing>>;
/// Optional per-module log file, shared into the log/status callbacks; swappable at runtime so
/// `:log` takes effect on already-running modules.
pub type FileSink = Arc<Mutex<Option<BufWriter<std::fs::File>>>>;

/// One module instance: a modbus client (reads an external server) or server (simulates a
/// device), plus its register set, shared memory and ring log.
pub struct Module {
    name: String,
    id: Id,
    instance: Instance<Id>,
    registers: Vec<(String, String, Register, Vec<NamedValue>)>,
    memory: ModuleMemory,
    log: ModuleLog,
    file_sink: FileSink,
    /// Per-register `update` Lua snippets (register name → code), run on the sim thread.
    scripts: Vec<(String, String)>,
    /// Simulation cycle period, derived from `AppConfig::interval_ms`.
    sim_interval: Duration,
    /// The running simulation thread, if any (started in `start`, stopped in `stop`).
    sim: Option<SimHandle>,
}

impl Module {
    /// Build a module from an instance spec, its device-type config and global timing.
    pub fn new(spec: &ModuleSpec, device: &DeviceConfig, app: &AppConfig) -> Self {
        let id: Id = 0;

        let mut memory = Memory::<Key<Id>>::default();
        let mut operations: Vec<Operation> = Vec::new();
        let mut registers: Vec<(String, String, Register, Vec<NamedValue>)> = Vec::new();
        let mut scripts: Vec<(String, String)> = Vec::new();

        for (name, def) in &device.definitions {
            registers.push((name.clone(), def.comment.clone(), def.register(), def.values.clone()));
            if let Some(code) = &def.update
                && !code.trim().is_empty()
            {
                scripts.push((name.clone(), code.clone()));
            }
            if let Some(range) = def.mem_range() {
                let key = Key {
                    id,
                    slave_id: def.slave_id,
                };
                let mem_kind = match def.access {
                    AccessCfg::ReadOnly => MemKind::Read(def.mem_type()),
                    AccessCfg::WriteOnly => MemKind::Write(def.mem_type()),
                    AccessCfg::ReadWrite => MemKind::Combined(def.mem_type()),
                };
                memory.add_ranges(key, &mem_kind, std::slice::from_ref(&range));
                operations.push(Operation {
                    slave_id: def.slave_id,
                    fn_code: def.function_code(),
                    range,
                });
            }
        }

        let memory: ModuleMemory = Arc::new(RwLock::new(memory));
        let operations = Arc::new(RwLock::new(operations));
        let log: ModuleLog = Arc::new(RwLock::new(LogRing::init()));

        let file_sink: FileSink = Arc::new(Mutex::new(None));
        open_sink(&file_sink, app.log_file.as_deref(), &spec.name);

        let net_config = endpoint_to_config(&spec.endpoint, app);
        let instance = build_instance(id, spec.role, net_config, operations, memory.clone());

        Self {
            name: spec.name.clone(),
            id,
            instance,
            registers,
            memory,
            log,
            file_sink,
            scripts,
            sim_interval: Duration::from_millis(app.interval_ms.max(1) as u64),
            sim: None,
        }
    }

    /// (Re)point this module's log file at `base` (None disables file logging). The filename is
    /// `<stem>.<tab-name>.<ext>` next to `base`. Takes effect on already-running modules.
    pub fn set_log_base(&self, base: Option<&str>) {
        open_sink(&self.file_sink, base, &self.name);
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn id(&self) -> Id {
        self.id
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
                async move |s: String| {
                    log.write().await.write(&s);
                    append(&log_sink, &s);
                },
                async move |s: String| {
                    let line = format!("[status] {s}");
                    status.write().await.write(&line);
                    append(&status_sink, &line);
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
            self.id,
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

    /// Send a write command to the underlying client (errors for servers / when stopped).
    pub async fn send_command(&self, command: ferrowl_net::Command) -> Result<(), Error> {
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
        app: &AppConfig,
    ) -> Result<(), Error> {
        // Best-effort stop of any running instance and its simulation thread; the caller is
        // expected to `start()` afterwards, which restarts the sim.
        self.stop_sim();
        let _ = self.instance.stop().await;

        let mut operations: Vec<Operation> = Vec::new();
        for (_, _, register, _) in &self.registers {
            if let Address::Fixed(addr) = register.address() {
                operations.push(Operation {
                    slave_id: *register.slave_id(),
                    fn_code: function_code(register),
                    range: Range::new(*addr as usize, register.format().width()),
                });
            }
        }

        let operations = Arc::new(RwLock::new(operations));
        let net_config = endpoint_to_config(endpoint, app);
        self.instance = build_instance(self.id, role, net_config, operations, self.memory.clone());
        Ok(())
    }
}

fn function_code(register: &Register) -> FunctionCode {
    use ferrowl_reg::Kind;
    match register.kind() {
        Kind::Coil => FunctionCode::ReadCoils,
        Kind::DiscreteInput => FunctionCode::ReadDiscreteInputs,
        Kind::HoldingRegister => FunctionCode::ReadHoldingRegisters,
        Kind::InputRegister => FunctionCode::ReadInputRegisters,
    }
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

/// Append a line to the file sink (if any), flushing so it's durable.
pub(crate) fn append(sink: &FileSink, line: &str) {
    if let Ok(mut guard) = sink.lock()
        && let Some(writer) = guard.as_mut()
    {
        let _ = writeln!(writer, "{line}");
        let _ = writer.flush();
    }
}

fn endpoint_to_config(endpoint: &Endpoint, app: &AppConfig) -> NetConfig {
    match endpoint {
        Endpoint::Tcp { ip, port } => NetConfig::Tcp(ferrowl_net::tcp::Config {
            ip: ip.clone(),
            port: *port,
            timeout_ms: app.timeout_ms,
            delay_ms: app.delay_ms,
            interval_ms: app.interval_ms,
        }),
        Endpoint::Rtu {
            path,
            baud_rate,
            parity,
            data_bits,
            stop_bits,
        } => NetConfig::Rtu(ferrowl_net::rtu::Config {
            path: path.clone(),
            baud_rate: *baud_rate,
            slave: 0,
            parity: parity.clone(),
            data_bits: *data_bits,
            stop_bits: *stop_bits,
            timeout_ms: app.timeout_ms,
            delay_ms: app.delay_ms,
            interval_ms: app.interval_ms,
        }),
    }
}

fn build_instance(
    id: Id,
    role: Role,
    config: NetConfig,
    operations: Arc<RwLock<Vec<Operation>>>,
    memory: ModuleMemory,
) -> Instance<Id> {
    match (role, config) {
        (Role::Client, NetConfig::Tcp(cfg)) => Instance::with_tcp_client(ClientConfig {
            id,
            config: Arc::new(RwLock::new(cfg)),
            operations,
            memory,
        }),
        (Role::Server, NetConfig::Tcp(cfg)) => Instance::with_tcp_server(ServerConfig {
            id,
            config: Arc::new(RwLock::new(cfg)),
            memory,
        }),
        (Role::Client, NetConfig::Rtu(cfg)) => Instance::with_rtu_client(ClientConfig {
            id,
            config: Arc::new(RwLock::new(cfg)),
            operations,
            memory,
        }),
        (Role::Server, NetConfig::Rtu(cfg)) => Instance::with_rtu_server(ServerConfig {
            id,
            config: Arc::new(RwLock::new(cfg)),
            memory,
        }),
    }
}

#[cfg(test)]
mod tests {
    use ferrowl_mem::{Kind as MemKind, Memory, Range, Type};
    use ferrowl_net::Key;
    use ferrowl_reg::format::{Endian, Resolution};
    use ferrowl_reg::{Access, Address, Format, Kind, RegisterBuilder};

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

    // Replicates the server `:set`/edit write path + the table decode read path.
    #[test]
    fn ut_server_value_write_roundtrip() {
        let mut memory: Memory<Key<u8>> = Memory::default();
        let key = Key {
            id: 0u8,
            slave_id: 1u8,
        };
        memory.add_ranges(
            key.clone(),
            &MemKind::Combined(Type::Register),
            &[Range::new(0, 1)],
        );

        let register = RegisterBuilder::default()
            .slave_id(1u8)
            .access(Access::ReadWrite)
            .kind(Kind::HoldingRegister)
            .address(Address::Fixed(0))
            .format(Format::U16((Endian::Big, Resolution(1.0))))
            .build()
            .unwrap();

        let raw = register.encode("50").unwrap();
        assert!(
            memory.write(
                key.clone(),
                &Type::Register,
                &Range::new(0, raw.len()),
                &raw
            ),
            "write should succeed for a Combined register cell"
        );

        let read = memory
            .read(
                key,
                &Type::Register,
                &Range::new(0, register.format().width()),
            )
            .expect("read should succeed");
        assert_eq!(read, vec![50]);
        assert_eq!(format!("{}", register.decode(&read).unwrap()), "50");
    }
}
