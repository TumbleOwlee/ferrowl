//! Render-path coverage for the `widgets`, `style`, and `traits` modules:
//! every widget is drawn into an off-screen [`Buffer`] in the configurations
//! that exercise its border, focus, wrapping, scrolling, and placeholder
//! branches. We assert that rendering does not panic and (where cheap) that
//! something was drawn; the goal is to drive the layout/scroll logic.

use ratatui::buffer::Buffer;
use ratatui::layout::{HorizontalAlignment, Margin, Rect};
use ratatui::widgets::{StatefulWidget, Widget as RWidget};

use ferrowl_syntax::{Language, SyntaxKind};
use ferrowl_ui::Border;
use ferrowl_ui::state::{
    ButtonStateBuilder, CodeInputFieldStateBuilder, InputFieldStateBuilder, ScrollingTabsState,
    SelectionStateBuilder, TableStateBuilder,
};
use ferrowl_ui::style::{
    ButtonStyle, InputFieldStyle, ScrollingTabsStyle, SelectionStyle, SyntaxTheme, TableStyle,
    TextStyle,
};
use ferrowl_ui::traits::{Init, ToLabel};
use ferrowl_ui::widgets::{
    ButtonBuilder, CodeInputFieldBuilder, Header, InputFieldBuilder, ScrollingTabsBuilder,
    SelectionBuilder, TableBuilder, TableEntry, TextBuilder, Title, Width,
};

fn buffer(w: u16, h: u16) -> Buffer {
    Buffer::empty(Rect::new(0, 0, w, h))
}

fn full_border() -> Border {
    Border::Full(Margin::new(0, 0))
}

// ---- traits ----

#[test]
fn init_and_to_label() {
    // `Init` for the standard streams.
    let _o = std::io::Stdout::init();
    let _e = std::io::Stderr::init();
    // `ToLabel` for owned and borrowed strings.
    assert_eq!(String::from("x").to_label(), "x");
    assert_eq!("y".to_label(), "y");
}

// ---- styles ----

#[test]
fn style_defaults_build() {
    let _ = ButtonStyle::default();
    let _ = InputFieldStyle::default();
    let _ = ScrollingTabsStyle::default();
    let _ = SelectionStyle::default();
    let _ = TableStyle::default();
    let _ = TextStyle::default();
}

// ---- Button ----

#[test]
fn button_render_variants() {
    let widget = ButtonBuilder::default().build().unwrap();

    // Focused (default), short label.
    let mut st = ButtonStateBuilder::default()
        .label("OK".to_string())
        .build()
        .unwrap();
    let mut b = buffer(20, 3);
    StatefulWidget::render(&widget, Rect::new(0, 0, 20, 3), &mut b, &mut st);

    // Disabled -> general style; owned-widget render path.
    let mut st = ButtonStateBuilder::default()
        .label("NO".to_string())
        .disabled(true)
        .build()
        .unwrap();
    let mut b = buffer(20, 3);
    StatefulWidget::render(widget.clone(), Rect::new(0, 0, 20, 3), &mut b, &mut st);

    // Long label in a narrow area -> wrapping fold; multiline + alignment.
    let wide = ButtonBuilder::default()
        .multiline(true)
        .horizontal_alignment(HorizontalAlignment::Center)
        .build()
        .unwrap();
    let mut st = ButtonStateBuilder::default()
        .label("a very long button label that wraps".to_string())
        .build()
        .unwrap();
    let mut b = buffer(8, 6);
    StatefulWidget::render(&wide, Rect::new(0, 0, 8, 6), &mut b, &mut st);
}

// ---- Text ----

#[test]
fn text_render_variants() {
    // Borderless, short.
    let t = TextBuilder::default().build().unwrap();
    let mut s = "hello".to_string();
    let mut b = buffer(20, 3);
    StatefulWidget::render(&t, Rect::new(0, 0, 20, 3), &mut b, &mut s);

    // Bordered + title + multiline + long text that wraps; owned render path.
    let t = TextBuilder::default()
        .border(full_border())
        .title(Some("title".into()))
        .multiline(true)
        .build()
        .unwrap();
    let mut s = "a fairly long string that should wrap across rows".to_string();
    let mut b = buffer(10, 8);
    StatefulWidget::render(t, Rect::new(0, 0, 10, 8), &mut b, &mut s);
}

// ---- InputField ----

