use std::collections::HashSet;

use ferrowl_syntax::markdown::{BlockKind, BlockLine, InlineKind, InlineSpan, escape_markers};
use ferrowl_syntax::{Language, LineState, highlight_line};
use ratatui::style::{Modifier, Style};

use crate::style::{MarkdownTheme, SyntaxTheme};

/// One source line rendered to display cells, before wrapping.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "the markdown input field widget renders through it"
)]
pub(crate) struct RenderedLine {
    /// Styled runs of the rendered text, markers already hidden.
    pub spans: Vec<(String, Style)>,
    /// Display columns continuation rows are indented by (UI-R-132).
    pub hanging_indent: usize,
    /// Fence delimiter/body: wrap at a character boundary, no hanging indent (UI-R-133).
    pub char_wrap: bool,
    /// Horizontal rule: fill the widget width with the rule style (UI-R-148).
    pub rule: bool,
}

/// Renders one classified source line. `carry` is the syntax-highlighter state threaded
/// through a `lua`/`json` fence body (UI-R-151); it is returned updated.
#[allow(
    dead_code,
    reason = "the markdown input field widget renders through it"
)]
pub(crate) fn render_line(
    block: &BlockLine,
    source: &str,
    fence_info: Option<&str>,
    carry: LineState,
    md: &MarkdownTheme,
    syntax: &SyntaxTheme,
    base: Style,
) -> (RenderedLine, LineState) {
    let chars: Vec<char> = source.chars().collect();

    match &block.kind {
        BlockKind::Paragraph => {
            let mut spans = inline_render(&chars, 0, &block.inline, md, base);
            if spans.is_empty() {
                spans.push((String::new(), base));
            }
            (
                RenderedLine {
                    spans,
                    hanging_indent: 0,
                    char_wrap: false,
                    rule: false,
                },
                carry,
            )
        }
        BlockKind::Heading { level } => {
            let style = md.heading(*level).add_modifier(Modifier::BOLD);
            let mut spans = inline_render(&chars, block.content_start, &block.inline, md, style);
            if spans.is_empty() {
                spans.push((String::new(), style));
            }
            (
                RenderedLine {
                    spans,
                    hanging_indent: 0,
                    char_wrap: false,
                    rule: false,
                },
                carry,
            )
        }
        BlockKind::UnorderedItem { .. } => {
            let leading = leading_spaces(&chars);
            let mut spans = Vec::new();
            let mut hanging_indent = 0usize;
            if leading > 0 {
                spans.push((" ".repeat(leading), base));
                hanging_indent += leading;
            }
            spans.push(("•".to_string(), md.bullet));
            spans.push((" ".to_string(), base));
            hanging_indent += 2;
            spans.extend(inline_render(
                &chars,
                block.content_start,
                &block.inline,
                md,
                base,
            ));
            (
                RenderedLine {
                    spans,
                    hanging_indent,
                    char_wrap: false,
                    rule: false,
                },
                carry,
            )
        }
        BlockKind::OrderedItem { .. } => {
            let prefix: String = chars[..block.content_start].iter().collect();
            let hanging_indent = prefix.chars().count();
            let mut spans = vec![(prefix, base)];
            spans.extend(inline_render(
                &chars,
                block.content_start,
                &block.inline,
                md,
                base,
            ));
            (
                RenderedLine {
                    spans,
                    hanging_indent,
                    char_wrap: false,
                    rule: false,
                },
                carry,
            )
        }
        BlockKind::TaskItem { checked, .. } => {
            let leading = leading_spaces(&chars);
            let box_ch = if *checked { "☑" } else { "☐" };
            let mut spans = Vec::new();
            let mut hanging_indent = 0usize;
            if leading > 0 {
                spans.push((" ".repeat(leading), base));
                hanging_indent += leading;
            }
            spans.push((box_ch.to_string(), md.bullet));
            spans.push((" ".to_string(), base));
            hanging_indent += 2;
            spans.extend(inline_render(
                &chars,
                block.content_start,
                &block.inline,
                md,
                base,
            ));
            (
                RenderedLine {
                    spans,
                    hanging_indent,
                    char_wrap: false,
                    rule: false,
                },
                carry,
            )
        }
        BlockKind::Quote { depth } => {
            let mut spans = Vec::new();
            for d in 1..=*depth {
                spans.push(("▎".to_string(), md.quote_bar(d)));
            }
            let hanging_indent = *depth;
            spans.extend(inline_render(
                &chars,
                block.content_start,
                &block.inline,
                md,
                md.quote_text()
                    .add_modifier(Modifier::DIM | Modifier::ITALIC),
            ));
            (
                RenderedLine {
                    spans,
                    hanging_indent,
                    char_wrap: false,
                    rule: false,
                },
                carry,
            )
        }
        BlockKind::Rule => (
            RenderedLine {
                spans: Vec::new(),
                hanging_indent: 0,
                char_wrap: false,
                rule: true,
            },
            carry,
        ),
        BlockKind::FenceDelimiter { .. } => (
            RenderedLine {
                spans: vec![(String::new(), *md.code())],
                hanging_indent: 0,
                char_wrap: true,
                rule: false,
            },
            carry,
        ),
        BlockKind::FenceBody => render_fence_body(&chars, source, fence_info, carry, md, syntax),
    }
}

