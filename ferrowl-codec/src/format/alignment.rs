//! Text alignment of an ASCII value inside its register block.

use serde::{Deserialize, Serialize};

/// Text alignment of an ASCII value inside its fixed-width register block.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Alignment {
    Left,
    Right,
}

crate::format::display_by_variant!(Alignment {
    Left => "Left",
    Right => "Right",
});

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
