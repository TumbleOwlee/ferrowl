//! Register descriptions and value conversion for Modbus data.
//!
//! The central type is [`Register`]: a description of where a logical value
//! lives (slave id, register [`Kind`], [`Address`]), how it may be accessed
//! ([`Access`]), and how its raw words are interpreted ([`Format`]).
//! [`decode`]/[`encode`] convert between raw `u16` register words and typed
//! [`Value`]s or user-entered strings.

mod access;
mod address;
pub mod codec;
pub mod error;
pub mod format;
mod kind;
pub mod traits;
pub mod value;

use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters, WithSetters};
use serde::{Deserialize, Serialize};
use tokio_modbus::SlaveId;

pub use crate::access::Access;
pub use crate::address::Address;
pub use crate::codec::{decode, encode, encode_value};
pub use crate::error::CodecError;
pub use crate::format::{Alignment, BitField, Endian, Format};
pub use crate::kind::Kind;
pub use crate::traits::{IntoVec, ParseFromU8};
pub use crate::value::Value;

/// Description of a single logical register: location, access rights, and
/// data format.
///
/// Build with [`RegisterBuilder`]; only `format` is required. Use
/// [`decode`](Self::decode)/[`encode`](Self::encode) to convert between raw
/// register words and display/input strings according to the format.
#[derive(
    Builder, Serialize, Deserialize, Debug, Clone, Getters, Setters, CopyGetters, WithSetters,
)]
#[getset(set = "pub")]
pub struct Register {
    #[getset(get = "pub")]
    #[builder(default = "0")]
    slave_id: SlaveId,
    #[getset(get = "pub")]
    #[builder(default = "Access::ReadWrite")]
    access: Access,
    #[getset(get = "pub")]
    #[builder(default = "Kind::HoldingRegister")]
    kind: Kind,
    #[getset(get = "pub")]
    #[builder(default = "Address::Virtual")]
    address: Address,
    #[getset(get = "pub")]
    format: Format,
}

impl Register {
    /// Decodes raw register words into a typed [`Value`] using this
    /// register's [`Format`].
    pub fn decode(&self, bytes: &[u16]) -> Result<Value, CodecError> {
        decode(&self.format, bytes)
    }

    /// Parses a user-entered string into raw register words using this
    /// register's [`Format`]. For a bit-field format the field is positioned
    /// per its mask/shift with all other bits left zero.
    pub fn encode(&self, s: &str) -> Result<Vec<u16>, CodecError> {
        encode(&self.format, s)
    }

    /// Encodes a typed [`Value`] into raw register words using this
    /// register's [`Format`]. Errors with [`CodecError::ValueFormatMismatch`]
    /// if `value`'s variant does not match the format's.
    pub fn encode_value(&self, value: &Value) -> Result<Vec<u16>, CodecError> {
        encode_value(&self.format, value)
    }

    /// Per-word mask selecting the bits this register owns (all `0xFFFF` for a
    /// full-width format). Used to read-modify-write a value while preserving
    /// bits belonging to sibling registers that alias the same address.
    pub fn write_mask(&self) -> Vec<u16> {
        crate::codec::mask_words(&self.format)
    }

    /// Merge freshly [`encode`](Self::encode)d `value` words into the existing
    /// `old` words: `(old & !mask) | (value & mask)` per word, using
    /// [`write_mask`](Self::write_mask). Missing `old` words are treated as 0.
    pub fn merge_write(&self, old: &[u16], value: &[u16]) -> Vec<u16> {
        let mask = self.write_mask();
        value
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                let m = mask.get(i).copied().unwrap_or(0xFFFF);
                let o = old.get(i).copied().unwrap_or(0);
                (o & !m) | (v & m)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::format::{Endian, Format, Resolution, Width};
    use crate::{Access, Address, Alignment, BitField, Kind, RegisterBuilder};

    fn u16_be() -> Format {
        Format::U16((Endian::Big, Resolution(1.0), BitField::default()))
    }

    #[test]
    /// MB-R-001 — a register is described by exactly five properties: slave id, access, kind, address, format.
    fn ut_register_carries_five_properties() {
        let r = RegisterBuilder::default()
            .slave_id(7)
            .access(Access::ReadOnly)
            .kind(Kind::InputRegister)
            .address(Address::Fixed(100))
            .format(u16_be())
            .build()
            .unwrap();

        assert_eq!(*r.slave_id(), 7);
        assert_eq!(*r.access(), Access::ReadOnly);
        assert_eq!(*r.kind(), Kind::InputRegister);
        assert_eq!(*r.address(), Address::Fixed(100));
        assert_eq!(r.format().width(), 1);
    }

    #[test]
    /// MB-R-006 — a register's format determines its width in 16-bit registers, the count of consecutive addresses it occupies.
    fn ut_format_width_is_consecutive_address_count() {
        // U16 = 1 word, F32 = 2 words, U64 = 4 words, U128 = 8 words, Ascii = its width.
        assert_eq!(u16_be().width(), 1);
        assert_eq!(Format::F32((Endian::Big, Resolution(1.0))).width(), 2);
        assert_eq!(
            Format::U64((Endian::Big, Resolution(1.0), BitField::default())).width(),
            4
        );
        assert_eq!(
            Format::U128((Endian::Big, Resolution(1.0), BitField::default())).width(),
            8
        );
        assert_eq!(Format::Ascii((Alignment::Left, Width(5))).width(), 5);
    }
}
