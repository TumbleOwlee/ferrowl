//! Contiguous runs of memory cells backing a [`Memory`](crate::Memory).

use crate::{range::Range, cell::CellType};
use itertools::Itertools;
use std::fmt::Debug;

use crate::cell::{CellKind, Cell, ValueRange};

/// A contiguous run of [`Cell`] cells covering an address [`Range`].
///
/// `buffer[i]` holds the cell at address `range.start + i`; the buffer
/// length always equals `range.length()`.
#[derive(Debug)]
pub struct Slice {
    /// The address range covered by this slice.
    pub range: Range,
    /// One cell per address, in ascending address order.
    pub buffer: Vec<Cell>,
}

impl Slice {
    /// Creates a slice covering `range` with zero-initialized cells of the
    /// given access `kind`.
    pub fn from_range(kind: &CellKind, range: Range) -> Self {
        Self {
            buffer: vec![Cell::default(kind); range.length()],
            range,
        }
    }

    /// Creates a slice from existing values, giving every cell the access
    /// rights of `kind`.
    pub fn from_value_range<'a>(kind: &CellKind, range: ValueRange<'a>) -> Self {
        Self {
            buffer: range
                .get_values()
                .iter()
                .map(|v| Cell::from_u16(kind, *v))
                .collect(),
            range: range.get_range(),
        }
    }

    /// Grows the slice by `range`, filling new cells with zero-initialized
    /// values of `kind`. `range` must be directly adjacent to the slice
    /// (ending at its start or starting at its end); returns `false` otherwise.
    pub fn extend(&mut self, kind: &CellKind, range: &Range) -> bool {
        // Extend slice while maintaining data consistency
        if range.end == self.range.start {
            let mut buffer: Vec<Cell> = vec![];
            std::mem::swap(&mut buffer, &mut self.buffer);
            self.buffer = itertools::repeat_n(Cell::default(kind), range.length())
                .chain(buffer)
                .collect();
            self.range = Range::new(range.start, range.length() + self.range.length());
            true
        } else if range.start == self.range.end {
            let mut buffer: Vec<Cell> = vec![];
            std::mem::swap(&mut buffer, &mut self.buffer);
            self.buffer = buffer
                .into_iter()
                .chain(itertools::repeat_n(Cell::default(kind), range.length()))
                .collect();
            self.range = Range::new(self.range.start, range.length() + self.range.length());
            true
        } else {
            false
        }
    }

    /// Returns `true` if `range` lies within the slice and every cell in it
    /// accepts writes of type `ty`.
    pub fn writable(&self, ty: &CellType, range: &Range) -> bool {
        let in_range = range.start >= self.range.start && range.end <= self.range.end;
        if in_range {
            self.buffer
                .iter()
                .skip(range.start - self.range.start)
                .take(range.length())
                .fold_while(true, |_, mem| match mem {
                    Cell::Write(t, _) | Cell::ReadWrite(t, _) if t == ty => {
                        itertools::FoldWhile::Continue(true)
                    }
                    _ => itertools::FoldWhile::Done(false),
                })
                .into_inner()
        } else {
            in_range
        }
    }

    /// Writes `values` into `range`, skipping read-only cells silently.
    /// Returns `false` if `range` is out of bounds or the value count does
    /// not match the range length.
    pub fn write(&mut self, range: &Range, values: &[u16]) -> bool {
        let in_range = range.start >= self.range.start && range.end <= self.range.end;
        let writable = range.length() == values.len() && in_range;
        if writable {
            for (mem, val) in self
                .buffer
                .iter_mut()
                .skip(range.start - self.range.start)
                .take(range.length())
                .zip(values.iter())
            {
                match mem {
                    Cell::Write(_, w) => *w = *val,
                    Cell::ReadWrite(_, rw) => *rw = *val,
                    Cell::Read(_, _) => {}
                };
            }
        }
        writable
    }

    /// Write values regardless of cell kind — forces writes to `Read` cells too.
    pub fn write_unchecked(&mut self, range: &Range, values: &[u16]) -> bool {
        let ok = range.start >= self.range.start
            && range.end <= self.range.end
            && range.length() == values.len();
        if ok {
            for (mem, val) in self
                .buffer
                .iter_mut()
                .skip(range.start - self.range.start)
                .take(range.length())
                .zip(values.iter())
            {
                match mem {
                    Cell::Write(_, w) => *w = *val,
                    Cell::ReadWrite(_, rw) => *rw = *val,
                    Cell::Read(_, r) => *r = *val,
                };
            }
        }
        ok
    }

    /// Reads the values in `range`. Returns `None` if `range` is out of
    /// bounds or contains a write-only cell.
    pub fn read(&self, range: &Range) -> Option<Vec<u16>> {
        let readable = range.start >= self.range.start && range.end <= self.range.end;
        if readable {
            self.buffer
                .iter()
                .skip(range.start - self.range.start)
                .take(range.length())
                .fold_while(Some(Vec::with_capacity(range.length())), |init, val| {
                    if let Some(mut values) = init {
                        match val {
                            Cell::Read(_, r) => values.push(*r),
                            Cell::ReadWrite(_, rw) => values.push(*rw),
                            Cell::Write(_, _) => return itertools::FoldWhile::Done(None),
                        };
                        itertools::FoldWhile::Continue(Some(values))
                    } else {
                        itertools::FoldWhile::Done(None)
                    }
                })
                .into_inner()
        } else {
            None
        }
    }

    /// Reads values regardless of cell kind — write-only cells return their
    /// stored value. Returns `None` only if `range` is out of bounds.
    pub fn read_unchecked(&self, range: &Range) -> Option<Vec<u16>> {
        let readable = range.start >= self.range.start && range.end <= self.range.end;
        if readable {
            self.buffer
                .iter()
                .skip(range.start - self.range.start)
                .take(range.length())
                .fold_while(Some(Vec::with_capacity(range.length())), |init, val| {
                    if let Some(mut values) = init {
                        match val {
                            Cell::Read(_, r) => values.push(*r),
                            Cell::ReadWrite(_, rw) => values.push(*rw),
                            Cell::Write(_, r) => values.push(*r),
                        };
                        itertools::FoldWhile::Continue(Some(values))
                    } else {
                        itertools::FoldWhile::Done(None)
                    }
                })
                .into_inner()
        } else {
            None
        }
    }

    /// Returns `true` if `range` lies within the slice and every cell in it
    /// is readable as type `ty`.
    pub fn readable(&self, ty: &CellType, range: &Range) -> bool {
        let in_range = range.start >= self.range.start && range.end <= self.range.end;
        if in_range {
            self.buffer
                .iter()
                .skip(range.start - self.range.start)
                .take(range.length())
                .fold_while(true, |_, mem| match mem {
                    Cell::Read(t, _) | Cell::ReadWrite(t, _) if t == ty => {
                        itertools::FoldWhile::Continue(true)
                    }
                    _ => itertools::FoldWhile::Done(false),
                })
                .into_inner()
        } else {
            in_range
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CellKind, Range, Slice, CellType, Cell, ValueRange};

    #[test]
    fn ut_slice_from_range() {
        let slice = Slice::from_range(&CellKind::Read(CellType::Coil), Range::new(123, 45));
        assert_eq!(slice.buffer.len(), 45);
        assert_eq!(slice.range.start, 123);
        assert_eq!(slice.range.end, 168);
        for value in slice.buffer.iter() {
            assert_eq!(*value, Cell::Read(CellType::Coil, 0));
        }

        let slice = Slice::from_range(&CellKind::Write(CellType::Coil), Range::new(123, 45));
        assert_eq!(slice.buffer.len(), 45);
        assert_eq!(slice.range.start, 123);
        assert_eq!(slice.range.end, 168);
        for value in slice.buffer.iter() {
            assert_eq!(*value, Cell::Write(CellType::Coil, 0));
        }

        let slice = Slice::from_range(&CellKind::ReadWrite(CellType::Coil), Range::new(123, 45));
        assert_eq!(slice.buffer.len(), 45);
        assert_eq!(slice.range.start, 123);
        assert_eq!(slice.range.end, 168);
        for value in slice.buffer.iter() {
            assert_eq!(*value, Cell::ReadWrite(CellType::Coil, 0));
        }
    }

    #[test]
    fn ut_slice_from_value_range() {
        let values: Vec<u16> = (1..46).collect();
        let slice = Slice::from_value_range(&CellKind::Read(CellType::Coil), ValueRange::new(123, &values));
        assert_eq!(slice.buffer.len(), 45);
        assert_eq!(slice.range.start, 123);
        assert_eq!(slice.range.end, 168);
        for (v1, v2) in slice.buffer.iter().zip(values) {
            assert_eq!(*v1, Cell::Read(CellType::Coil, v2));
        }

        let values: Vec<u16> = (1..46).collect();
        let slice =
            Slice::from_value_range(&CellKind::Write(CellType::Coil), ValueRange::new(123, &values));
        assert_eq!(slice.buffer.len(), 45);
        assert_eq!(slice.range.start, 123);
        assert_eq!(slice.range.end, 168);
        for (v1, v2) in slice.buffer.iter().zip(values) {
            assert_eq!(*v1, Cell::Write(CellType::Coil, v2));
        }

        let values: Vec<u16> = (1..46).collect();
        let slice =
            Slice::from_value_range(&CellKind::ReadWrite(CellType::Coil), ValueRange::new(123, &values));
        assert_eq!(slice.buffer.len(), 45);
        assert_eq!(slice.range.start, 123);
        assert_eq!(slice.range.end, 168);
        for (v1, v2) in slice.buffer.iter().zip(values) {
            assert_eq!(*v1, Cell::ReadWrite(CellType::Coil, v2));
        }
    }

    #[test]
    fn ut_slice_extend() {
        let mut slice = Slice::from_range(&CellKind::Read(CellType::Coil), Range::new(123, 45));
        assert_eq!(slice.buffer.len(), 45);
        assert_eq!(slice.range.start, 123);
        assert_eq!(slice.range.end, 168);
        assert!(slice.extend(&CellKind::Write(CellType::Coil), &Range::new(168, 32)));
        assert_eq!(slice.buffer.len(), 77);
        assert_eq!(slice.range.start, 123);
        assert_eq!(slice.range.end, 200);

        for (idx, value) in slice.buffer.iter().enumerate() {
            if idx < 45 {
                assert_eq!(*value, Cell::Read(CellType::Coil, 0));
            } else {
                assert_eq!(*value, Cell::Write(CellType::Coil, 0));
            }
        }

        assert!(slice.extend(&CellKind::ReadWrite(CellType::Coil), &Range::new(200, 50)));
        assert_eq!(slice.buffer.len(), 127);
        assert_eq!(slice.range.start, 123);
        assert_eq!(slice.range.end, 250);

        assert!(slice.extend(&CellKind::ReadWrite(CellType::Coil), &Range::new(0, 123)));
        assert_eq!(slice.buffer.len(), 250);
        assert_eq!(slice.range.start, 0);
        assert_eq!(slice.range.end, 250);

        for (idx, value) in slice.buffer.iter().enumerate() {
            if idx < 123 {
                assert_eq!(*value, Cell::ReadWrite(CellType::Coil, 0));
            } else if idx < 168 {
                assert_eq!(*value, Cell::Read(CellType::Coil, 0));
            } else if idx < 200 {
                assert_eq!(*value, Cell::Write(CellType::Coil, 0));
            } else if idx < 250 {
                assert_eq!(*value, Cell::ReadWrite(CellType::Coil, 0));
            } else {
                unreachable!();
            }
        }
    }

    #[test]
    fn ut_slice_writable() {
        let mut slice = Slice::from_range(&CellKind::Read(CellType::Coil), Range::new(123, 45));
        assert!(slice.extend(&CellKind::Write(CellType::Coil), &Range::new(168, 32)));
        assert!(slice.extend(&CellKind::Write(CellType::Coil), &Range::new(200, 50)));
        assert!(slice.extend(&CellKind::ReadWrite(CellType::Coil), &Range::new(250, 50)));

        assert!(!slice.writable(&CellType::Coil, &Range::new(130, 10)));
        assert!(slice.writable(&CellType::Coil, &Range::new(175, 10)));
        assert!(slice.writable(&CellType::Coil, &Range::new(210, 10)));
        assert!(slice.writable(&CellType::Coil, &Range::new(270, 10)));
    }

    #[test]
    fn ut_slice_write() {
        let mut slice = Slice::from_range(&CellKind::Read(CellType::Coil), Range::new(123, 45));
        assert!(slice.extend(&CellKind::Write(CellType::Coil), &Range::new(168, 32)));
        assert!(slice.extend(&CellKind::Write(CellType::Coil), &Range::new(200, 50)));
        assert!(slice.extend(&CellKind::ReadWrite(CellType::Coil), &Range::new(250, 50)));

        let values: Vec<u16> = (1..21).collect();
        assert!(slice.write(&Range::new(175, 20), &values));
        for (v1, v2) in slice.buffer[175 - slice.range.start..]
            .iter()
            .take(20)
            .zip(values.iter())
        {
            match v1 {
                Cell::Write(_, w) => assert_eq!(w, v2),
                Cell::Read(_, _) => unreachable!(),
                Cell::ReadWrite(_, rw) => assert_eq!(rw, v2),
            };
        }

        let values: Vec<u16> = (1..21).map(|c| 2 * c).collect();
        assert!(slice.write(&Range::new(190, 20), &values));
        for (v1, v2) in slice.buffer[190 - slice.range.start..]
            .iter()
            .take(20)
            .zip(values.iter())
        {
            match v1 {
                Cell::Write(_, w) => assert_eq!(w, v2),
                Cell::Read(_, _) => unreachable!(),
                Cell::ReadWrite(_, rw) => assert_eq!(rw, v2),
            };
        }

        assert!(!slice.writable(&CellType::Coil, &Range::new(0, 20)));
        assert!(!slice.writable(&CellType::Coil, &Range::new(160, 20)));
        assert!(!slice.writable(&CellType::Coil, &Range::new(500, 20)));

        let values: Vec<u16> = (1..21).map(|c| 3 * c).collect();
        assert!(!slice.write(&Range::new(0, 20), &values));

        let values: Vec<u16> = (1..21).map(|c| 4 * c).collect();
        assert!(slice.write(&Range::new(160, 20), &values));

        let values: Vec<u16> = (1..21).map(|c| 5 * c).collect();
        assert!(!slice.write(&Range::new(500, 20), &values));

        assert!(slice.writable(&CellType::Coil, &Range::new(190, 20)));
        assert!(!slice.writable(&CellType::Register, &Range::new(190, 20)));
    }

    #[test]
    fn ut_slice_readable() {
        let mut slice = Slice::from_range(&CellKind::Read(CellType::Coil), Range::new(123, 45));
        assert!(slice.extend(&CellKind::Write(CellType::Coil), &Range::new(168, 32)));
        assert!(slice.extend(&CellKind::Write(CellType::Coil), &Range::new(200, 50)));
        assert!(slice.extend(&CellKind::ReadWrite(CellType::Coil), &Range::new(250, 50)));

        assert!(slice.readable(&CellType::Coil, &Range::new(130, 10)));
        assert!(!slice.readable(&CellType::Coil, &Range::new(175, 10)));
        assert!(!slice.readable(&CellType::Coil, &Range::new(210, 10)));
        assert!(slice.readable(&CellType::Coil, &Range::new(270, 10)));
    }

    #[test]
    fn ut_slice_read() {
        let mut slice = Slice::from_range(&CellKind::Read(CellType::Coil), Range::new(123, 45));
        assert!(slice.extend(&CellKind::Write(CellType::Coil), &Range::new(168, 32)));
        assert!(slice.extend(&CellKind::Write(CellType::Coil), &Range::new(200, 50)));
        assert!(slice.extend(&CellKind::ReadWrite(CellType::Coil), &Range::new(250, 50)));

        let values: Vec<u16> = (1..21).collect();
        for (v1, v2) in slice.buffer[130 - slice.range.start..]
            .iter_mut()
            .zip(values)
        {
            match v1 {
                Cell::Write(_, _) => unreachable!(),
                Cell::Read(_, r) => *r = v2,
                Cell::ReadWrite(_, rw) => *rw = v2,
            };
        }

        let result = slice.read(&Range::new(130, 20));
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.len(), 20);

        for (v1, v2) in slice.buffer[130 - slice.range.start..]
            .iter()
            .take(20)
            .zip(result.iter())
        {
            match v1 {
                Cell::Write(_, _) => unreachable!(),
                Cell::Read(_, r) => assert_eq!(r, v2),
                Cell::ReadWrite(_, rw) => assert_eq!(rw, v2),
            };
        }

        let values: Vec<u16> = (1..21).map(|c| 2 * c).collect();
        for (v1, v2) in slice.buffer[250 - slice.range.start..]
            .iter_mut()
            .zip(values)
        {
            match v1 {
                Cell::Write(_, _) => unreachable!(),
                Cell::Read(_, r) => *r = v2,
                Cell::ReadWrite(_, rw) => *rw = v2,
            };
        }

        let result = slice.read(&Range::new(250, 20));
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.len(), 20);

        for (v1, v2) in slice.buffer[250 - slice.range.start..]
            .iter()
            .take(20)
            .zip(result.iter())
        {
            match v1 {
                Cell::Write(_, _) => unreachable!(),
                Cell::Read(_, r) => assert_eq!(r, v2),
                Cell::ReadWrite(_, rw) => assert_eq!(rw, v2),
            };
        }

        assert!(slice.read(&Range::new(0, 20)).is_none());
        assert!(slice.read(&Range::new(190, 20)).is_none());
        assert!(slice.read(&Range::new(500, 20)).is_none());
    }

    #[test]
    fn ut_slice_extend_non_adjacent() {
        let mut slice = Slice::from_range(&CellKind::Read(CellType::Coil), Range::new(100, 10));
        // Range neither ends at start nor starts at end -> no-op, returns false.
        assert!(!slice.extend(&CellKind::Write(CellType::Coil), &Range::new(200, 10)));
        assert_eq!(slice.buffer.len(), 10);
        assert_eq!(slice.range, Range::new(100, 10));
    }

    #[test]
    fn ut_slice_write_unchecked() {
        let mut slice = Slice::from_range(&CellKind::Read(CellType::Coil), Range::new(0, 5));
        slice.extend(&CellKind::Write(CellType::Coil), &Range::new(5, 3));
        slice.extend(&CellKind::ReadWrite(CellType::Coil), &Range::new(8, 2));

        // Forces writes into Read, Write and ReadWrite cells alike.
        let values: Vec<u16> = (1..=10).collect();
        assert!(slice.write_unchecked(&Range::new(0, 10), &values));
        let read = slice.read_unchecked(&Range::new(0, 10)).unwrap();
        assert_eq!(read, values);

        // Out of bounds and length-mismatch both fail.
        assert!(!slice.write_unchecked(&Range::new(0, 20), &(1..=20).collect::<Vec<u16>>()));
        assert!(!slice.write_unchecked(&Range::new(0, 5), &[1, 2, 3]));
    }

    #[test]
    fn ut_slice_read_unchecked() {
        let mut slice = Slice::from_range(&CellKind::Write(CellType::Register), Range::new(0, 4));
        // Write-only cells are unreadable via read() but read_unchecked returns stored values.
        assert!(slice.read(&Range::new(0, 4)).is_none());
        assert!(slice.write(&Range::new(0, 4), &[11, 22, 33, 44]));
        assert_eq!(slice.read_unchecked(&Range::new(0, 4)).unwrap(), vec![11, 22, 33, 44]);

        // Out of bounds -> None.
        assert!(slice.read_unchecked(&Range::new(0, 99)).is_none());
    }

    #[test]
    fn ut_slice_readable_out_of_range() {
        let slice = Slice::from_range(&CellKind::Read(CellType::Coil), Range::new(10, 5));
        assert!(!slice.readable(&CellType::Coil, &Range::new(0, 5)));
        assert!(!slice.writable(&CellType::Coil, &Range::new(0, 5)));
    }
}
