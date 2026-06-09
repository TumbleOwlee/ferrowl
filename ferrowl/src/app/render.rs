//! Whole-frame rendering: tab bar, register table, log pane, command line and overlay.

use ferrowl_ui::COLOR_SCHEME;
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, StatefulWidget},
};

use crate::view::command::CommandLine;
use crate::view::tabs::render_tabs;

use super::{Focus, Overlay, Tab};

pub(super) fn render(
    frame: &mut Frame,
    tabs: &mut [Tab],
    active: usize,
    focus: Focus,
    command: &mut CommandLine,
    overlay: Option<&mut Overlay>,
) {
    let area = frame.area();
    let [tabs_area, table_area, log_area, cmd_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(10),
        Constraint::Length(1),
    ])
    .areas(area);

    let buf = frame.buffer_mut();
    buf.set_style(area, Style::default().bg(COLOR_SCHEME.bg));

    let names: Vec<String> = tabs.iter().map(|t| t.name.clone()).collect();
    render_tabs(&names, active, tabs_area, buf);

    if let Some(tab) = tabs.get_mut(active) {
        tab.table.table.state.set_focused(focus == Focus::Table);
        tab.table.render(table_area, buf);
        StatefulWidget::render(&tab.log_view.widget, log_area, buf, &mut tab.log_view.state);
    }

    render_command(command, focus, cmd_area, buf);
    if focus == Focus::Command {
        render_command_help(cmd_area, buf);
    }

    // Overlay dialog (drawn last; it clears its own area).
    if let Some(dialog) = overlay {
        dialog.render(area, buf);
    }
}

fn render_command_help(cmd_area: Rect, buf: &mut Buffer) {
    const COLS: &[(&str, &str)] = &[
        (":q | :quit", "quit tab"),
        (":qa | :qall", "quit all tabs"),
        (":e | :edit", "edit module setup"),
        (":n | :new", "new module tab"),
        (":l | :load [path]", "load device config"),
        (":a | :add", "add register to device"),
        (":start", "start module"),
        (":stop", "stop module"),
        (":restart", "restart module"),
        (":set <reg> <val>", "write register value"),
        (":s | :save | :w | :write [path]", "save session"),
        (":wd | :write-device [path]", "save device config"),
        (":log [file]", "set log file"),
        (":lua start|stop", "start|stop lua execution"),
        (":reload", "reload device config"),
        (":compact", "toggle compact mode"),
        (":order [col] [asc|desc]", "sort table by column"),
    ];
    let popup_w: u16 = 62;
    let popup_h: u16 = COLS.len() as u16 + 2;
    let x = cmd_area.x;
    let y = cmd_area.y.saturating_sub(popup_h);
    let popup = Rect {
        x,
        y,
        width: popup_w.min(cmd_area.width),
        height: popup_h,
    };

    ratatui::prelude::Widget::render(Clear, popup, buf);
    let block = Block::bordered().style(Style::default().fg(COLOR_SCHEME.hi).bg(COLOR_SCHEME.bg));
    let inner = block.inner(popup);
    ratatui::prelude::Widget::render(block, popup, buf);

    let lines: Vec<Line> = COLS
        .iter()
        .map(|(cmd, desc)| {
            Line::from(vec![
                Span::styled(
                    format!("{cmd:<34}"),
                    Style::default().fg(COLOR_SCHEME.hi).bg(COLOR_SCHEME.bg),
                ),
                Span::styled(
                    desc.to_string(),
                    Style::default().fg(COLOR_SCHEME.text).bg(COLOR_SCHEME.bg),
                ),
            ])
        })
        .collect();
    ratatui::prelude::Widget::render(
        Paragraph::new(lines).style(Style::default().bg(COLOR_SCHEME.bg)),
        inner,
        buf,
    );
}

fn render_command(command: &mut CommandLine, focus: Focus, area: Rect, buf: &mut Buffer) {
    if focus == Focus::Command {
        buf.set_string(
            area.x,
            area.y,
            ":",
            Style::default().fg(COLOR_SCHEME.hi).bg(COLOR_SCHEME.bg),
        );
        let input_area = Rect {
            x: area.x.saturating_add(1),
            y: area.y,
            width: area.width.saturating_sub(1),
            height: area.height,
        };
        StatefulWidget::render(&command.widget, input_area, buf, &mut command.state);
    } else {
        buf.set_style(area, Style::default().bg(COLOR_SCHEME.bg));
        buf.set_string(
            area.x,
            area.y,
            "  :  command    |    Tab  table/log    |    ] [  tabs    |    gt gT  tabs",
            Style::default().fg(COLOR_SCHEME.text).bg(COLOR_SCHEME.bg),
        );
    }
}
