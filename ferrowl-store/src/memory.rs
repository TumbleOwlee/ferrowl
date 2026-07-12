use itertools::Itertools;

use crate::cell::{Cell, CellKind, CellType};
use crate::range::Range;
use crate::slice::Slice;
use std::collections::BTreeMap;
use std::{collections::HashMap, fmt::Debug, hash::Hash};

/// Why a [`Memory`] read or write operation failed.
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum MemoryError {
    /// The device key has no registered memory regions.
    #[error("key not registered")]
    UnknownKey,
    /// The requested address range is not registered or not readable as the given cell type.
    #[error("address not readable")]
    AddressNotReadable,
    /// The requested address range is not registered or not writable as the given cell type.
    #[error("address not writable")]
    AddressNotWritable,
    /// The number of supplied values does not match the range length.
    #[error("length mismatch: expected {expected}, got {got}")]
    LengthMismatch { expected: usize, got: usize },
}

/// Register storage for multiple devices, keyed by `K` (e.g. a unit/slave id).
///
/// Each key maps to a set of non-overlapping [`Slice`]s ordered by address.
/// Regions must be declared up front with [`add_ranges`](Self::add_ranges);
/// reads and writes only succeed on addresses that are fully covered by
/// declared slices and permit the requested access.
#[derive(Debug, Default)]
pub struct Memory<K>
where
    K: Hash + Eq + Clone + Default,
{
    slices: HashMap<K, BTreeMap<Range, Slice>>,
}

