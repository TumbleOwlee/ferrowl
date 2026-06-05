use derive_builder::Builder;
use modbus_derive::{Focus, focusable};
use modbus_ui::traits::IsFocus;

#[derive(Default, Clone, Debug)]
struct Widget {
    focused: bool,
}

impl modbus_ui::traits::SetFocus for Widget {
    fn set_focused(&mut self, focus: bool) {
        self.focused = focus;
    }
}

impl modbus_ui::traits::IsFocus for Widget {
    fn is_focused(&self) -> bool {
        self.focused
    }
}

impl modbus_ui::traits::HandleEvents for Widget {
    fn handle_events(
        &mut self,
        _modifiers: crossterm::event::KeyModifiers,
        _code: crossterm::event::KeyCode,
    ) -> modbus_ui::EventResult {
        modbus_ui::EventResult::Consumed
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
