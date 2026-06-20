//! Minimal OCPP creation dialog: a single name field. OCPP-specific settings (CS `url`,
//! CSMS `host`/`port`, role) are a deliberate follow-up.

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{InputFieldState, InputFieldStateBuilder},
    style::{InputFieldStyle, TextStyle},
    traits::HandleEvents,
    widgets::{GetValue, InputField, InputFieldBuilder, Text, TextBuilder, Widget},
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, Clear, StatefulWidget, Widget as UiWidget},
};

use crate::module::ocpp::view::OcppModuleView;
use crate::module::type_descriptor::{ModuleViewFactory, SetupView};

/// Setup dialog for the OCPP module type.
pub struct OcppSetupView {
    name: Widget<InputFieldState, InputField<String>>,
    keybinds: Widget<String, Text>,
}

impl OcppSetupView {
    pub fn new() -> Self {
        let name = Widget {
            state: InputFieldStateBuilder::default()
                .focused(true)
                .disabled(false)
                .placeholder(Some("ocpp-1".to_string()))
                .build()
                .unwrap(),
            widget: InputFieldBuilder::default()
                .border(Border::Full(Margin::new(1, 0)))
                .title(Some(("Name", HorizontalAlignment::Left).into()))
                .margin(Margin {
                    vertical: 0,
                    horizontal: 1,
                })
                .style(InputFieldStyle::default())
                .build()
                .unwrap(),
        };

        let keybinds = Widget {
            state: "<Enter>: confirm | <Esc>: cancel".to_string(),
            widget: TextBuilder::default()
                .margin(Margin {
                    vertical: 0,
                    horizontal: 1,
                })
                .horizontal_alignment(HorizontalAlignment::Center)
                .style(TextStyle::default())
                .build()
                .unwrap(),
        };

        Self { name, keybinds }
    }
}

impl SetupView for OcppSetupView {
    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        // 2 border + 3 name field + 1 keybind line.
        let box_height = 6;
        let box_width = 40;

        let horizontal: [Rect; 3] = Layout::horizontal([
            Constraint::Min(1),
            Constraint::Length(box_width),
            Constraint::Min(1),
        ])
        .areas(area);
        let vertical: [Rect; 3] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(box_height),
            Constraint::Min(1),
        ])
        .areas(horizontal[1]);

        let block = Block::bordered()
            .style(
                ratatui::prelude::Style::default()
                    .fg(COLOR_SCHEME.hi)
                    .bg(COLOR_SCHEME.bg),
            )
            .title_alignment(HorizontalAlignment::Center)
            .title("New OCPP Module");

        let inner = block.inner(vertical[1]);
        UiWidget::render(&Clear, vertical[1], buf);
        block.render(vertical[1], buf);

        let rows: [Rect; 2] =
            Layout::vertical([Constraint::Length(3), Constraint::Length(1)]).areas(inner);

        StatefulWidget::render(&self.name.widget, rows[0], buf, &mut self.name.state);
        StatefulWidget::render(&self.keybinds.widget, rows[1], buf, &mut self.keybinds.state);
    }

    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) {
        let _ = self.name.state.handle_events(modifiers, code);
    }

    fn focus_next(&mut self) {}

    fn focus_previous(&mut self) {}

    fn confirm(&self) -> Option<(String, ModuleViewFactory)> {
        let name = self.name.state.get_value();
        if name.trim().is_empty() {
            return None;
        }
        let view_name = name.clone();
        let factory: ModuleViewFactory = Box::new(move || Box::new(OcppModuleView::new(name)));
        Some((view_name, factory))
    }
}
