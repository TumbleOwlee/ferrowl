// Example binary: unwrap keeps the demo focused on the widget being shown.
#![allow(clippy::unwrap_used)]

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ferrowl_ui::{AlternateScreen, Border, style::TextStyle, widgets::TextBuilder};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::palette::tailwind,
};
use std::{io::Stdout, time::Duration};

// Simple app consisting of single input field
#[derive(Default)]
struct App {
    texts: Vec<String>,
}

// Render simple input field
fn ui(f: &mut Frame, app: &mut App) {
    let text = TextBuilder::default()
        .title(Some("Text".into()))
        .border(Border::Full(Margin::new(1, 0)))
        .multiline(false)
        .margin(Margin {
            vertical: 0,
            horizontal: 1,
        })
        .style(TextStyle {
            general: ratatui::prelude::Style::default().fg(tailwind::INDIGO.c400),
        })
        .build()
        .unwrap();

    let text_multiline = TextBuilder::default()
        .title(Some("Text (M)".into()))
        .border(Border::Full(Margin::new(1, 0)))
        .multiline(true)
        .margin(Margin {
            vertical: 0,
            horizontal: 1,
        })
        .style(TextStyle {
            general: ratatui::prelude::Style::default().fg(tailwind::INDIGO.c400),
        })
        .horizontal_alignment(ratatui::layout::HorizontalAlignment::Center)
        .build()
        .unwrap();

    let horizontal_layout: [Rect; 2] =
        Layout::horizontal([Constraint::Min(1), Constraint::Min(1)]).areas(f.area());

    let inputs = [text, text_multiline];
    for i in 0..2 {
        let vertical_layout: [Rect; 2] =
            Layout::vertical([Constraint::Length(10), Constraint::Min(1)])
                .areas(horizontal_layout[i]);
        f.render_stateful_widget(&inputs[i], vertical_layout[0], &mut app.texts[i]);
    }
}

fn main() {
    let mut screen: AlternateScreen<Stdout> =
        AlternateScreen::new().expect("Failed to create alternate screen.");

    // Create app state
    let mut app = App {
        texts: vec![
            "Some special text".to_string(),
            "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.".to_string()
        ],
    };

    loop {
        // Draw app
        screen.draw(|f| ui(f, &mut app)).unwrap();

        // Check for events
        if event::poll(Duration::from_millis(50)).unwrap()
            && let Event::Key(key) = event::read().unwrap()
            && key.kind == KeyEventKind::Press
            && let KeyCode::Esc = key.code
        {
            break;
        }
    }

    drop(screen);
}