fn render_fence_body(
    chars: &[char],
    source: &str,
    fence_info: Option<&str>,
    carry: LineState,
    md: &MarkdownTheme,
    syntax: &SyntaxTheme,
) -> (RenderedLine, LineState) {
    let lang = match fence_info {
        Some("lua") => Some(Language::Lua),
        Some("json") => Some(Language::Json),
        _ => None,
    };
    let Some(lang) = lang else {
        return (
            RenderedLine {
                spans: vec![(source.to_string(), *md.code())],
                hanging_indent: 0,
                char_wrap: true,
                rule: false,
            },
            carry,
        );
    };
    let (hl_spans, next_carry) = highlight_line(lang, source, carry);
    let mut spans = Vec::new();
    let mut prev = 0usize;
    for (start, end, kind) in hl_spans {
        if start > prev {
            spans.push((chars[prev..start].iter().collect(), *md.code()));
        }
        spans.push((chars[start..end].iter().collect(), syntax.style(kind)));
        prev = end;
    }
    if prev < chars.len() {
        spans.push((chars[prev..].iter().collect(), *md.code()));
    }
    (
        RenderedLine {
            spans,
            hanging_indent: 0,
            char_wrap: true,
            rule: false,
        },
        next_carry,
    )
}

fn leading_spaces(chars: &[char]) -> usize {
    chars.iter().take_while(|c| **c == ' ').count()
}

