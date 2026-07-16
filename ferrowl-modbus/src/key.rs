//! Memory keying: how a request maps to a region of the shared store.

use std::fmt::Debug;
use std::hash::Hash;

use ferrowl_codec::Kind;
use tokio_modbus::{FunctionCode, SlaveId};

/// Parameters identifying a memory region for a request.
///
/// Implementations derive a key from the slave id and the requested function
/// code, deciding how the shared [`Memory`](ferrowl_store::Memory) is
/// partitioned. See [`SlaveKey`] for the default.
pub trait KeyParams: Hash + Eq + Clone + Default + Debug + Send + Sync + 'static {
    /// Derives the key for a request addressed at `slave_id` with `fn_code`.
    fn from_slave_fn(slave_id: SlaveId, fn_code: FunctionCode) -> Self;
}

/// Memory key wrapping [`KeyParams`]; used as the device key of the shared
/// [`Memory`](ferrowl_store::Memory).
#[derive(Hash, Debug, PartialEq, Eq, Clone, Default)]
pub struct Key<T: KeyParams> {
    pub id: T,
}

impl<T: KeyParams> Key<T> {
    pub fn new(id: T) -> Self {
        Self { id }
    }
}

/// Default concrete key params: slave address + register kind. Each
/// (slave, register table) pair gets its own memory region; the kind is
/// derived from the request's function code.
#[derive(Hash, Debug, PartialEq, Eq, Clone, Default)]
pub struct SlaveKey {
    pub slave_id: SlaveId,
    pub kind: Kind,
}

impl KeyParams for SlaveKey {
    fn from_slave_fn(slave_id: SlaveId, fn_code: FunctionCode) -> Self {
        Self {
            slave_id,
            kind: match fn_code {
                FunctionCode::ReadCoils
                | FunctionCode::WriteSingleCoil
                | FunctionCode::WriteMultipleCoils => Kind::Coil,
                FunctionCode::ReadDiscreteInputs => Kind::DiscreteInput,
                FunctionCode::ReadHoldingRegisters
                | FunctionCode::WriteSingleRegister
                | FunctionCode::WriteMultipleRegisters => Kind::HoldingRegister,
                FunctionCode::ReadInputRegisters => Kind::InputRegister,
                _ => Kind::HoldingRegister,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Key, KeyParams, SlaveKey};
    use ferrowl_codec::Kind;
    use tokio_modbus::FunctionCode;

    #[test]
    fn ut_key_new_stores_fields() {
        let sk = SlaveKey {
            slave_id: 7,
            kind: Kind::HoldingRegister,
        };
        let key = Key::new(sk.clone());
        assert_eq!(key.id, sk);
    }

    #[test]
    /// MB-R-026 — the default device key is the (slave id, register table) pair.
    fn ut_key_default_is_slave_kind_default() {
        let key = Key::<SlaveKey>::default();
        assert_eq!(key.id, SlaveKey::default());
    }

    #[test]
    /// MB-R-027 — coil-family function codes derive the coil register table.
    fn ut_slave_kind_from_slave_fn_coil() {
        let sk = SlaveKey::from_slave_fn(3, FunctionCode::ReadCoils);
        assert_eq!(sk.slave_id, 3);
        assert_eq!(sk.kind, Kind::Coil);
    }
}
