use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    fmt::{Debug, Display},
};

/// A half-open address range `[start, end)`.
///
/// Ordering compares `start` first, then `end`, so ranges sort by position
/// in the address space (used as `BTreeMap` keys in [`Memory`](crate::Memory)).
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Range {
    /// First address contained in the range.
    pub(crate) start: usize,
    /// First address past the end of the range.
    pub(crate) end: usize,
}

impl<'de> Deserialize<'de> for Range {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RangeRepr {
            start: usize,
            end: usize,
        }
        let repr = RangeRepr::deserialize(deserializer)?;
        if repr.end < repr.start {
            return Err(serde::de::Error::custom(format!(
                "invalid Range: end ({}) < start ({})",
                repr.end, repr.start
            )));
        }
        Ok(Range {
            start: repr.start,
            end: repr.end,
        })
    }
}

impl Display for Range {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}, {})", self.start, self.end)
    }
}

impl Range {
    /// Creates the range `[start, start + size)`.
    pub fn new(start: usize, size: usize) -> Self {
        let end = start
            .checked_add(size)
            .expect("Range::new: start + size overflows usize");
        Self { start, end }
    }

    /// First address contained in the range.
    pub fn start(&self) -> usize {
        self.start
    }

    /// First address past the end of the range.
    pub fn end(&self) -> usize {
        self.end
    }

    /// Returns the number of addresses in the range.
    pub fn length(&self) -> usize {
        self.end - self.start
    }

    /// Returns the overlap of `self` and `range`, or `None` if they are
    /// disjoint or merely adjacent (zero-length overlap).
    pub fn intersect(&self, range: &Range) -> Option<Range> {
        let start = std::cmp::max(self.start, range.start);
        let end = std::cmp::min(self.end, range.end);
        if start >= end {
            None
        } else {
            Some(Range::new(start, end - start))
        }
    }
}

impl Ord for Range {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.start < other.start {
            Ordering::Less
        } else if other.start < self.start {
            Ordering::Greater
        } else if self.end < other.end {
            Ordering::Less
        } else if self.end > other.end {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    }
}

impl PartialOrd for Range {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::Range;

    #[test]
    fn ut_range_new() {
        let range = Range::new(123, 45);
        assert_eq!(range.start, 123);
        assert_eq!(range.end, 168);

        let range = Range::new(321, 45);
        assert_eq!(range.start, 321);
        assert_eq!(range.end, 366);
    }

    #[test]
    fn ut_range_length() {
        let range = Range::new(123, 45);
        assert_eq!(range.length(), 45);

        let range = Range::new(321, 54);
        assert_eq!(range.length(), 54);
    }

    #[test]
    fn ut_range_cmp() {
        let range0 = Range::new(100, 100);

        let range1 = Range::new(0, 50);
        let range2 = Range::new(200, 50);
        let range3 = Range::new(50, 100);
        let range4 = Range::new(125, 50);
        let range5 = Range::new(100, 50);
        let range6 = Range::new(150, 50);
        let range7 = Range::new(100, 100);

        assert_eq!(range0.cmp(&range1), Ordering::Greater);
        assert_eq!(range1.cmp(&range0), Ordering::Less);

        assert_eq!(range0.cmp(&range2), Ordering::Less);
        assert_eq!(range2.cmp(&range0), Ordering::Greater);

        assert_eq!(range0.cmp(&range3), Ordering::Greater);
        assert_eq!(range3.cmp(&range0), Ordering::Less);

        assert_eq!(range0.cmp(&range4), Ordering::Less);
        assert_eq!(range4.cmp(&range0), Ordering::Greater);

        assert_eq!(range0.cmp(&range5), Ordering::Greater);
        assert_eq!(range5.cmp(&range0), Ordering::Less);

        assert_eq!(range0.cmp(&range6), Ordering::Less);
        assert_eq!(range6.cmp(&range0), Ordering::Greater);

        assert_eq!(range0.cmp(&range7), Ordering::Equal);
    }

