//! Per-module Lua simulation: a `RegisterBridge` exposes the module's `Memory` to Lua as the
//! `C_Register` global (`Get*`/`Set(name, value)`), and `run_sim` drives every register's
//! `update` script on a dedicated thread (because `mlua::Lua` is `!Send`).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ferrowl_lua::module::{Read, RegisterModule, TimeModule, ValueType, Write};
use ferrowl_lua::{ContextBuilder, Error, Result};
use ferrowl_mem::Range;
use ferrowl_net::{Key, SlaveKind};
use ferrowl_reg::{Address, Register, Value};

use crate::module::{FileSink, ModuleLog, ModuleMemory, VirtualStore, append};

/// Bridges Lua register access (`C_Register`) to the module's shared `Memory` (fixed-address
/// registers) and `VirtualStore` (virtual registers). Runs on the dedicated simulation thread, so
/// the tokio `RwLock`s are locked with their `blocking_*` ops (safe off a runtime worker thread).
pub struct RegisterBridge {
    memory: ModuleMemory,
    virtual_store: VirtualStore,
    registers: HashMap<String, Register>,
}

impl RegisterBridge {
    pub fn new(
        memory: ModuleMemory,
        virtual_store: VirtualStore,
        registers: HashMap<String, Register>,
    ) -> Self {
        Self {
            memory,
            virtual_store,
            registers,
        }
    }

    fn register(&self, name: &str) -> Result<&Register> {
        self.registers
            .get(name)
            .ok_or_else(|| Error::RuntimeError(format!("unknown register '{name}'")))
    }
}

/// Infer a Lua `ValueType` from a virtual register's stored string (int, then float, else string).
fn parse_value_type(s: &str) -> ValueType {
    let t = s.trim();
    if let Ok(i) = t.parse::<i128>() {
        ValueType::Int(i)
    } else if let Ok(f) = t.parse::<f64>() {
        ValueType::Float(f)
    } else {
        ValueType::String(s.to_string())
    }
}

impl Read for RegisterBridge {
    fn read(&self, name: String) -> Result<ValueType> {
        let register = self.register(&name)?;
        let addr = match register.address() {
            Address::Fixed(addr) => *addr,
            Address::Virtual => {
                return self
                    .virtual_store
                    .blocking_read()
                    .get(&name)
                    .map(|s| parse_value_type(s))
                    .ok_or_else(|| {
                        Error::RuntimeError(format!("virtual register '{name}' not set"))
                    });
            }
        };
        let width = register.format().width();
        let key = Key {
            id: SlaveKind {
                slave_id: *register.slave_id(),
                kind: register.kind().clone(),
            },
        };
        let raw = self
            .memory
            .blocking_read()
            .read_unchecked(key, &Range::new(addr as usize, width))
            .ok_or_else(|| Error::RuntimeError(format!("register '{name}' not readable")))?;
        let value = register
            .decode(&raw)
            .map_err(|e| Error::RuntimeError(format!("decode '{name}': {e}")))?;
        Ok(value_to_type(value))
    }
}

impl Write for RegisterBridge {
    fn write(&self, name: String, value: String) -> Result<()> {
        let register = self.register(&name)?;
        let addr = match register.address() {
            Address::Fixed(addr) => *addr,
            Address::Virtual => {
                self.virtual_store.blocking_write().insert(name, value);
                return Ok(());
            }
        };
        let raw = register
            .encode(&value)
            .map_err(|e| Error::RuntimeError(format!("encode '{name}': {e}")))?;
        let key = Key {
            id: SlaveKind {
                slave_id: *register.slave_id(),
                kind: register.kind().clone(),
            },
        };
        let ok = self.memory.blocking_write().write_unchecked(
            key,
            &Range::new(addr as usize, raw.len()),
            &raw,
        );
        if ok {
            Ok(())
        } else {
            Err(Error::RuntimeError(format!(
                "write to '{name}' rejected (not writable)"
            )))
        }
    }
}

