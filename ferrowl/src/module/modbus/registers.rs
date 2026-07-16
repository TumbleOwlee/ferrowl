//! Free helpers translating between `Register`s, device-config `RegisterDef`s, module memory
//! bindings and live table values.

use ferrowl_codec::{Access, Address, Kind, Register};
use ferrowl_modbus::{Command, Key, SlaveKey};
use ferrowl_store::{CellKind as MemKind, CellType, Range};

use crate::config::device::{
    AccessCfg, AlignmentCfg, EndianCfg, RegisterDef, ValueType as DevValueType,
};

/// Modbus memory type backing a register.
fn mem_type(register: &Register) -> CellType {
    match register.kind() {
        Kind::Coil | Kind::DiscreteInput => CellType::Coil,
        Kind::HoldingRegister | Kind::InputRegister => CellType::Register,
    }
}

/// (name, code) pairs for every enabled, non-empty global script (run on the sim thread).
pub(crate) fn collect_scripts(device: &crate::config::DeviceConfig) -> Vec<(String, String)> {
    device
        .scripts
        .iter()
        .filter(|s| s.enabled && !s.code.trim().is_empty())
        .map(|s| (s.name.clone(), s.code.clone()))
        .collect()
}

/// Memory binding `(kind, key, range)` backing a fixed-address register, or `None` if virtual.
pub(crate) fn register_mem_binding(register: &Register) -> Option<(MemKind, Key<SlaveKey>, Range)> {
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
pub(crate) fn write_command(register: &Register, slave: u8, addr: u16, raw: &[u16]) -> Command {
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

/// Sync the mutable `RegisterDef` fields (address, format, access, kind) from an edited
/// `Register`. Named values are handled separately in `apply_edit`.
pub(crate) fn sync_register_def(def: &mut RegisterDef, register: &Register) {
    use ferrowl_codec::Format;

    def.slave_id = *register.slave_id();
    def.access = match register.access() {
        Access::ReadOnly => AccessCfg::ReadOnly,
        Access::WriteOnly => AccessCfg::WriteOnly,
        Access::ReadWrite => AccessCfg::ReadWrite,
    };
    def.kind = register.kind().clone();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::script::ScriptDef;
    use ferrowl_codec::format::{BitField, Endian, Resolution};
    use ferrowl_codec::{Address, Format, RegisterBuilder};

    fn reg(kind: Kind, address: Address) -> Register {
        RegisterBuilder::default()
            .slave_id(1u8)
            .access(Access::ReadWrite)
            .kind(kind)
            .address(address)
            .format(Format::U16((Endian::Big, Resolution(1.0), BitField::default())))
            .build()
            .unwrap()
    }

    /// MB-R-046 — a client write command is single/multiple by width and coil/register by kind.
    #[test]
    fn ut_write_command_selects_by_kind_and_width() {
        let coil = reg(Kind::Coil, Address::Fixed(0));
        assert!(matches!(
            write_command(&coil, 1, 0, &[1]),
            Command::WriteSingleCoil(1, 0, true)
        ));
        assert!(matches!(
            write_command(&coil, 1, 0, &[0, 1]),
            Command::WriteMultipleCoils(1, 0, _)
        ));
        let hr = reg(Kind::HoldingRegister, Address::Fixed(0));
        assert!(matches!(
            write_command(&hr, 1, 5, &[7]),
            Command::WriteSingleRegister(1, 5, 7)
        ));
        assert!(matches!(
            write_command(&hr, 1, 5, &[7, 8]),
            Command::WriteMultipleRegister(1, 5, _)
        ));
    }

    /// MB-R-080 — a virtual register occupies no store memory, so it has no memory binding.
    #[test]
    fn ut_register_mem_binding_virtual_is_none() {
        assert!(register_mem_binding(&reg(Kind::HoldingRegister, Address::Virtual)).is_none());
    }

    /// MB-R-078 — coil/holding bind read/write cells; discrete-input/input bind read-only cells.
    #[test]
    fn ut_register_mem_binding_kind_direction() {
        let bind = |k| register_mem_binding(&reg(k, Address::Fixed(2))).unwrap().0;
        assert!(matches!(bind(Kind::Coil), MemKind::ReadWrite(CellType::Coil)));
        assert!(matches!(
            bind(Kind::DiscreteInput),
            MemKind::Read(CellType::Coil)
        ));
        assert!(matches!(
            bind(Kind::HoldingRegister),
            MemKind::ReadWrite(CellType::Register)
        ));
        assert!(matches!(
            bind(Kind::InputRegister),
            MemKind::Read(CellType::Register)
        ));
    }

    #[test]
    fn ut_collect_scripts_keeps_enabled_nonempty() {
        let device = crate::config::DeviceConfig {
            scripts: vec![
                ScriptDef {
                    name: "a".into(),
                    code: "x=1".into(),
                    enabled: true,
                },
                ScriptDef {
                    name: "disabled".into(),
                    code: "y=2".into(),
                    enabled: false,
                },
                ScriptDef {
                    name: "blank".into(),
                    code: "   ".into(),
                    enabled: true,
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            collect_scripts(&device),
            vec![("a".to_string(), "x=1".to_string())]
        );
    }

    #[test]
    fn ut_sync_register_def_writes_back_edited_fields() {
        let mut def = RegisterDef {
            slave_id: 0,
            kind: Kind::HoldingRegister,
            address: Some(0),
            is_virtual: false,
            access: AccessCfg::ReadOnly,
            value_type: DevValueType::U16,
            endian: EndianCfg::Big,
            resolution: 1.0,
            bitmask: None,
            length: 4,
            alignment: AlignmentCfg::Right,
            values: vec![],
            update: None,
            description: String::new(),
            default: None,
        };
        let register = RegisterBuilder::default()
            .slave_id(9u8)
            .access(Access::WriteOnly)
            .kind(Kind::Coil)
            .address(Address::Virtual)
            .format(Format::F32((Endian::Little, Resolution(0.5))))
            .build()
            .unwrap();
        sync_register_def(&mut def, &register);
        assert_eq!(def.slave_id, 9);
        assert!(matches!(def.access, AccessCfg::WriteOnly));
        assert_eq!(def.kind, Kind::Coil);
        assert!(def.is_virtual && def.address.is_none());
        assert!(matches!(def.value_type, DevValueType::F32));
        assert!(matches!(def.endian, EndianCfg::Little));
        assert_eq!(def.resolution, 0.5);
        assert!(def.bitmask.is_none());
    }
}
