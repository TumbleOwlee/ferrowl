//! Error type for register decode/encode failures.

use crate::format::Format;

/// Why a [`decode`](crate::codec::decode) or [`encode`](crate::codec::encode)
/// call failed.
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    /// `bytes` was shorter than the format's word width.
    #[error("Too few bytes to parse {0}")]
    TooFewBytes(Format),
    /// Decoded bytes were not valid UTF-8 for a packed-ASCII format.
    #[error("Parse PackedAscii failed.")]
    PackedAscii,
    /// A numeric literal failed to parse as an integer.
    #[error("{0}")]
    ParseInt(#[from] std::num::ParseIntError),
    /// A numeric literal failed to parse as a float.
    #[error("{0}")]
    ParseFloat(#[from] std::num::ParseFloatError),
    /// [`encode_value`](crate::codec::encode_value) was called with a [`Value`](crate::value::Value)
    /// variant that does not match the target `Format`'s variant.
    #[error("Value does not match format {0}")]
    ValueFormatMismatch(Format),
    /// A [`BitField`](crate::format::BitField)'s mask sets bits outside the
    /// format's own integer width (e.g. a mask of `0x1FF` on `Format::u8(...)`).
    #[error("Bit field mask does not fit format {0}")]
    BitFieldWidth(Format),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{BitField, Endian, Resolution, WordOrder};

    #[test]
    /// MB-R-155 — a `Format`-carrying error renders the format via its `Display`
    /// text, not the derived `Debug` form.
    fn ut_bit_field_width_error_uses_format_display_text() {
        let format = Format::u8(
            Endian::Big,
            WordOrder::Normal,
            Resolution(1.0),
            BitField::default(),
        );
        let err = CodecError::BitFieldWidth(format.clone());
        let rendered = err.to_string();
        assert!(
            rendered.contains(&format.to_string()),
            "expected the display text {:?} in {rendered:?}",
            format.to_string()
        );
        assert!(
            !rendered.contains(&format!("{format:?}")),
            "expected no debug form in {rendered:?}"
        );
    }
}
