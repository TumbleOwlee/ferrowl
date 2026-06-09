//! Helper traits for converting between byte streams and register words.

use std::borrow::Borrow;

/// Builds an integer from a big-endian stream of bytes.
///
/// Implemented for every `Iterator<Item = u8>`: each byte shifts the
/// accumulator left by 8 bits, so the first byte ends up most significant.
/// Reverse the iterator first to parse little-endian data.
pub trait ParseFromU8<V> {
    /// Consumes the iterator and folds its bytes into a `V`.
    fn parse(self) -> V;
}

impl<I, V> ParseFromU8<V> for I
where
    I: Iterator<Item = u8>,
    V: Default + std::ops::Shl<usize, Output = V> + std::ops::AddAssign<V> + std::convert::From<u8>,
{
    fn parse(self) -> V {
        let mut output = V::default();
        for v in self {
            output = output << 8;
            output += v.into();
        }
        output
    }
}

/// Packs a byte stream into register words.
///
/// Implemented for every iterator over `u8` (or references to it): bytes are
/// paired big-endian into `u16` words; a trailing odd byte becomes the high
/// byte of the final word.
pub trait IntoVec<T> {
    /// Consumes the iterator and packs its items into a `Vec<T>`.
    fn into_vec(self) -> anyhow::Result<Vec<T>>;
}

impl<I, T> IntoVec<u16> for I
where
    I: Iterator<Item = T>,
    T: Borrow<u8>,
{
    fn into_vec(self) -> anyhow::Result<Vec<u16>> {
        let mut values = vec![];
        let mut idx: usize = 0;
        let mut val: u16 = 0;
        for v in self {
            val |= *v.borrow() as u16;
            idx += 1;
            if idx.is_multiple_of(2) {
                values.push(val);
                val = 0;
            } else {
                val <<= 8;
            }
        }
        if !idx.is_multiple_of(2) {
            values.push(val);
        }
        Ok(values)
    }
}

#[cfg(test)]
mod tests {
    use super::{IntoVec, ParseFromU8};

    #[test]
    fn ut_into_vec_empty() {
        let empty: Vec<u8> = vec![];
        let result = empty.iter().into_vec().unwrap();
        assert_eq!(result, Vec::<u16>::new());
    }

    #[test]
    fn ut_into_vec_two_bytes() {
        let bytes = [0x01u8, 0x02u8];
        let result = bytes.iter().into_vec().unwrap();
        assert_eq!(result, vec![0x0102u16]);
    }

    #[test]
    fn ut_into_vec_four_bytes() {
        let bytes = [0x01u8, 0x02u8, 0x03u8, 0x04u8];
        let result = bytes.iter().into_vec().unwrap();
        assert_eq!(result, vec![0x0102u16, 0x0304u16]);
    }

    #[test]
    fn ut_into_vec_odd_bytes() {
        // Odd byte count: trailing byte goes into the high byte of the final register
        let bytes = [0x01u8, 0x02u8, 0x03u8];
        let result = bytes.iter().into_vec().unwrap();
        assert_eq!(result, vec![0x0102u16, 0x0300u16]);
    }

    #[test]
    fn ut_parse_from_u8_u16() {
        let bytes = vec![0x01u8, 0x02u8];
        let result: u16 = bytes.into_iter().parse();
        assert_eq!(result, 0x0102u16);
    }

    #[test]
    fn ut_parse_from_u8_u32() {
        let bytes = vec![0x01u8, 0x02u8, 0x03u8, 0x04u8];
        let result: u32 = bytes.into_iter().parse();
        assert_eq!(result, 0x01020304u32);
    }

    #[test]
    fn ut_parse_from_u8_reversed() {
        // Bytes in little-endian order: [0x01, 0x02, 0x03, 0x04] (LSB first)
        // Reversed: [0x04, 0x03, 0x02, 0x01], parsed big-endian = 0x04030201
        let bytes = vec![0x01u8, 0x02u8, 0x03u8, 0x04u8];
        let result: u32 = bytes.into_iter().rev().parse();
        assert_eq!(result, 0x04030201u32);
    }
}