/// Map a decoded register `Value` to the Lua `ValueType`, exposing the raw (unscaled) stored
/// number so that a subsequent `Set` round-trips through `Register::encode`.
fn value_to_type(value: Value) -> ValueType {
    match value {
        Value::U8((v, _)) => ValueType::Int(v as i128),
        Value::U16((v, _)) => ValueType::Int(v as i128),
        Value::U32((v, _)) => ValueType::Int(v as i128),
        Value::U64((v, _)) => ValueType::Int(v as i128),
        Value::U128((v, _)) => ValueType::Int(v as i128),
        Value::I8((v, _)) => ValueType::Int(v as i128),
        Value::I16((v, _)) => ValueType::Int(v as i128),
        Value::I32((v, _)) => ValueType::Int(v as i128),
        Value::I64((v, _)) => ValueType::Int(v as i128),
        Value::I128((v, _)) => ValueType::Int(v),
        Value::F32((v, _)) => ValueType::Float(v as f64),
        Value::F64((v, _)) => ValueType::Float(v),
        Value::Ascii(s) => ValueType::String(s),
    }
}

/// Controls a running simulation thread: setting the flag and joining the handle stops it.
pub struct SimHandle {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl SimHandle {
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for SimHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Spawn the simulation thread for a module. Returns `None` when there are no `update` scripts.
/// The Lua `Context` is built inside the thread because it is `!Send`. Each cycle runs every
/// script via `call_all`; script errors are surfaced into the module log (ring + file).
#[allow(clippy::too_many_arguments)]
pub fn run_sim(
    memory: ModuleMemory,
    virtual_store: VirtualStore,
    registers: HashMap<String, Register>,
    scripts: Vec<(String, String)>,
    interval: Duration,
    log: ModuleLog,
    sink: FileSink,
) -> Option<SimHandle> {
    if scripts.is_empty() {
        return None;
    }

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let handle = std::thread::spawn(move || {
        let bridge = RegisterBridge::new(memory, virtual_store, registers);
        let mut builder = ContextBuilder::<String>::default()
            .with_stdlib()
            .with_module(RegisterModule::init(bridge))
            .with_module(TimeModule::default());
        for (name, code) in &scripts {
            builder = builder.with_script(name.clone(), code);
        }
        let mut context = match builder.build() {
            Ok(context) => context,
            Err(e) => {
                emit(
                    &log,
                    &sink,
                    &format!("[sim] failed to build Lua context: {e}"),
                );
                return;
            }
        };

        while !thread_stop.load(Ordering::Relaxed) {
            if let Err(errors) = context.call_all(Duration::ZERO) {
                for e in errors {
                    emit(&log, &sink, &format!("[sim] {e}"));
                }
            }
            sleep_responsive(interval, &thread_stop);
        }
    });

    Some(SimHandle {
        stop,
        handle: Some(handle),
    })
}

/// Append a line to the module's ring log and file sink from the (non-runtime) sim thread.
fn emit(log: &ModuleLog, sink: &FileSink, line: &str) {
    log.blocking_write().write(line);
    append(sink, line);
}

/// Sleep up to `interval` in small chunks so the stop flag is observed promptly.
fn sleep_responsive(interval: Duration, stop: &AtomicBool) {
    let chunk = Duration::from_millis(50);
    let mut slept = Duration::ZERO;
    while slept < interval && !stop.load(Ordering::Relaxed) {
        let step = chunk.min(interval - slept);
        std::thread::sleep(step);
        slept += step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrowl_mem::{Kind as MemKind, Memory, Type};
    use ferrowl_net::SlaveKind;
    use ferrowl_reg::format::{Endian, Resolution};
    use ferrowl_reg::{Access, Format, Kind, RegisterBuilder};
    use tokio::sync::RwLock;

    fn holding(addr: u16) -> Register {
        RegisterBuilder::default()
            .slave_id(1u8)
            .access(Access::ReadWrite)
            .kind(Kind::HoldingRegister)
            .address(Address::Fixed(addr))
            .format(Format::U16((Endian::Big, Resolution(1.0))))
            .build()
            .unwrap()
    }

    /// Memory holding two U16 registers (setpoint@0, power@1), both read/write.
    fn evse_memory() -> ModuleMemory {
        let mut memory: Memory<Key<SlaveKind>> = Memory::default();
        let key = Key {
            id: SlaveKind {
                slave_id: 1u8,
                kind: Kind::HoldingRegister,
            },
        };
        memory.add_ranges(
            key,
            &MemKind::ReadWrite(Type::Register),
            &[Range::new(0, 2)],
        );
        Arc::new(RwLock::new(memory))
    }

    fn evse_registers() -> HashMap<String, Register> {
        let mut registers = HashMap::new();
        registers.insert("setpoint".to_string(), holding(0));
        registers.insert("power".to_string(), holding(1));
        registers
    }

    fn vstore() -> VirtualStore {
        Arc::new(RwLock::new(HashMap::new()))
    }

    #[test]
    fn ut_bridge_virtual_register_roundtrip() {
        let virtual_store = vstore();
        let mut registers = evse_registers();
        registers.insert(
            "calc".to_string(),
            RegisterBuilder::default()
                .slave_id(1u8)
                .access(Access::ReadWrite)
                .kind(Kind::HoldingRegister)
                .address(ferrowl_reg::Address::Virtual)
                .format(Format::U16((Endian::Big, Resolution(1.0))))
                .build()
                .unwrap(),
        );
        let bridge = RegisterBridge::new(evse_memory(), virtual_store.clone(), registers);

        // Reading before any write errors; after a write the value round-trips via the store.
        assert!(bridge.read("calc".to_string()).is_err());
        bridge
            .write("calc".to_string(), "7".to_string())
            .expect("virtual write");
        match bridge.read("calc".to_string()).expect("virtual read") {
            ValueType::Int(v) => assert_eq!(v, 7),
            _ => panic!("expected Int"),
        }
        assert_eq!(
            virtual_store
                .blocking_read()
                .get("calc")
                .map(String::as_str),
            Some("7")
        );
    }

    #[test]
    fn ut_bridge_write_then_read() {
        let bridge = RegisterBridge::new(evse_memory(), vstore(), evse_registers());
        bridge
            .write("setpoint".to_string(), "100".to_string())
            .expect("write");
        match bridge.read("setpoint".to_string()).expect("read") {
            ValueType::Int(v) => assert_eq!(v, 100),
            _ => panic!("expected Int"),
        }
    }

    #[test]
    fn ut_bridge_unknown_register_errors() {
        let bridge = RegisterBridge::new(evse_memory(), vstore(), evse_registers());
        assert!(bridge.read("nope".to_string()).is_err());
        assert!(bridge.write("nope".to_string(), "1".to_string()).is_err());
    }

    // Mirrors `run_sim`'s body once: a `power` update copying `setpoint` reflects after a cycle.
    #[test]
    fn ut_sim_script_mirrors_register() {
        let memory = evse_memory();
        let bridge = RegisterBridge::new(memory.clone(), vstore(), evse_registers());
        bridge
            .write("setpoint".to_string(), "42".to_string())
            .expect("seed setpoint");

        let mut context = ContextBuilder::<String>::default()
            .with_stdlib()
            .with_module(RegisterModule::init(bridge))
            .with_module(TimeModule::default())
            .with_script(
                "power".to_string(),
                "C_Register:Set(\"power\", C_Register:GetInt(\"setpoint\"))",
            )
            .build()
            .expect("build context");

        context.call_all(Duration::ZERO).expect("run script");

        let power = memory
            .blocking_read()
            .read(
                Key {
                    id: SlaveKind {
                        slave_id: 1,
                        kind: Kind::HoldingRegister,
                    },
                },
                &Type::Register,
                &Range::new(1, 1),
            )
            .expect("read power");
        assert_eq!(power, vec![42]);
    }
}
