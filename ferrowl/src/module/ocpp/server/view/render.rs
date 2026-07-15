//! Frame rendering: the two `ModuleView` render entry points and the widget builders used by
//! [`super::ServerView::new`].

use ferrowl_syntax::Language;
use ferrowl_ui::traits::IsFocus;
use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{
        ButtonState, ButtonStateBuilder, CodeInputFieldState, CodeInputFieldStateBuilder,
        SelectionState, SelectionStateBuilder, TableState, TableStateBuilder,
    },
    style::{
        ButtonStyle, InputFieldStyleBuilder, SelectionStyleBuilder, TableStyleBuilder, TextStyle,
    },
    widgets::{
        Button, ButtonBuilder, CodeInputField, CodeInputFieldBuilder, Selection, SelectionBuilder,
        TableBuilder, TableEntry, TextBuilder, Widget,
    },
};
use ratatui::style::Style;
use ratatui::{
    Frame,
    layout::{Constraint, HorizontalAlignment, Layout, Rect},
    widgets::StatefulWidget,
};

use super::{CsRow, CsTable, MsgRow, MsgTable, ServerOverlay, ServerVersion, ServerView, msg_row};
use crate::view::border_style;

impl<V: ServerVersion> ServerView<V>
where
    V::Action: Clone,
{
    pub(super) fn render_impl(&mut self, frame: &mut Frame, area: Rect) {
        let buf = frame.buffer_mut();
        let view_focused = self.is_focused();
        let [body, status_area] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
        let [left, right] =
            Layout::horizontal([Constraint::Length(54), Constraint::Min(1)]).areas(body);
        let [cs_area, scripts_btn_area, actions_area] = Layout::vertical([
            Constraint::Min(3),
            Constraint::Length(3),
            Constraint::Max(2 + self.actions.state.values().len() as u16),
        ])
        .areas(left);
        let [right_top, right_bottom] =
            Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(right);

        // Keep the right-hand panes in sync with the current selection.
        self.sync_actions();
        let rows: Vec<MsgRow> = self
            .selected()
            .map(|i| {
                self.entries[i]
                    .rows_for_state()
                    .iter()
                    .map(msg_row)
                    .collect()
            })
            .unwrap_or_default();
        let at_bottom = msg_log_at_bottom(&self.msg_table.state);
        self.msg_table.state.set_values(rows);
        // Tail the log to the newest message so incoming traffic shows instantly, but never while
        // the user is reading it (Messages scrolled up) or scrolling the payload pane (whose
        // content is driven by the selected message row).
        let follow = if view_focused {
            match self.focus {
                super::ServerViewFocus::Code => false,
                super::ServerViewFocus::MsgTable => at_bottom,
                _ => true,
            }
        } else {
            true
        };
        if follow {
            self.msg_table.state.move_to_bottom();
        }
        let cs_rows: Vec<CsRow> = self
            .entries
            .iter()
            .map(|e| CsRow {
                name: e.identity.clone(),
                connector: e.scope.label(),
                state: if e.online {
                    "Connected"
                } else {
                    "Disconnected"
                }
                .to_string(),
            })
            .collect();
        self.cs_table.state.set_values(cs_rows);
        self.sync_code();

        // Per-widget focus is maintained by the derived `SetFocus`/`focus_next` at focus-change
        // time (no per-frame recompute).

        StatefulWidget::render(
            &self.cs_table.widget,
            cs_area,
            buf,
            &mut self.cs_table.state,
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
        StatefulWidget::render(
            &self.msg_table.widget,
            right_top,
            buf,
            &mut self.msg_table.state,
        );
        StatefulWidget::render(&self.code.widget, right_bottom, buf, &mut self.code.state);

        // ONLINE/OFFLINE status line (with the bound address when running).
        let online = self.backend.is_online();
        let status_widget = TextBuilder::default()
            .horizontal_alignment(HorizontalAlignment::Center)
            .style(TextStyle {
                general: Style::default()
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
        let mut status = if online {
            format!("ONLINE  {}", self.backend.bound_addr().unwrap_or_default())
        } else {
            "OFFLINE".to_string()
        };
        StatefulWidget::render(&status_widget, status_area, buf, &mut status);
    }

    pub(super) fn render_overlay_impl(&mut self, frame: &mut Frame, area: Rect) {
        if matches!(self.overlay, ServerOverlay::Detail(_)) {
            self.refresh_detail();
        }
        let buf = frame.buffer_mut();
        match &mut self.overlay {
            ServerOverlay::Detail(detail) => detail.render(area, buf),
            ServerOverlay::Confirm(confirm) => confirm.render(area, buf),
            ServerOverlay::Setup(setup) => setup.render(area, buf),
            ServerOverlay::Scripts(dialog) => dialog.render(area, buf),
            ServerOverlay::Action(boxed) => boxed.2.render(area, buf),
            ServerOverlay::None => {}
        }
    }
}

/// Whether a message table's selection is on (or past) the last row — i.e. the user is tailing it.
/// An empty table or no selection counts as tailing.
fn msg_log_at_bottom<E: TableEntry<N>, const N: usize>(state: &TableState<E, N>) -> bool {
    let len = state.values().len();
    len == 0
        || state
            .table_state()
            .selected()
            .map(|s| s + 1 >= len)
            .unwrap_or(true)
}

pub(super) fn cs_table() -> CsTable {
    Widget {
        state: TableStateBuilder::default()
            .values(Vec::new())
            .build()
            .expect("all required builder fields are set"),
        widget: TableBuilder::default()
            .border(Border::Full(ratatui::layout::Margin::new(1, 0)))
            .title(Some("Charging Stations".into()))
            .style(
                TableStyleBuilder::default()
                    .build()
                    .expect("all required builder fields are set"),
            )
            .row_margin(ratatui::layout::Margin {
                vertical: 1,
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
            .border(Border::Full(ratatui::layout::Margin::new(1, 0)))
            .title(Some("Messages".into()))
            .style(
                TableStyleBuilder::default()
                    .build()
                    .expect("all required builder fields are set"),
            )
            .row_margin(ratatui::layout::Margin {
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
            .border(Border::Full(ratatui::layout::Margin::new(1, 0)))
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
            .margin(ratatui::layout::Margin {
                vertical: 0,
                horizontal: 0,
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
            .border_margin(ratatui::layout::Margin::new(1, 0))
            .margin(ratatui::layout::Margin {
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
            .border(Border::Full(ratatui::layout::Margin::new(1, 0)))
            .title(Some("Payload".into()))
            .style(
                InputFieldStyleBuilder::default()
                    .border(border_style())
                    .build()
                    .expect("all required builder fields are set"),
            )
            .margin(ratatui::layout::Margin {
                vertical: 0,
                horizontal: 0,
            })
            .build()
            .expect("all required builder fields are set"),
    }
}
