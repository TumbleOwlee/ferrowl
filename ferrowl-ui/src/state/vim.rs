//! Vim-mode primitives for [`super::CodeInputFieldState`]: the mode enum,
//! word-motion helpers, and the OSC 52 clipboard side-effect used by yank/delete.

use std::io::Write;

/// Editing mode for a vim-enabled [`super::CodeInputFieldState`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VimMode {
    Normal,
    Insert,
    /// `linewise` is `true` for `V` (line-visual), `false` for `v` (charwise).
    Visual {
        linewise: bool,
    },
}

/// Word-class used by the `w`/`b`/`e` motions: a "word" is a maximal run of
/// [`Word`](CharClass::Word) chars, a maximal run of [`Punct`](CharClass::Punct)
/// chars is its own separate word, and [`Space`](CharClass::Space) is skipped.
#[derive(Debug, Clone, Copy, PartialEq)]
enum CharClass {
    Word,
    Punct,
    Space,
}

fn char_class(c: char) -> CharClass {
    if c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else if c.is_whitespace() {
        CharClass::Space
    } else {
        CharClass::Punct
    }
}

fn line_chars(lines: &[String], line: usize) -> Vec<char> {
    lines[line].chars().collect()
}

/// `w`: move forward to the start of the next word, crossing lines like vim.
pub fn word_forward(lines: &[String], line: usize, col: usize) -> (usize, usize) {
    let mut line = line;
    let mut col = col;
    let chars = line_chars(lines, line);
    // Skip the run the cursor is currently sitting in (if any).
    if let Some(&c) = chars.get(col) {
        let class = char_class(c);
        if class != CharClass::Space {
            while let Some(&c2) = line_chars(lines, line).get(col) {
                if char_class(c2) != class {
                    break;
                }
                col += 1;
            }
        }
    }
    loop {
        let chars = line_chars(lines, line);
        if col >= chars.len() {
            if line + 1 >= lines.len() {
                return (line, chars.len().saturating_sub(1));
            }
            line += 1;
            col = 0;
            if lines[line].is_empty() {
                return (line, 0);
            }
            continue;
        }
        if char_class(chars[col]) != CharClass::Space {
            return (line, col);
        }
        col += 1;
    }
}

/// `e`: move forward to the end of the current/next word.
pub fn word_end_forward(lines: &[String], line: usize, col: usize) -> (usize, usize) {
    let mut line = line;
    let mut col = col + 1;
    loop {
        let chars = line_chars(lines, line);
        if col >= chars.len() {
            if line + 1 >= lines.len() {
                return (line, chars.len().saturating_sub(1));
            }
            line += 1;
            col = 0;
            continue;
        }
        if char_class(chars[col]) != CharClass::Space {
            break;
        }
        col += 1;
    }
    loop {
        let chars = line_chars(lines, line);
        let class = char_class(chars[col]);
        let next = chars.get(col + 1).map(|c| char_class(*c));
        if next != Some(class) {
            return (line, col);
        }
        col += 1;
    }
}

/// `b`: move backward to the start of the previous word.
pub fn word_backward(lines: &[String], line: usize, col: usize) -> (usize, usize) {
    let mut line = line;
    let mut col = col;
    loop {
        if col == 0 {
            if line == 0 {
                return (0, 0);
            }
            line -= 1;
            col = line_chars(lines, line).len();
            if col == 0 {
                return (line, 0);
            }
            continue;
        }
        col -= 1;
        let chars = line_chars(lines, line);
        if char_class(chars[col]) != CharClass::Space {
            break;
        }
    }
    let class = char_class(line_chars(lines, line)[col]);
    loop {
        if col == 0 {
            return (line, 0);
        }
        let prev = char_class(line_chars(lines, line)[col - 1]);
        if prev != class {
            return (line, col);
        }
        col -= 1;
    }
}

const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Minimal standard-alphabet base64 encoder (with `=` padding) — hand-rolled
/// so this module doesn't need a new dependency just for OSC 52 payloads.
fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(B64_ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(B64_ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            B64_ALPHABET[((n >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64_ALPHABET[(n & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Copies `text` to the system clipboard via an OSC 52 escape sequence
/// written to stdout. Errors are ignored — this is a best-effort side
/// effect, not something an editor should ever fail over.
pub fn emit_osc52(text: &str) {
    let encoded = base64_encode(text.as_bytes());
    let seq = format!("\x1b]52;c;{encoded}\x07");
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(seq.as_bytes());
    let _ = stdout.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn emit_osc52_does_not_panic() {
        emit_osc52("hello");
    }

    #[test]
    fn word_forward_skips_punct_run_as_own_word() {
        let lines = vec!["foo.bar baz".to_string()];
        assert_eq!(word_forward(&lines, 0, 0), (0, 3));
        assert_eq!(word_forward(&lines, 0, 3), (0, 4));
        assert_eq!(word_forward(&lines, 0, 4), (0, 8));
    }

    #[test]
    fn word_forward_crosses_lines() {
        let lines = vec!["foo".to_string(), "bar".to_string()];
        assert_eq!(word_forward(&lines, 0, 0), (1, 0));
    }

    #[test]
    fn word_backward_basic() {
        let lines = vec!["foo bar baz".to_string()];
        assert_eq!(word_backward(&lines, 0, 8), (0, 4));
        assert_eq!(word_backward(&lines, 0, 4), (0, 0));
    }

    #[test]
    fn word_end_forward_basic() {
        let lines = vec!["foo bar".to_string()];
        assert_eq!(word_end_forward(&lines, 0, 0), (0, 2));
        assert_eq!(word_end_forward(&lines, 0, 2), (0, 6));
    }
}