impl<K> Memory<K>
where
    K: Hash + Eq + Clone + Default,
{
    /// Declares memory regions for device `id` with the given access `kind`.
    ///
    /// A range is merged with *every* slice it intersects -- the range and all
    /// those slices become one slice spanning their union, keeping each merged
    /// slice's cells at their own addresses. Compatible access kinds on
    /// overlapping cells are widened to `ReadWrite` (a read
    /// range over a write cell, or vice versa). Returns `false` if an overlap
    /// has an incompatible register [`CellType`] or access combination, in which
    /// case `self`'s memory for `id` is left completely unchanged -- the call
    /// is all-or-nothing even when `ranges` has multiple entries.
    pub fn add_ranges(&mut self, id: K, kind: &CellKind, ranges: &[Range]) -> bool {
        let ranges = ranges.iter().sorted_by(|r1, r2| r1.start.cmp(&r2.start));

        // Work on a private copy of the device's slice map so an incompatible
        // overlap found partway through `ranges` can abort without leaving
        // `self.slices` partially merged. The copy also lets range N in this
        // call see the merged result of range N-1, matching the sequential
        // semantics multiple `add_ranges` calls would have.
        let mut m = self.slices.get(&id).cloned().unwrap_or_default();
        for r in ranges {
            // A range can bridge several existing slices (e.g. [5,25) over [0,10) and [20,30)), so
            // every intersecting slice is absorbed -- merging only the first would leave the rest
            // keyed at addresses the merged slice now also covers, breaking the non-overlap
            // invariant and shadowing their values.
            let targets: Vec<Range> = m
                .keys()
                .filter(|range| r.intersect(range).is_some())
                .cloned()
                .collect();
            let (Some(first), Some(last)) = (targets.first(), targets.last()) else {
                m.insert(r.clone(), Slice::from_range(kind, r.clone()));
                continue;
            };
            let start = std::cmp::min(r.start, first.start);
            let end = std::cmp::max(r.end, last.end);

            // Cells the union newly covers start out zero-initialized as `kind`; every absorbed
            // slice then writes its own cells back over them, preserving type and value.
            let mut merged = Slice::from_range(kind, Range::new(start, end - start));
            for t in &targets {
                let slice = m.remove(t).unwrap();
                let offset = slice.range.start - start;
                for (i, cell) in slice.buffer.into_iter().enumerate() {
                    merged.buffer[offset + i] = cell;
                }
            }

            for t in &targets {
                let Some(rg) = r.intersect(t) else { continue };
                for i in (rg.start - start)..(rg.end - start) {
                    // Same register type: a Read+Write (in either order) widens to ReadWrite;
                    // matching access is a no-op. Any other combination is incompatible.
                    match (&merged.buffer[i], kind) {
                        (Cell::Read(t1, _), CellKind::Read(t2)) if t1 == t2 => {}
                        (Cell::Write(t1, _), CellKind::Write(t2)) if t1 == t2 => {}
                        (Cell::ReadWrite(t1, _), CellKind::ReadWrite(t2)) if t1 == t2 => {}
                        (Cell::Read(t1, v1), CellKind::Write(t2)) if t1 == t2 => {
                            merged.buffer[i] = Cell::ReadWrite(*t1, *v1);
                        }
                        (Cell::Write(t1, v1), CellKind::Read(t2)) if t1 == t2 => {
                            merged.buffer[i] = Cell::ReadWrite(*t1, *v1);
                        }
                        // `self.slices` still holds the pre-call map: safe to bail here.
                        _ => return false,
                    }
                }
            }
            m.insert(Range::new(start, end - start), merged);
        }

        // Every range validated and merged cleanly against the copy: commit it.
        if !m.is_empty() {
            self.slices.insert(id, m);
        }
        true
    }

    /// Writes `values` to device `id` starting at `range.start`.
    ///
    /// Returns [`MemoryError::LengthMismatch`] if the value count does not match the range length,
    /// [`MemoryError::UnknownKey`] if `id` has no registered regions, and
    /// [`MemoryError::AddressNotWritable`] if any addressed cell is not writable as type `ty`.
    pub fn write(
        &mut self,
        id: K,
        ty: &CellType,
        range: &Range,
        values: &[u16],
    ) -> Result<(), MemoryError> {
        if range.length() != values.len() {
            return Err(MemoryError::LengthMismatch {
                expected: range.length(),
                got: values.len(),
            });
        }
        self.writable(&id, ty, range)?;
        let map = self.slices.get_mut(&id).unwrap();
        let mut idx = 0;
        walk_slices_mut(map, range, |slice, seg| {
            let count = seg.length();
            slice.write(&seg, &values[idx..(idx + count)]);
            idx += count;
            true
        });
        Ok(())
    }

    /// Write values regardless of cell kind — bypasses the `writable` check and forces writes to
    /// `Read` cells too. Intended for administrative UI writes; do not use on the hot path.
    pub fn write_unchecked(&mut self, id: K, range: &Range, values: &[u16]) -> bool {
        if range.length() != values.len() {
            return false;
        }
        match self.slices.get_mut(&id) {
            Some(map) => {
                let mut idx = 0;
                walk_slices_mut(map, range, |slice, seg| {
                    let count = seg.length();
                    slice.write_unchecked(&seg, &values[idx..(idx + count)]);
                    idx += count;
                    true
                })
            }
            _ => false,
        }
    }

    /// Returns `Ok(())` if every cell in `range` exists and accepts writes of type `ty`,
    /// otherwise returns [`MemoryError::UnknownKey`] or [`MemoryError::AddressNotWritable`].
    pub fn writable(&mut self, id: &K, ty: &CellType, range: &Range) -> Result<(), MemoryError> {
        match self.slices.get(id) {
            Some(map) => {
                if walk_slices(map, range, |slice, seg| slice.writable(ty, &seg)) {
                    Ok(())
                } else {
                    Err(MemoryError::AddressNotWritable)
                }
            }
            None => Err(MemoryError::UnknownKey),
        }
    }

    /// Reads the values in `range` from device `id`.
    ///
    /// Returns [`MemoryError::UnknownKey`] if `id` has no registered regions and
    /// [`MemoryError::AddressNotReadable`] if any addressed cell is missing or not
    /// readable as type `ty`.
    pub fn read(&self, id: K, ty: &CellType, range: &Range) -> Result<Vec<u16>, MemoryError> {
        self.readable(&id, ty, range)?;
        let map = self.slices.get(&id).unwrap();
        let mut output: Vec<u16> = Vec::with_capacity(range.length());
        walk_slices(map, range, |slice, seg| {
            if let Some(mut v) = slice.read(&seg) {
                output.append(&mut v);
            }
            true
        });
        Ok(output)
    }

    /// Reads values regardless of cell kind — returns the stored value of
    /// `Write` cells too. Returns `None` only if `range` is not fully covered.
    pub fn read_unchecked(&self, id: K, range: &Range) -> Option<Vec<u16>> {
        match self.slices.get(&id) {
            Some(map) => {
                let mut output: Vec<u16> = Vec::with_capacity(range.length());
                let covered = walk_slices(map, range, |slice, seg| {
                    if let Some(mut v) = slice.read_unchecked(&seg) {
                        output.append(&mut v);
                    }
                    true
                });
                if covered { Some(output) } else { None }
            }
            _ => None,
        }
    }

    /// Returns `Ok(())` if every cell in `range` exists and is readable as type `ty`,
    /// otherwise returns [`MemoryError::UnknownKey`] or [`MemoryError::AddressNotReadable`].
    pub fn readable(&self, id: &K, ty: &CellType, range: &Range) -> Result<(), MemoryError> {
        match self.slices.get(id) {
            Some(map) => {
                if walk_slices(map, range, |slice, seg| slice.readable(ty, &seg)) {
                    Ok(())
                } else {
                    Err(MemoryError::AddressNotReadable)
                }
            }
            None => Err(MemoryError::UnknownKey),
        }
    }
}

