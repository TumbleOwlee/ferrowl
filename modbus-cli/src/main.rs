#![feature(async_fn_traits)]

mod dialog;
mod instance;
mod module;
mod ui;
mod view;

use std::{io::Stdout, sync::Arc, time::Duration};

use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use modbus_mem::Memory;
use modbus_reg::{
    Access, Address, Alignment, Endian, Format, Kind, RegisterBuilder,
    format::{Resolution, Width},
};
use modbus_ui::{AlternateScreen, EventResult, traits::HandleEvents};
use modbus_util::{Expect, tokio::spawn_detach};
use ratatui::layout::{Constraint, Layout, Rect};
use tokio::runtime::Runtime;

use crate::{
    dialog::{EditInputDialog, EditSelectionDialog},
    module::Definition,
    view::main::{DefinitionBuilder, TableView},
};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct CliArgs {}

async fn run() {
    let config = modbus_net::tcp::Config {
        ip: "127.0.0.1".to_string(),
        port: 8080,
        timeout_ms: 3000,
        delay_ms: 1000,
        interval_ms: 1000,
    };

    let mut module1 = module::Module::new(
        modbus_net::Key::create(1),
        module::Config {
            client: false,
            config: modbus_net::Config::Tcp(config.clone()),
            definitions: vec![Definition {
                address: 0,
                length: 10,
            }],
        },
    );

    let mut module2 = module::Module::new(
        modbus_net::Key::create(1),
        module::Config {
            client: true,
            config: modbus_net::Config::Tcp(config.clone()),
            definitions: vec![Definition {
                address: 0,
                length: 10,
            }],
        },
    );

    module1.start().await.panic(|e| format!("{}", e));
    module2.start().await.panic(|e| format!("{}", e));

    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    module1.stop().await.panic(|e| format!("{}", e));
    module2.stop().await.panic(|e| format!("{}", e));
}

fn main() {
    // Initialize tokio runtime for modbus server
    let runtime = Runtime::new().panic(|e| format!("Failed to create runtime. [{}]", e));

    // Spawn modbus modules in the background
    runtime.block_on(async move {
        spawn_detach(async move {
            run().await;
        })
        .await
    });

    let mut screen: AlternateScreen<Stdout> =
        AlternateScreen::new().expect("Failed to create alternate screen.");

    // Release terminal on panic to show error messages
    let handler = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        AlternateScreen::<Stdout>::release();
        handler(panic);
    }));

    let memory = Arc::new(Memory::default());

    let mut values = vec![];
    for i in 0..10 {
        values.push(
            DefinitionBuilder::default()
                .name(format!("Name {}", i))
                .comment(format!("Some comment {}", i))
                .register(
                    RegisterBuilder::default()
                        .slave_id(1)
                        .access(Access::ReadWrite)
                        .kind(Kind::InputRegister)
                        .address(Address::Fixed(100))
                        .format(Format::U16((Endian::Big, Resolution(1.0))))
                        .build()
                        .unwrap(),
                )
                .memory(memory.clone())
                .build()
                .unwrap(),
        );
    }

    values.push(
        DefinitionBuilder::default()
            .name(format!("Name {}", 11))
            .comment(format!("Some comment {}", 11))
            .register(
                RegisterBuilder::default()
                    .slave_id(1)
                    .access(Access::ReadWrite)
                    .kind(Kind::InputRegister)
                    .address(Address::Fixed(100))
                    .format(Format::Ascii((Alignment::Left, Width(40))))
                    .build()
                    .unwrap(),
            )
            .memory(memory.clone())
            .build()
            .unwrap(),
    );

    let mut edit_input_dialog = EditInputDialog::new();
    let mut edit_selection_dialog = EditSelectionDialog::new(vec!["Dog", "Cat", "Horse"]);
    let mut table_view = TableView::new(values);

    let mut i = 0;

    loop {
        // Draw app
        if let Err(e) = screen.draw(|f| {
            let layout: [Rect; 2] =
                Layout::horizontal([Constraint::Min(1), Constraint::Min(1)]).areas(f.area());
            if i == 0 {
                edit_input_dialog.render(layout[0], f.buffer_mut());
            } else if i == 1 {
                edit_selection_dialog.render(layout[1], f.buffer_mut());
            } else {
                table_view.render(f.area(), f.buffer_mut());
            }
        }) {
            drop(screen);
            eprintln!("Failed to draw screen. [{}]", e);
            return;
        }

        // Check for events
        match event::poll(Duration::from_millis(50)) {
            Ok(true) => {
                if let Ok(Event::Key(key)) = event::read() {
                    if key.kind == KeyEventKind::Press {
                        if let KeyCode::Esc = key.code {
                            break;
                        } else {
                            let event_result: EventResult = if i == 0 {
                                edit_input_dialog.handle_events(key.modifiers, key.code)
                            } else if i == 1 {
                                edit_selection_dialog.handle_events(key.modifiers, key.code)
                            } else {
                                table_view.handle_events(key.modifiers, key.code)
                            };
                            match event_result {
                                EventResult::Unhandled(KeyModifiers::ALT, KeyCode::Char('k')) => {
                                    i = (i + 1) % 3;
                                }
                                EventResult::Unhandled(_, KeyCode::Enter) => {
                                    break;
                                }
                                EventResult::Unhandled(KeyModifiers::SHIFT, KeyCode::BackTab)
                                | EventResult::Unhandled(KeyModifiers::SHIFT, KeyCode::Tab) => {
                                    if i == 0 {
                                        edit_input_dialog.focus_previous();
                                    } else if i == 1 {
                                        edit_selection_dialog.focus_previous();
                                    } else {
                                        table_view.focus_previous();
                                    }
                                }
                                EventResult::Unhandled(_, KeyCode::Tab) => {
                                    if i == 0 {
                                        edit_input_dialog.focus_next();
                                    } else if i == 1 {
                                        edit_selection_dialog.focus_next();
                                    } else {
                                        table_view.focus_next();
                                    }
                                }
                                _ => {}
                            }
                        }
                    } else {
                        break;
                    }
                }
            }
            Ok(false) => {}
            Err(_) => break,
        }
    }
}
