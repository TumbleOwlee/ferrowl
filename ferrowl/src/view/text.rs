//! Plain-text word wrapping shared by the help-style overlays. Manual (not `Paragraph::wrap`)
//! because those overlays compute popup height and scroll clamps from logical line counts, which
//! would desync from ratatui's own wrapping.

/// Greedy word-wrap of `s` into segments of at most `width` chars. A single word longer than
/// `width` is hard-split across multiple segments. Char-count based (content here is ASCII).
///
/// Always returns at least one segment, even for empty input, so callers can unconditionally emit
/// one `Line` per returned segment.
pub fn wrap(s: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![s.to_string()];
    }
    if s.is_empty() {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;

    for word in s.split(' ') {
        let word_len = word.chars().count();
        if word_len > width {
            // Hard-split the oversized word, flushing whatever's pending first.
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                current_len = 0;
            }
            let mut chunk = String::new();
            let mut chunk_len = 0usize;
            for ch in word.chars() {
                if chunk_len == width {
                    lines.push(std::mem::take(&mut chunk));
                    chunk_len = 0;
                }
                chunk.push(ch);
                chunk_len += 1;
            }
            if !chunk.is_empty() {
                current = chunk;
                current_len = chunk_len;
            }
            continue;
        }

        let needed = if current.is_empty() {
            word_len
        } else {
            current_len + 1 + word_len
        };
        if needed > width {
            lines.push(std::mem::take(&mut current));
            current = word.to_string();
            current_len = word_len;
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
            current_len = needed;
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_wrap_empty() {
        assert_eq!(wrap("", 10), vec![String::new()]);
    }

    #[test]
    fn ut_wrap_short_fits() {
        assert_eq!(wrap("hello world", 20), vec!["hello world".to_string()]);
    }

    #[test]
    fn ut_wrap_word_boundary() {
        assert_eq!(
            wrap("one two three four", 9),
            vec![
                "one two".to_string(),
                "three".to_string(),
                "four".to_string()
            ]
        );
    }

    #[test]
    fn ut_wrap_long_word_hard_split() {
        assert_eq!(
            wrap("abcdefghij", 4),
            vec!["abcd".to_string(), "efgh".to_string(), "ij".to_string()]
        );
    }

    #[test]
    fn ut_wrap_long_word_mixed_with_short() {
        assert_eq!(
            wrap("hi abcdefghij", 4),
            vec![
                "hi".to_string(),
                "abcd".to_string(),
                "efgh".to_string(),
                "ij".to_string()
            ]
        );
    }

    #[test]
    fn ut_wrap_width_zero_safe() {
        assert_eq!(wrap("hello world", 0), vec!["hello world".to_string()]);
    }
}