/// Walk the slices intersecting `range` in ascending order, calling `f` with each slice and the
/// sub-range that falls inside it. `f` returns `false` to abort (helper then returns `false`).
/// Returns `true` iff the visited slices fully covered `range`.
fn walk_slices<F>(map: &BTreeMap<Range, Slice>, range: &Range, mut f: F) -> bool
where
    F: FnMut(&Slice, Range) -> bool,
{
    let mut range = range.clone();
    for (r, slice) in map.iter() {
        if r.start <= range.start && r.end > range.start {
            let start = std::cmp::min(range.start, r.end);
            let end = std::cmp::min(range.end, r.end);
            let count = end - start;
            if count != 0 {
                if !f(slice, Range::new(start, count)) {
                    return false;
                }
                range = Range::new(range.start + count, range.length() - count);
            }
        }
    }
    range.length() == 0
}

/// Mutable counterpart of [`walk_slices`] for the write paths.
fn walk_slices_mut<F>(map: &mut BTreeMap<Range, Slice>, range: &Range, mut f: F) -> bool
where
    F: FnMut(&mut Slice, Range) -> bool,
{
    let mut range = range.clone();
    for (r, slice) in map.iter_mut() {
        if r.start <= range.start && r.end > range.start {
            let start = std::cmp::min(range.start, r.end);
            let end = std::cmp::min(range.end, r.end);
            let count = end - start;
            if count != 0 {
                if !f(slice, Range::new(start, count)) {
                    return false;
                }
                range = Range::new(range.start + count, range.length() - count);
            }
        }
    }
    range.length() == 0
}

#[cfg(test)]
mod tests {
    use crate::{Cell, CellKind, CellType, Memory, MemoryError, range::Range};

    #[test]
    fn ut_memory() {
        assert_eq!(
            Cell::default(&CellKind::Read(CellType::Coil)),
            Cell::Read(CellType::Coil, 0)
        );
        assert_eq!(
            Cell::default(&CellKind::Write(CellType::Coil)),
            Cell::Write(CellType::Coil, 0)
        );
        assert_eq!(
            Cell::default(&CellKind::ReadWrite(CellType::Coil)),
            Cell::ReadWrite(CellType::Coil, 0)
        );
    }

    #[test]
    fn ut_memory_add_ranges_1() {
        let mut memory = Memory::default();
        memory.add_ranges(1, &CellKind::Read(CellType::Coil), &[Range::new(0, 10)]);
        memory.add_ranges(1, &CellKind::Read(CellType::Coil), &[Range::new(5, 10)]);
        assert_eq!(memory.slices.len(), 1);
        let slices = memory.slices.get(&1);
        assert!(slices.is_some());
        let slices = slices.unwrap();
        assert!(slices.get(&Range::new(0, 15)).is_some());
    }

