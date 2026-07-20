//! Register (word) order of a multi-register value.

use serde::{Deserialize, Serialize};

/// Order of the 16-bit register words of a multi-register value, independent of
/// the byte order ([`Endian`](super::Endian)).
///
/// `Normal` keeps the words in natural order; `Reversed` reverses the whole word
/// sequence (`[w0,w1,w2,w3]` → `[w3,w2,w1,w0]`). For a single-register format it
/// is a no-op. Defaults to `Normal`, which reproduces the byte-order rule alone.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WordOrder {
    #[default]
    Normal,
    Reversed,
}

impl std::fmt::Display for WordOrder {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WordOrder::Normal => write!(fmt, "Normal"),
            WordOrder::Reversed => write!(fmt, "Reversed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WordOrder;

    #[test]
    /// MB-R-099 — register order is `Normal` or `Reversed`, defaulting to `Normal`.
    fn ut_word_order_display_and_default() {
        assert_eq!(WordOrder::Normal.to_string(), "Normal");
        assert_eq!(WordOrder::Reversed.to_string(), "Reversed");
        assert_eq!(WordOrder::default(), WordOrder::Normal);
    }
}
