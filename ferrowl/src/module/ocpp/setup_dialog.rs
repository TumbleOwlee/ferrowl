//! OCPP module setup dialog (`:new`). Collects name, version, role, protocol and the
//! websocket endpoint (ip/port), validating live like the Modbus dialog.

use derive_builder::Builder;
use ferrowl_focus::{Focus, focusable};
use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{InputFieldState, InputFieldStateBuilder, SelectionState, SelectionStateBuilder},
    style::{InputFieldStyle, SelectionStyle, TextStyle},
    traits::ToLabel,
    widgets::{
        GetValue, InputField, InputFieldBuilder, Selection, SelectionBuilder, Text, TextBuilder,
        Validate, Widget,
    },
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, Clear, StatefulWidget, Widget as UiWidget},
};

use crate::module::ocpp::config::session::{OcppProtocol, OcppRole, OcppSpec, OcppVersion};

#[focusable]
#[derive(Builder, Focus)]
pub struct OcppSetupDialog {
    #[focus]
    pub name: Widget<InputFieldState, InputField<String>>,
    #[focus]
    pub version: Widget<SelectionState<OcppVersion>, Selection<OcppVersion>>,
    #[focus]
    pub role: Widget<SelectionState<OcppRole>, Selection<OcppRole>>,
    #[focus]
    pub protocol: Widget<SelectionState<OcppProtocol>, Selection<OcppProtocol>>,
    #[focus]
    pub ip: Widget<InputFieldState, InputField<String>>,
    #[focus]
    pub port: Widget<InputFieldState, InputField<u16>>,
    pub error: Widget<String, Text>,
    pub keybinds: Widget<String, Text>,
}

impl OcppSetupDialog {
    pub fn new() -> Self {
        let input_style = InputFieldStyle::default();
        let selection_style = SelectionStyle::default();

        OcppSetupDialogBuilder::default()
            .name(input("Name", "cs-1", &input_style, true))
            .version(selection(
                "Version",
                vec![OcppVersion::V1_6, OcppVersion::V2_0_1],
                &selection_style,
            ))
            .role(selection(
                "Role",
                vec![OcppRole::Client, OcppRole::Server],
                &selection_style,
            ))
            .protocol(selection(
                "Protocol",
                vec![OcppProtocol::Ws, OcppProtocol::Wss],
                &selection_style,
            ))
            .ip(input("IP", "127.0.0.1", &input_style, false))
            .port(input("Port", "9000", &input_style, false))
            .error(text(TextStyle {
                general: ratatui::prelude::Style::default()
                    .fg(COLOR_SCHEME.error)
                    .bg(COLOR_SCHEME.bg),
            }))
            .keybinds(keybinds_text())
            .focus(OcppSetupDialogFocus::Name)
            .build()
            .unwrap()
    }

    /// Build a dialog pre-filled with an existing spec, for `:edit`.
    pub fn edit(spec: &OcppSpec) -> Self {
        let mut d = Self::new();
        set_text(&mut d.name, &spec.name);
        d.version.state.set_selection(match spec.version {
            OcppVersion::V1_6 => 0,
            OcppVersion::V2_0_1 => 1,
        });
        d.role.state.set_selection(match spec.role {
            OcppRole::Client => 0,
            OcppRole::Server => 1,
        });
        d.protocol.state.set_selection(match spec.protocol {
            OcppProtocol::Ws => 0,
            OcppProtocol::Wss => 1,
        });
        set_text(&mut d.ip, &spec.ip);
        set_text(&mut d.port, &spec.port.to_string());
        d
    }

