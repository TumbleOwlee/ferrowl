//! Free helpers translating between `Register`s, device-config `RegisterDef`s, module memory
//! bindings and live table values.

use ferrowl_store::{CellKind as MemKind, Memory, Range, CellType};
use ferrowl_modbus::{Command, Key, SlaveKey};
use ferrowl_codec::{Access, Address, Kind, Register, Value};

use crate::config::device::{
    AccessCfg, AlignmentCfg, EndianCfg, RegisterDef, ValueType as DevValueType,
};
use crate::view::main::Definition;

/// Modbus memory type backing a register.
fn mem_type(register: &Register) -> CellType {
    match register.kind() {
        Kind::Coil | Kind::DiscreteInput => CellType::Coil,
        Kind::HoldingRegister | Kind::InputRegister => CellType::Register,
    }
}

/// (name, script) pairs for every register carrying a non-empty `update` Lua snippet.
pub(super) fn collect_scripts(device: &crate::config::DeviceConfig) -> Vec<(String, String)> {
    device
        .definitions
        .iter()
        .filter_map(|(name, def)| {
            def.update
                .as_ref()
                .filter(|s| !s.trim().is_empty())
                .map(|s| (name.clone(), s.clone()))
        })
        .collect()
}

/// Memory binding `(kind, key, range)` backing a fixed-address register, or `None` if virtual.
pub(super) fn register_mem_binding(
    register: &Register,
) -> Option<(MemKind, Key<SlaveKey>, Range)> {
    let Address::Fixed(addr) = register.address() else {
        return None;
    };
    let ty = mem_type(register);
    let kind = match register.kind() {
        Kind::Coil | Kind::HoldingRegister => MemKind::ReadWrite(ty),
        Kind::DiscreteInput | Kind::InputRegister => MemKind::Read(ty),
    };
    let key = Key {
        id: SlaveKey {
            slave_id: *register.slave_id(),
            kind: register.kind().clone(),
        },
    };
    Some((
        kind,
        key,
        Range::new(*addr as usize, register.format().width()),
    ))
}

/// Build the appropriate write command for a client, based on the register kind/width.
pub(super) fn write_command(register: &Register, slave: u8, addr: u16, raw: &[u16]) -> Command {
    match register.kind() {
        Kind::Coil | Kind::DiscreteInput => {
            if raw.len() == 1 {
                Command::WriteSingleCoil(slave, addr, raw[0] != 0)
            } else {
                Command::WriteMultipleCoils(slave, addr, raw.iter().map(|v| *v != 0).collect())
            }
        }
        Kind::HoldingRegister | Kind::InputRegister => {
            if raw.len() == 1 {
                Command::WriteSingleRegister(slave, addr, raw[0])
            } else {
                Command::WriteMultipleRegister(slave, addr, raw.to_vec())
            }
        }
    }
}

/// Decode one register's live value from the module memory snapshot.
pub(super) fn decode_definition(
    mut d: Definition,
    memory: &Memory<Key<SlaveKey>>,
    virtual_values: &std::collections::HashMap<String, Value>,
) -> Definition {
    match d.register.address() {
        Address::Fixed(addr) => {
            let width = d.register.format().width();
            let key = Key {
                id: SlaveKey {
                    slave_id: *d.register.slave_id(),
                    kind: d.register.kind().clone(),
                },
            };
            let raw = memory
                .read_unchecked(key, &Range::new(*addr as usize, width))
                .unwrap_or_else(|| vec![0; width]);
            d.value = match d.register.decode(&raw) {
                Ok(v) => v,
                Err(_) => Value::Ascii("Error".to_string()),
            };
            d.raw_value = raw_hex(&raw);
        }
        Address::Virtual => {
            // No Modbus address: value comes from the virtual store (Lua sim / server `:set`);
            // derive the raw view by re-encoding it through the register's format.
            match virtual_values.get(&d.name) {
                Some(v) => {
                    d.value = v.clone();
                    d.raw_value = d
                        .register
                        .encode(&v.clone().unscaled().to_string())
                        .map(|raw| raw_hex(&raw))
                        .unwrap_or_default();
                }
                None => {
                    d.value = Value::Ascii(String::new());
                    d.raw_value.clear();
                }
            }
        }
    }
    d
}

