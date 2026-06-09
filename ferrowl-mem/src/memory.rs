use itertools::Itertools;

use crate::range::Range;
use crate::slice::Slice;
use crate::value::{Kind, Type, Value};
use std::collections::BTreeMap;
use std::{collections::HashMap, fmt::Debug, hash::Hash};

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
    /// Ranges overlapping an existing slice are merged into it; compatible
    /// access kinds on overlapping cells are widened to `ReadWrite` (a read
    /// range over a write cell, or vice versa). Returns `false` if an overlap
    /// has an incompatible register [`Type`] or access combination, in which
    /// case the memory may be left partially updated.
    pub fn add_ranges(&mut self, id: K, kind: &Kind, ranges: &[Range]) -> bool {
        let mut ranges = ranges.iter().sorted_by(|r1, r2| r1.start.cmp(&r2.start));
        match self.slices.entry(id.clone()) {
            std::collections::hash_map::Entry::Vacant(e) => {
                if let Some(r) = ranges.next() {
                    let mut m = BTreeMap::new();
                    m.insert(r.clone(), Slice::from_range(kind, r.clone()));
                    e.insert(m);
                }
            }
            std::collections::hash_map::Entry::Occupied(_) => {}
        }

        // Only `None` when the entry was vacant and `ranges` was empty: nothing to do.
        let Some(m) = self.slices.get_mut(&id) else {
            return true;
        };
        for r in ranges {
            let val = m.iter_mut().find(|(range, _)| r.intersect(range).is_some());
            if let Some((range, _)) = val {
                let range = range.clone();
                let end = std::cmp::max(r.end, range.end);
                let start = std::cmp::min(r.start, range.start);
                let mut slice = m.remove(&range).unwrap();
                if let Some(rg) = r.intersect(&slice.range) {
                    for i in (rg.start - slice.range.start)..(rg.end - slice.range.start) {
                        if let Value::Read(t1, v1) = &slice.buffer[i] {
                            match kind {
                                Kind::Read(t2) if *t1 == *t2 => {}
                                Kind::Write(t2) if *t1 == *t2 => {
                                    slice.buffer[i] = Value::ReadWrite(*t1, *v1);
                                }
                                _ => {
                                    return false;
                                }
                            }
                        } else if let Value::Write(t1, v1) = &slice.buffer[i] {
                            match kind {
                                Kind::Read(t2) if *t1 == *t2 => {
                                    slice.buffer[i] = Value::ReadWrite(*t1, *v1);
                                }
                                Kind::Write(t2) if *t1 == *t2 => {}
                                _ => {
                                    return false;
                                }
                            }
                        } else if let Value::ReadWrite(t1, _v1) = &slice.buffer[i] {
                            match kind {
                                Kind::ReadWrite(t2) if *t1 == *t2 => {}
                                _ => {
                                    return false;
                                }
                            }
                        } else {
                            return false;
                        }
                    }
                }
                slice.extend(kind, &Range::new(range.end, end - range.end));
                m.insert(Range::new(start, end - start), slice);
            } else {
                m.insert(r.clone(), Slice::from_range(kind, r.clone()));
            }
        }
        true
    }

    /// Writes `values` to device `id` starting at `range.start`.
    ///
    /// Fails (returns `false`) if the value count does not match the range
    /// length, or if any addressed cell is not writable as type `ty`.
    pub fn write(&mut self, id: K, ty: &Type, range: &Range, values: &[u16]) -> bool {
        if range.length() != values.len() || !self.writable(&id, ty, range) {
            return false;
        }
        match self.slices.get_mut(&id) {
            Some(map) => {
                let mut idx = 0;
                walk_slices_mut(map, range, |slice, seg| {
                    let count = seg.length();
                    slice.write(&seg, &values[idx..(idx + count)]);
                    idx += count;
                    true
                })
            }
            _ => false,
        }
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

    /// Returns `true` if every cell in `range` exists and accepts writes of type `ty`.
    pub fn writable(&mut self, id: &K, ty: &Type, range: &Range) -> bool {
        match self.slices.get(id) {
            Some(map) => walk_slices(map, range, |slice, seg| slice.writable(ty, &seg)),
            _ => false,
        }
    }

    /// Reads the values in `range` from device `id`.
    ///
    /// Returns `None` if any addressed cell is missing or not readable as
    /// type `ty`.
    pub fn read(&self, id: K, ty: &Type, range: &Range) -> Option<Vec<u16>> {
        if !self.readable(&id, ty, range) {
            return None;
        }
        match self.slices.get(&id) {
            Some(map) => {
                let mut output: Vec<u16> = Vec::with_capacity(range.length());
                let covered = walk_slices(map, range, |slice, seg| {
                    if let Some(mut v) = slice.read(&seg) {
                        output.append(&mut v);
                    }
                    true
                });
                if covered { Some(output) } else { None }
            }
            _ => None,
        }
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

    /// Returns `true` if every cell in `range` exists and is readable as type `ty`.
    pub fn readable(&self, id: &K, ty: &Type, range: &Range) -> bool {
        match self.slices.get(id) {
            Some(map) => walk_slices(map, range, |slice, seg| slice.readable(ty, &seg)),
            _ => false,
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
    use crate::{Kind, Memory, Type, Value, range::Range};

    #[test]
    fn ut_memory() {
        assert_eq!(
            Value::default(&Kind::Read(Type::Coil)),
            Value::Read(Type::Coil, 0)
        );
        assert_eq!(
            Value::default(&Kind::Write(Type::Coil)),
            Value::Write(Type::Coil, 0)
        );
        assert_eq!(
            Value::default(&Kind::ReadWrite(Type::Coil)),
            Value::ReadWrite(Type::Coil, 0)
        );
    }

    #[test]
    fn ut_memory_add_ranges_1() {
        let mut memory = Memory::default();
        memory.add_ranges(1, &Kind::Read(Type::Coil), &[Range::new(0, 10)]);
        memory.add_ranges(1, &Kind::Read(Type::Coil), &[Range::new(5, 10)]);
        assert_eq!(memory.slices.len(), 1);
        let slices = memory.slices.get(&1);
        assert!(slices.is_some());
        let slices = slices.unwrap();
        assert!(slices.get(&Range::new(0, 15)).is_some());
    }

    #[test]
    fn ut_memory_add_ranges_2() {
        let mut memory = Memory::default();
        memory.add_ranges(1, &Kind::Read(Type::Coil), &[Range::new(0, 10)]);
        memory.add_ranges(1, &Kind::Read(Type::Coil), &[Range::new(5, 3)]);
        assert_eq!(memory.slices.len(), 1);
        let slices = memory.slices.get(&1);
        assert!(slices.is_some());
        let slices = slices.unwrap();
        assert!(slices.get(&Range::new(0, 10)).is_some());
    }

    #[test]
    fn ut_memory_add_ranges_3() {
        let mut memory = Memory::default();
        memory.add_ranges(1, &Kind::Read(Type::Coil), &[Range::new(10, 10)]);
        memory.add_ranges(1, &Kind::Read(Type::Coil), &[Range::new(5, 10)]);
        assert_eq!(memory.slices.len(), 1);
        let slices = memory.slices.get(&1);
        assert!(slices.is_some());
        let slices = slices.unwrap();
        assert!(slices.get(&Range::new(5, 15)).is_some());
    }

    #[test]
    fn ut_memory_add_ranges_4() {
        let mut memory = Memory::default();
        memory.add_ranges(1, &Kind::Read(Type::Coil), &[Range::new(15, 10)]);
        memory.add_ranges(1, &Kind::Read(Type::Coil), &[Range::new(5, 5)]);
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
        memory.add_ranges(1u8, &Kind::ReadWrite(Type::Coil), &[Range::new(0, 5)]);

        let values: Vec<u16> = vec![10, 20, 30, 40, 50];
        assert!(memory.write(1u8, &Type::Coil, &Range::new(0, 5), &values));

        let result = memory.read(1u8, &Type::Coil, &Range::new(0, 5));
        assert_eq!(result, Some(values));
    }

    #[test]
    fn ut_memory_write_read_partial_range() {
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(1u8, &Kind::ReadWrite(Type::Register), &[Range::new(0, 10)]);

        let values: Vec<u16> = vec![1, 2, 3];
        assert!(memory.write(1u8, &Type::Register, &Range::new(3, 3), &values));

        let result = memory.read(1u8, &Type::Register, &Range::new(3, 3));
        assert_eq!(result, Some(values));
    }

    #[test]
    fn ut_memory_write_wrong_length() {
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(1u8, &Kind::ReadWrite(Type::Coil), &[Range::new(0, 5)]);

        let values: Vec<u16> = vec![1, 2, 3]; // length 3, range length 5
        assert!(!memory.write(1u8, &Type::Coil, &Range::new(0, 5), &values));
    }

    #[test]
    fn ut_memory_write_unknown_key() {
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(1u8, &Kind::ReadWrite(Type::Coil), &[Range::new(0, 5)]);

        let values: Vec<u16> = vec![1, 2, 3, 4, 5];
        assert!(!memory.write(99u8, &Type::Coil, &Range::new(0, 5), &values));
    }

    #[test]
    fn ut_memory_read_unknown_key() {
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(1u8, &Kind::Read(Type::Coil), &[Range::new(0, 5)]);

        assert!(memory.read(99u8, &Type::Coil, &Range::new(0, 5)).is_none());
    }

    #[test]
    fn ut_memory_writable_wrong_type() {
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(1u8, &Kind::ReadWrite(Type::Coil), &[Range::new(0, 5)]);

        assert!(!memory.writable(&1u8, &Type::Register, &Range::new(0, 5)));
    }

    #[test]
    fn ut_memory_readable_wrong_type() {
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(1u8, &Kind::ReadWrite(Type::Register), &[Range::new(0, 5)]);

        assert!(!memory.readable(&1u8, &Type::Coil, &Range::new(0, 5)));
    }

    #[test]
    fn ut_memory_readonly_not_writable() {
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(1u8, &Kind::Read(Type::Coil), &[Range::new(0, 5)]);

        assert!(!memory.writable(&1u8, &Type::Coil, &Range::new(0, 5)));
    }

    #[test]
    fn ut_memory_writeonly_not_readable() {
        let mut memory: Memory<u8> = Memory::default();
        memory.add_ranges(1u8, &Kind::Write(Type::Coil), &[Range::new(0, 5)]);

        assert!(!memory.readable(&1u8, &Type::Coil, &Range::new(0, 5)));
    }
}