    /// Validate every field and produce the spec, or an error message for the live display.
    pub fn resolve(&self) -> Result<OcppSpec, String> {
        let name = self.name.state.input().trim().to_string();
        if name.is_empty() {
            return Err("Name is required.".into());
        }
        let mut ip = self.ip.state.input().trim().to_string();
        if ip.is_empty() {
            ip = "127.0.0.1".to_string();
        }
        let port_in = self.port.state.input();
        let port = if port_in.trim().is_empty() {
            9000
        } else {
            port_in
                .trim()
                .parse::<u16>()
                .map_err(|_| "Port must be a number (0-65535).".to_string())?
        };

        Ok(OcppSpec {
            name,
            version: self.version.state.get_value(),
            role: self.role.state.get_value(),
            protocol: self.protocol.state.get_value(),
            ip,
            port,
            timeout_ms: None,
        })
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        match self.resolve() {
            Ok(_) => self.error.state.clear(),
            Err(e) => self.error.state = e,
        }

        let has_error = !self.error.state.is_empty();
        // border(2) + inner margin(2) + name(3) + version|role(3) + protocol|ip|port(3)
        // + keybinds(1), plus the error box (3) only when there is a message.
        let box_height = if has_error { 17 } else { 14 };
        let box_width = 60;

        let [_, hcenter, _] = Layout::horizontal([
            Constraint::Min(1),
            Constraint::Length(box_width),
            Constraint::Min(1),
        ])
        .areas(area);
        let [_, vcenter, _] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(box_height),
            Constraint::Min(1),
        ])
        .areas(hcenter);

        let block = Block::bordered()
            .style(
                ratatui::prelude::Style::default()
                    .fg(COLOR_SCHEME.hi)
                    .bg(COLOR_SCHEME.bg),
            )
            .title_alignment(HorizontalAlignment::Center)
            .title("New OCPP Module");
        let inner = block.inner(vcenter).inner(Margin::new(2, 1));
        UiWidget::render(&Clear, vcenter, buf);
        block.render(vcenter, buf);

        let error_height = if has_error { 3 } else { 0 };
        let rows = Layout::vertical([
            Constraint::Length(3),            // name
            Constraint::Length(3),            // version | role
            Constraint::Length(3),            // protocol | ip | port
            Constraint::Length(error_height), // error (hidden when empty)
            Constraint::Length(1),            // keybinds
        ])
        .split(inner);

        StatefulWidget::render(&self.name.widget, rows[0], buf, &mut self.name.state);

        let [vl, vr] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .areas(rows[1]);
        StatefulWidget::render(&self.version.widget, vl, buf, &mut self.version.state);
        StatefulWidget::render(&self.role.widget, vr, buf, &mut self.role.state);

        let [proto, ip, port] = Layout::horizontal([
            Constraint::Length(12),
            Constraint::Min(1),
            Constraint::Length(14),
        ])
        .areas(rows[2]);
        StatefulWidget::render(&self.protocol.widget, proto, buf, &mut self.protocol.state);
        StatefulWidget::render(&self.ip.widget, ip, buf, &mut self.ip.state);
        StatefulWidget::render(&self.port.widget, port, buf, &mut self.port.state);

        if has_error {
            StatefulWidget::render(&self.error.widget, rows[3], buf, &mut self.error.state);
        }
        StatefulWidget::render(&self.keybinds.widget, rows[4], buf, &mut self.keybinds.state);
    }
}

fn input<T: Validate + Clone>(
    title: &str,
    placeholder: &str,
    style: &InputFieldStyle,
    focused: bool,
) -> Widget<InputFieldState, InputField<T>> {
    Widget {
        state: InputFieldStateBuilder::default()
            .focused(focused)
            .disabled(false)
            .placeholder(Some(placeholder.to_string()))
            .build()
            .unwrap(),
        widget: InputFieldBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some((title, HorizontalAlignment::Left).into()))
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .style(style.clone())
            .build()
            .unwrap(),
    }
}

fn set_text<T: Validate + Clone>(w: &mut Widget<InputFieldState, InputField<T>>, value: &str) {
    w.state.set_input(value.to_string());
    w.state.set_cursor(value.chars().count());
}

fn selection<T: ToLabel + Clone>(
    title: &str,
    values: Vec<T>,
    style: &SelectionStyle,
) -> Widget<SelectionState<T>, Selection<T>> {
    Widget {
        state: SelectionStateBuilder::default()
            .focused(false)
            .values(values)
            .build()
            .unwrap(),
        widget: SelectionBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some((title, HorizontalAlignment::Left).into()))
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .style(style.clone())
            .build()
            .unwrap(),
    }
}

fn text(style: TextStyle) -> Widget<String, Text> {
    Widget {
        state: String::new(),
        widget: TextBuilder::default()
            .multiline(true)
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some(("Error", HorizontalAlignment::Left).into()))
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .horizontal_alignment(HorizontalAlignment::Center)
            .style(style)
            .build()
            .unwrap(),
    }
}

fn keybinds_text() -> Widget<String, Text> {
    Widget {
        state: "<Tab>: next | <\u{2191}/\u{2193}>: select | <Enter>: confirm | <Esc>: cancel"
            .to_string(),
        widget: TextBuilder::default()
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .horizontal_alignment(HorizontalAlignment::Center)
            .style(TextStyle::default())
            .build()
            .unwrap(),
    }
}
