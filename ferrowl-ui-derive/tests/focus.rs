use derive_builder::Builder;
use ferrowl_ui::traits::IsFocus;
use ferrowl_ui_derive::{Focus, focusable};

#[derive(Default, Clone, Debug)]
struct Widget {
    focused: bool,
    /// Number of events routed to this widget, used to assert event dispatch targets.
    events: u32,
}

impl ferrowl_ui::traits::SetFocus for Widget {
    fn set_focused(&mut self, focus: bool) {
        self.focused = focus;
    }
}

impl ferrowl_ui::traits::IsFocus for Widget {
    fn is_focused(&self) -> bool {
        self.focused
    }
}

impl ferrowl_ui::traits::HandleEvents for Widget {
    fn handle_events(
        &mut self,
        _modifiers: crossterm::event::KeyModifiers,
        _code: crossterm::event::KeyCode,
    ) -> ferrowl_ui::EventResult {
        self.events += 1;
        ferrowl_ui::EventResult::Consumed
    }
}

#[focusable]
#[derive(Builder, Debug, Focus)]
struct TestApp {
    #[focus]
    pub first: Widget,
    #[focus]
    pub second: Widget,
    #[focus]
    pub third: Widget,
}

fn make_app() -> TestApp {
    TestAppBuilder::default()
        .first(Widget::default())
        .second(Widget::default())
        .third(Widget::default())
        .focus(TestAppFocus::First)
        .build()
        .expect("TestApp builder failed")
}

#[test]
fn ut_focus_next_advances() {
    let mut app = make_app();
    // starts at First, moves to Second
    app.focus_next();
    assert!(app.second.is_focused());
}

#[test]
fn ut_focus_next_wraps_around() {
    let mut app = make_app();
    app.focus_next(); // → Second
    app.focus_next(); // → Third
    app.focus_next(); // → wraps back to First
    assert!(app.first.is_focused());
}

#[test]
fn ut_focus_previous_wraps_backward() {
    let mut app = make_app();
    // at First, previous wraps to Third
    app.focus_previous();
    assert!(app.third.is_focused());
}

#[test]
fn ut_focus_previous_reverses_next() {
    let mut app = make_app();
    app.focus_next(); // → Second
    app.focus_previous(); // → First
    assert!(app.first.is_focused());
}

#[test]
fn ut_exactly_one_widget_focused_after_switch() {
    let mut app = make_app();
    app.focus_next(); // → Second
    let focused = [&app.first, &app.second, &app.third]
        .iter()
        .filter(|w| w.is_focused())
        .count();
    assert_eq!(focused, 1);
    assert!(app.second.is_focused());

    app.focus_next(); // → Third; previous (Second) must be cleared
    assert!(!app.second.is_focused());
    assert!(app.third.is_focused());
}

#[test]
fn ut_handle_events_routes_to_focused_widget() {
    use crossterm::event::{KeyCode, KeyModifiers};
    use ferrowl_ui::traits::HandleEvents;

    let mut app = make_app(); // focus = First
    app.handle_events(KeyModifiers::NONE, KeyCode::Char('a'));
    assert_eq!(app.first.events, 1);
    assert_eq!(app.second.events, 0);
    assert_eq!(app.third.events, 0);

    app.focus_next(); // → Second
    app.handle_events(KeyModifiers::NONE, KeyCode::Char('b'));
    assert_eq!(app.first.events, 1);
    assert_eq!(app.second.events, 1);
    assert_eq!(app.third.events, 0);
}

// A view whose middle widget is focusable only when `second_enabled` is set, exercising the
// `#[focus(when = ...)]` gating path of the derive macro.
#[focusable]
#[derive(Builder, Debug, Focus)]
struct GatedApp {
    #[focus]
    pub first: Widget,
    #[focus(when = self.second_enabled)]
    pub second: Widget,
    #[focus]
    pub third: Widget,
    pub second_enabled: bool,
}

fn make_gated(second_enabled: bool, start: GatedAppFocus) -> GatedApp {
    GatedAppBuilder::default()
        .first(Widget::default())
        .second(Widget::default())
        .third(Widget::default())
        .second_enabled(second_enabled)
        .focus(start)
        .build()
        .expect("GatedApp builder failed")
}

#[test]
fn ut_focus_next_skips_disabled_gated_widget() {
    let mut app = make_gated(false, GatedAppFocus::First);
    app.focus_next(); // First → (Second disabled, skipped) → Third
    assert!(app.third.is_focused());
    assert!(!app.second.is_focused());
}

#[test]
fn ut_focus_next_lands_on_enabled_gated_widget() {
    let mut app = make_gated(true, GatedAppFocus::First);
    app.focus_next(); // First → Second (enabled)
    assert!(app.second.is_focused());
    assert!(!app.third.is_focused());
}

#[test]
fn ut_focus_previous_skips_disabled_gated_widget() {
    let mut app = make_gated(false, GatedAppFocus::Third);
    app.focus_previous(); // Third → (Second disabled, skipped) → First
    assert!(app.first.is_focused());
    assert!(!app.second.is_focused());
}