    #[test]
    fn ut_memory_add_ranges_2() {
        let mut memory = Memory::default();
        memory.add_ranges(1, &CellKind::Read(CellType::Coil), &[Range::new(0, 10)]);
        memory.add_ranges(1, &CellKind::Read(CellType::Coil), &[Range::new(5, 3)]);
        assert_eq!(memory.slices.len(), 1);
        let slices = memory.slices.get(&1);
        assert!(slices.is_some());
        let slices = slices.unwrap();
        assert!(slices.get(&Range::new(0, 10)).is_some());
    }

    #[test]
    fn ut_memory_add_ranges_bridging_two_slices_merges_all() {
        let kind = CellKind::ReadWrite(CellType::Register);
        let mut memory = Memory::default();
        memory.add_ranges(1, &kind, &[Range::new(0, 10), Range::new(20, 10)]);
        memory
            .write(1, &CellType::Register, &Range::new(20, 5), &[7; 5])
            .unwrap();

        // Bridges [0,10) and [20,30): all three must collapse into one slice.
        assert!(memory.add_ranges(1, &kind, &[Range::new(5, 20)]));

        let slices = memory.slices.get(&1).unwrap();
        assert_eq!(slices.len(), 1);
        assert!(slices.get(&Range::new(0, 30)).is_some());
        // The absorbed slice's values survive at their own addresses.
        assert_eq!(
            memory
                .read(1, &CellType::Register, &Range::new(20, 5))
                .unwrap(),
            vec![7; 5]
        );
        // The gap the new range newly covers is zero-initialized and addressable.
        assert_eq!(
            memory
                .read(1, &CellType::Register, &Range::new(10, 10))
                .unwrap(),
            vec![0; 10]
        );
    }

    #[test]
    fn ut_memory_add_ranges_3() {
        let mut memory = Memory::default();
        memory.add_ranges(1, &CellKind::Read(CellType::Coil), &[Range::new(10, 10)]);
        memory.add_ranges(1, &CellKind::Read(CellType::Coil), &[Range::new(5, 10)]);
        assert_eq!(memory.slices.len(), 1);
        let slices = memory.slices.get(&1);
        assert!(slices.is_some());
        let slices = slices.unwrap();
        assert!(slices.get(&Range::new(5, 15)).is_some());
    }

    #[test]
    fn ut_memory_add_ranges_4() {
        let mut memory = Memory::default();
        memory.add_ranges(1, &CellKind::Read(CellType::Coil), &[Range::new(15, 10)]);
        memory.add_ranges(1, &CellKind::Read(CellType::Coil), &[Range::new(5, 5)]);
        assert_eq!(memory.slices.len(), 1);
        let slices = memory.slices.get(&1);
        assert!(slices.is_some());
        let slices = slices.unwrap();
        assert!(slices.get(&Range::new(15, 10)).is_some());
        assert!(slices.get(&Range::new(5, 5)).is_some());
    }

    #[test]
    fn ut_memory_write_read_combined() {
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(
            1u8,
            &CellKind::ReadWrite(CellType::Coil),
            &[Range::new(0, 5)],
        );

        let values: Vec<u16> = vec![10, 20, 30, 40, 50];
        assert!(
            memory
                .write(1u8, &CellType::Coil, &Range::new(0, 5), &values)
                .is_ok()
        );

        let result = memory.read(1u8, &CellType::Coil, &Range::new(0, 5));
        assert_eq!(result.unwrap(), values);
    }

    #[test]
    fn ut_memory_write_read_partial_range() {
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(
            1u8,
            &CellKind::ReadWrite(CellType::Register),
            &[Range::new(0, 10)],
        );

        let values: Vec<u16> = vec![1, 2, 3];
        assert!(
            memory
                .write(1u8, &CellType::Register, &Range::new(3, 3), &values)
                .is_ok()
        );

        let result = memory.read(1u8, &CellType::Register, &Range::new(3, 3));
        assert_eq!(result.unwrap(), values);
    }

