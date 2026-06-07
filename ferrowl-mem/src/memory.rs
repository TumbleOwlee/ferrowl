use itertools::Itertools;

use crate::range::Range;
use crate::slice::Slice;
use crate::value::{Kind, Type, Value};
use std::collections::BTreeMap;
use std::{collections::HashMap, fmt::Debug, hash::Hash};

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

        let m = self.slices.get_mut(&id).unwrap();
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

    pub fn write(&mut self, id: K, ty: &Type, range: &Range, values: &[u16]) -> bool {
        if range.length() != values.len() || !self.writable(&id, ty, range) {
            return false;
        }

        let mut idx = 0;
        let mut range = range.clone();
        match self.slices.get_mut(&id) {
            Some(map) => {
                let entries: Vec<_> = map.iter_mut().collect();
                for (r, slice) in entries.into_iter() {
                    if r.start <= range.start && r.end > range.start {
                        let start = std::cmp::min(range.start, r.end);
                        let end = std::cmp::min(range.end, r.end);
                        let count = end - start;

                        if count != 0 {
                            slice.write(&Range::new(start, count), &values[idx..(idx + count)]);
                            range = Range::new(range.start + count, range.length() - count);
                            idx += count;
                        }
                    }
                }

                range.length() == 0
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

        let mut idx = 0;
        let mut range = range.clone();
        match self.slices.get_mut(&id) {
            Some(map) => {
                let entries: Vec<_> = map.iter_mut().collect();
                for (r, slice) in entries.into_iter() {
                    if r.start <= range.start && r.end > range.start {
                        let start = std::cmp::min(range.start, r.end);
                        let end = std::cmp::min(range.end, r.end);
                        let count = end - start;

                        if count != 0 {
                            slice.write_unchecked(
                                &Range::new(start, count),
                                &values[idx..(idx + count)],
                            );
                            range = Range::new(range.start + count, range.length() - count);
                            idx += count;
                        }
                    }
                }

                range.length() == 0
            }
            _ => false,
        }
    }

    pub fn writable(&mut self, id: &K, ty: &Type, range: &Range) -> bool {
        let mut range = range.clone();
        match self.slices.get_mut(id) {
            Some(map) => {
                let entries: Vec<_> = map.iter().collect();
                for (r, slice) in entries.into_iter() {
                    if r.start <= range.start && r.end > range.start {
                        let start = std::cmp::min(range.start, r.end);
                        let end = std::cmp::min(range.end, r.end);
                        let count = end - start;

                        if count != 0 {
                            if !slice.writable(ty, &Range::new(start, count)) {
                                return false;
                            }
                            range = Range::new(range.start + count, range.length() - count);
                        }
                    }
                }

                range.length() == 0
            }
            _ => false,
        }
    }

    pub fn read(&self, id: K, ty: &Type, range: &Range) -> Option<Vec<u16>> {
        if !self.readable(&id, ty, range) {
            return None;
        }

        let mut range = range.clone();
        match self.slices.get(&id) {
            Some(map) => {
                let mut output: Vec<u16> = Vec::with_capacity(range.length());
                let entries: Vec<_> = map.iter().collect();
                for (r, slice) in entries.into_iter() {
                    if r.start <= range.start && r.end > range.start {
                        let start = std::cmp::min(range.start, r.end);
                        let end = std::cmp::min(range.end, r.end);
                        let count = end - start;

                        if count != 0 {
                            if let Some(mut v) = slice.read(&Range::new(start, count)) {
                                output.append(&mut v)
                            };
                            range = Range::new(range.start + count, range.length() - count);
                        }
                    }
                }

                if range.length() == 0 {
                    Some(output)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn read_unchecked(&self, id: K, range: &Range) -> Option<Vec<u16>> {
        let mut range = range.clone();
        match self.slices.get(&id) {
            Some(map) => {
                let mut output: Vec<u16> = Vec::with_capacity(range.length());
                let entries: Vec<_> = map.iter().collect();
                for (r, slice) in entries.into_iter() {
                    if r.start <= range.start && r.end > range.start {
                        let start = std::cmp::min(range.start, r.end);
                        let end = std::cmp::min(range.end, r.end);
                        let count = end - start;

                        if count != 0 {
                            if let Some(mut v) = slice.read_unchecked(&Range::new(start, count)) {
                                output.append(&mut v)
                            };
                            range = Range::new(range.start + count, range.length() - count);
                        }
                    }
                }

                if range.length() == 0 {
                    Some(output)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn readable(&self, id: &K, ty: &Type, range: &Range) -> bool {
        let mut range = range.clone();
        match self.slices.get(id) {
            Some(map) => {
                let entries: Vec<_> = map.iter().collect();
                for (r, slice) in entries.into_iter() {
                    if r.start <= range.start && r.end > range.start {
                        let start = std::cmp::min(range.start, r.end);
                        let end = std::cmp::min(range.end, r.end);
                        let count = end - start;

                        if count != 0 {
                            if !slice.readable(ty, &Range::new(start, count)) {
                                return false;
                            };
                            range = Range::new(range.start + count, range.length() - count);
                        }
                    }
                }

                range.length() == 0
            }
            _ => false,
        }
    }
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
