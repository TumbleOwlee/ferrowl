//! Small scalar parameters of a format: ASCII width and display resolution.

use serde::{Deserialize, Serialize};

/// Width of an ASCII value, in 16-bit registers (2 characters each).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Width(pub usize);

/// Scale factor applied when displaying a numeric value
/// (`displayed = raw * resolution`).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Resolution(pub f64);

impl Default for Resolution {
    fn default() -> Self {
        Resolution(1.0)
    }
}

impl std::fmt::Display for Resolution {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(fmt, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::Resolution;

    #[test]
    fn ut_resolution_default_and_display() {
        assert_eq!(Resolution::default().0, 1.0);
        assert_eq!(Resolution(2.5).to_string(), "2.5");
    }
}
