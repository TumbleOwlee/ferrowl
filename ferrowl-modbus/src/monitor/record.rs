//! MB-R-146/147 — the monitor's per-slave-id message record ring and MB-R-147's recency helper.
//! Additional to (never a replacement for) MB-R-143's free-text log entry.

use ferrowl_codec::Kind;
use parking_lot::RwLock;
use rust_modbus::{ExceptionCode, FunctionCode, UnitId};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// MB-R-146's per-record outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordStatus {
    Ok,
    Unmatched,
    Exception(ExceptionCode),
}

/// MB-R-146's address/quantity/value(s) for the 9 table-shaping operations (the 8 MB-R-144 ops
/// plus `ReadWriteMultipleRegisters`); a record with no `TableShape` is any other operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableShape {
    pub kind: Kind,
    pub address: u16,
    pub quantity: u16,
    /// Set only for `ReadWriteMultipleRegisters` (edge-cases.md's Monitor boundaries row): its
    /// own second (write) address/quantity pair.
    pub write_address: Option<u16>,
    pub write_quantity: Option<u16>,
    /// The value(s) transacted — empty when the record's `status` is `Unmatched`/`Exception(_)`
    /// (nothing was actually applied to the observed-value table for those). For
    /// `ReadWriteMultipleRegisters`, the *read* response's registers only.
    pub values: Vec<u16>,
}

/// MB-R-146 — one captured message record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorRecord {
    pub timestamp: Instant,
    pub status: RecordStatus,
    pub operation: FunctionCode,
    pub shape: Option<TableShape>,
}

/// MB-R-146 — each slave id's bounded ring of the 200 most recently captured records, oldest
/// evicted first.
pub const RECORD_RING_CAPACITY: usize = 200;

/// One slave id's bounded ring plus a monotonic "how many pushes ever" counter — lets a caller
/// (the monitor view's incremental refresh) detect "nothing changed since I last looked" without
/// cloning/diffing records. Never reset except by constructing a fresh `RecordLog` (a
/// `:reload`/edit-confirm rebuild).
#[derive(Default)]
struct SlaveRing {
    ring: VecDeque<MonitorRecord>,
    generation: u64,
}

/// MB-R-146's per-slave-id record store, independent of every other slave id's ring.
#[derive(Default)]
pub struct RecordLog {
    records: HashMap<UnitId, SlaveRing>,
}

/// Shared handle to a [`RecordLog`], written by the monitor's receive-loop driver and read by
/// the view.
pub type SharedRecordLog = Arc<RwLock<RecordLog>>;

impl RecordLog {
    /// Push `record` onto `slave`'s ring, evicting the oldest entry once at capacity, and bump
    /// its generation counter (regardless of whether this push caused an eviction).
    pub fn push(&mut self, slave: UnitId, record: MonitorRecord) {
        let entry = self.records.entry(slave).or_default();
        if entry.ring.len() == RECORD_RING_CAPACITY {
            entry.ring.pop_front();
        }
        entry.ring.push_back(record);
        entry.generation += 1;
    }

