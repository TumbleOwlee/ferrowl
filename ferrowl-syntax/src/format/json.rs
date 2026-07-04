//! Hand-rolled JSON validator + pretty-printer. Any parse error returns `None` so the
//! caller never overwrites invalid (or mid-edit) content. Numbers and strings are copied
//! verbatim from the source token — never reparsed/reformatted — so escapes, unicode
//! sequences and float representations survive byte-for-byte. Key order is preserved
//! (no sorting).

/// A parsed JSON value. Scalars (strings/numbers/literals) keep their raw source text.
enum Value {
    Object(Vec<(String, Value)>),
    Array(Vec<Value>),
    Scalar(String),
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.pos += 1;
        }
    }

    fn parse_value(&mut self) -> Option<Value> {
        match self.peek()? {
            '{' => self.parse_object(),
            '[' => self.parse_array(),
            '"' => self.parse_string_raw().map(Value::Scalar),
            '-' | '0'..='9' => self.parse_number_raw().map(Value::Scalar),
            't' => self.parse_keyword("true").map(Value::Scalar),
            'f' => self.parse_keyword("false").map(Value::Scalar),
            'n' => self.parse_keyword("null").map(Value::Scalar),
            _ => None,
        }
    }

    fn parse_keyword(&mut self, word: &str) -> Option<String> {
        let word_chars: Vec<char> = word.chars().collect();
        let end = self.pos + word_chars.len();
        if end > self.chars.len() || self.chars[self.pos..end] != word_chars[..] {
            return None;
        }
        if self.chars.get(end).is_some_and(|c| c.is_alphanumeric() || *c == '_') {
            return None;
        }
        self.pos = end;
        Some(word.to_string())
    }

    fn parse_number_raw(&mut self) -> Option<String> {
        let start = self.pos;
        if self.peek() == Some('-') {
            self.pos += 1;
        }
        match self.peek()? {
            '0' => self.pos += 1,
            '1'..='9' => {
                self.pos += 1;
                while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                    self.pos += 1;
                }
            }
            _ => return None,
        }
        if self.peek() == Some('.') {
            self.pos += 1;
            if !matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                return None;
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some('e') | Some('E')) {
            self.pos += 1;
            if matches!(self.peek(), Some('+') | Some('-')) {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                return None;
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        Some(self.chars[start..self.pos].iter().collect())
    }

    fn parse_string_raw(&mut self) -> Option<String> {
        let start = self.pos;
        if self.peek() != Some('"') {
            return None;
        }
        self.pos += 1;
        loop {
            match self.peek()? {
                '"' => {
                    self.pos += 1;
                    break;
                }
                '\\' => {
                    let esc = *self.chars.get(self.pos + 1)?;
                    match esc {
                        '"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' => {
                            self.pos += 2;
                        }
                        'u' => {
                            let hex_start = self.pos + 2;
                            let hex_end = hex_start + 4;
                            if hex_end > self.chars.len()
                                || !self.chars[hex_start..hex_end]
                                    .iter()
                                    .all(|c| c.is_ascii_hexdigit())
                            {
                                return None;
                            }
                            self.pos = hex_end;
                        }
                        _ => return None,
                    }
                }
                c if (c as u32) < 0x20 => return None,
                _ => self.pos += 1,
            }
        }
        Some(self.chars[start..self.pos].iter().collect())
    }

    fn parse_object(&mut self) -> Option<Value> {
        self.pos += 1; // '{'
        self.skip_ws();
        let mut entries = Vec::new();
        if self.peek() == Some('}') {
            self.pos += 1;
            return Some(Value::Object(entries));
        }
        loop {
            self.skip_ws();
            let key = self.parse_string_raw()?;
            self.skip_ws();
            if self.peek() != Some(':') {
                return None;
            }
            self.pos += 1;
            self.skip_ws();
            let value = self.parse_value()?;
            entries.push((key, value));
            self.skip_ws();
            match self.peek()? {
                ',' => {
                    self.pos += 1;
                }
                '}' => {
                    self.pos += 1;
                    break;
                }
                _ => return None,
            }
        }
        Some(Value::Object(entries))
    }

    fn parse_array(&mut self) -> Option<Value> {
        self.pos += 1; // '['
        self.skip_ws();
        let mut items = Vec::new();
        if self.peek() == Some(']') {
            self.pos += 1;
            return Some(Value::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.parse_value()?);
            self.skip_ws();
            match self.peek()? {
                ',' => {
                    self.pos += 1;
                }
                ']' => {
                    self.pos += 1;
                    break;
                }
                _ => return None,
            }
        }
        Some(Value::Array(items))
    }
}

fn parse(source: &str) -> Option<Value> {
    let mut p = Parser {
        chars: source.chars().collect(),
        pos: 0,
    };
    p.skip_ws();
    let value = p.parse_value()?;
    p.skip_ws();
    if p.pos != p.chars.len() {
        return None;
    }
    Some(value)
}

fn print_value(value: &Value, depth: usize, out: &mut String) {
    match value {
        Value::Scalar(s) => out.push_str(s),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                out.push_str(&"  ".repeat(depth + 1));
                print_value(item, depth + 1, out);
                if i + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&"  ".repeat(depth));
            out.push(']');
        }
        Value::Object(entries) => {
            if entries.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{\n");
            for (i, (key, value)) in entries.iter().enumerate() {
                out.push_str(&"  ".repeat(depth + 1));
                out.push_str(key);
                out.push_str(": ");
                print_value(value, depth + 1, out);
                if i + 1 < entries.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&"  ".repeat(depth));
            out.push('}');
        }
    }
}

pub(super) fn format(source: &str) -> Option<String> {
    let value = parse(source)?;
    let mut out = String::new();
    print_value(&value, 0, &mut out);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_pretty_prints_messy_valid_json() {
        let out = format(r#"{"b":1,"a":[1,2]}"#).unwrap();
        assert_eq!(out, "{\n  \"b\": 1,\n  \"a\": [\n    1,\n    2\n  ]\n}");
    }

    #[test]
    fn ut_idempotent() {
        let once = format(r#"{"b":1,"a":[1,2],"c":{}}"#).unwrap();
        let twice = format(&once).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn ut_invalid_json_returns_none() {
        assert!(format("{\"a\": ").is_none());
        assert!(format("{\"a\" 1}").is_none());
        assert!(format("[1, 2,]").is_none());
        assert!(format("not json").is_none());
    }

    #[test]
    fn ut_partial_json_returns_none() {
        assert!(format(r#"{"a": 1"#).is_none());
    }

    #[test]
    fn ut_trailing_garbage_returns_none() {
        assert!(format(r#"{"a": 1} garbage"#).is_none());
    }

    #[test]
    fn ut_escaped_quotes_and_unicode_survive_verbatim() {
        let input = "{\"a\":\"x\\\"y\\u00e9zé\"}";
        let out = format(input).unwrap();
        assert_eq!(out, "{\n  \"a\": \"x\\\"y\\u00e9zé\"\n}");
    }

    #[test]
    fn ut_empty_containers() {
        let out = format(r#"{"a": [], "b": {}}"#).unwrap();
        assert_eq!(out, "{\n  \"a\": [],\n  \"b\": {}\n}");
    }

    #[test]
    fn ut_numbers_copied_verbatim() {
        let out = format(r#"[1.50, -3e10]"#).unwrap();
        assert_eq!(out, "[\n  1.50,\n  -3e10\n]");
    }

    #[test]
    fn ut_no_trailing_newline() {
        let out = format(r#"{"a": 1}"#).unwrap();
        assert!(!out.ends_with('\n'));
    }
}
