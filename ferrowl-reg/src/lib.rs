//! Register descriptions and value conversion for Modbus data.
//!
//! The central type is [`Register`]: a description of where a logical value
//! lives (slave id, register [`Kind`], [`Address`]), how it may be accessed
//! ([`Access`]), and how its raw words are interpreted ([`Format`]).
//! [`decode`]/[`encode`] convert between raw `u16` register words and typed
//! [`Value`]s or user-entered strings.

pub mod codec;
pub mod enums;
pub mod format;
pub mod traits;
pub mod value;

use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters, WithSetters};
use serde::{Deserialize, Serialize};
use tokio_modbus::SlaveId;

pub use crate::codec::{decode, encode};
pub use crate::enums::{Access, Address, Kind};
pub use crate::format::{Alignment, Endian, Format};
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
    /// register's [`Format`].
    pub fn encode(&self, s: &str) -> anyhow::Result<Vec<u16>> {
        encode(&self.format, s)
    }
}
