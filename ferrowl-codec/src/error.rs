//! Error type for register decode/encode failures.

use crate::format::Format;

/// Why a [`decode`](crate::codec::decode) or [`encode`](crate::codec::encode)
/// call failed.
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    /// `bytes` was shorter than the format's word width.
    #[error("Too few bytes to parse {0:?}")]
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
    #[error("Value does not match format {0:?}")]
    ValueFormatMismatch(Format),
}
