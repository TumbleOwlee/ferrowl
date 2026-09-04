//! Markdown inline model: pure text-to-spans parsing of the inline constructs a
//! consumer needs to render (hide markers) or highlight (UI-R-122).

use std::collections::HashSet;

/// Inline construct kinds recognized on one source line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineKind {
    Bold,
    Italic,
    Code,
    Strike,
    Link,
    Image,
}

/// One inline construct, in source character columns of its line: the columns that carry
/// markup (`markers`) and the columns a renderer shows (`content`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineSpan {
    pub kind: InlineKind,
    pub markers: Vec<(usize, usize)>,
    pub content: (usize, usize),
}

/// Inline constructs of one source line, sorted by content start, non-overlapping.
pub fn inline_spans(line: &str) -> Vec<InlineSpan> {
    let chars: Vec<char> = line.chars().collect();
    let escaped = escaped_positions(&chars);
    let mut spans = Vec::new();
    let mut i = 0usize;
    let len = chars.len();

    while i < len {
        if escaped.contains(&i) {
            i += 1;
            continue;
        }

        if chars[i] == '!'
            && i + 1 < len
            && chars[i + 1] == '['
            && let Some(span) = try_link_or_image(&chars, i, &escaped, true)
        {
            i = span.markers.last().expect("link/image has markers").1;
            spans.push(span);
            continue;
        }

        if chars[i] == '['
            && let Some(span) = try_link_or_image(&chars, i, &escaped, false)
        {
            i = span.markers.last().expect("link/image has markers").1;
            spans.push(span);
            continue;
        }

        if chars[i] == '`'
            && let Some(span) = try_delim(&chars, i, &['`'], InlineKind::Code, &escaped)
        {
            i = span.markers[1].1;
            spans.push(span);
            continue;
        }

        if chars[i] == '~'
            && i + 1 < len
            && chars[i + 1] == '~'
            && let Some(span) = try_delim(&chars, i, &['~', '~'], InlineKind::Strike, &escaped)
        {
            i = span.markers[1].1;
            spans.push(span);
            continue;
        }

        if chars[i] == '*'
            && i + 2 < len
            && chars[i + 1] == '*'
            && chars[i + 2] == '*'
            && let Some((bold, italic, end)) = try_triple(&chars, i, &escaped)
        {
            spans.push(bold);
            spans.push(italic);
            i = end;
            continue;
        }

        if chars[i] == '*'
            && i + 1 < len
            && chars[i + 1] == '*'
            && let Some(span) = try_delim(&chars, i, &['*', '*'], InlineKind::Bold, &escaped)
        {
            i = span.markers[1].1;
            spans.push(span);
            continue;
        }

        if chars[i] == '*'
            && let Some(span) = try_delim(&chars, i, &['*'], InlineKind::Italic, &escaped)
        {
            i = span.markers[1].1;
            spans.push(span);
            continue;
        }

        if chars[i] == '_'
            && let Some(span) = try_delim(&chars, i, &['_'], InlineKind::Italic, &escaped)
        {
            i = span.markers[1].1;
            spans.push(span);
            continue;
        }

        i += 1;
    }

    spans.sort_by_key(|s| s.content.0);
    spans
}

/// Backslash-escape marker columns of a line: each `\` that makes the following character
/// literal. The escaped character itself never opens or closes an inline construct.
pub fn escape_markers(line: &str) -> Vec<usize> {
    let chars: Vec<char> = line.chars().collect();
    backslash_positions(&chars)
}

