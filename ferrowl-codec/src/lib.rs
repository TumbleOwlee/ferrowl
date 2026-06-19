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
pub use crate::codec::{decode, encode};
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
    #[builder(default = "Kind::InputRegister")]
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
    pub fn decode(&self, bytes: &[u16]) -> anyhow::Result<Value> {
        decode(&self.format, bytes)
    }

    /// Parses a user-entered string into raw register words using this
    /// register's [`Format`]. For a bit-field format the field is positioned
    /// per its mask/shift with all other bits left zero.
    pub fn encode(&self, s: &str) -> anyhow::Result<Vec<u16>> {
        encode(&self.format, s)
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
