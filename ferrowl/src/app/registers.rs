//! Free helpers translating between `Register`s, device-config `RegisterDef`s, module memory
//! bindings and live table values.

use ferrowl_mem::{Kind as MemKind, Memory, Range, Type};
use ferrowl_net::{Command, Key, SlaveKind};
use ferrowl_reg::{Access, Address, Kind, Register};

use crate::config::device::{
    AccessCfg, AlignmentCfg, EndianCfg, RegisterDef, ValueType as DevValueType,
};
use crate::view::main::Definition;

/// Modbus memory type backing a register.
fn mem_type(register: &Register) -> Type {
    match register.kind() {
        Kind::Coil | Kind::DiscreteInput => Type::Coil,
        Kind::HoldingRegister | Kind::InputRegister => Type::Register,
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
) -> Option<(MemKind, Key<SlaveKind>, Range)> {
    let Address::Fixed(addr) = register.address() else {
        return None;
    };
    let ty = mem_type(register);
    let kind = match register.kind() {
        Kind::Coil | Kind::HoldingRegister => MemKind::ReadWrite(ty),
        Kind::DiscreteInput | Kind::InputRegister => MemKind::Read(ty),
    };
    let key = Key {
        id: SlaveKind {
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
    memory: &Memory<Key<SlaveKind>>,
    virtual_values: &std::collections::HashMap<String, String>,
) -> Definition {
    match d.register.address() {
        Address::Fixed(addr) => {
            let width = d.register.format().width();
            let key = Key {
                id: SlaveKind {
                    slave_id: *d.register.slave_id(),
                    kind: d.register.kind().clone(),
                },
            };
            let raw = memory
                .read_unchecked(key, &Range::new(*addr as usize, width))
                .unwrap_or_else(|| vec![0; width]);
            d.value = match d.register.decode(&raw) {
                Ok(v) => format!("{v}"),
                Err(_) => "Error".to_string(),
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
                        .encode(v)
                        .map(|raw| raw_hex(&raw))
                        .unwrap_or_default();
                }
                None => {
                    d.value.clear();
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
    use ferrowl_reg::Format;

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
    // Every numeric format carries the same (endian, resolution) payload.
    macro_rules! numeric {
        ($vt:ident, $e:expr, $r:expr) => {{
            def.value_type = DevValueType::$vt;
            def.endian = endian_cfg($e);
            def.resolution = $r.0;
        }};
    }
    match register.format() {
        Format::U8((e, r)) => numeric!(U8, e, r),
        Format::U16((e, r)) => numeric!(U16, e, r),
        Format::U32((e, r)) => numeric!(U32, e, r),
        Format::U64((e, r)) => numeric!(U64, e, r),
        Format::U128((e, r)) => numeric!(U128, e, r),
        Format::I8((e, r)) => numeric!(I8, e, r),
        Format::I16((e, r)) => numeric!(I16, e, r),
        Format::I32((e, r)) => numeric!(I32, e, r),
        Format::I64((e, r)) => numeric!(I64, e, r),
        Format::I128((e, r)) => numeric!(I128, e, r),
        Format::F32((e, r)) => numeric!(F32, e, r),
        Format::F64((e, r)) => numeric!(F64, e, r),
        Format::Ascii((align, width)) => {
            def.value_type = DevValueType::Ascii;
            def.alignment = match align {
                ferrowl_reg::format::Alignment::Left => AlignmentCfg::Left,
                ferrowl_reg::format::Alignment::Right => AlignmentCfg::Right,
            };
            def.length = width.0;
        }
    }
}

fn endian_cfg(e: &ferrowl_reg::format::Endian) -> EndianCfg {
    match e {
        ferrowl_reg::format::Endian::Big => EndianCfg::Big,
        ferrowl_reg::format::Endian::Little => EndianCfg::Little,
    }
}
