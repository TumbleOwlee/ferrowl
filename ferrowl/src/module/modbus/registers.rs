//! Free helpers translating between `Register`s, device-config `RegisterDef`s, module memory
//! bindings and live table values.

use ferrowl_codec::{Access, Address, Kind, Register};
use ferrowl_modbus::{Address as WireAddress, Command, Key, SlaveKey, UnitId, Word};
use ferrowl_store::{CellKind as MemKind, CellType, Range};

use crate::config::device::{
    AccessCfg, AlignmentCfg, EndianCfg, RegisterDef, ValueType as DevValueType, WordOrderCfg,
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
pub(crate) fn write_command(
    register: &Register,
    slave: UnitId,
    addr: WireAddress,
    raw: &[u16],
) -> Command {
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
                Command::WriteSingleRegister(slave, addr, Word(raw[0]))
            } else {
                Command::WriteMultipleRegister(slave, addr, raw.iter().copied().map(Word).collect())
            }
        }
    }
}

/// Sync the mutable `RegisterDef` fields (address, format, access, kind) from an edited
/// `Register`. Named values are handled separately in `apply_edit`.
pub(crate) fn sync_register_def(def: &mut RegisterDef, register: &Register) {
    use ferrowl_codec::{FloatKind, Format, IntKind};

    def.slave_id = register.slave_id().0;
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
        ($vt:ident, $e:expr, $w:expr, $r:expr, $bf:expr) => {{
            def.value_type = DevValueType::$vt;
            def.endian = endian_cfg($e);
            def.word_order = word_order_cfg($w);
            def.resolution = $r.0;
            def.bitmask = if $bf.is_full() {
                None
            } else {
                Some(format!("0x{:X}", $bf.mask))
            };
        }};
    }
    // Float formats carry (endian, word order, resolution); they never have a bitfield.
    macro_rules! float {
        ($vt:ident, $e:expr, $w:expr, $r:expr) => {{
            def.value_type = DevValueType::$vt;
            def.endian = endian_cfg($e);
            def.word_order = word_order_cfg($w);
            def.resolution = $r.0;
            def.bitmask = None;
        }};
    }
    match register.format() {
        Format::Numeric(nf) => {
            let (e, w, r, bf) = (&nf.endian, &nf.word_order, &nf.resolution, &nf.bit_field);
            match nf.kind {
                IntKind::U8 => integer!(U8, e, w, r, bf),
                IntKind::U16 => integer!(U16, e, w, r, bf),
                IntKind::U32 => integer!(U32, e, w, r, bf),
                IntKind::U64 => integer!(U64, e, w, r, bf),
                IntKind::U128 => integer!(U128, e, w, r, bf),
                IntKind::I8 => integer!(I8, e, w, r, bf),
                IntKind::I16 => integer!(I16, e, w, r, bf),
                IntKind::I32 => integer!(I32, e, w, r, bf),
                IntKind::I64 => integer!(I64, e, w, r, bf),
                IntKind::I128 => integer!(I128, e, w, r, bf),
            }
        }
        Format::Float(ff) => {
            let (e, w, r) = (&ff.endian, &ff.word_order, &ff.resolution);
            match ff.kind {
                FloatKind::F32 => float!(F32, e, w, r),
                FloatKind::F64 => float!(F64, e, w, r),
            }
        }
        Format::Ascii(align, width) => {
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

fn word_order_cfg(w: &ferrowl_codec::format::WordOrder) -> WordOrderCfg {
    match w {
        ferrowl_codec::format::WordOrder::Normal => WordOrderCfg::Normal,
        ferrowl_codec::format::WordOrder::Reversed => WordOrderCfg::Reversed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::script::ScriptDef;
    use ferrowl_codec::format::{BitField, Endian, Resolution, WordOrder};
    use ferrowl_codec::{Address, Format, RegisterBuilder};
    use ferrowl_modbus::UnitId;

    fn reg(kind: Kind, address: Address) -> Register {
        RegisterBuilder::default()
            .slave_id(UnitId(1))
            .access(Access::ReadWrite)
            .kind(kind)
            .address(address)
            .format(Format::u16(
                Endian::Big,
                WordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            ))
            .build()
            .unwrap()
    }

    #[test]
    /// MB-R-046 — a client write command is single/multiple by width and coil/register by kind.
    fn ut_write_command_selects_by_kind_and_width() {
        let coil = reg(Kind::Coil, Address::Fixed(0));
        assert!(matches!(
            write_command(&coil, UnitId(1), WireAddress(0), &[1]),
            Command::WriteSingleCoil(UnitId(1), WireAddress(0), true)
        ));
        assert!(matches!(
            write_command(&coil, UnitId(1), WireAddress(0), &[0, 1]),
            Command::WriteMultipleCoils(UnitId(1), WireAddress(0), _)
        ));
        let hr = reg(Kind::HoldingRegister, Address::Fixed(0));
        assert!(matches!(
            write_command(&hr, UnitId(1), WireAddress(5), &[7]),
            Command::WriteSingleRegister(UnitId(1), WireAddress(5), Word(7))
        ));
        assert!(matches!(
            write_command(&hr, UnitId(1), WireAddress(5), &[7, 8]),
            Command::WriteMultipleRegister(UnitId(1), WireAddress(5), _)
        ));
    }

    #[test]
    /// MB-R-080 — a virtual register occupies no store memory, so it has no memory binding.
    fn ut_register_mem_binding_virtual_is_none() {
        assert!(register_mem_binding(&reg(Kind::HoldingRegister, Address::Virtual)).is_none());
    }

    #[test]
    /// MB-R-078 — coil/holding bind read/write cells; discrete-input/input bind read-only cells.
    fn ut_register_mem_binding_kind_direction() {
        let bind = |k| register_mem_binding(&reg(k, Address::Fixed(2))).unwrap().0;
        assert!(matches!(
            bind(Kind::Coil),
            MemKind::ReadWrite(CellType::Coil)
        ));
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
    /// MB-R-099 — sync writes the codec register's order back into the config def.
    fn ut_sync_register_def_writes_back_edited_fields() {
        let mut def = RegisterDef {
            slave_id: 0,
            kind: Kind::HoldingRegister,
            address: Some(0),
            is_virtual: false,
            access: AccessCfg::ReadOnly,
            value_type: DevValueType::U16,
            endian: EndianCfg::Big,
            word_order: WordOrderCfg::Normal,
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
            .slave_id(UnitId(9))
            .access(Access::WriteOnly)
            .kind(Kind::Coil)
            .address(Address::Virtual)
            .format(Format::f32(
                Endian::Little,
                WordOrder::Reversed,
                Resolution(0.5),
            ))
            .build()
            .unwrap();
        sync_register_def(&mut def, &register);
        assert_eq!(def.slave_id, 9);
        assert!(matches!(def.access, AccessCfg::WriteOnly));
        assert_eq!(def.kind, Kind::Coil);
        assert!(def.is_virtual && def.address.is_none());
        assert!(matches!(def.value_type, DevValueType::F32));
        assert!(matches!(def.endian, EndianCfg::Little));
        assert!(matches!(def.word_order, WordOrderCfg::Reversed));
        assert_eq!(def.resolution, 0.5);
        assert!(def.bitmask.is_none());
    }
}