#[test]
fn input_field_render_variants() {
    // Empty + focused -> placeholder fallback text and cursor drawn.
    let f = InputFieldBuilder::<String>::default().build().unwrap();
    let mut st = InputFieldStateBuilder::default().build().unwrap();
    let mut b = buffer(20, 1);
    StatefulWidget::render(&f, Rect::new(0, 0, 20, 1), &mut b, &mut st);

    // Autofill on an empty field.
    let mut st = InputFieldStateBuilder::default()
        .autofill(Some("auto".to_string()))
        .build()
        .unwrap();
    let mut b = buffer(20, 1);
    StatefulWidget::render(&f, Rect::new(0, 0, 20, 1), &mut b, &mut st);

    // Numeric field with invalid, overflowing input -> error style + scroll window.
    let f = InputFieldBuilder::<i32>::default()
        .border(full_border())
        .title(Some("n".into()))
        .build()
        .unwrap();
    let mut st = InputFieldStateBuilder::default()
        .input("not-a-number-and-very-long-indeed".to_string())
        .cursor(12)
        .build()
        .unwrap();
    let mut b = buffer(24, 3);
    StatefulWidget::render(&f, Rect::new(0, 0, 24, 3), &mut b, &mut st);

    // Unfocused + error input -> unfocused error-style border branch.
    let f = InputFieldBuilder::<i32>::default()
        .border(full_border())
        .title(Some("n".into()))
        .build()
        .unwrap();
    let mut st = InputFieldStateBuilder::default()
        .focused(false)
        .input("not-a-number".to_string())
        .build()
        .unwrap();
    let mut b = buffer(24, 3);
    StatefulWidget::render(&f, Rect::new(0, 0, 24, 3), &mut b, &mut st);

    // Disabled (no cursor) and the plain `Widget` impl (builds default state).
    let f = InputFieldBuilder::<String>::default().build().unwrap();
    let mut st = InputFieldStateBuilder::default()
        .disabled(true)
        .build()
        .unwrap();
    let mut b = buffer(20, 1);
    StatefulWidget::render(&f, Rect::new(0, 0, 20, 1), &mut b, &mut st);

    let mut b = buffer(20, 1);
    RWidget::render(
        InputFieldBuilder::<String>::default().build().unwrap(),
        Rect::new(0, 0, 20, 1),
        &mut b,
    );
}

// ---- CodeInputField ----

#[test]
fn code_input_field_render_variants() {
    // Empty + unfocused + placeholder -> placeholder branch (bordered, titled).
    let w = CodeInputFieldBuilder::default()
        .border(full_border())
        .title(Some("code".into()))
        .build()
        .unwrap();
    let mut st = CodeInputFieldStateBuilder::default()
        .focused(false)
        .placeholder(Some("type lua...".to_string()))
        .build()
        .unwrap();
    let mut b = buffer(20, 6);
    StatefulWidget::render(&w, Rect::new(0, 0, 20, 6), &mut b, &mut st);

    // Multi-line content, focused, narrow area -> gutter, scroll, cursor, h-scroll.
    let w = CodeInputFieldBuilder::default().build().unwrap();
    let mut st = CodeInputFieldStateBuilder::default().build().unwrap();
    st.set_content("local x = 1\nlocal y = 2\nprint(x + y)\nreturn x");
    st.set_active_line(3);
    st.set_cursor_col(50);
    let mut b = buffer(8, 2);
    StatefulWidget::render(w, Rect::new(0, 0, 8, 2), &mut b, &mut st);
}

#[test]
fn code_input_field_lua_syntax_highlighting() {
    let theme = SyntaxTheme::default();
    let content = "local x = 1 -- hi";
    let w = CodeInputFieldBuilder::default().build().unwrap();
    let mut st = CodeInputFieldStateBuilder::default()
        .language(Some(Language::Lua))
        .build()
        .unwrap();
    st.set_content(content);
    let mut b = buffer(40, 1);
    StatefulWidget::render(&w, Rect::new(0, 0, 40, 1), &mut b, &mut st);

    // gutter_width = line_count.to_string().len() + 1 = "1".len() + 1 = 2.
    let content_x = 2u16;

    // "local" keyword starts at char 0.
    assert_eq!(b[(content_x, 0)].fg, theme.keyword().fg.unwrap());

    // "-- hi" comment: find its start via the syntax crate directly.
    let (spans, _) =
        ferrowl_syntax::highlight_line(Language::Lua, content, ferrowl_syntax::LineState::default());
    let comment_start = spans
        .iter()
        .find(|(_, _, kind)| *kind == SyntaxKind::Comment)
        .map(|(start, _, _)| *start)
        .expect("expected a comment span");
    assert_eq!(
        b[(content_x + comment_start as u16, 0)].fg,
        theme.comment().fg.unwrap()
    );
}

