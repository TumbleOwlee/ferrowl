//! Lua re-indenter. Not a full formatter: only leading whitespace is rewritten, per
//! line, to `4 * depth` spaces. Everything after the leading whitespace is byte-for-byte
//! unchanged. Depth is a running balance of block openers/closers, computed by
//! token-scanning each line with the existing Lua lexer — so keywords or brackets that
//! appear inside strings/comments never affect it. Always succeeds (re-indenting can't
//! fail the way JSON validation can).

use crate::lang::lua::highlight_line;
use crate::{LineState, LuaCarry, SyntaxKind};

pub(crate) fn is_opener(text: &str, kind: SyntaxKind) -> bool {
    // `(` is deliberately excluded: it tracks call/signature grouping, not block
    // nesting, and counting it alongside `function`/`do`/etc. double-counts depth
    // on multi-line signatures and calls (e.g. `function foo(\n  a,\n)`).
    matches!(text, "function" | "do" | "then" | "repeat")
        || (kind == SyntaxKind::Punct && text == "{")
}

pub(crate) fn is_closer(text: &str, kind: SyntaxKind) -> bool {
    matches!(text, "end" | "until") || (kind == SyntaxKind::Punct && text == "}")
}

pub(crate) fn is_else(text: &str) -> bool {
    matches!(text, "else" | "elseif")
}

pub(super) fn format(source: &str) -> String {
    let mut state = LineState::default();
    let mut depth: usize = 0;
    let mut out_lines: Vec<String> = Vec::new();

    for raw_line in source.split('\n') {
        let starts_inside = !matches!(state.0, LuaCarry::None);
        let (spans, next_state) = highlight_line(raw_line, state);

        if starts_inside {
            out_lines.push(raw_line.to_string());
        } else if raw_line.trim().is_empty() {
            out_lines.push(String::new());
        } else {
            let chars: Vec<char> = raw_line.chars().collect();
            let mut leading = true;
            let mut print_depth: Option<usize> = None;
            for (start, end, kind) in &spans {
                let text: String = chars[*start..*end].iter().collect();
                let (opener, closer) = (is_opener(&text, *kind), is_closer(&text, *kind));
                if leading {
                    if closer {
                        depth = depth.saturating_sub(1);
                        continue;
                    } else if is_else(&text) {
                        // `else`/`elseif` sit at the parent's depth. `elseif` restores
                        // the level itself via its own trailing `then` (handled below,
                        // like a fresh `if`); plain `else` has no such token, so restore
                        // immediately — its body is still one level deeper.
                        let dedented = depth.saturating_sub(1);
                        print_depth = Some(dedented);
                        depth = if text == "else" {
                            dedented + 1
                        } else {
                            dedented
                        };
                        leading = false;
                        continue;
                    } else {
                        leading = false;
                        print_depth = Some(depth);
                    }
                }
                if opener {
                    depth += 1;
                } else if closer {
                    depth = depth.saturating_sub(1);
                }
            }
            let print_depth = print_depth.unwrap_or(depth);
            let rest = raw_line.trim_start();
            out_lines.push(format!("{}{}", " ".repeat(4 * print_depth), rest));
        }

        state = next_state;
    }

    out_lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// UI-R-033 — the Lua formatter reindents nested function/if/for blocks four spaces per level.
    fn ut_nested_function_if_for_blocks_reindent() {
        let src = "function foo()\nfor i=1,10 do\nif i>5 then\nprint(i)\nend\nend\nend";
        let expected = "function foo()\n    for i=1,10 do\n        if i>5 then\n            print(i)\n        end\n    end\nend";
        assert_eq!(format(src), expected);
    }

    #[test]
    /// UI-R-033 — elseif/else are reindented to the parent block depth.
    fn ut_elseif_and_else_sit_at_parent_depth() {
        let src = "if x then\na()\nelseif y then\nb()\nelse\nc()\nend";
        let expected = "if x then\n    a()\nelseif y then\n    b()\nelse\n    c()\nend";
        assert_eq!(format(src), expected);
    }

    #[test]
    /// UI-R-033 — the formatter leaves long-string interior lines byte-for-byte unchanged.
    fn ut_long_string_interior_lines_byte_untouched() {
        let src = "local s = [[\n    keep me exactly    \nend]]";
        let expected = "local s = [[\n    keep me exactly    \nend]]";
        assert_eq!(format(src), expected);
    }

    #[test]
    /// UI-R-033 — an `end` inside a string does not change reindent depth.
    fn ut_end_inside_string_does_not_dedent() {
        let src = "function foo()\nlocal s = \"end\"\nend";
        let expected = "function foo()\n    local s = \"end\"\nend";
        assert_eq!(format(src), expected);
    }

    #[test]
    /// UI-R-033 — repeat/until blocks reindent correctly.
    fn ut_repeat_until_blocks() {
        let src = "repeat\nx()\nuntil done";
        let expected = "repeat\n    x()\nuntil done";
        assert_eq!(format(src), expected);
    }

    #[test]
    /// UI-R-033 — blank lines are normalized to empty by the formatter.
    fn ut_blank_lines_become_empty() {
        let src = "function foo()\n\nend";
        let expected = "function foo()\n\nend";
        assert_eq!(format(src), expected);
    }

    #[test]
    /// UI-R-033 — the Lua formatter is idempotent.
    fn ut_idempotent() {
        let src = "function foo()\n  for i=1,10 do\n      print(i)\n end\nend";
        let once = format(src);
        let twice = format(&once);
        assert_eq!(once, twice);
    }

    #[test]
    /// UI-R-033 — the Lua formatter always returns output, even for un-parseable input.
    fn ut_always_returns_something_for_garbage() {
        let src = "]==] unterminated [==[";
        let _ = format(src);
    }

    #[test]
    /// UI-R-033 — multi-line call parentheses do not double-indent.
    fn ut_multiline_call_parens_do_not_double_indent() {
        // Regression: `(` used to be tracked as a block opener alongside `function`,
        // doubling the indent depth for continuation lines inside a multi-line signature.
        let src = "function foo(\na,\nb\n)\nbody()\nend";
        let expected = "function foo(\n    a,\n    b\n    )\n    body()\nend";
        assert_eq!(format(src), expected);
    }
}