fn backslash_positions(chars: &[char]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            out.push(i);
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

fn escaped_positions(chars: &[char]) -> HashSet<usize> {
    backslash_positions(chars)
        .into_iter()
        .flat_map(|i| [i, i + 1])
        .collect()
}

fn find_delim(
    chars: &[char],
    from: usize,
    delim: &[char],
    escaped: &HashSet<usize>,
) -> Option<usize> {
    let dl = delim.len();
    if dl == 0 || from + dl > chars.len() {
        return None;
    }
    let mut j = from;
    while j + dl <= chars.len() {
        if !escaped.contains(&j) && chars[j..j + dl] == *delim {
            return Some(j);
        }
        j += 1;
    }
    None
}

fn try_delim(
    chars: &[char],
    start: usize,
    delim: &[char],
    kind: InlineKind,
    escaped: &HashSet<usize>,
) -> Option<InlineSpan> {
    let dl = delim.len();
    let content_start = start + dl;
    let close = find_delim(chars, content_start, delim, escaped)?;
    if close == content_start {
        return None;
    }
    let marker_end = close + dl;
    Some(InlineSpan {
        kind,
        markers: vec![(start, content_start), (close, marker_end)],
        content: (content_start, close),
    })
}

fn try_triple(
    chars: &[char],
    start: usize,
    escaped: &HashSet<usize>,
) -> Option<(InlineSpan, InlineSpan, usize)> {
    let delim = ['*', '*', '*'];
    let content_start = start + 3;
    let close = find_delim(chars, content_start, &delim, escaped)?;
    if close == content_start {
        return None;
    }
    let bold = InlineSpan {
        kind: InlineKind::Bold,
        markers: vec![(start, start + 2), (close + 1, close + 3)],
        content: (start + 2, close + 1),
    };
    let italic = InlineSpan {
        kind: InlineKind::Italic,
        markers: vec![(start + 2, start + 3), (close, close + 1)],
        content: (start + 3, close),
    };
    Some((bold, italic, close + 3))
}

fn try_link_or_image(
    chars: &[char],
    start: usize,
    escaped: &HashSet<usize>,
    is_image: bool,
) -> Option<InlineSpan> {
    let bracket_pos = if is_image { start + 1 } else { start };
    if bracket_pos >= chars.len() || chars[bracket_pos] != '[' {
        return None;
    }
    let text_start = bracket_pos + 1;
    let close_bracket = find_delim(chars, text_start, &[']'], escaped)?;
    let paren_open = close_bracket + 1;
    if paren_open >= chars.len() || chars[paren_open] != '(' {
        return None;
    }
    let url_start = paren_open + 1;
    let close_paren = find_delim(chars, url_start, &[')'], escaped)?;
    let end = close_paren + 1;
    let kind = if is_image {
        InlineKind::Image
    } else {
        InlineKind::Link
    };
    let markers = vec![
        (start, bracket_pos + 1),
        (close_bracket, paren_open + 1),
        (url_start, close_paren),
        (close_paren, end),
    ];
    Some(InlineSpan {
        kind,
        markers,
        content: (text_start, close_bracket),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<InlineKind> {
        inline_spans(line).into_iter().map(|s| s.kind).collect()
    }

    #[test]
    /// UI-R-122 — bold, code and strike each separate their marker columns from their content columns.
    fn ut_emphasis_code_and_strike_separate_markers_from_content() {
        let spans = inline_spans("**bold**");
        assert_eq!(spans.len(), 1);
        let s = &spans[0];
        assert_eq!(s.kind, InlineKind::Bold);
        assert_eq!(s.markers, vec![(0, 2), (6, 8)]);
        assert_eq!(s.content, (2, 6));

        let spans = inline_spans("*italic*");
        assert_eq!(spans[0].kind, InlineKind::Italic);
        assert_eq!(spans[0].markers, vec![(0, 1), (7, 8)]);
        assert_eq!(spans[0].content, (1, 7));

        let spans = inline_spans("_italic_");
        assert_eq!(spans[0].kind, InlineKind::Italic);
        assert_eq!(spans[0].markers, vec![(0, 1), (7, 8)]);
        assert_eq!(spans[0].content, (1, 7));

        let spans = inline_spans("`code`");
        assert_eq!(spans[0].kind, InlineKind::Code);
        assert_eq!(spans[0].markers, vec![(0, 1), (5, 6)]);
        assert_eq!(spans[0].content, (1, 5));

        let spans = inline_spans("~~strike~~");
        assert_eq!(spans[0].kind, InlineKind::Strike);
        assert_eq!(spans[0].markers, vec![(0, 2), (8, 10)]);
        assert_eq!(spans[0].content, (2, 8));
    }

    #[test]
    /// UI-R-122 — a link and an image each separate bracket/paren/URL marker columns from their text content.
    fn ut_link_and_image_separate_markers_from_content() {
        let spans = inline_spans("[text](url)");
        assert_eq!(spans.len(), 1);
        let s = &spans[0];
        assert_eq!(s.kind, InlineKind::Link);
        assert_eq!(s.content, (1, 5));
        assert_eq!(s.markers, vec![(0, 1), (5, 7), (7, 10), (10, 11)]);

        let spans = inline_spans("![alt](url)");
        let s = &spans[0];
        assert_eq!(s.kind, InlineKind::Image);
        assert_eq!(s.content, (2, 5));
        assert_eq!(s.markers, vec![(0, 2), (5, 7), (7, 10), (10, 11)]);
    }

    #[test]
    /// UI-R-123 — an escaped `*` neither opens nor closes an italic construct; the backslash is a marker column.
    fn ut_backslash_escape_neither_opens_nor_closes_a_construct() {
        let line = r"\*not italic\*";
        assert!(inline_spans(line).is_empty());
        let escapes = escape_markers(line);
        assert_eq!(escapes, vec![0, 12]);
    }

    #[test]
    /// Constructs the block model does not recognize report no inline spans on the inline path.
    fn ut_unsupported_inline_constructs_report_no_spans() {
        let cases = [
            "| a | b |",
            "raw <b>html</b>",
            "footnote [^1] ref",
            "[ref][1]",
            "<https://example.com>",
        ];
        for case in cases {
            let spans = inline_spans(case);
            assert!(spans.is_empty(), "unexpected span in {case:?}: {spans:?}");
        }
    }

    #[test]
    /// UI-E-075 — `***x***` resolves best-effort as nested Bold and Italic over the same text.
    fn ut_triple_marker_yields_bold_and_italic() {
        let ks = kinds("***x***");
        assert_eq!(ks, vec![InlineKind::Bold, InlineKind::Italic]);
        let spans = inline_spans("***x***");
        let bold = spans
            .iter()
            .find(|s| s.kind == InlineKind::Bold)
            .expect("bold span");
        let italic = spans
            .iter()
            .find(|s| s.kind == InlineKind::Italic)
            .expect("italic span");
        assert_eq!(italic.content, (3, 4));
        assert!(bold.content.0 <= italic.content.0 && bold.content.1 >= italic.content.1);
    }
}