#[test]
fn code_input_field_json_key_and_string_styles() {
    let theme = SyntaxTheme::default();
    let content = r#"{"key": "value"}"#;
    let w = CodeInputFieldBuilder::default().build().unwrap();
    let mut st = CodeInputFieldStateBuilder::default()
        .language(Some(Language::Json))
        .build()
        .unwrap();
    st.set_content(content);
    let mut b = buffer(40, 1);
    StatefulWidget::render(&w, Rect::new(0, 0, 40, 1), &mut b, &mut st);

    let content_x = 2u16;

    let (spans, _) = ferrowl_syntax::highlight_line(
        Language::Json,
        content,
        ferrowl_syntax::LineState::default(),
    );
    let key_start = spans
        .iter()
        .find(|(_, _, kind)| *kind == SyntaxKind::Key)
        .map(|(start, _, _)| *start)
        .expect("expected a key span");
    let string_start = spans
        .iter()
        .find(|(_, _, kind)| *kind == SyntaxKind::String)
        .map(|(start, _, _)| *start)
        .expect("expected a string span");

    assert_eq!(
        b[(content_x + key_start as u16, 0)].fg,
        theme.key().fg.unwrap()
    );
    assert_eq!(
        b[(content_x + string_start as u16, 0)].fg,
        theme.string().fg.unwrap()
    );
}

#[test]
fn code_input_field_h_scroll_clips_mid_span() {
    let theme = SyntaxTheme::default();
    let content = r#"local s = "abcdefghijklmnopqrstuvwxyz""#;
    let (spans, _) =
        ferrowl_syntax::highlight_line(Language::Lua, content, ferrowl_syntax::LineState::default());
    let (string_start, string_end, _) = spans
        .into_iter()
        .find(|(_, _, kind)| *kind == SyntaxKind::String)
        .expect("expected a string span");
    assert!(string_end - string_start >= 6, "string span too short for test");
    let h_scroll = string_start + 2;

    let w = CodeInputFieldBuilder::default().build().unwrap();
    let mut st = CodeInputFieldStateBuilder::default()
        .language(Some(Language::Lua))
        .build()
        .unwrap();
    st.set_content(content);
    // Keep the cursor away from content_x (offset 0) so its overlay style doesn't
    // mask the syntax color we're asserting on there.
    st.set_cursor_col(h_scroll + 3);
    st.set_h_scroll(h_scroll);
    let mut b = buffer(12, 1);
    StatefulWidget::render(&w, Rect::new(0, 0, 12, 1), &mut b, &mut st);

    // gutter_width = 2 (single line); the clipped span should appear at content_x
    // even though its unclipped start (`string_start`) is earlier in the line.
    let content_x = 2u16;
    assert_eq!(
        b[(content_x, 0)].fg,
        theme.string().fg.unwrap(),
        "expected string style at the clipped screen offset"
    );
}

// ---- ScrollingTabs ----

#[test]
fn scrolling_tabs_render_variants() {
    let w = ScrollingTabsBuilder::<String>::default().build().unwrap();

    // Empty -> early return.
    let mut st = ScrollingTabsState::<String> {
        titles: vec![],
        selected: 0,
    };
    let mut b = buffer(20, 1);
    StatefulWidget::render(&w, Rect::new(0, 0, 20, 1), &mut b, &mut st);

    // Several tabs, selected in the middle, narrow width -> centering logic.
    let titles = vec![
        "alpha".to_string(),
        "beta".to_string(),
        "gamma".to_string(),
        "delta".to_string(),
        "epsilon".to_string(),
    ];
    let mut st = ScrollingTabsState {
        titles,
        selected: 2,
    };
    let mut b = buffer(12, 1);
    StatefulWidget::render(w, Rect::new(0, 0, 12, 1), &mut b, &mut st);
}

// ---- Selection ----