    #[test]
    fn ut_memory_write_wrong_length() {
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(
            1u8,
            &CellKind::ReadWrite(CellType::Coil),
            &[Range::new(0, 5)],
        );

        let values: Vec<u16> = vec![1, 2, 3]; // length 3, range length 5
        assert_eq!(
            memory.write(1u8, &CellType::Coil, &Range::new(0, 5), &values),
            Err(MemoryError::LengthMismatch {
                expected: 5,
                got: 3
            })
        );
    }

    #[test]
    fn ut_memory_write_unknown_key() {
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(
            1u8,
            &CellKind::ReadWrite(CellType::Coil),
            &[Range::new(0, 5)],
        );

        let values: Vec<u16> = vec![1, 2, 3, 4, 5];
        assert_eq!(
            memory.write(99u8, &CellType::Coil, &Range::new(0, 5), &values),
            Err(MemoryError::UnknownKey)
        );
    }

    #[test]
    fn ut_memory_read_unknown_key() {
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(1u8, &CellKind::Read(CellType::Coil), &[Range::new(0, 5)]);

        assert_eq!(
            memory.read(99u8, &CellType::Coil, &Range::new(0, 5)),
            Err(MemoryError::UnknownKey)
        );
    }

    #[test]
    fn ut_memory_writable_wrong_type() {
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(
            1u8,
            &CellKind::ReadWrite(CellType::Coil),
            &[Range::new(0, 5)],
        );

        assert_eq!(
            memory.writable(&1u8, &CellType::Register, &Range::new(0, 5)),
            Err(MemoryError::AddressNotWritable)
        );
    }

    #[test]
    fn ut_memory_readable_wrong_type() {
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(
            1u8,
            &CellKind::ReadWrite(CellType::Register),
            &[Range::new(0, 5)],
        );

        assert_eq!(
            memory.readable(&1u8, &CellType::Coil, &Range::new(0, 5)),
            Err(MemoryError::AddressNotReadable)
        );
    }

    #[test]
    fn ut_memory_readonly_not_writable() {
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(1u8, &CellKind::Read(CellType::Coil), &[Range::new(0, 5)]);

        assert_eq!(
            memory.writable(&1u8, &CellType::Coil, &Range::new(0, 5)),
            Err(MemoryError::AddressNotWritable)
        );
    }

    #[test]
    fn ut_memory_writeonly_not_readable() {
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(1u8, &CellKind::Write(CellType::Coil), &[Range::new(0, 5)]);

        assert_eq!(
            memory.readable(&1u8, &CellType::Coil, &Range::new(0, 5)),
            Err(MemoryError::AddressNotReadable)
        );
    }

    #[test]
    fn ut_memory_add_ranges_empty() {
        let mut memory: Memory<u8> = Memory::default();
        // Vacant key with no ranges: nothing to insert, still succeeds.
        assert!(memory.add_ranges(1u8, &CellKind::Read(CellType::Coil), &[]));
        assert!(!memory.slices.contains_key(&1u8));
    }

    #[test]
    fn ut_memory_widen_read_then_write() {
        // Read cell + overlapping Write of same type -> ReadWrite.
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(
            1u8,
            &CellKind::Read(CellType::Register),
            &[Range::new(0, 5)],
        );
        assert!(memory.add_ranges(
            1u8,
            &CellKind::Write(CellType::Register),
            &[Range::new(0, 5)]
        ));
        assert!(
            memory
                .readable(&1u8, &CellType::Register, &Range::new(0, 5))
                .is_ok()
        );
        assert!(
            memory
                .writable(&1u8, &CellType::Register, &Range::new(0, 5))
                .is_ok()
        );
    }

    #[test]
    fn ut_memory_widen_write_then_read() {
        // Write cell + overlapping Read of same type -> ReadWrite.
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(1u8, &CellKind::Write(CellType::Coil), &[Range::new(0, 5)]);
        assert!(memory.add_ranges(1u8, &CellKind::Read(CellType::Coil), &[Range::new(0, 5)]));
        assert!(
            memory
                .readable(&1u8, &CellType::Coil, &Range::new(0, 5))
                .is_ok()
        );
        assert!(
            memory
                .writable(&1u8, &CellType::Coil, &Range::new(0, 5))
                .is_ok()
        );
    }

