//! Diff lexer: classifies whole lines of a unified diff, never mid-line spans.

use crate::{LineState, SyntaxKind};

pub(crate) fn highlight_line(
    line: &str,
    state: LineState,
) -> (Vec<(usize, usize, SyntaxKind)>, LineState) {
    let len = line.chars().count();
    let kind = if line.starts_with("@@")
        || line.starts_with("---")
        || line.starts_with("+++")
        || line.starts_with("diff ")
        || line.starts_with("index ")
    {
        Some(SyntaxKind::Meta)
    } else if line.starts_with('+') {
        Some(SyntaxKind::Added)
    } else if line.starts_with('-') {
        Some(SyntaxKind::Removed)
    } else {
        None
    };

    let spans = match kind {
        Some(k) => vec![(0, len, k)],
        None => Vec::new(),
    };
    (spans, state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Language;

    #[test]
    /// UI-R-037 — Diff is a full language under `highlight_line`'s contract: char-indexed
    /// spans (not byte offsets) over the crate's public entry point.
    fn ut_diff_is_a_full_highlightable_language() {
        let (spans, _) = crate::highlight_line(Language::Diff, "+added", LineState::default());
        assert_eq!(spans, vec![(0, 6, SyntaxKind::Added)]);

        let (multibyte_spans, _) = highlight_line("+héllo wörld", LineState::default());
        assert_eq!(
            multibyte_spans,
            vec![(0, "+héllo wörld".chars().count(), SyntaxKind::Added)],
            "span end must be a char index, not a byte offset"
        );
    }

    #[test]
    /// UI-R-156 — a Diff line yields at most one span covering the whole line.
    fn ut_diff_span_covers_whole_line() {
        let (spans, _) = highlight_line("+hello world", LineState::default());
        assert_eq!(spans, vec![(0, 12, SyntaxKind::Added)]);
    }

    #[test]
    /// UI-R-157 — a `+`-prefixed line is classified diff added.
    fn ut_diff_plus_line_is_added() {
        let (spans, _) = highlight_line("+added line", LineState::default());
        assert_eq!(spans, vec![(0, 11, SyntaxKind::Added)]);
    }

    #[test]
    /// UI-R-158 — a `-`-prefixed line is classified diff removed.
    fn ut_diff_minus_line_is_removed() {
        let (spans, _) = highlight_line("-removed line", LineState::default());
        assert_eq!(spans, vec![(0, 13, SyntaxKind::Removed)]);
    }

    #[test]
    /// UI-R-159 — each of the five meta prefixes classifies as diff meta.
    fn ut_diff_meta_prefixes_are_meta() {
        for line in [
            "@@ -1,2 +1,2 @@",
            "--- a/file",
            "+++ b/file",
            "diff --git a/file b/file",
            "index abc123..def456 100644",
        ] {
            let (spans, _) = highlight_line(line, LineState::default());
            assert_eq!(
                spans,
                vec![(0, line.chars().count(), SyntaxKind::Meta)],
                "line {line:?} must be meta"
            );
        }
    }

    #[test]
    /// UI-R-160 — a context line, an empty line, or arbitrary text yields no span at all;
    /// the resulting general-style render is asserted at the widget layer, where styling
    /// is actually applied.
    fn ut_diff_context_and_empty_lines_have_no_span() {
        for line in [" ctx", "", "no marker here"] {
            let (spans, _) = highlight_line(line, LineState::default());
            assert!(spans.is_empty(), "line {line:?} must yield no span");
        }
    }

    #[test]
    /// UI-R-161 — Diff highlighting neither reads nor changes the carry-over line state.
    fn ut_diff_ignores_and_preserves_carry_state() {
        let carry = crate::lang::lua::highlight_line("local s = [[", LineState::default()).1;
        let (spans, out_state) = highlight_line("+added", carry);
        assert_eq!(spans, vec![(0, 6, SyntaxKind::Added)]);
        assert_eq!(out_state, carry);
    }

    #[test]
    /// UI-E-081 — `---`/`+++` classify as meta, not removed/added, because meta is matched first.
    fn ut_diff_triple_dash_and_plus_are_meta() {
        let (spans_minus, _) = highlight_line("---", LineState::default());
        assert_eq!(spans_minus, vec![(0, 3, SyntaxKind::Meta)]);
        let (spans_plus, _) = highlight_line("+++", LineState::default());
        assert_eq!(spans_plus, vec![(0, 3, SyntaxKind::Meta)]);
    }

    #[test]
    /// UI-E-082 — a lone `+` or `-` yields a one-character span of the added/removed kind.
    fn ut_diff_lone_marker_is_one_char_span() {
        let (spans_plus, _) = highlight_line("+", LineState::default());
        assert_eq!(spans_plus, vec![(0, 1, SyntaxKind::Added)]);
        let (spans_minus, _) = highlight_line("-", LineState::default());
        assert_eq!(spans_minus, vec![(0, 1, SyntaxKind::Removed)]);
    }
}