fn inline_render(
    chars: &[char],
    from: usize,
    inline: &[InlineSpan],
    md: &MarkdownTheme,
    base: Style,
) -> Vec<(String, Style)> {
    let len = chars.len();
    if from >= len {
        return Vec::new();
    }
    let text: String = chars[from..].iter().collect();
    let mut hidden: HashSet<usize> = escape_markers(&text)
        .into_iter()
        .map(|i| i + from)
        .collect();
    for s in inline {
        for (a, b) in &s.markers {
            for p in *a..*b {
                hidden.insert(p);
            }
        }
    }

    let mut style_at: Vec<Option<Style>> = vec![None; len - from];
    for s in inline {
        let style = match s.kind {
            InlineKind::Bold => base.add_modifier(Modifier::BOLD),
            InlineKind::Italic => base.add_modifier(Modifier::ITALIC),
            InlineKind::Code => *md.code(),
            InlineKind::Strike => base.add_modifier(Modifier::CROSSED_OUT),
            InlineKind::Link => md.link().add_modifier(Modifier::UNDERLINED),
            InlineKind::Image => *md.image(),
        };
        for p in s.content.0..s.content.1 {
            if p >= from && p < len {
                style_at[p - from] = Some(style);
            }
        }
    }

    let mut spans = Vec::new();
    let mut i = from;
    while i < len {
        if hidden.contains(&i) {
            i += 1;
            continue;
        }
        let style = style_at[i - from].unwrap_or(base);
        let start = i;
        while i < len && !hidden.contains(&i) && style_at[i - from].unwrap_or(base) == style {
            i += 1;
        }
        spans.push((chars[start..i].iter().collect(), style));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::MarkdownThemeBuilder;
    use ferrowl_syntax::markdown::{BlockState, block_line};

    fn render(line: &str) -> RenderedLine {
        let (block, _) = block_line(line, &BlockState::default());
        let md = MarkdownTheme::default();
        let syntax = SyntaxTheme::default();
        render_line(
            &block,
            line,
            None,
            LineState::default(),
            &md,
            &syntax,
            Style::default(),
        )
        .0
    }

    fn text(rl: &RenderedLine) -> String {
        rl.spans.iter().map(|(t, _)| t.as_str()).collect()
    }

    #[test]
    /// UI-R-142 — every classified source line renders to exactly one `RenderedLine`.
    fn ut_each_source_line_renders_to_exactly_one_line() {
        let lines = ["first paragraph", "second paragraph", "third paragraph"];
        let md = MarkdownTheme::default();
        let syntax = SyntaxTheme::default();
        let mut state = BlockState::default();
        let mut carry = LineState::default();
        let mut rendered = Vec::new();
        for line in lines {
            let (block, next_state) = block_line(line, &state);
            state = next_state;
            let (rl, next_carry) =
                render_line(&block, line, None, carry, &md, &syntax, Style::default());
            carry = next_carry;
            rendered.push(rl);
        }
        assert_eq!(rendered.len(), lines.len());
        for (rl, line) in rendered.iter().zip(lines.iter()) {
            let t = text(rl);
            assert!(
                !t.contains('\n'),
                "rendered line joined multiple source lines: {t:?}"
            );
            assert_eq!(t, *line, "line {line:?} was not preserved 1:1");
        }
    }

    #[test]
    /// UI-R-143 — a heading hides its `#` markers and draws the text bold in the level's style.
    fn ut_heading_hides_markers_and_uses_level_style_bold() {
        let md = MarkdownTheme::default();
        let rl = render("### Title");
        assert_eq!(text(&rl), "Title");
        assert_eq!(rl.spans[0].1, md.heading(3));

        let bare = MarkdownThemeBuilder::default()
            .heading([Style::default(); 6])
            .build()
            .expect("builder");
        let (block, _) = block_line("### Title", &BlockState::default());
        let (rl, _) = render_line(
            &block,
            "### Title",
            None,
            LineState::default(),
            &bare,
            &SyntaxTheme::default(),
            Style::default(),
        );
        assert!(
            rl.spans[0].1.add_modifier.contains(Modifier::BOLD),
            "heading must be bold even when the theme style carries no modifier"
        );
    }

    #[test]
    /// UI-R-144 — an unordered item's marker becomes a bullet; leading indentation is kept.
    fn ut_unordered_item_marker_becomes_bullet_keeping_indent() {
        let md = MarkdownTheme::default();
        let rl = render("  - item");
        assert_eq!(text(&rl), "  • item");
        let bullet_span = rl
            .spans
            .iter()
            .find(|(t, _)| t == "•")
            .expect("bullet span");
        assert_eq!(bullet_span.1, md.bullet);
    }

    #[test]
    /// UI-R-145 — an ordered item keeps its number and delimiter exactly as written.
    fn ut_ordered_item_keeps_number_and_delimiter() {
        let rl = render("12. item");
        assert_eq!(text(&rl), "12. item");
    }

    #[test]
    /// UI-R-146 — a task item's brackets become a checkbox glyph, checked state preserved.
    fn ut_task_item_markers_become_boxes() {
        assert_eq!(text(&render("- [ ] todo")), "☐ todo");
        assert_eq!(text(&render("- [x] done")), "☑ done");
    }

    #[test]
    /// UI-R-147 — quote markers become bars in the theme's per-depth color; text is dim+italic.
    fn ut_quote_markers_become_bars_and_text_is_dim_italic() {
        let md = MarkdownTheme::default();
        let rl = render("> quoted text");
        assert_eq!(text(&rl), "▎quoted text");
        let bar_span = rl.spans.iter().find(|(t, _)| t == "▎").expect("bar span");
        assert_eq!(bar_span.1, md.quote_bar(1));
        let text_span = rl
            .spans
            .iter()
            .find(|(t, _)| t == "quoted text")
            .expect("text span");
        assert_eq!(text_span.1, *md.quote_text());
        assert!(text_span.1.add_modifier.contains(Modifier::DIM));
        assert!(text_span.1.add_modifier.contains(Modifier::ITALIC));

        let bare = MarkdownThemeBuilder::default()
            .quote_text(Style::default())
            .build()
            .expect("builder");
        let (block, _) = block_line("> quoted text", &BlockState::default());
        let (rl, _) = render_line(
            &block,
            "> quoted text",
            None,
            LineState::default(),
            &bare,
            &SyntaxTheme::default(),
            Style::default(),
        );
        let text_span = rl
            .spans
            .iter()
            .find(|(t, _)| t == "quoted text")
            .expect("text span");
        assert!(
            text_span.1.add_modifier.contains(Modifier::DIM)
                && text_span.1.add_modifier.contains(Modifier::ITALIC),
            "quoted text must be dim+italic even when the theme style carries no modifier"
        );
    }

    #[test]
    /// UI-R-148 — a horizontal rule renders as `RenderedLine { rule: true, .. }`.
    fn ut_horizontal_rule_is_a_rule_line() {
        let rl = render("---");
        assert!(rl.rule);
    }

    #[test]
    /// UI-R-149 — a fence delimiter renders empty in the theme's code style.
    fn ut_fence_delimiter_renders_empty_in_code_style() {
        let md = MarkdownTheme::default();
        let rl = render("```lua");
        assert_eq!(text(&rl), "");
        assert_eq!(rl.spans[0].1, *md.code());
        assert!(rl.char_wrap);
    }

    #[test]
    /// UI-R-150 — a fence body with no recognized info string is drawn verbatim in the code style.
    fn ut_fence_body_is_verbatim_in_code_style() {
        let (open, state) = block_line("```", &BlockState::default());
        assert!(matches!(open.kind, BlockKind::FenceDelimiter { .. }));
        let (body, _) = block_line("plain text here", &state);
        let md = MarkdownTheme::default();
        let syntax = SyntaxTheme::default();
        let (rl, _) = render_line(
            &body,
            "plain text here",
            None,
            LineState::default(),
            &md,
            &syntax,
            Style::default(),
        );
        assert_eq!(text(&rl), "plain text here");
        assert_eq!(rl.spans[0].1, *md.code());
        assert!(rl.char_wrap);
    }

    #[test]
    /// UI-R-151 — a `lua` fence body is highlighted through the syntax highlighter, with the
    /// carry-over line state threaded across the block (a long string opened on one line and
    /// closed on the next proves the carry).
    fn ut_lua_and_json_fence_bodies_are_syntax_highlighted_with_threaded_carry() {
        let (open, mut state) = block_line("```lua", &BlockState::default());
        assert!(matches!(open.kind, BlockKind::FenceDelimiter { .. }));
        let fence_info = Some("lua");
        let md = MarkdownTheme::default();
        let syntax = SyntaxTheme::default();
        let mut carry = LineState::default();

        let (body1, next_state) = block_line("s = [[", &state);
        state = next_state;
        let (rl1, next_carry) = render_line(
            &body1,
            "s = [[",
            fence_info,
            carry,
            &md,
            &syntax,
            Style::default(),
        );
        carry = next_carry;
        assert_eq!(text(&rl1), "s = [[");

        let (body2, _) = block_line("still a string]]", &state);
        let (rl2, _) = render_line(
            &body2,
            "still a string]]",
            fence_info,
            carry,
            &md,
            &syntax,
            Style::default(),
        );
        assert_eq!(text(&rl2), "still a string]]");
        let string_span = rl2
            .spans
            .iter()
            .find(|(t, _)| t.contains("still a string"))
            .expect("carried string span");
        assert_eq!(string_span.1, *syntax.string());

        let (open, state) = block_line("```json", &BlockState::default());
        assert!(matches!(open.kind, BlockKind::FenceDelimiter { .. }));
        let (body, _) = block_line(r#"{"a": true}"#, &state);
        let (rl, _) = render_line(
            &body,
            r#"{"a": true}"#,
            Some("json"),
            LineState::default(),
            &md,
            &syntax,
            Style::default(),
        );
        assert_eq!(text(&rl), r#"{"a": true}"#);
        let literal_span = rl
            .spans
            .iter()
            .find(|(t, _)| t == "true")
            .expect("json literal span");
        assert_eq!(literal_span.1, *syntax.literal());
    }

    #[test]
    /// UI-R-152 — inline bold, italic, code and strike hide their markers and style the content.
    fn ut_inline_emphasis_code_and_strike_hide_markers() {
        let md = MarkdownTheme::default();
        let rl = render("**bold** *italic* `code` ~~strike~~");
        assert_eq!(text(&rl), "bold italic code strike");
        let bold_span = rl
            .spans
            .iter()
            .find(|(t, _)| t == "bold")
            .expect("bold span");
        assert!(bold_span.1.add_modifier.contains(Modifier::BOLD));
        let italic_span = rl
            .spans
            .iter()
            .find(|(t, _)| t == "italic")
            .expect("italic span");
        assert!(italic_span.1.add_modifier.contains(Modifier::ITALIC));
        let code_span = rl
            .spans
            .iter()
            .find(|(t, _)| t == "code")
            .expect("code span");
        assert_eq!(code_span.1, *md.code());
        let strike_span = rl
            .spans
            .iter()
            .find(|(t, _)| t == "strike")
            .expect("strike span");
        assert!(strike_span.1.add_modifier.contains(Modifier::CROSSED_OUT));
    }

    #[test]
    /// UI-R-153 — a link renders its text underlined in the theme's link style; markers hidden.
    fn ut_link_renders_text_underlined_in_link_style() {
        let md = MarkdownTheme::default();
        let rl = render("[a link](http://x)");
        assert_eq!(text(&rl), "a link");
        assert_eq!(rl.spans[0].1, *md.link());
        assert!(rl.spans[0].1.add_modifier.contains(Modifier::UNDERLINED));

        let bare = MarkdownThemeBuilder::default()
            .link(Style::default())
            .build()
            .expect("builder");
        let (block, _) = block_line("[a link](http://x)", &BlockState::default());
        let (rl, _) = render_line(
            &block,
            "[a link](http://x)",
            None,
            LineState::default(),
            &bare,
            &SyntaxTheme::default(),
            Style::default(),
        );
        assert!(
            rl.spans[0].1.add_modifier.contains(Modifier::UNDERLINED),
            "link text must be underlined even when the theme style carries no modifier"
        );
    }

    #[test]
    /// UI-R-154 — an image renders its alt text in the theme's image style; markers hidden.
    fn ut_image_renders_alt_in_image_style() {
        let md = MarkdownTheme::default();
        let rl = render("![alt](img.png)");
        assert_eq!(text(&rl), "alt");
        assert_eq!(rl.spans[0].1, *md.image());
    }

    #[test]
    /// UI-E-074 — tables, raw HTML, footnotes, reference links and autolinks render as plain text.
    fn ut_unsupported_constructs_render_as_plain_text() {
        for line in [
            "| a | b |",
            "raw <b>html</b>",
            "[ref][1]",
            "<https://example.com>",
            "  indented paragraph text",
        ] {
            let rl = render(line);
            assert_eq!(text(&rl), line);
        }
    }
}
