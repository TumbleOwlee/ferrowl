//! Per-module Lua simulation: a `RegisterBridge` exposes the module's `Memory` to Lua as the
//! `C_Register` global (`Get(name)`/`Set(name, value)`), and `run_sim` drives every register's
//! `update` script on a dedicated thread (because `mlua::Lua` is `!Send`).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ferrowl_codec::{Address, Format, Register, Value};
use ferrowl_lua::module::{LogModule, LogSink, Read, RegisterModule, TimeModule, ValueType, Write};
use ferrowl_lua::{ContextBuilder, Error, Result};
use ferrowl_modbus::{Key, SlaveKey};
use ferrowl_store::Range;

use crate::module::modbus::{FileSink, ModuleLog, ModuleMemory, VirtualStore, append};

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
                    .map(|v| value_to_type(v.clone()))
                    .ok_or_else(|| {
                        Error::RuntimeError(format!("virtual register '{name}' not set"))
                    });
            }
        };
        let width = register.format().width();
        let key = Key {
            id: SlaveKey {
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
    fn write(&self, name: String, value: ValueType) -> Result<()> {
        let register = self.register(&name)?;
        let addr = match register.address() {
            Address::Fixed(addr) => *addr,
            Address::Virtual => {
                let value = virtual_value_from_type(value, register)?;
                self.virtual_store.blocking_write().insert(name, value);
                return Ok(());
            }
        };
        let raw = match value {
            // A Lua string is already a string — no round-trip to avoid, so this still goes
            // through the shared `encode`/`:set` string path.
            ValueType::String(s) => register
                .encode(&s)
                .map_err(|e| Error::RuntimeError(format!("encode '{name}': {e}")))?,
            _ => {
                let typed = typed_value_from_type(value, register.format())?;
                register
                    .encode_value(&typed)
                    .map_err(|e| Error::RuntimeError(format!("encode '{name}': {e}")))?
            }
        };
        let key = Key {
            id: SlaveKey {
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

/// Converts a Lua `ValueType` into a codec [`Value`] for a virtual register, mirroring
/// `str_to_value`'s `Scalar`-based semantics (an `Int`/`Float`/`Bool` is stored as `I64`/`F64`
/// regardless of the register's declared format — virtual registers ignore it) without the
/// string round-trip `str_to_value` requires for genuinely string input.
fn virtual_value_from_type(value: ValueType, register: &Register) -> Result<Value> {
    let res = register.format().resolution().unwrap_or_default();
    match value {
        ValueType::Nil => Err(Error::RuntimeError("cannot Set nil value".to_string())),
        ValueType::String(s) => Ok(crate::module::modbus::str_to_value(&s, register)),
        ValueType::Bool(b) => Ok(Value::I64((b as i64, res))),
        ValueType::Int(v) => Ok(match i64::try_from(v) {
            Ok(v) => Value::I64((v, res)),
            // Mirrors `Scalar::from_input`'s fallback for an out-of-i64-range literal: it fails
            // to parse as `i64` and is retried as `f64`.
            Err(_) => Value::F64((v as f64, res)),
        }),
        ValueType::Float(v) => Ok(Value::F64((v, res))),
    }
}

/// Converts a Lua `ValueType` into the codec [`Value`] variant `format` expects, for the
/// fixed-address `encode_value` path. `Nil` errors cleanly instead of round-tripping through the
/// literal string `"nil"`; `Int` is range-checked against the target integer width instead of
/// silently truncating.
fn typed_value_from_type(value: ValueType, format: &Format) -> Result<Value> {
    match value {
        ValueType::Nil => Err(Error::RuntimeError("cannot Set nil value".to_string())),
        ValueType::String(_) => unreachable!("String is handled by the caller via `encode`"),
        ValueType::Bool(b) => int_value_for_format(b as i128, format),
        ValueType::Int(v) => int_value_for_format(v, format),
        ValueType::Float(v) => float_value_for_format(v, format),
    }
}

/// Builds the codec [`Value`] variant `format` expects from an integer, range-checking against
/// the target width. Mirrors the string path's rule for non-integer formats: any integer is a
/// valid float (`v as f32/f64`), and stringifies verbatim onto ASCII.
fn int_value_for_format(v: i128, format: &Format) -> Result<Value> {
    let res = format.resolution().unwrap_or_default();
    macro_rules! int_variant {
        ($variant:ident, $ty:ty) => {
            <$ty>::try_from(v)
                .map(|val| Value::$variant((val, res.clone())))
                .map_err(|_| {
                    Error::RuntimeError(format!("value {v} out of range for format {format}"))
                })
        };
    }
    match format {
        Format::U8(_) => int_variant!(U8, u8),
        Format::U16(_) => int_variant!(U16, u16),
        Format::U32(_) => int_variant!(U32, u32),
        Format::U64(_) => int_variant!(U64, u64),
        Format::U128(_) => u128::try_from(v)
            .map(|val| Value::U128((val, res.clone())))
            .map_err(|_| {
                Error::RuntimeError(format!("value {v} out of range for format {format}"))
            }),
        Format::I8(_) => int_variant!(I8, i8),
        Format::I16(_) => int_variant!(I16, i16),
        Format::I32(_) => int_variant!(I32, i32),
        Format::I64(_) => int_variant!(I64, i64),
        Format::I128(_) => Ok(Value::I128((v, res))),
        Format::F32(_) => Ok(Value::F32((v as f32, res))),
        Format::F64(_) => Ok(Value::F64((v as f64, res))),
        Format::Ascii(_) => Ok(Value::Ascii(v.to_string())),
    }
}

/// Builds the codec [`Value`] variant `format` expects from a float. Mirrors the string path's
/// rule for integer formats: only a whole number in range converts; a fractional or non-finite
/// value errors cleanly instead (the string path would have failed the same conversion via a
/// confusing `ParseIntError`, since `v.to_string()` of e.g. `3.5` isn't valid integer syntax).
fn float_value_for_format(v: f64, format: &Format) -> Result<Value> {
    let res = format.resolution().unwrap_or_default();
    macro_rules! float_int_variant {
        ($variant:ident, $ty:ty) => {{
            if !v.is_finite() || v.fract() != 0.0 {
                Err(Error::RuntimeError(format!(
                    "value {v} is not a whole number for integer format {format}"
                )))
            } else if v < <$ty>::MIN as f64 || v > <$ty>::MAX as f64 {
                Err(Error::RuntimeError(format!(
                    "value {v} out of range for format {format}"
                )))
            } else {
                Ok(Value::$variant((v as $ty, res.clone())))
            }
        }};
    }
    match format {
        Format::U8(_) => float_int_variant!(U8, u8),
        Format::U16(_) => float_int_variant!(U16, u16),
        Format::U32(_) => float_int_variant!(U32, u32),
        Format::U64(_) => float_int_variant!(U64, u64),
        Format::U128(_) => float_int_variant!(U128, u128),
        Format::I8(_) => float_int_variant!(I8, i8),
        Format::I16(_) => float_int_variant!(I16, i16),
        Format::I32(_) => float_int_variant!(I32, i32),
        Format::I64(_) => float_int_variant!(I64, i64),
        Format::I128(_) => float_int_variant!(I128, i128),
        Format::F32(_) => Ok(Value::F32((v as f32, res))),
        Format::F64(_) => Ok(Value::F64((v, res))),
        Format::Ascii(_) => Ok(Value::Ascii(v.to_string())),
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
            .with_module(TimeModule::default())
            .with_module(LogModule::init(LuaLogSink {
                log: log.clone(),
                sink: sink.clone(),
            }))
            .with_print_sink(LuaLogSink {
                log: log.clone(),
                sink: sink.clone(),
            });
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
        emit(&log, &sink, "[sim] stopped completely ");
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

/// Routes `C_Log:Print(..)` lines from a Modbus sim into the module's ring log and file sink,
/// mirroring the `emit` path used for sim diagnostics.
struct LuaLogSink {
    log: ModuleLog,
    sink: FileSink,
}

impl LogSink for LuaLogSink {
    fn print(&self, line: &str) {
        emit(&self.log, &self.sink, line);
    }
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
    use ferrowl_codec::format::{BitField, Endian, Resolution};
    use ferrowl_codec::{Access, Format, Kind, RegisterBuilder};
    use ferrowl_modbus::SlaveKey;
    use ferrowl_store::{CellKind as MemKind, CellType, Memory};
    use tokio::sync::RwLock;

    fn holding(addr: u16) -> Register {
        RegisterBuilder::default()
            .slave_id(1u8)
            .access(Access::ReadWrite)
            .kind(Kind::HoldingRegister)
            .address(Address::Fixed(addr))
            .format(Format::U16((
                Endian::Big,
                Resolution(1.0),
                BitField::default(),
            )))
            .build()
            .unwrap()
    }

    /// Memory holding two U16 registers (setpoint@0, power@1), both read/write.
    fn evse_memory() -> ModuleMemory {
        let mut memory: Memory<Key<SlaveKey>> = Memory::default();
        let key = Key {
            id: SlaveKey {
                slave_id: 1u8,
                kind: Kind::HoldingRegister,
            },
        };
        memory.add_ranges(
            key,
            &MemKind::ReadWrite(CellType::Register),
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
                .address(ferrowl_codec::Address::Virtual)
                .format(Format::U16((
                    Endian::Big,
                    Resolution(1.0),
                    BitField::default(),
                )))
                .build()
                .unwrap(),
        );
        let bridge = RegisterBridge::new(evse_memory(), virtual_store.clone(), registers);

        // Reading before any write errors; after a write the value round-trips via the store.
        assert!(bridge.read("calc".to_string()).is_err());
        bridge
            .write("calc".to_string(), ValueType::Int(7))
            .expect("virtual write");
        match bridge.read("calc".to_string()).expect("virtual read") {
            ValueType::Int(v) => assert_eq!(v, 7),
            _ => panic!("expected Int"),
        }
        assert_eq!(
            virtual_store
                .blocking_read()
                .get("calc")
                .map(|v| v.clone().unscaled().to_string()),
            Some("7".to_string())
        );
    }

    #[test]
    fn ut_bridge_write_then_read() {
        let bridge = RegisterBridge::new(evse_memory(), vstore(), evse_registers());
        bridge
            .write("setpoint".to_string(), ValueType::Int(100))
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
        assert!(bridge.write("nope".to_string(), ValueType::Int(1)).is_err());
    }

    // Mirrors `run_sim`'s body once: a `power` update copying `setpoint` reflects after a cycle.
    #[test]
    fn ut_sim_script_mirrors_register() {
        let memory = evse_memory();
        let bridge = RegisterBridge::new(memory.clone(), vstore(), evse_registers());
        bridge
            .write("setpoint".to_string(), ValueType::Int(42))
            .expect("seed setpoint");

        let mut context = ContextBuilder::<String>::default()
            .with_stdlib()
            .with_module(RegisterModule::init(bridge))
            .with_module(TimeModule::default())
            .with_script(
                "power".to_string(),
                "C_Register:Set(\"power\", C_Register:Get(\"setpoint\"))",
            )
            .build()
            .expect("build context");

        context.call_all(Duration::ZERO).expect("run script");

        let power = memory
            .blocking_read()
            .read(
                Key {
                    id: SlaveKey {
                        slave_id: 1,
                        kind: Kind::HoldingRegister,
                    },
                },
                &CellType::Register,
                &Range::new(1, 1),
            )
            .expect("read power");
        assert_eq!(power, vec![42]);
    }

    // --- Typed write path (no string round-trip) ---

    #[test]
    fn ut_bridge_typed_int_float_bool_roundtrip() {
        let bridge = RegisterBridge::new(evse_memory(), vstore(), evse_registers());

        bridge
            .write("setpoint".to_string(), ValueType::Int(1234))
            .expect("int write");
        match bridge.read("setpoint".to_string()).expect("read") {
            ValueType::Int(v) => assert_eq!(v, 1234),
            _ => panic!("expected Int"),
        }

        bridge
            .write("power".to_string(), ValueType::Bool(true))
            .expect("bool write");
        match bridge.read("power".to_string()).expect("read") {
            ValueType::Int(v) => assert_eq!(v, 1),
            _ => panic!("expected Int"),
        }
    }

    #[test]
    fn ut_bridge_typed_float_whole_number_onto_int_format() {
        // Mirrors the string path: a whole-number float parses onto an integer format.
        let bridge = RegisterBridge::new(evse_memory(), vstore(), evse_registers());
        bridge
            .write("setpoint".to_string(), ValueType::Float(42.0))
            .expect("whole float write");
        match bridge.read("setpoint".to_string()).expect("read") {
            ValueType::Int(v) => assert_eq!(v, 42),
            _ => panic!("expected Int"),
        }
    }

    #[test]
    fn ut_bridge_typed_float_fractional_onto_int_format_errors() {
        let bridge = RegisterBridge::new(evse_memory(), vstore(), evse_registers());
        assert!(
            bridge
                .write("setpoint".to_string(), ValueType::Float(3.5))
                .is_err()
        );
    }

    #[test]
    fn ut_bridge_typed_string_roundtrip() {
        let bridge = RegisterBridge::new(evse_memory(), vstore(), evse_registers());
        bridge
            .write("setpoint".to_string(), ValueType::String("77".to_string()))
            .expect("string write");
        match bridge.read("setpoint".to_string()).expect("read") {
            ValueType::Int(v) => assert_eq!(v, 77),
            _ => panic!("expected Int"),
        }
    }

    #[test]
    fn ut_bridge_typed_nil_errors_cleanly() {
        let bridge = RegisterBridge::new(evse_memory(), vstore(), evse_registers());
        let err = bridge
            .write("setpoint".to_string(), ValueType::Nil)
            .unwrap_err();
        assert!(err.to_string().contains("cannot Set nil value"));
    }

    #[test]
    fn ut_bridge_typed_int_overflow_errors_cleanly() {
        // "setpoint" is U16 (max 65535); a too-large Int must error, not silently truncate.
        let bridge = RegisterBridge::new(evse_memory(), vstore(), evse_registers());
        assert!(
            bridge
                .write("setpoint".to_string(), ValueType::Int(100_000))
                .is_err()
        );
    }

    #[test]
    fn ut_bridge_virtual_typed_int_overflow_falls_back_to_float() {
        // Virtual registers ignore the declared format (mirrors `str_to_value`'s `Scalar`
        // fallback): an out-of-i64-range `Int` is stored as `F64` rather than erroring.
        let virtual_store = vstore();
        let mut registers = evse_registers();
        registers.insert(
            "calc".to_string(),
            RegisterBuilder::default()
                .slave_id(1u8)
                .access(Access::ReadWrite)
                .kind(Kind::HoldingRegister)
                .address(ferrowl_codec::Address::Virtual)
                .format(Format::U16((
                    Endian::Big,
                    Resolution(1.0),
                    BitField::default(),
                )))
                .build()
                .unwrap(),
        );
        let bridge = RegisterBridge::new(evse_memory(), virtual_store, registers);
        bridge
            .write("calc".to_string(), ValueType::Int(i128::MAX))
            .expect("virtual int overflow falls back to float");
        match bridge.read("calc".to_string()).expect("read") {
            ValueType::Float(_) => {}
            _ => panic!("expected Float fallback"),
        }
    }

    #[test]
    fn ut_bridge_virtual_typed_nil_errors_cleanly() {
        let virtual_store = vstore();
        let mut registers = evse_registers();
        registers.insert(
            "calc".to_string(),
            RegisterBuilder::default()
                .slave_id(1u8)
                .access(Access::ReadWrite)
                .kind(Kind::HoldingRegister)
                .address(ferrowl_codec::Address::Virtual)
                .format(Format::U16((
                    Endian::Big,
                    Resolution(1.0),
                    BitField::default(),
                )))
                .build()
                .unwrap(),
        );
        let bridge = RegisterBridge::new(evse_memory(), virtual_store, registers);
        let err = bridge
            .write("calc".to_string(), ValueType::Nil)
            .unwrap_err();
        assert!(err.to_string().contains("cannot Set nil value"));
    }
}
