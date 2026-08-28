//! Byte order of a multi-byte value across registers.

use serde::{Deserialize, Serialize};

/// Byte order of a multi-byte value across registers.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Endian {
    Little,
    Big,
}

crate::format::display_by_variant!(Endian {
    Little => "Little Endian",
    Big => "Big Endian",
});

#[cfg(test)]
mod tests {
    use super::Endian;

    #[test]
    /// MB-R-013 — every integer/float format carries a byte order of `Big` or `Little`.
    fn ut_endian_display() {
        assert_eq!(Endian::Little.to_string(), "Little Endian");
        assert_eq!(Endian::Big.to_string(), "Big Endian");
    }
}
