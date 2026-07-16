//! Text alignment of an ASCII value inside its register block.

use serde::{Deserialize, Serialize};

/// Text alignment of an ASCII value inside its fixed-width register block.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Alignment {
    Left,
    Right,
}

impl std::fmt::Display for Alignment {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Alignment::Left => {
                write!(fmt, "Left")
            }
            Alignment::Right => {
                write!(fmt, "Right")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Alignment;

    #[test]
    /// MB-R-019 — `Ascii` carries an alignment of `Left` or `Right`.
    fn ut_alignment_display() {
        assert_eq!(Alignment::Left.to_string(), "Left");
        assert_eq!(Alignment::Right.to_string(), "Right");
    }
}
