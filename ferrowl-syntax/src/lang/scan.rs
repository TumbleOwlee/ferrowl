//! Scanning helpers shared by lexers in this module (currently [`super::json`] and
//! [`super::lua`]).

/// Scans a quoted string literal starting at `chars[start]` (the opening `quote`
/// character) to its closing quote, skipping escaped characters. Returns the index just
/// past the closing quote, or `chars.len()` if unterminated on this line.
pub(super) fn scan_quoted(chars: &[char], start: usize, quote: char) -> usize {
    let len = chars.len();
    let mut i = start + 1;
    while i < len {
        if chars[i] == '\\' && i + 1 < len {
            i += 2;
            continue;
        }
        if chars[i] == quote {
            i += 1;
            break;
        }
        i += 1;
    }
    i
}

/// Scans an optional decimal fraction (`.` + digits) followed by an optional exponent
/// (`e`/`E` + optional sign + digits), starting at `chars[start]`. Returns the index just
/// past what was consumed.
///
/// If `backtrack_bare_exponent` is `true` and an `e`/`E` (with optional sign) is not
/// followed by at least one digit, the exponent is not consumed at all — the returned
/// index is rewound to before the `e`/`E`, leaving it for the caller's next token (this is
/// Lua's behavior: `1e` is not a valid number literal). If `false`, a bare `e`/`E` (with
/// optional sign) is consumed regardless of trailing digits, matching JSON's simpler scan.
pub(super) fn scan_fraction_exponent(
    chars: &[char],
    start: usize,
    backtrack_bare_exponent: bool,
) -> usize {
    let len = chars.len();
    let mut i = start;
    if i < len && chars[i] == '.' {
        i += 1;
        while i < len && chars[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i < len && (chars[i] == 'e' || chars[i] == 'E') {
        let save = i;
        i += 1;
        if i < len && (chars[i] == '+' || chars[i] == '-') {
            i += 1;
        }
        if i < len && chars[i].is_ascii_digit() {
            while i < len && chars[i].is_ascii_digit() {
                i += 1;
            }
        } else if backtrack_bare_exponent {
            i = save;
        }
    }
    i
}
