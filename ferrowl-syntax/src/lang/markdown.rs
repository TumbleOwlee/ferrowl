//! Markdown lexer: maps the inline model ([`crate::markdown`]) plus the block-leading
//! markup visible on one line (heading/list/quote/rule/fence markers) onto the fixed
//! [`SyntaxKind`] enumeration. The block model proper (fence carry state, block kinds)
//! is a separate per-line entry point built on top of this; this lexer never carries
//! state across lines.

use crate::markdown::{InlineKind, escape_markers, inline_spans};
use crate::{LineState, SyntaxKind};

pub(crate) fn highlight_line(
    line: &str,
    state: LineState,
) -> (Vec<(usize, usize, SyntaxKind)>, LineState) {
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut spans: Vec<(usize, usize, SyntaxKind)> = Vec::new();

    if is_horizontal_rule(line) {
        spans.push((0, len, SyntaxKind::Punct));
        return (finish(spans), state);
    }

    let mut i = 0usize;
    while i < len && chars[i] == ' ' {
        i += 1;
    }

    if i < len && (chars[i] == '`' || chars[i] == '~') && starts_with_run(&chars, i, chars[i], 3) {
        let fence_char = chars[i];
        let mut j = i;
        while j < len && chars[j] == fence_char {
            j += 1;
        }
        spans.push((i, j, SyntaxKind::Punct));
        if j < len {
            spans.push((j, len, SyntaxKind::Keyword));
        }
        return (finish(spans), state);
    }

    if i < len && chars[i] == '#' {
        let mut j = i;
        while j < len && chars[j] == '#' && j - i < 6 {
            j += 1;
        }
        if j < len && chars[j] == ' ' {
            spans.push((i, len, SyntaxKind::Keyword));
            return (finish(spans), state);
        }
    }

    let quote_start = i;
    let mut k = i;
    while k < len && chars[k] == '>' {
        spans.push((k, k + 1, SyntaxKind::Punct));
        k += 1;
        if k < len && chars[k] == ' ' {
            k += 1;
        }
    }
    if k > quote_start {
        let rest: String = chars[k..].iter().collect();
        spans.extend(inline_to_syntax_spans(&rest, k));
        return (finish(spans), state);
    }

    if let Some((marker_end, mut m)) = list_marker_bounds(&chars, i) {
        spans.push((i, marker_end, SyntaxKind::Punct));
        if m + 2 < len
            && chars[m] == '['
            && matches!(chars[m + 1], ' ' | 'x' | 'X')
            && chars[m + 2] == ']'
        {
            spans.push((m, m + 3, SyntaxKind::Punct));
            m += 3;
            if m < len && chars[m] == ' ' {
                m += 1;
            }
        }
        let rest: String = chars[m..].iter().collect();
        spans.extend(inline_to_syntax_spans(&rest, m));
        return (finish(spans), state);
    }

    spans.extend(inline_to_syntax_spans(line, 0));
    (finish(spans), state)
}

fn starts_with_run(chars: &[char], from: usize, c: char, min: usize) -> bool {
    let mut n = 0usize;
    let mut j = from;
    while j < chars.len() && chars[j] == c {
        n += 1;
        j += 1;
    }
    n >= min
}

fn is_horizontal_rule(line: &str) -> bool {
    let cs: Vec<char> = line.chars().filter(|c| !c.is_whitespace()).collect();
    if cs.len() < 3 {
        return false;
    }
    let c0 = cs[0];
    if !matches!(c0, '-' | '*' | '_') {
        return false;
    }
    cs.iter().all(|c| *c == c0)
}

/// For a list marker (`- `, `* `, `+ ` or `1. `) starting at `from`, returns the index
/// just past the marker glyph itself (excluding the following space) and the index just
/// past the marker and its trailing space, or `None` if `from` doesn't start one.
fn list_marker_bounds(chars: &[char], from: usize) -> Option<(usize, usize)> {
    let len = chars.len();
    if from < len
        && matches!(chars[from], '-' | '*' | '+')
        && from + 1 < len
        && chars[from + 1] == ' '
    {
        return Some((from + 1, from + 2));
    }
    let mut d = from;
    while d < len && chars[d].is_ascii_digit() {
        d += 1;
    }
    if d > from && d < len && chars[d] == '.' && d + 1 < len && chars[d + 1] == ' ' {
        return Some((d + 1, d + 2));
    }
    None
}