#[test]
fn selection_render_variants() {
    // Borderless, focused, selected item wider than the area -> horizontal offset.
    let w = SelectionBuilder::<String>::default().build().unwrap();
    let values: Vec<String> = vec![
        "short".to_string(),
        "a selected entry that is wider than the area".to_string(),
        "tail".to_string(),
    ];
    let mut st = SelectionStateBuilder::default()
        .values(values.clone())
        .build()
        .unwrap();
    st.move_down(); // select the wide row
    st.move_right(); // give it a horizontal offset
    let mut b = buffer(10, 5);
    StatefulWidget::render(&w, Rect::new(0, 0, 10, 5), &mut b, &mut st);

    // Bordered + title, unfocused; plain Widget impl (default state).
    let w = SelectionBuilder::<String>::default()
        .border(full_border())
        .title(Some("pick".into()))
        .build()
        .unwrap();
    let mut st = SelectionStateBuilder::default()
        .values(values)
        .focused(false)
        .build()
        .unwrap();
    let mut b = buffer(20, 6);
    StatefulWidget::render(&w, Rect::new(0, 0, 20, 6), &mut b, &mut st);
}

// ---- Title conversions + Widget<S, W> pair forwarding ----

#[test]
fn title_conversions_and_widget_pair() {
    use ferrowl_ui::traits::{HandleEvents, IsFocus, Margins, SetFocus};
    use ferrowl_ui::widgets::{GetValue, Widget as WidgetPair};
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    // All four `From` impls for `Title`.
    let _: Title = "t".into();
    let _: Title = String::from("t").into();
    let _: Title = ("t", HorizontalAlignment::Center).into();
    let _: Title = (String::from("t"), HorizontalAlignment::Right).into();

    // The pair forwards each trait to the appropriate half.
    let state = InputFieldStateBuilder::default()
        .input("hi".to_string())
        .build()
        .unwrap();
    let widget = InputFieldBuilder::<String>::default().build().unwrap();
    let mut pair = WidgetPair { state, widget };

    assert_eq!(pair.get_value(), "hi");
    let _ = pair.margins();
    pair.set_focused(false);
    assert!(!pair.is_focused());
    let _ = pair.handle_events(KeyModifiers::NONE, KeyCode::Char('x'));

    // Render through both the borrowed and owned `RenderWidget`/`StatefulWidget` impls.
    let mut b = buffer(20, 1);
    RWidget::render(&pair, Rect::new(0, 0, 20, 1), &mut b);
    let mut b = buffer(20, 1);
    RWidget::render(pair.clone(), Rect::new(0, 0, 20, 1), &mut b);

    let mut s = InputFieldStateBuilder::default().build().unwrap();
    let mut b = buffer(20, 1);
    StatefulWidget::render(&pair, Rect::new(0, 0, 20, 1), &mut b, &mut s);
    let mut b = buffer(20, 1);
    StatefulWidget::render(pair, Rect::new(0, 0, 20, 1), &mut b, &mut s);
}

// ---- Table ----

#[derive(Clone, Default)]
struct Row(String, String);

impl TableEntry<2> for Row {
    fn values(&self) -> [String; 2] {
        [self.0.clone(), self.1.clone()]
    }
    fn height(&self) -> u16 {
        1
    }
}

#[derive(Clone)]
struct Cols;

impl Header<2> for Cols {
    fn header() -> [String; 2] {
        ["Name".to_string(), "Value".to_string()]
    }
    fn widths() -> [Width; 2] {
        [Width { min: 4, max: 8 }, Width { min: 4, max: 20 }]
    }
}

#[test]
fn table_render_variants() {
    let rows = vec![
        Row(
            "alpha".to_string(),
            "a value with several words to wrap".to_string(),
        ),
        Row("beta".to_string(), "short".to_string()),
        Row(
            "gamma".to_string(),
            "supercalifragilisticexpialidocious".to_string(),
        ),
    ];

    // Focused, area wide enough that total_width <= area.width (no h-scroll).
    let w = TableBuilder::<Row, Cols, 2>::default().build().unwrap();
    let mut st = TableStateBuilder::default()
        .values(rows.clone())
        .build()
        .unwrap();
    let mut b = buffer(40, 8);
    StatefulWidget::render(&w, Rect::new(0, 0, 40, 8), &mut b, &mut st);

    // Bordered + title, unfocused, narrow area -> total_width > area.width (h-scroll copy),
    // and a column rendered without whitespace splitting.
    let w = TableBuilder::<Row, Cols, 2>::default()
        .border(full_border())
        .title(Some("regs".into()))
        .split_by_whitespace([true, false])
        .build()
        .unwrap();
    let mut st = TableStateBuilder::default()
        .values(rows.clone())
        .focused(false)
        .build()
        .unwrap();
    let mut b = buffer(14, 8);
    StatefulWidget::render(&w, Rect::new(0, 0, 14, 8), &mut b, &mut st);
}
