//! Memory keying: how a request maps to a region of the shared store.

use std::fmt::Debug;
use std::hash::Hash;

use ferrowl_codec::Kind;
use rust_modbus::{FunctionCode, UnitId};

/// Parameters identifying a memory region for a request.
///
/// Implementations derive a key from the slave id and the requested function
/// code, deciding how the shared [`Memory`](ferrowl_store::Memory) is
/// partitioned. See [`SlaveKey`] for the default.
pub trait KeyParams: Hash + Eq + Clone + Default + Debug + Send + Sync + 'static {
    /// Derives the key for a request addressed at `slave_id` with `fn_code`.
    fn from_slave_fn(slave_id: UnitId, fn_code: FunctionCode) -> Self;

    /// Every key this scheme can produce for `slave_id`, one per register table — used by
    /// MB-R-128 to test whether *any* region is declared for a slave id, independent of which
    /// table a particular failing request's own key names.
    fn all_kinds_for(slave_id: UnitId) -> Vec<Self>;
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
#[derive(Hash, Debug, PartialEq, Eq, Clone)]
pub struct SlaveKey {
    pub slave_id: UnitId,
    pub kind: Kind,
}

// Hand-written because `UnitId` is a transparent wrapper with no `Default` of its
// own. The default unit is 1, not 0: on RTU, 0 is the broadcast address, which no
// server answers and no client may read from (MB-R-101, MB-R-103), so it is the one
// value an unconfigured key must not take.
impl Default for SlaveKey {
    fn default() -> Self {
        Self {
            slave_id: UnitId(1),
            kind: Kind::default(),
        }
    }
}

impl KeyParams for SlaveKey {
    fn from_slave_fn(slave_id: UnitId, fn_code: FunctionCode) -> Self {
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

    fn all_kinds_for(slave_id: UnitId) -> Vec<Self> {
        [
            Kind::Coil,
            Kind::DiscreteInput,
            Kind::HoldingRegister,
            Kind::InputRegister,
        ]
        .into_iter()
        .map(|kind| SlaveKey { slave_id, kind })
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{Key, KeyParams, SlaveKey};
    use ferrowl_codec::Kind;
    use rust_modbus::{FunctionCode, UnitId};

    #[test]
    fn ut_key_new_stores_fields() {
        let sk = SlaveKey {
            slave_id: UnitId(7),
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
        let sk = SlaveKey::from_slave_fn(UnitId(3), FunctionCode::ReadCoils);
        assert_eq!(sk.slave_id, UnitId(3));
        assert_eq!(sk.kind, Kind::Coil);
    }

    #[test]
    /// MB-R-128 — `all_kinds_for` enumerates the four register tables (`Kind`'s only variants —
    /// ferrowl-codec/src/kind.rs) for one slave id, giving every key that slave id could ever
    /// occupy in a `Memory`.
    fn ut_slave_key_all_kinds_for_covers_every_table() {
        let keys = SlaveKey::all_kinds_for(UnitId(7));
        assert_eq!(keys.len(), 4);
        for kind in [
            Kind::Coil,
            Kind::DiscreteInput,
            Kind::HoldingRegister,
            Kind::InputRegister,
        ] {
            assert!(keys.contains(&SlaveKey {
                slave_id: UnitId(7),
                kind
            }));
        }
    }
}
