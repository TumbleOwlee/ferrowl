//! Frame rendering: the left-column layout shared with the Edit overlay, the two `ModuleView`
//! render entry points, and the widget builders used by [`super::ClientView::new`].

use ferrowl_syntax::Language;
use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{
        ButtonState, ButtonStateBuilder, CodeInputFieldState, CodeInputFieldStateBuilder,
        InputFieldState, InputFieldStateBuilder, SelectionState, SelectionStateBuilder, TableState,
        TableStateBuilder,
    },
    style::{
        ButtonStyle, InputFieldStyle, InputFieldStyleBuilder, SelectionStyle,
        SelectionStyleBuilder, TableStyleBuilder, TextStyle,
    },
    widgets::{
        Button, ButtonBuilder, CodeInputField, CodeInputFieldBuilder, InputField,
        InputFieldBuilder, Selection, SelectionBuilder, TableBuilder, TableEntry, TextBuilder,
        Widget,
    },
};
use ratatui::style::Style;
use ratatui::{
    Frame,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, Clear, StatefulWidget, Widget as UiWidget},
};

use super::{
    ClientOverlay, ClientVersion, ClientView, ConfigRow, ConfigTable, ConnRow, ConnTable, EditKind,
    MsgTable, NvRow, StateTable,
};
use crate::view::border_style;

impl<V: ClientVersion> ClientView<V> {
    /// The left-column vertical split of the content area:
    /// `[conn_input, conn, state, scripts_btn, actions, config]`. Shared by `render` and
    /// `render_overlay` so the Edit overlay anchors to the same `state` rect drawn this frame.
    pub(super) fn body_layout(&self, area: Rect) -> [Rect; 6] {
        let [body, _status] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
        let [left, _right] =
            Layout::horizontal([Constraint::Length(66), Constraint::Min(1)]).areas(body);
        let n_conn = self.conn_table.state.values().len() as u16;
        let n_actions = self.actions.state.values().len() as u16;
        // Config block only when the CS row is selected (13 table + 3 input).
        let config_len = if self.cs_selected() { 16 } else { 0 };
        Layout::vertical([
            Constraint::Length(3),                         // Add-connector input (top)
            Constraint::Length((n_conn + 3).clamp(6, 12)), // Connectors (compact, ≥3 entries)
            Constraint::Min(16),                           // State (≥5 entries + header)
            Constraint::Length(3),                         // Scripts button
            Constraint::Max(2 + n_actions),                // Actions
            Constraint::Length(config_len),                // Config block (CS only)
        ])
        .areas(left)
    }

    pub(super) fn render_impl(&mut self, frame: &mut Frame, area: Rect) {
        let buf = frame.buffer_mut();
        let cs = self.cs_selected();
        let [body, status_area] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
        let [_left, right] =
            Layout::horizontal([Constraint::Length(66), Constraint::Min(1)]).areas(body);

        let [
            conn_input_area,
            conn_area,
            state_area,
            scripts_btn_area,
            actions_area,
            config_area,
        ] = self.body_layout(area);
        let [config_table_area, config_input_area] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).areas(config_area);
        let [key_area, value_area] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .areas(config_input_area);
        let [right_top, right_bottom] =
            Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(right);

        // Per-widget focus is maintained by the derived `SetFocus`/`focus_next` at focus-change
        // time (no per-frame recompute).

        StatefulWidget::render(
            &self.conn_table.widget,
            conn_area,
            buf,
            &mut self.conn_table.state,
        );
        StatefulWidget::render(
            &self.conn_input.widget,
            conn_input_area,
            buf,
            &mut self.conn_input.state,
        );
        StatefulWidget::render(
            &self.state_table.widget,
            state_area,
            buf,
            &mut self.state_table.state,
        );
        StatefulWidget::render(
            &self.scripts_button.widget,
            scripts_btn_area,
            buf,
            &mut self.scripts_button.state,
        );
        StatefulWidget::render(
            &self.actions.widget,
            actions_area,
            buf,
            &mut self.actions.state,
        );
        if cs {
            StatefulWidget::render(
                &self.config_table.widget,
                config_table_area,
                buf,
                &mut self.config_table.state,
            );
            StatefulWidget::render(
                &self.key_input.widget,
                key_area,
                buf,
                &mut self.key_input.state,
            );
            StatefulWidget::render(
                &self.value_input.widget,
                value_area,
                buf,
                &mut self.value_input.state,
            );
        }
        StatefulWidget::render(
            &self.msg_table.widget,
            right_top,
            buf,
            &mut self.msg_table.state,
        );
        StatefulWidget::render(&self.code.widget, right_bottom, buf, &mut self.code.state);

