//! Formatting: turns source text into a reformatted version, or `None` when the input
//! can't be safely reformatted (e.g. invalid JSON). Callers leave the buffer untouched
//! on `None` — this module never mangles content it can't fully understand.

mod json;
pub(crate) mod lua;

use crate::Language;

/// Formats `source` for `lang`. `None` means "leave the content unchanged" — the caller
/// (an editor widget losing focus) should keep the existing buffer as-is.
pub fn format(lang: Language, source: &str) -> Option<String> {
    match lang {
        Language::Json => json::format(source),
        Language::Lua => Some(lua::format(source)),
    }
}