    /// Records for `slave`, oldest first. Empty (not absent) for a slave id never pushed to.
    pub fn records_for(&self, slave: UnitId) -> Vec<MonitorRecord> {
        self.records
            .get(&slave)
            .map(|e| e.ring.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Current generation for `slave` (total pushes ever applied to its ring) — `0` for a slave
    /// id never pushed to. The monitor view uses this to skip a refresh entirely when unchanged
    /// since the last tick.
    pub fn generation_for(&self, slave: UnitId) -> u64 {
        self.records.get(&slave).map_or(0, |e| e.generation)
    }

    /// The most recently pushed `n` records for `slave`, oldest-of-the-batch first — clones only
    /// those `n` entries, not the whole ring (the monitor view's incremental refresh: format only
    /// the rows that are actually new). `n` is clamped to the ring's current length.
    pub fn recent_for(&self, slave: UnitId, n: usize) -> Vec<MonitorRecord> {
        match self.records.get(&slave) {
            Some(e) => {
                let skip = e.ring.len().saturating_sub(n);
                e.ring.iter().skip(skip).cloned().collect()
            }
            None => Vec::new(),
        }
    }
}

/// MB-R-147 — active for 2 seconds after the most recent record (any status) whose `shape`
/// covers `(kind, address)`. Checks the cheap timestamp condition before the costlier
/// (kind, address)-range check — `&&` short-circuits left-to-right, so this is purely an
/// evaluation-order change, output identical.
pub fn recency_active_at(
    records: &[MonitorRecord],
    kind: Kind,
    address: u16,
    now: Instant,
) -> bool {
    records.iter().rev().any(|r| {
        now.duration_since(r.timestamp) < Duration::from_secs(2)
            && r.shape.as_ref().is_some_and(|s| {
                s.kind == kind
                    && (address as u32) >= s.address as u32
                    && (address as u32) < s.address as u32 + s.quantity as u32
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(shape: Option<TableShape>) -> MonitorRecord {
        MonitorRecord {
            timestamp: Instant::now(),
            status: RecordStatus::Ok,
            operation: FunctionCode::ReadHoldingRegisters,
            shape,
        }
    }

    fn shape(kind: Kind, address: u16, quantity: u16) -> TableShape {
        TableShape {
            kind,
            address,
            quantity,
            write_address: None,
            write_quantity: None,
            values: vec![],
        }
    }

    /// MB-R-199 — the ring caps at 200, evicting the oldest first, independent per slave id.
    #[test]
    fn ut_record_ring_caps_at_200_evicts_oldest_first() {
        let mut log = RecordLog::default();
        for i in 0..205u16 {
            log.push(UnitId(1), record(Some(shape(Kind::HoldingRegister, i, 1))));
        }
        let records = log.records_for(UnitId(1));
        assert_eq!(records.len(), 200);
        // The oldest 5 (addresses 0..5) were evicted; the ring now starts at address 5.
        assert_eq!(records.first().unwrap().shape.as_ref().unwrap().address, 5);
        assert_eq!(records.last().unwrap().shape.as_ref().unwrap().address, 204);

        log.push(UnitId(2), record(Some(shape(Kind::HoldingRegister, 0, 1))));
        assert_eq!(
            log.records_for(UnitId(1)).len(),
            200,
            "slave 1's ring must be untouched by a push to slave 2"
        );
        assert_eq!(log.records_for(UnitId(2)).len(), 1);
    }

    /// MB-R-146 — a slave id never pushed to returns an empty (not absent/panicking) vec.
    #[test]
    fn ut_records_for_empty_for_unpushed_slave() {
        let log = RecordLog::default();
        assert_eq!(log.records_for(UnitId(9)), Vec::new());
    }

    /// MB-R-147 — a marker is active within its 2s window and lapses after.
    #[test]
    fn ut_recency_active_within_window_lapses_after() {
        let now = Instant::now();
        let fresh = MonitorRecord {
            timestamp: now - Duration::from_millis(500),
            ..record(Some(shape(Kind::HoldingRegister, 10, 2)))
        };
        let stale = MonitorRecord {
            timestamp: now - Duration::from_secs(3),
            ..record(Some(shape(Kind::HoldingRegister, 10, 2)))
        };
        assert!(recency_active_at(
            std::slice::from_ref(&fresh),
            Kind::HoldingRegister,
            10,
            now
        ));
        assert!(!recency_active_at(
            std::slice::from_ref(&stale),
            Kind::HoldingRegister,
            10,
            now
        ));
    }

    /// MB-R-147 — recency is scoped to the exact (kind, address) range the record's shape
    /// covers; outside that range, or under a different table kind, it is never active.
    #[test]
    fn ut_recency_scoped_to_kind_and_address_range() {
        let now = Instant::now();
        let r = record(Some(shape(Kind::HoldingRegister, 10, 2))); // covers addresses 10..12
        let records = [r];
        assert!(recency_active_at(&records, Kind::HoldingRegister, 10, now));
        assert!(recency_active_at(&records, Kind::HoldingRegister, 11, now));
        assert!(!recency_active_at(&records, Kind::HoldingRegister, 9, now));
        assert!(!recency_active_at(&records, Kind::HoldingRegister, 12, now));
        assert!(!recency_active_at(&records, Kind::Coil, 10, now));
    }

    /// MB-R-146 — a record with no shape (a non-table-shaping operation) never contributes a
    /// recency marker.
    #[test]
    fn ut_recency_ignores_records_with_no_shape() {
        let now = Instant::now();
        let r = record(None);
        assert!(!recency_active_at(&[r], Kind::HoldingRegister, 0, now));
    }

    /// MB-R-147 — a range ending exactly at 0xFFFF is fully covered, including its last address;
    /// `address.saturating_add(quantity)` must not silently clamp the exclusive upper bound below
    /// the true last touched address.
    #[test]
    fn ut_recency_covers_last_address_at_0xffff_boundary() {
        let now = Instant::now();
        let r = record(Some(shape(Kind::HoldingRegister, 0xFFFE, 2))); // covers 0xFFFE..=0xFFFF
        let records = [r];
        assert!(recency_active_at(
            &records,
            Kind::HoldingRegister,
            0xFFFE,
            now
        ));
        assert!(recency_active_at(
            &records,
            Kind::HoldingRegister,
            0xFFFF,
            now
        ));
    }

    /// (perf, no spec ID) — `generation_for` counts every push, is independent of ring
    /// eviction, and is `0` for a slave id never pushed to.
    #[test]
    fn ut_generation_for_increments_on_every_push_and_is_zero_for_unpushed_slave() {
        let mut log = RecordLog::default();
        log.push(UnitId(1), record(None));
        log.push(UnitId(1), record(None));
        log.push(UnitId(1), record(None));
        assert_eq!(log.generation_for(UnitId(1)), 3);
        assert_eq!(log.generation_for(UnitId(9)), 0);
    }

    /// (perf, no spec ID) — each slave id's generation counter is independent of every other's.
    #[test]
    fn ut_generation_for_is_independent_per_slave() {
        let mut log = RecordLog::default();
        log.push(UnitId(1), record(None));
        log.push(UnitId(1), record(None));
        log.push(UnitId(2), record(None));
        assert_eq!(log.generation_for(UnitId(1)), 2);
        assert_eq!(log.generation_for(UnitId(2)), 1);
    }

    /// (perf, no spec ID) — generation counts pushes, not ring occupancy: it keeps incrementing
    /// past the 200-entry eviction cap.
    #[test]
    fn ut_generation_keeps_incrementing_past_ring_eviction() {
        let mut log = RecordLog::default();
        for _ in 0..205u16 {
            log.push(UnitId(1), record(None));
        }
        assert_eq!(log.generation_for(UnitId(1)), 205);
    }

    /// (perf, no spec ID) — `recent_for` clones only the last `n` pushed records, oldest-of-the
    /// -batch first, matching `records_for`'s own tail order.
    #[test]
    fn ut_recent_for_returns_only_the_last_n_oldest_of_batch_first() {
        let mut log = RecordLog::default();
        for i in 0..10u16 {
            log.push(UnitId(1), record(Some(shape(Kind::HoldingRegister, i, 1))));
        }
        let recent = log.recent_for(UnitId(1), 3);
        let addrs: Vec<u16> = recent
            .iter()
            .map(|r| r.shape.as_ref().unwrap().address)
            .collect();
        assert_eq!(addrs, vec![7, 8, 9]);
    }

    /// (perf, no spec ID) — `recent_for` clamps `n` to the ring's current length rather than
    /// panicking or padding.
    #[test]
    fn ut_recent_for_clamps_n_to_ring_length() {
        let mut log = RecordLog::default();
        log.push(UnitId(1), record(None));
        log.push(UnitId(1), record(None));
        assert_eq!(log.recent_for(UnitId(1), 10).len(), 2);
    }

    /// (perf, no spec ID) — `recent_for` is empty (not absent/panicking) for a slave never
    /// pushed to, and empty for `n == 0` on a slave with records.
    #[test]
    fn ut_recent_for_empty_for_unpushed_slave_or_zero_n() {
        let mut log = RecordLog::default();
        log.push(UnitId(1), record(None));
        assert_eq!(log.recent_for(UnitId(9), 5), Vec::new());
        assert_eq!(log.recent_for(UnitId(1), 0), Vec::new());
    }
}