        // ONLINE/OFFLINE status line.
        let online = self.backend.is_online();
        let status_widget = TextBuilder::default()
            .horizontal_alignment(HorizontalAlignment::Center)
            .style(TextStyle {
                general: ratatui::prelude::Style::default()
                    .bg(if online {
                        COLOR_SCHEME.success
                    } else {
                        COLOR_SCHEME.error
                    })
                    .fg(COLOR_SCHEME.text_status)
                    .bold(),
            })
            .build()
            .expect("all required builder fields are set");
        let mut status = if online { "ONLINE" } else { "OFFLINE" }.to_string();
        StatefulWidget::render(&status_widget, status_area, buf, &mut status);
    }

    pub(super) fn render_overlay_impl(&mut self, frame: &mut Frame, area: Rect) {
        // Recompute the left-column split so the Edit overlay can anchor to the state table area,
        // matching what `render` drew this frame (shared via `body_layout`, no cached rects).
        let [_, _, state_area, _, _, _] = self.body_layout(area);
        let buf = frame.buffer_mut();
        match &mut self.overlay {
            // State-row edit overlay over the state table.
            ClientOverlay::Edit(edit) => {
                let title = edit.field.label();
                let height = match &edit.kind {
                    EditKind::Choice(sel) => sel.state.values().len() as u16 + 2,
                    EditKind::Number(_) | EditKind::Text(_) => 3,
                };
                let width = state_area.width.min(30);
                let [_, hc, _] = Layout::horizontal([
                    Constraint::Min(0),
                    Constraint::Length(width),
                    Constraint::Min(0),
                ])
                .areas(state_area);
                let [_, vc, _] = Layout::vertical([
                    Constraint::Min(0),
                    Constraint::Length(height),
                    Constraint::Min(0),
                ])
                .areas(hc);
                UiWidget::render(&Clear, vc, buf);
                let block = boxed(title);
                let inner = block.inner(vc);
                block.render(vc, buf);
                match &mut edit.kind {
                    EditKind::Choice(sel) => {
                        StatefulWidget::render(&sel.widget, inner, buf, &mut sel.state)
                    }
                    EditKind::Number(input) => {
                        StatefulWidget::render(&input.widget, inner, buf, &mut input.state)
                    }
                    EditKind::Text(input) => {
                        StatefulWidget::render(&input.widget, inner, buf, &mut input.state)
                    }
                }
            }
            ClientOverlay::Config(dialog) => dialog.render(area, buf),
            ClientOverlay::Action(dlg) => dlg.render(area, buf),
            ClientOverlay::Setup(setup) => setup.render(area, buf),
            ClientOverlay::Scripts(dialog) => dialog.render(area, buf),
            ClientOverlay::None => {}
        }
    }
}

// --- Widget builders -------------------------------------------------------

/// Whether a message table's selection is on (or past) the last row — i.e. the user is tailing it.
/// An empty table or no selection counts as tailing.
pub(super) fn msg_log_at_bottom<E: TableEntry<N>, const N: usize>(
    state: &TableState<E, N>,
) -> bool {
    let len = state.values().len();
    len == 0
        || state
            .table_state()
            .selected()
            .map(|s| s + 1 >= len)
            .unwrap_or(true)
}

fn boxed(title: &str) -> Block<'_> {
    Block::bordered()
        .style(
            ratatui::prelude::Style::default()
                .fg(COLOR_SCHEME.hi)
                .bg(COLOR_SCHEME.bg),
        )
        .title_alignment(HorizontalAlignment::Center)
        .title(title.to_string())
}

pub(super) fn nv_table(rows: Vec<NvRow>) -> StateTable {
    Widget {
        state: TableStateBuilder::default()
            .values(rows)
            .build()
            .expect("all required builder fields are set"),
        widget: TableBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some("State".into()))
            .style(
                TableStyleBuilder::default()
                    .build()
                    .expect("all required builder fields are set"),
            )
            .row_margin(Margin {
                vertical: 1,
                horizontal: 0,
            })
            .build()
            .expect("all required builder fields are set"),
    }
}

pub(super) fn conn_table(rows: Vec<ConnRow>) -> ConnTable {
    Widget {
        state: TableStateBuilder::default()
            .values(rows)
            .build()
            .expect("all required builder fields are set"),
        widget: TableBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some("Connectors".into()))
            .style(
                TableStyleBuilder::default()
                    .build()
                    .expect("all required builder fields are set"),
            )
            // Always compact (no vertical margin), independent of `:compact`, to save space.
            .row_margin(Margin {
                vertical: 0,
                horizontal: 0,
            })
            .build()
            .expect("all required builder fields are set"),
    }
}

pub(super) fn config_table<V: ClientVersion>(rows: Vec<ConfigRow>) -> ConfigTable {
    Widget {
        state: TableStateBuilder::default()
            .values(rows)
            .build()
            .expect("all required builder fields are set"),
        widget: TableBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some(V::config_title().into()))
            .style(
                TableStyleBuilder::default()
                    .build()
                    .expect("all required builder fields are set"),
            )
            .row_margin(Margin {
                vertical: 1,
                horizontal: 0,
            })
            .build()
            .expect("all required builder fields are set"),
    }
}