    #[test]
    fn ut_memory_widen_write_then_write_noop() {
        // Write cell + overlapping Write of same type -> unchanged, still write-only.
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(1u8, &CellKind::Write(CellType::Coil), &[Range::new(0, 5)]);
        assert!(memory.add_ranges(1u8, &CellKind::Write(CellType::Coil), &[Range::new(0, 5)]));
        assert!(
            memory
                .readable(&1u8, &CellType::Coil, &Range::new(0, 5))
                .is_err()
        );
        assert!(
            memory
                .writable(&1u8, &CellType::Coil, &Range::new(0, 5))
                .is_ok()
        );
    }

    #[test]
    fn ut_memory_widen_readwrite_then_readwrite_noop() {
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(
            1u8,
            &CellKind::ReadWrite(CellType::Coil),
            &[Range::new(0, 5)],
        );
        assert!(memory.add_ranges(
            1u8,
            &CellKind::ReadWrite(CellType::Coil),
            &[Range::new(0, 5)]
        ));
        assert!(
            memory
                .readable(&1u8, &CellType::Coil, &Range::new(0, 5))
                .is_ok()
        );
        assert!(
            memory
                .writable(&1u8, &CellType::Coil, &Range::new(0, 5))
                .is_ok()
        );
    }

    #[test]
    fn ut_memory_widen_incompatible_read_cell() {
        // Read cell + overlapping incompatible access (wrong type) -> false.
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(1u8, &CellKind::Read(CellType::Coil), &[Range::new(0, 5)]);
        assert!(!memory.add_ranges(
            1u8,
            &CellKind::Write(CellType::Register),
            &[Range::new(0, 5)]
        ));
    }

    #[test]
    fn ut_memory_widen_incompatible_write_cell() {
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(1u8, &CellKind::Write(CellType::Coil), &[Range::new(0, 5)]);
        assert!(!memory.add_ranges(
            1u8,
            &CellKind::Read(CellType::Register),
            &[Range::new(0, 5)]
        ));
    }

    #[test]
    fn ut_memory_add_ranges_partial_failure_leaves_map_untouched() {
        // Multi-range call: range1 merges cleanly, range2 hits an incompatible
        // overlap. The whole call must fail atomically -- range1's merge must
        // not have been committed to `self.slices`.
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(1u8, &CellKind::Read(CellType::Coil), &[Range::new(0, 10)]);
        memory.add_ranges(1u8, &CellKind::Write(CellType::Coil), &[Range::new(20, 10)]);

        let before = format!("{:?}", memory.slices.get(&1u8).unwrap());

        // range1 = [5,15) overlaps the Read(Coil) slice compatibly (widens to
        // ReadWrite); range2 = [20,30) overlaps the Write(Coil) slice with an
        // incompatible type (Register vs Coil) and must fail the whole call.
        let ok = memory.add_ranges(
            1u8,
            &CellKind::Read(CellType::Register),
            &[Range::new(5, 10), Range::new(20, 10)],
        );
        assert!(!ok);

        let after = format!("{:?}", memory.slices.get(&1u8).unwrap());
        assert_eq!(before, after);
        assert!(
            memory
                .readable(&1u8, &CellType::Coil, &Range::new(0, 10))
                .is_ok()
        );
        assert!(
            memory
                .writable(&1u8, &CellType::Coil, &Range::new(0, 10))
                .is_err()
        );
    }

