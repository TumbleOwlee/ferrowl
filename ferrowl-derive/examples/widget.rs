extern crate proc_macro;
use derive_builder::Builder;
use ferrowl_derive::{Focus, focusable};

#[derive(Default, Clone, Debug)]
struct State;

impl ferrowl_ui::traits::SetFocus for State {
    fn set_focused(&mut self, _focus: bool) {}
}

impl ferrowl_ui::traits::IsFocus for State {
    fn is_focused(&self) -> bool {
        true
    }
}

impl ferrowl_ui::traits::HandleEvents for State {
    fn handle_events(
        &mut self,
        _modifiers: crossterm::event::KeyModifiers,
        _code: crossterm::event::KeyCode,
    ) -> ferrowl_ui::EventResult {
        ferrowl_ui::EventResult::Consumed
    }
}

#[focusable]
#[derive(Builder, Debug, Focus)]
struct App {
    #[focus]
    pub name: State,
    #[focus]
    pub lastname: State,
}

fn main() {
    let mut app = AppBuilder::default()
        .name(State)
        .lastname(State)
        .focus(AppFocus::Name)
        .build()
        .expect("App builder failed.");
    app.focus_previous();
    println!("{:?}", app);
}