pub(super) fn panel_input(title: &str) -> Widget<InputFieldState, InputField<String>> {
    Widget {
        state: InputFieldStateBuilder::default()
            .focused(false)
            .disabled(false)
            .placeholder(Some(title.to_string()))
            .build()
            .expect("all required builder fields are set"),
        widget: InputFieldBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some((title, HorizontalAlignment::Left).into()))
            .style(
                InputFieldStyleBuilder::default()
                    .border(border_style())
                    .build()
                    .expect("all required builder fields are set"),
            )
            .margin(Margin {
                vertical: 0,
                horizontal: 0,
            })
            .build()
            .expect("all required builder fields are set"),
    }
}

pub(super) fn msg_table() -> MsgTable {
    Widget {
        state: TableStateBuilder::default()
            .values(Vec::new())
            .build()
            .expect("all required builder fields are set"),
        widget: TableBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some("Messages".into()))
            .style(
                TableStyleBuilder::default()
                    .build()
                    .expect("all required builder fields are set"),
            )
            .row_margin(Margin {
                vertical: 1,
                horizontal: 0,
            })
            .build()
            .expect("all required builder fields are set"),
    }
}

pub(super) fn action_list(
    values: Vec<String>,
) -> Widget<SelectionState<String>, Selection<String>> {
    Widget {
        state: SelectionStateBuilder::default()
            .focused(false)
            .values(values)
            .build()
            .expect("all required builder fields are set"),
        widget: SelectionBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some(("Actions", HorizontalAlignment::Left).into()))
            .style(
                SelectionStyleBuilder::default()
                    .general(border_style())
                    .focused(
                        Style::default()
                            .fg(COLOR_SCHEME.bg)
                            .bg(COLOR_SCHEME.hi)
                            .bold(),
                    )
                    .build()
                    .expect("all required builder fields are set"),
            )
            .margin(Margin {
                vertical: 0,
                horizontal: 0,
            })
            .build()
            .expect("all required builder fields are set"),
    }
}

/// A choice-list overlay widget, preselecting `current` if present (for the Status/Phases editors).
pub fn choice(
    options: &[&str],
    current: &str,
) -> Widget<SelectionState<String>, Selection<String>> {
    let values: Vec<String> = options.iter().map(|s| s.to_string()).collect();
    let mut state = SelectionStateBuilder::default()
        .focused(true)
        .values(values)
        .build()
        .expect("all required builder fields are set");
    if let Some(idx) = options.iter().position(|o| *o == current) {
        state.set_selection(idx);
    }
    Widget {
        state,
        widget: SelectionBuilder::default()
            .style(SelectionStyle::default())
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .build()
            .expect("all required builder fields are set"),
    }
}

/// A numeric input overlay widget seeded with `current` (for metering editors).
pub fn number(current: f64) -> Widget<InputFieldState, InputField<f64>> {
    let mut state = InputFieldStateBuilder::default()
        .focused(true)
        .disabled(false)
        .allowed_for::<f64>()
        .build()
        .expect("all required builder fields are set");
    let text = format!("{current}");
    state.set_input(text.clone());
    state.set_cursor(text.chars().count());
    Widget {
        state,
        widget: InputFieldBuilder::default()
            .style(InputFieldStyle::default())
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .build()
            .expect("all required builder fields are set"),
    }
}

/// A text input overlay widget seeded with `current` (for the RFID / identity editors).
pub fn text_input(current: &str) -> Widget<InputFieldState, InputField<String>> {
    let mut state = InputFieldStateBuilder::default()
        .focused(true)
        .disabled(false)
        .build()
        .expect("all required builder fields are set");
    state.set_input(current.to_string());
    state.set_cursor(current.chars().count());
    Widget {
        state,
        widget: InputFieldBuilder::default()
            .style(InputFieldStyle::default())
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .build()
            .expect("all required builder fields are set"),
    }
}

pub(super) fn scripts_button() -> Widget<ButtonState, Button> {
    Widget {
        state: ButtonStateBuilder::default()
            .focused(false)
            .label("Lua Scripts".to_string())
            .disabled(false)
            .build()
            .expect("all required builder fields are set"),
        widget: ButtonBuilder::default()
            .border_margin(Margin::new(1, 0))
            .margin(Margin {
                vertical: 0,
                horizontal: 0,
            })
            .style(ButtonStyle {
                general: border_style(),
                ..ButtonStyle::default()
            })
            .horizontal_alignment(HorizontalAlignment::Center)
            .build()
            .expect("all required builder fields are set"),
    }
}

pub(super) fn code_view() -> Widget<CodeInputFieldState, CodeInputField> {
    Widget {
        state: CodeInputFieldStateBuilder::default()
            .focused(false)
            .disabled(true)
            .placeholder(Some("select a message".to_string()))
            .language(Some(Language::Json))
            .build()
            .expect("all required builder fields are set"),
        widget: CodeInputFieldBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some("Payload".into()))
            .style(
                InputFieldStyleBuilder::default()
                    .border(border_style())
                    .build()
                    .expect("all required builder fields are set"),
            )
            .margin(Margin {
                vertical: 0,
                horizontal: 0,
            })
            .build()
            .expect("all required builder fields are set"),
    }
}