    #[test]
    fn ut_memory_add_ranges_partial_failure_third_range_leaves_map_untouched() {
        // Three ranges in one call: range1 and range2 each merge cleanly
        // against separate pre-existing slices (range2 merging against a
        // snapshot that already reflects range1's merge), then range3 hits
        // an incompatible overlap. The whole call must fail atomically, with
        // none of range1's or range2's merges committed either.
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(1u8, &CellKind::Read(CellType::Coil), &[Range::new(0, 10)]);
        memory.add_ranges(1u8, &CellKind::Write(CellType::Coil), &[Range::new(10, 10)]);
        memory.add_ranges(1u8, &CellKind::Write(CellType::Coil), &[Range::new(30, 10)]);

        let before = format!("{:?}", memory.slices.get(&1u8).unwrap());

        // range1 = [5,15) widens the Read(Coil)/Write(Coil) slices at [0,10)
        // and [10,20) (compatible, merges into one ReadWrite run). range2 =
        // [20,25) is a fresh disjoint insert. range3 = [30,40) overlaps the
        // Write(Coil) slice at [30,40) with an incompatible Register type.
        let ok = memory.add_ranges(
            1u8,
            &CellKind::Read(CellType::Register),
            &[Range::new(5, 10), Range::new(20, 5), Range::new(30, 10)],
        );
        assert!(!ok);

        let after = format!("{:?}", memory.slices.get(&1u8).unwrap());
        assert_eq!(before, after, "map must be untouched after a failing call");
        assert!(
            memory
                .writable(&1u8, &CellType::Coil, &Range::new(10, 10))
                .is_ok(),
            "range1's merge must not have been committed"
        );
        assert!(
            memory
                .readable(&1u8, &CellType::Coil, &Range::new(20, 5))
                .is_err(),
            "range2's fresh insert must not have been committed"
        );
    }

    #[test]
    fn ut_memory_widen_incompatible_readwrite_cell() {
        // ReadWrite cell + non-ReadWrite overlapping access -> false.
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(
            1u8,
            &CellKind::ReadWrite(CellType::Coil),
            &[Range::new(0, 5)],
        );
        assert!(!memory.add_ranges(1u8, &CellKind::Read(CellType::Coil), &[Range::new(0, 5)]));
    }

    #[test]
    fn ut_memory_write_read_unchecked() {
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(
            1u8,
            &CellKind::Read(CellType::Register),
            &[Range::new(0, 5)],
        );

        // Checked write fails on read-only cells; unchecked forces it.
        let values: Vec<u16> = vec![5, 6, 7, 8, 9];
        assert!(
            memory
                .write(1u8, &CellType::Register, &Range::new(0, 5), &values)
                .is_err()
        );
        assert!(memory.write_unchecked(1u8, &Range::new(0, 5), &values));
        assert_eq!(memory.read_unchecked(1u8, &Range::new(0, 5)), Some(values));

        // Length mismatch and unknown key both fail / return None.
        assert!(!memory.write_unchecked(1u8, &Range::new(0, 5), &[1, 2]));
        assert!(!memory.write_unchecked(9u8, &Range::new(0, 5), &[1, 2, 3, 4, 5]));
        assert!(memory.read_unchecked(9u8, &Range::new(0, 5)).is_none());
    }

    #[test]
    fn ut_memory_walk_multiple_slices() {
        // Two adjacent but non-overlapping slices: a read/write spanning both
        // walks each slice in turn.
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(
            1u8,
            &CellKind::ReadWrite(CellType::Register),
            &[Range::new(0, 5)],
        );
        memory.add_ranges(
            1u8,
            &CellKind::ReadWrite(CellType::Register),
            &[Range::new(5, 5)],
        );
        assert_eq!(memory.slices.get(&1u8).unwrap().len(), 2);

        let values: Vec<u16> = (1..=10).collect();
        assert!(
            memory
                .write(1u8, &CellType::Register, &Range::new(0, 10), &values)
                .is_ok()
        );
        assert_eq!(
            memory
                .read(1u8, &CellType::Register, &Range::new(0, 10))
                .unwrap(),
            values
        );

        // A range living only in the second slice: the first slice is skipped.
        assert!(
            memory
                .write(1u8, &CellType::Register, &Range::new(7, 3), &[70, 80, 90])
                .is_ok()
        );
        assert_eq!(
            memory
                .read(1u8, &CellType::Register, &Range::new(7, 3))
                .unwrap(),
            vec![70, 80, 90]
        );
    }
}
