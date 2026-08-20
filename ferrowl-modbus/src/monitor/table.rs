//! MB-R-144/MB-R-145 — the monitor's observed-value table: a plain `slave id, table kind ->
//! sparse address -> word` map, deliberately not `ferrowl_store::Memory` (see module doc).

use crate::{Key, SlaveKey};
use parking_lot::RwLock;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

/// A monitor's observed-value table (MB-R-144), keyed by (slave id, table kind) the same way
/// `ferrowl_store::Memory` is (MB-R-026), but backed by a plain sparse map rather than a
/// declared-range store: the monitor never declares a range up front, it only discovers
/// addresses as bus traffic arrives, and an address never written must be distinguishable from
/// one written with `0` (MB-R-145's "not-yet-observed").
#[derive(Default)]
pub struct ObservedTable {
    values: HashMap<Key<SlaveKey>, BTreeMap<u16, u16>>,
}

/// Shared handle to an [`ObservedTable`], read by the view and written by the monitor's
/// receive-loop driver.
pub type SharedObservedTable = Arc<RwLock<ObservedTable>>;

impl ObservedTable {
    /// Write `words` starting at `address` under `key`. Overwrites whatever was there;
    /// addresses not yet written stay absent from the map (MB-R-145's "not-yet-observed").
    pub fn write_words(&mut self, key: Key<SlaveKey>, address: u16, words: &[u16]) {
        let region = self.values.entry(key).or_default();
        for (offset, word) in words.iter().enumerate() {
            let Some(addr) = address.checked_add(offset as u16) else {
                break;
            };
            region.insert(addr, *word);
        }
    }

    /// `None` if any address in `[address, address+width)` has never been written.
    pub fn read_words(&self, key: &Key<SlaveKey>, address: u16, width: usize) -> Option<Vec<u16>> {
        let region = self.values.get(key)?;
        let mut out = Vec::with_capacity(width);
        for offset in 0..width {
            let addr = address.checked_add(offset as u16)?;
            out.push(*region.get(&addr)?);
        }
        Some(out)
    }

    /// Distinct slave ids observed so far, across every table kind, in sorted order (UI-R-060
    /// sorts for display regardless, so any deterministic order is fine here).
    pub fn unit_ids(&self) -> Vec<rust_modbus::UnitId> {
        self.values
            .keys()
            .map(|key| key.id.slave_id)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::ObservedTable;
    use crate::{Key, SlaveKey};
    use ferrowl_codec::Kind;
    use rust_modbus::UnitId;

    fn key(slave: u8, kind: Kind) -> Key<SlaveKey> {
        Key::new(SlaveKey {
            slave_id: UnitId(slave),
            kind,
        })
    }

    /// MB-R-145 — an address never written is `None`, distinguishable from one written with a
    /// real `0` value: `read_words` must not decode absent memory as zero.
    #[test]
    fn ut_observed_table_read_words_none_when_unobserved_vs_some_zero() {
        let mut table = ObservedTable::default();
        let k = key(1, Kind::HoldingRegister);

        assert_eq!(table.read_words(&k, 10, 1), None);

        table.write_words(k.clone(), 10, &[0]);
        assert_eq!(table.read_words(&k, 10, 1), Some(vec![0]));
    }

    #[test]
    fn ut_observed_table_write_words_overwrites_existing() {
        let mut table = ObservedTable::default();
        let k = key(1, Kind::HoldingRegister);
        table.write_words(k.clone(), 0, &[1, 2, 3]);
        table.write_words(k.clone(), 1, &[9]);
        assert_eq!(table.read_words(&k, 0, 3), Some(vec![1, 9, 3]));
    }

    #[test]
    fn ut_observed_table_read_words_none_when_partial_range_unobserved() {
        let mut table = ObservedTable::default();
        let k = key(1, Kind::HoldingRegister);
        table.write_words(k.clone(), 0, &[1]);
        assert_eq!(table.read_words(&k, 0, 2), None);
    }

    /// MB-R-144 — the table is keyed by (slave id, table kind); `unit_ids` enumerates the
    /// distinct slave ids observed across every kind.
    #[test]
    fn ut_observed_table_unit_ids_deduplicates_across_kinds() {
        let mut table = ObservedTable::default();
        table.write_words(key(3, Kind::HoldingRegister), 0, &[1]);
        table.write_words(key(3, Kind::Coil), 0, &[1]);
        table.write_words(key(1, Kind::HoldingRegister), 0, &[1]);
        assert_eq!(table.unit_ids(), vec![UnitId(1), UnitId(3)]);
    }
}