    #[test]
    fn ut_range_partial_cmp() {
        let range0 = Range::new(100, 100);

        let range1 = Range::new(0, 50);
        let range2 = Range::new(200, 50);
        let range3 = Range::new(50, 100);
        let range4 = Range::new(125, 50);
        let range5 = Range::new(100, 50);
        let range6 = Range::new(150, 50);
        let range7 = Range::new(100, 100);

        assert_eq!(range0.partial_cmp(&range1), Some(Ordering::Greater));
        assert_eq!(range1.partial_cmp(&range0), Some(Ordering::Less));

        assert_eq!(range0.partial_cmp(&range2), Some(Ordering::Less));
        assert_eq!(range2.partial_cmp(&range0), Some(Ordering::Greater));

        assert_eq!(range0.partial_cmp(&range3), Some(Ordering::Greater));
        assert_eq!(range3.partial_cmp(&range0), Some(Ordering::Less));

        assert_eq!(range0.partial_cmp(&range4), Some(Ordering::Less));
        assert_eq!(range4.partial_cmp(&range0), Some(Ordering::Greater));

        assert_eq!(range0.partial_cmp(&range5), Some(Ordering::Greater));
        assert_eq!(range5.partial_cmp(&range0), Some(Ordering::Less));

        assert_eq!(range0.partial_cmp(&range6), Some(Ordering::Less));
        assert_eq!(range6.partial_cmp(&range0), Some(Ordering::Greater));

        assert_eq!(range0.partial_cmp(&range7), Some(Ordering::Equal));
    }

    #[test]
    fn ut_range_intersect_overlap() {
        let a = Range::new(0, 10); // [0, 10)
        let b = Range::new(5, 10); // [5, 15)
        let r = a.intersect(&b).unwrap();
        assert_eq!(r.start, 5);
        assert_eq!(r.end, 10);
        assert_eq!(r.length(), 5);
    }

    #[test]
    fn ut_range_intersect_disjoint() {
        let a = Range::new(0, 5); // [0, 5)
        let b = Range::new(10, 5); // [10, 15)
        assert!(a.intersect(&b).is_none());
    }

    #[test]
    fn ut_range_intersect_adjacent_is_empty() {
        // [0, 5) and [5, 10) touch at exactly one point → zero-length intersection
        let a = Range::new(0, 5);
        let b = Range::new(5, 5);
        let r = a.intersect(&b);
        assert!(r.is_none());
    }

    #[test]
    fn ut_range_intersect_identical() {
        let a = Range::new(5, 10);
        let r = a.intersect(&a).unwrap();
        assert_eq!(r.start, 5);
        assert_eq!(r.end, 15);
        assert_eq!(r.length(), 10);
    }

    #[test]
    fn ut_range_intersect_contained() {
        // b is fully inside a
        let a = Range::new(0, 20);
        let b = Range::new(5, 5);
        let r = a.intersect(&b).unwrap();
        assert_eq!(r.start, 5);
        assert_eq!(r.end, 10);
        assert_eq!(r.length(), 5);
    }

    #[test]
    fn ut_range_intersect_symmetric() {
        let a = Range::new(3, 7);
        let b = Range::new(6, 8);
        assert_eq!(a.intersect(&b), b.intersect(&a));
    }

    #[test]
    fn ut_range_display() {
        assert_eq!(format!("{}", Range::new(10, 5)), "[10, 15)");
        assert_eq!(format!("{}", Range::new(0, 0)), "[0, 0)");
    }

    /// MB-R-028 — a range whose end precedes its start is rejected on deserialization.
    #[test]
    fn ut_range_deserialize_invalid_end_lt_start() {
        let json = r#"{"start": 10, "end": 5}"#;
        let result = serde_json::from_str::<Range>(json);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("invalid Range"));
    }

    #[test]
    #[should_panic(expected = "Range::new: start + size overflows usize")]
    fn ut_range_new_overflow() {
        let _ = Range::new(usize::MAX, 1);
    }
}
