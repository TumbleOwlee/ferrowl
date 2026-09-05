// Example binary: unwrap keeps the demo focused on the widget being shown.
#![allow(clippy::unwrap_used)]

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ferrowl_ui::{
    AlternateScreen, Border,
    state::TabBarState,
    widgets::{TabBarBuilder, TextBuilder},
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin, Rect},
};
use std::{io::Stdout, time::Duration};

struct App {
    tabs: TabBarState<String>,
    body: String,
}

fn ui(f: &mut Frame, app: &mut App) {
    let [tabs_area, body_area]: [Rect; 2] =
        Layout::horizontal([Constraint::Length(5), Constraint::Min(1)]).areas(f.area());

    // Two blank columns each side of the title (H = 2), one blank row above
    // and below each title (V = 1).
    let tabs = TabBarBuilder::<String>::default()
        .padding(Margin::new(2, 1))
        .direction(Direction::Vertical)
        .build()
        .unwrap();
    f.render_stateful_widget(&tabs, tabs_area, &mut app.tabs);

    let text = TextBuilder::default()
        .title(Some("Content".into()))
        .border(Border::Full(Margin::new(1, 0)))
        .multiline(false)
        .build()
        .unwrap();
    f.render_stateful_widget(&text, body_area, &mut app.body);
}

fn main() {
    let mut screen: AlternateScreen<Stdout> =
        AlternateScreen::new().expect("Failed to create alternate screen.");

    let mut app = App {
        tabs: TabBarState {
            titles: vec!["BOARD".to_string(), "REPOSITORY".to_string()],
            active: 0,
            offset: 0,
        },
        body: String::new(),
    };

    loop {
        app.body = format!("Tab {}", app.tabs.active);
        screen.draw(|f| ui(f, &mut app)).unwrap();

        if event::poll(Duration::from_millis(50)).unwrap()
            && let Event::Key(key) = event::read().unwrap()
            && key.kind == KeyEventKind::Press
        {
            let len = app.tabs.titles.len();
            match key.code {
                KeyCode::Esc => break,
                KeyCode::Down => {
                    app.tabs.active = (app.tabs.active + 1) % len;
                }
                KeyCode::Up => {
                    app.tabs.active = app.tabs.active.checked_sub(1).unwrap_or(len - 1);
                }
                _ => {}
            }
        }
    }

    drop(screen);
}