fn inline_to_syntax_spans(text: &str, offset: usize) -> Vec<(usize, usize, SyntaxKind)> {
    let mut out = Vec::new();
    for s in inline_spans(text) {
        match s.kind {
            InlineKind::Code => {
                for m in &s.markers {
                    out.push((offset + m.0, offset + m.1, SyntaxKind::String));
                }
                out.push((
                    offset + s.content.0,
                    offset + s.content.1,
                    SyntaxKind::String,
                ));
            }
            InlineKind::Link | InlineKind::Image => {
                out.push((
                    offset + s.markers[0].0,
                    offset + s.markers[0].1,
                    SyntaxKind::Punct,
                ));
                out.push((
                    offset + s.markers[1].0,
                    offset + s.markers[1].1,
                    SyntaxKind::Punct,
                ));
                out.push((
                    offset + s.markers[2].0,
                    offset + s.markers[2].1,
                    SyntaxKind::String,
                ));
                out.push((
                    offset + s.markers[3].0,
                    offset + s.markers[3].1,
                    SyntaxKind::Punct,
                ));
                out.push((
                    offset + s.content.0,
                    offset + s.content.1,
                    SyntaxKind::Ident,
                ));
            }
            InlineKind::Bold | InlineKind::Italic | InlineKind::Strike => {
                for m in &s.markers {
                    out.push((offset + m.0, offset + m.1, SyntaxKind::Punct));
                }
                out.push((
                    offset + s.content.0,
                    offset + s.content.1,
                    SyntaxKind::Ident,
                ));
            }
        }
    }
    for e in escape_markers(text) {
        out.push((offset + e, offset + e + 1, SyntaxKind::Punct));
    }
    out
}

fn finish(mut spans: Vec<(usize, usize, SyntaxKind)>) -> Vec<(usize, usize, SyntaxKind)> {
    spans.sort_by_key(|s| s.0);
    let mut out = Vec::new();
    let mut last_end = 0usize;
    for s in spans {
        if s.0 < s.1 && s.0 >= last_end {
            last_end = s.1;
            out.push(s);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Language, highlight_line as top_highlight_line};

    fn text_kinds(line: &str, spans: &[(usize, usize, SyntaxKind)]) -> Vec<(String, SyntaxKind)> {
        let chars: Vec<char> = line.chars().collect();
        spans
            .iter()
            .map(|(s, e, k)| (chars[*s..*e].iter().collect(), *k))
            .collect()
    }

    fn assert_invariant(line: &str, spans: &[(usize, usize, SyntaxKind)]) {
        for w in spans.windows(2) {
            assert!(w[0].1 <= w[1].0, "overlap in {line:?}: {spans:?}");
        }
        for (start, end, _) in spans {
            assert!(start < end);
            assert!(*end <= line.chars().count());
        }
    }

    #[test]
    /// UI-R-037 — markdown highlight spans are sorted, non-overlapping, and carry the
    /// right `SyntaxKind` over the right source text for heading/list/quote/paragraph lines.
    fn ut_markdown_spans_are_sorted_and_non_overlapping() {
        let heading = "# Heading with **bold** text";
        let (spans, _) = highlight_line(heading, LineState::default());
        assert_invariant(heading, &spans);
        assert_eq!(
            text_kinds(heading, &spans),
            vec![(heading.to_string(), SyntaxKind::Keyword)]
        );

        let list = "- a list item with `code` and [a link](http://x)";
        let (spans, _) = highlight_line(list, LineState::default());
        assert_invariant(list, &spans);
        let found = text_kinds(list, &spans);
        assert!(found.contains(&("-".to_string(), SyntaxKind::Punct)));
        assert!(found.contains(&("code".to_string(), SyntaxKind::String)));
        assert!(found.contains(&("a link".to_string(), SyntaxKind::Ident)));
        assert!(found.contains(&("http://x".to_string(), SyntaxKind::String)));

        let quote = "> quoted **bold** _italic_ text";
        let (spans, _) = highlight_line(quote, LineState::default());
        assert_invariant(quote, &spans);
        let found = text_kinds(quote, &spans);
        assert!(found.contains(&(">".to_string(), SyntaxKind::Punct)));
        assert!(found.contains(&("bold".to_string(), SyntaxKind::Ident)));
        assert!(found.contains(&("italic".to_string(), SyntaxKind::Ident)));

        let ordered = "1. ordered ***x*** item";
        let (spans, _) = highlight_line(ordered, LineState::default());
        assert_invariant(ordered, &spans);
        let found = text_kinds(ordered, &spans);
        assert!(found.contains(&("1.".to_string(), SyntaxKind::Punct)));
        assert!(
            found
                .iter()
                .any(|(t, k)| *k == SyntaxKind::Ident && t.contains('x')),
            "expected an emphasis content span containing 'x' in {found:?}"
        );

        let paragraph = "plain paragraph with ~~strike~~ and ![alt](img.png)";
        let (spans, _) = highlight_line(paragraph, LineState::default());
        assert_invariant(paragraph, &spans);
        let found = text_kinds(paragraph, &spans);
        assert!(found.contains(&("~~".to_string(), SyntaxKind::Punct)));
        assert!(found.contains(&("strike".to_string(), SyntaxKind::Ident)));
        assert!(found.contains(&("![".to_string(), SyntaxKind::Punct)));
        assert!(found.contains(&("alt".to_string(), SyntaxKind::Ident)));
        assert!(found.contains(&("img.png".to_string(), SyntaxKind::String)));
    }

    #[test]
    /// UI-R-037 — `Language::Markdown` dispatches through the top-level `highlight_line`.
    fn ut_language_markdown_dispatches_through_highlight_line() {
        let (spans, _) = top_highlight_line(Language::Markdown, "# Title", LineState::default());
        assert_eq!(spans, vec![(0, 7, SyntaxKind::Keyword)]);
    }
}