/// Format register words as `[aaaa bbbb …]` lowercase hex for the table's raw column.
fn raw_hex(raw: &[u16]) -> String {
    let mut out = String::with_capacity(raw.len() * 5 + 2);
    out.push('[');
    for (i, v) in raw.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out += &format!("{v:04x}");
    }
    out.push(']');
    out
}

/// Sync the mutable `RegisterDef` fields (address, format, access, kind) from an edited
/// `Register`. Named values are handled separately in `apply_edit`.
pub(super) fn sync_register_def(def: &mut RegisterDef, register: &Register) {
    use ferrowl_codec::Format;

    def.slave_id = *register.slave_id();
    def.access = match register.access() {
        Access::ReadOnly => AccessCfg::ReadOnly,
        Access::WriteOnly => AccessCfg::WriteOnly,
        Access::ReadWrite => AccessCfg::ReadWrite,
    };
    def.read_code = match register.kind() {
        Kind::Coil => 1,
        Kind::DiscreteInput => 2,
        Kind::HoldingRegister => 4,
        Kind::InputRegister => 3,
    };
    match register.address() {
        Address::Fixed(addr) => {
            def.address = Some(*addr);
            def.is_virtual = false;
        }
        Address::Virtual => {
            def.address = None;
            def.is_virtual = true;
        }
    }
    // Integer formats carry (endian, resolution, bitfield); the bitfield is
    // written back as a hex string (or cleared when it's the full no-op mask).
    macro_rules! integer {
        ($vt:ident, $e:expr, $r:expr, $bf:expr) => {{
            def.value_type = DevValueType::$vt;
            def.endian = endian_cfg($e);
            def.resolution = $r.0;
            def.bitmask = if $bf.is_full() {
                None
            } else {
                Some(format!("0x{:X}", $bf.mask))
            };
        }};
    }
    // Float formats carry only (endian, resolution); they never have a bitfield.
    macro_rules! float {
        ($vt:ident, $e:expr, $r:expr) => {{
            def.value_type = DevValueType::$vt;
            def.endian = endian_cfg($e);
            def.resolution = $r.0;
            def.bitmask = None;
        }};
    }
    match register.format() {
        Format::U8((e, r, bf)) => integer!(U8, e, r, bf),
        Format::U16((e, r, bf)) => integer!(U16, e, r, bf),
        Format::U32((e, r, bf)) => integer!(U32, e, r, bf),
        Format::U64((e, r, bf)) => integer!(U64, e, r, bf),
        Format::U128((e, r, bf)) => integer!(U128, e, r, bf),
        Format::I8((e, r, bf)) => integer!(I8, e, r, bf),
        Format::I16((e, r, bf)) => integer!(I16, e, r, bf),
        Format::I32((e, r, bf)) => integer!(I32, e, r, bf),
        Format::I64((e, r, bf)) => integer!(I64, e, r, bf),
        Format::I128((e, r, bf)) => integer!(I128, e, r, bf),
        Format::F32((e, r)) => float!(F32, e, r),
        Format::F64((e, r)) => float!(F64, e, r),
        Format::Ascii((align, width)) => {
            def.value_type = DevValueType::Ascii;
            def.alignment = match align {
                ferrowl_codec::format::Alignment::Left => AlignmentCfg::Left,
                ferrowl_codec::format::Alignment::Right => AlignmentCfg::Right,
            };
            def.length = width.0;
            def.bitmask = None;
        }
    }
}

fn endian_cfg(e: &ferrowl_codec::format::Endian) -> EndianCfg {
    match e {
        ferrowl_codec::format::Endian::Big => EndianCfg::Big,
        ferrowl_codec::format::Endian::Little => EndianCfg::Little,
    }
}
