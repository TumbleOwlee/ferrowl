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
    pub fn decode(&self, bytes: &[u16]) -> anyhow::Result<Value> {
        decode(&self.format, bytes)
    }

    pub fn encode(&self, s: &str) -> anyhow::Result<Vec<u16>> {
        encode(&self.format, s)
    }
}
