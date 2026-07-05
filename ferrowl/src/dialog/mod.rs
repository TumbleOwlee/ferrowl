//! Modal dialogs: module setup and shared register-edit data types.

pub mod path_suggest;
pub mod scripts;

pub use crate::module::modbus::dialog::{EditedRegister, parse_raw_value};
pub use crate::module::modbus::setup_dialog::SetupDialog;
use ferrowl_ui::widgets::{Validate, ValidateResult};

#[derive(Clone, Debug)]
pub struct NonEmpty();

impl Validate for NonEmpty {
    fn validate(input: &str) -> ValidateResult {
        if input.is_empty() {
            ValidateResult::Error("Non-empty input required".to_string())
        } else {
            String::validate(input)
        }
    }
}

#[derive(Clone, Debug)]
pub struct Address();

impl Validate for Address {
    fn validate(input: &str) -> ValidateResult {
        if input == "virtual" {
            ValidateResult::Success
        } else if let ValidateResult::Error(e) = i16::validate(input) {
            ValidateResult::Error(e.to_string())
        } else {
            ValidateResult::None
        }
    }
}

#[derive(Clone, Debug)]
pub struct Bitmask();

impl Validate for Bitmask {
    fn validate(input: &str) -> ValidateResult {
        if input.is_empty() {
            ValidateResult::None
        } else if let Some(hex) = input
            .strip_prefix("0x")
            .or_else(|| input.strip_prefix("0X"))
        {
            if let Err(e) =
                u128::from_str_radix(hex, 16).map_err(|_| "must be a hex (0x…) or decimal number")
            {
                ValidateResult::Error(e.to_string())
            } else {
                ValidateResult::None
            }
        } else if let Err(e) = input
            .parse::<u128>()
            .map_err(|_| "must be a hex (0x…) or decimal number")
        {
            ValidateResult::Error(e.to_string())
        } else {
            ValidateResult::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrowl_ui::widgets::ValidateResult;

    // ---- NonEmpty ----

    #[test]
    fn non_empty_rejects_empty() {
        assert!(matches!(NonEmpty::validate(""), ValidateResult::Error(_)));
    }

    #[test]
    fn non_empty_accepts_text() {
        assert!(matches!(NonEmpty::validate("hello"), ValidateResult::None));
        assert!(matches!(NonEmpty::validate(" "), ValidateResult::None));
    }

    // ---- Address ----

    #[test]
    fn address_virtual_keyword() {
        assert!(matches!(
            Address::validate("virtual"),
            ValidateResult::Success
        ));
    }

    #[test]
    fn address_valid_i16() {
        assert!(matches!(Address::validate("0"), ValidateResult::None));
        assert!(matches!(Address::validate("32767"), ValidateResult::None));
        assert!(matches!(Address::validate("-32768"), ValidateResult::None));
        assert!(matches!(Address::validate("100"), ValidateResult::None));
    }

    #[test]
    fn address_overflow_i16() {
        assert!(matches!(
            Address::validate("32768"),
            ValidateResult::Error(_)
        ));
        assert!(matches!(
            Address::validate("-32769"),
            ValidateResult::Error(_)
        ));
        assert!(matches!(
            Address::validate("99999"),
            ValidateResult::Error(_)
        ));
    }

    #[test]
    fn address_non_numeric() {
        assert!(matches!(Address::validate("abc"), ValidateResult::Error(_)));
        assert!(matches!(Address::validate(""), ValidateResult::Error(_)));
    }

    // ---- Bitmask ----

    #[test]
    fn bitmask_empty_is_none() {
        assert!(matches!(Bitmask::validate(""), ValidateResult::None));
    }

    #[test]
    fn bitmask_valid_hex_lowercase_prefix() {
        assert!(matches!(Bitmask::validate("0xFF"), ValidateResult::None));
        assert!(matches!(Bitmask::validate("0x0"), ValidateResult::None));
        assert!(matches!(
            Bitmask::validate("0xDEADBEEF"),
            ValidateResult::None
        ));
    }

    #[test]
    fn bitmask_valid_hex_uppercase_prefix() {
        assert!(matches!(Bitmask::validate("0XFF"), ValidateResult::None));
        assert!(matches!(Bitmask::validate("0X0"), ValidateResult::None));
    }

    #[test]
    fn bitmask_invalid_hex() {
        assert!(matches!(
            Bitmask::validate("0xGG"),
            ValidateResult::Error(_)
        ));
        assert!(matches!(Bitmask::validate("0x"), ValidateResult::Error(_)));
    }

    #[test]
    fn bitmask_valid_decimal() {
        assert!(matches!(Bitmask::validate("0"), ValidateResult::None));
        assert!(matches!(Bitmask::validate("255"), ValidateResult::None));
        assert!(matches!(
            Bitmask::validate("340282366920938463463374607431768211455"),
            ValidateResult::None
        )); // u128::MAX
    }

    #[test]
    fn bitmask_invalid_decimal() {
        assert!(matches!(Bitmask::validate("abc"), ValidateResult::Error(_)));
        assert!(matches!(Bitmask::validate("-1"), ValidateResult::Error(_)));
    }
}
