//! Whole-frame rendering: tab bar, module view, log pane, command line and overlay.

use ferrowl_ui::COLOR_SCHEME;
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, StatefulWidget},
};

use crate::module::view::CommandDescriptor;
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
    let [tabs_area, view_area, log_area, cmd_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(10),
        Constraint::Length(1),
    ])
    .areas(area);

    // Phase 1: background and tab bar.
    {
        let buf = frame.buffer_mut();
        buf.set_style(area, Style::default().bg(COLOR_SCHEME.bg));

        let names: Vec<String> = tabs.iter().map(|t| format!(" {} ", t.name)).collect();
        render_tabs(&names, active, tabs_area, buf);
    }

    // Phase 2: module content view (includes its own status bar). Focus is carried by the view's
    // own stored state (set at focus-change time), not recomputed here.
    if let Some(tab) = tabs.get_mut(active) {
        tab.view.render(frame, view_area);
    }

    // Phase 3: log pane and command line.
    {
        let buf = frame.buffer_mut();
        if let Some(tab) = tabs.get_mut(active) {
            StatefulWidget::render(&tab.log_view.widget, log_area, buf, &mut tab.log_view.state);
        }
        render_command(command, focus, cmd_area, buf);
    }

    // Phase 4: overlays, painted on top of content and log (bottom-to-top z-order).
    // 1. Module dialogs. Drawn first so command help and the app dialog sit above them. The view's
    //    own match no-ops when no overlay is open, so this is called unconditionally.
    if let Some(tab) = tabs.get_mut(active) {
        tab.view.render_overlay(frame, view_area);
    }
    // 2. Command help popup and 3. app-level modal dialog. Both draw to the buffer; the module
    //    overlay above needed `&mut Frame`, so these go in a separate, sequential borrow.
    {
        let buf = frame.buffer_mut();
        if focus == Focus::Command {
            let module_cmds = tabs.get(active).map(|t| t.view.commands()).unwrap_or(&[]);
            render_command_help(cmd_area, buf, module_cmds);
        }
        if let Some(dialog) = overlay {
            dialog.render(area, buf);
        }
    }
}

fn render_command_help(cmd_area: Rect, buf: &mut Buffer, module_cmds: &[CommandDescriptor]) {
    const COLS: &[(&str, &str)] = &[
        (":q | :quit", "quit tab"),
        (":qa | :qall", "quit all tabs"),
        (":n | :new", "new module tab"),
        (":l | :load [path]", "load device config"),
        (":s | :save | :w | :write [path]", "save session"),
        (":log clear", "clear log view"),
    ];
    let popup_w: u16 = 62;
    let popup_h: u16 = (COLS.len() + module_cmds.len()) as u16 + 2;
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

    let make_line = |(cmd, desc): (&str, &str)| {
        Line::from(vec![
            Span::styled(
                format!("{cmd:<34}"),
                Style::default()
                    .fg(COLOR_SCHEME.hi)
                    .bg(COLOR_SCHEME.bg)
                    .bold(),
            ),
            Span::styled(
                desc.to_string(),
                Style::default().fg(COLOR_SCHEME.text).bg(COLOR_SCHEME.bg),
            ),
        ])
    };
    let lines: Vec<Line> = COLS
        .iter()
        .map(|(cmd, desc)| make_line((cmd, desc)))
        .chain(
            module_cmds
                .iter()
                .map(|c| make_line((c.name, c.description))),
        )
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
            "  :  command    |    <C-w>j <C-w>k  table/log    |    ] [  tabs    |    <C-t>h <C-t>l  tabs",
            Style::default().fg(COLOR_SCHEME.text).bg(COLOR_SCHEME.bg),
        );
    }
}
