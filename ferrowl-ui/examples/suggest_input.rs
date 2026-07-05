use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ferrowl_ui::{
    AlternateScreen, Border, EventResult,
    state::{SuggestInputState, SuggestInputStateBuilder},
    traits::{HandleEvents, Suggestion, SuggestionProvider},
    widgets::{SuggestInput, SuggestInputBuilder},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin},
    text::Text,
    widgets::Widget,
};
use std::{io::Stdout, time::Duration};

/// Static fake provider: filters a hardcoded word list by prefix.
#[derive(Debug, Clone)]
struct WordListProvider(Vec<&'static str>);

impl SuggestionProvider for WordListProvider {
    fn suggest(&self, input: &str) -> Vec<Suggestion> {
        if input.is_empty() {
            return vec![];
        }
        self.0
            .iter()
            .filter(|w| w.starts_with(input))
            .map(|w| Suggestion {
                value: w.to_string(),
                label: w.to_string(),
                partial: false,
            })
            .collect()
    }
}

struct App {
    state: SuggestInputState<WordListProvider>,
    done: bool,
}

impl Default for App {
    fn default() -> Self {
        let provider = WordListProvider(vec![
            "apple",
            "apricot",
            "avocado",
            "banana",
            "blueberry",
            "cherry",
            "clementine",
            "coconut",
        ]);
        let state = SuggestInputStateBuilder::default()
            .provider(provider)
            .build()
            .unwrap();
        Self { state, done: false }
    }
}

fn ui(f: &mut Frame, app: &mut App) {
    let widget: SuggestInput<String, WordListProvider> = SuggestInputBuilder::default()
        .input_field(
            ferrowl_ui::widgets::InputFieldBuilder::default()
                .title(Some("Fruit".into()))
                .border(Border::Full(Margin::new(1, 0)))
                .build()
                .unwrap(),
        )
        .build()
        .unwrap();

    let area = f.area();
    let layout = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(area);

    f.render_stateful_widget(&widget, layout[0], &mut app.state);

    // Sibling content rendered *after* the field but *before* the popup
    // overlay, to demonstrate the popup drawing on top of it.
    Text::from("Content below the field, painted over by the popup when it opens.")
        .render(layout[1], f.buffer_mut());

    // Popup drawn last so it paints over any sibling content beneath it.
    widget.render_overlay(area, f.buffer_mut(), &mut app.state);
}

fn main() {
    let mut screen: AlternateScreen<Stdout> =
        AlternateScreen::new().expect("Failed to create alternate screen.");

    let mut app = App::default();

    while !app.done {
        screen.draw(|f| ui(f, &mut app)).unwrap();

        if event::poll(Duration::from_millis(50)).unwrap()
            && let Event::Key(key) = event::read().unwrap()
            && key.kind == KeyEventKind::Press
        {
            if let KeyCode::Esc = key.code
                && !app.state.suggestions_open()
            {
                app.done = true;
            } else {
                let event_result = app.state.handle_events(key.modifiers, key.code);
                if let EventResult::Unhandled(_, KeyCode::Enter) = event_result {
                    app.done = true;
                }
            }
        }
    }

    drop(screen);

    println!("Input: {:?}", app.state.input());
}
