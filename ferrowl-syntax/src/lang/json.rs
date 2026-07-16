//! JSON lexer. Tolerant of invalid/partial JSON (this highlights a live editor mid
//! keystroke) — never panics, no carried state across lines.

use crate::{LineState, SyntaxKind};

use super::scan;

pub(crate) fn highlight_line(
    line: &str,
    state: LineState,
) -> (Vec<(usize, usize, SyntaxKind)>, LineState) {
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut spans = Vec::new();
    let mut i = 0usize;

    while i < len {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }

        if c == '"' {
            let start = i;
            i = scan::scan_quoted(&chars, i, '"');
            let end = i;
            let mut j = end;
            while j < len && chars[j].is_whitespace() {
                j += 1;
            }
            let kind = if j < len && chars[j] == ':' {
                SyntaxKind::Key
            } else {
                SyntaxKind::String
            };
            spans.push((start, end, kind));
            continue;
        }

        if c == '-' || c.is_ascii_digit() {
            let start = i;
            if c == '-' {
                i += 1;
            }
            while i < len && chars[i].is_ascii_digit() {
                i += 1;
            }
            i = scan::scan_fraction_exponent(&chars, i, false);
            spans.push((start, i, SyntaxKind::Number));
            continue;
        }

        if c.is_alphabetic() {
            let start = i;
            while i < len && chars[i].is_alphanumeric() {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if matches!(word.as_str(), "true" | "false" | "null") {
                spans.push((start, i, SyntaxKind::Literal));
            }
            continue;
        }

        if matches!(c, '{' | '}' | '[' | ']' | ',' | ':') {
            spans.push((i, i + 1, SyntaxKind::Punct));
            i += 1;
            continue;
        }

        // Unrecognized character: no span, just skip it.
        i += 1;
    }

    (spans, state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Language, highlight_line as top_highlight_line};

    fn spans_for(line: &str) -> Vec<(usize, usize, SyntaxKind)> {
        top_highlight_line(Language::Json, line, LineState::default()).0
    }

    #[test]
    /// UI-R-037 — JSON highlight spans are sorted, non-overlapping, and character-indexed.
    fn ut_spans_sorted_non_overlapping() {
        let spans = spans_for(r#"{"key": "value", "n": 1.5e-3}"#);
        for w in spans.windows(2) {
            assert!(w[0].1 <= w[1].0);
            assert!(w[0].0 < w[0].1);
        }
    }

    #[test]
    /// UI-R-039 — a string in key position takes the JSON Key kind, a value string the String kind.
    fn ut_key_vs_value_string() {
        let spans = spans_for(r#"{"key": "value"}"#);
        let strings: Vec<_> = spans
            .iter()
            .filter(|s| s.2 == SyntaxKind::Key || s.2 == SyntaxKind::String)
            .collect();
        assert_eq!(strings.len(), 2);
        assert_eq!(strings[0].2, SyntaxKind::Key);
        assert_eq!(strings[1].2, SyntaxKind::String);
    }

    #[test]
    /// UI-R-037 — an escaped quote does not split a JSON string span.
    fn ut_escaped_quote_does_not_terminate_string() {
        let spans = spans_for(r#""a\"b""#);
        let strings: Vec<_> = spans
            .iter()
            .filter(|s| s.2 == SyntaxKind::String || s.2 == SyntaxKind::Key)
            .collect();
        assert_eq!(strings.len(), 1);
    }

    #[test]
    /// UI-R-039 — exponent and negative numerals are Number-kind spans.
    fn ut_numbers_exponent_and_negative() {
        let spans = spans_for(r#"[1e10, -3.5]"#);
        let nums: Vec<_> = spans.iter().filter(|s| s.2 == SyntaxKind::Number).collect();
        assert_eq!(nums.len(), 2);
    }

    #[test]
    /// UI-R-039 — true/false/null map to the Literal kind.
    fn ut_literals() {
        let spans = spans_for("[true, false, null]");
        let lits: Vec<_> = spans
            .iter()
            .filter(|s| s.2 == SyntaxKind::Literal)
            .collect();
        assert_eq!(lits.len(), 3);
    }

    #[test]
    /// UI-R-037 — malformed JSON never panics and span indices stay in range.
    fn ut_garbage_input_does_not_panic() {
        let cases = [
            "\"abc",
            "]",
            "]=]",
            "",
            "{\"a\": ",
            "garbage ] , : trailing",
        ];
        for case in cases {
            let (spans, _state) = top_highlight_line(Language::Json, case, LineState::default());
            let len = case.chars().count();
            for (start, end, _) in spans {
                assert!(start <= end);
                assert!(end <= len);
            }
        }
    }

    #[test]
    /// UI-R-037 — a multi-byte JSON string is one character-indexed span.
    fn ut_non_ascii_string_value() {
        let line = r#"{"msg": "café"}"#;
        let spans = spans_for(line);
        let len = line.chars().count();
        for (start, end, _) in &spans {
            assert!(*start <= *end);
            assert!(*end <= len);
        }
        let strings: Vec<_> = spans
            .iter()
            .filter(|s| s.2 == SyntaxKind::String || s.2 == SyntaxKind::Key)
            .collect();
        assert_eq!(strings.len(), 2);
        assert_eq!(strings[0].2, SyntaxKind::Key);
        assert_eq!(strings[1].2, SyntaxKind::String);
    }

    #[test]
    /// UI-R-037 — emoji content keeps JSON span indices character-based and in range.
    fn ut_emoji_in_json_string() {
        let line = r#"{"emoji": "🚀 🌟"}"#;
        let spans = spans_for(line);
        let len = line.chars().count();
        for (start, end, _) in &spans {
            assert!(*start <= *end);
            assert!(*end <= len);
        }
        let strings: Vec<_> = spans
            .iter()
            .filter(|s| s.2 == SyntaxKind::String || s.2 == SyntaxKind::Key)
            .collect();
        assert_eq!(strings.len(), 2);
        let (s_start, s_end, _) = *strings[1];
        let string_chars: String = line.chars().skip(s_start).take(s_end - s_start).collect();
        assert!(string_chars.contains("🚀"));
        assert!(string_chars.contains("🌟"));
    }
}
