//! OCPP module setup dialog (`:new`). Collects name, version, role, protocol and the
//! websocket endpoint (ip/port), validating live like the Modbus dialog.

use derive_builder::Builder;
use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{
        InputFieldState, InputFieldStateBuilder, SelectionState, SelectionStateBuilder,
        SuggestInputState, SuggestInputStateBuilder,
    },
    style::{InputFieldStyle, SelectionStyle, SuggestInputStyle, TextStyle},
    traits::ToLabel,
    widgets::{
        GetValue, InputField, InputFieldBuilder, Selection, SelectionBuilder, SuggestInput,
        SuggestInputBuilder, Text, TextBuilder, Validate, ValidateResult, Widget,
    },
};
use ferrowl_ui_derive::{Focus, focusable};
use ferrowl_util::convert::FileType;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, Clear, StatefulWidget, Widget as UiWidget},
};

use crate::dialog::path_suggest::FsPathProvider;
use crate::module::ocpp::config::session::{OcppProtocol, OcppRole, OcppSpec, OcppVersion};

/// Live validator for the device-config path field: empty is allowed (a fresh empty config),
/// otherwise the path must be a TOML/JSON file, and — if it exists — a loadable OCPP device
/// config. Mirrors the Modbus dialog's `ConfigPath`.
#[derive(Debug, Clone)]
pub struct ConfigPath;

impl Validate for ConfigPath {
    fn validate(input: &str) -> ValidateResult {
        let input = input.trim();
        let path = std::path::Path::new(input);

        if input.is_empty() {
            ValidateResult::None
        } else if FileType::from_path(input).is_some() {
            if path.exists() {
                match crate::config::load_ocpp_device(input) {
                    Ok(_) => ValidateResult::Success,
                    Err(e) => ValidateResult::Error(format!("Config: {e}")),
                }
            } else {
                ValidateResult::None
            }
        } else {
            ValidateResult::Error("Invalid filetype, TOML or JSON expected.".to_string())
        }
    }
}

#[focusable]
#[derive(Builder, Focus)]
pub struct OcppSetupDialog {
    #[focus]
    pub name: Widget<InputFieldState, InputField<String>>,
    /// Path to the OCPP device-config file (empty = a fresh, empty device config).
    #[focus]
    pub config_path:
        Widget<SuggestInputState<FsPathProvider>, SuggestInput<ConfigPath, FsPathProvider>>,
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
    /// Optional URL path appended after the endpoint, e.g. `/ocpp/cp001`.
    #[focus]
    pub path: Widget<InputFieldState, InputField<String>>,
    pub error: Widget<String, Text>,
    pub keybinds: Widget<String, Text>,
}

impl OcppSetupDialog {
    pub fn new() -> Self {
        let input_style = InputFieldStyle::default();
        let selection_style = SelectionStyle::default();

        OcppSetupDialogBuilder::default()
            .name(input("Name", "cs-1", &input_style, true))
            .config_path(suggest_input(
                "Config",
                "device.toml",
                &input_style,
                FsPathProvider::with_extensions(&["toml", "json"]),
            ))
            .version(selection(
                "Version",
                vec![OcppVersion::V1_6, OcppVersion::V2_0_1, OcppVersion::V2_1],
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
            .path(input("Path", "/ocpp/cp001", &input_style, false))
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

    /// Build a dialog pre-filled with an existing spec + device-config path, for `:edit`.
    pub fn edit(spec: &OcppSpec, device_path: &str) -> Self {
        let mut d = Self::new();
        set_text(&mut d.name, &spec.name);
        set_suggest_text(&mut d.config_path, device_path);
        d.version.state.set_selection(match spec.version {
            OcppVersion::V1_6 => 0,
            OcppVersion::V2_0_1 => 1,
            OcppVersion::V2_1 => 2,
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
        set_text(&mut d.path, &spec.path);
        d
    }

    /// Validate every field and produce the spec, or an error message for the live display.
    pub fn resolve(&self) -> Result<OcppSpec, String> {
        let name = self.name.state.input().trim().to_string();
        if name.is_empty() {
            return Err("Name is required.".into());
        }
        if let ValidateResult::Error(e) = ConfigPath::validate(self.config_path.state.input()) {
            return Err(e);
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

        // Normalize the optional URL path: trim, and ensure a leading '/' when non-empty. The
        // server role has no URL path, so it is always empty there.
        let mut path = if self.path_hidden() {
            String::new()
        } else {
            self.path.state.input().trim().to_string()
        };
        if !path.is_empty() && !path.starts_with('/') {
            path.insert(0, '/');
        }

        Ok(OcppSpec {
            name,
            version: self.version.state.get_value(),
            role: self.role.state.get_value(),
            protocol: self.protocol.state.get_value(),
            ip,
            port,
            path,
            timeout_ms: None,
        })
    }

    /// The entered device-config path (trimmed; empty when none).
    pub fn config_path(&self) -> String {
        self.config_path.state.input().trim().to_string()
    }

    /// The URL `path` field is only meaningful for the client (CS) role — the CSMS server binds a
    /// host:port and ignores it — so it is hidden (and skipped by focus) when the role is Server.
    fn path_hidden(&self) -> bool {
        self.role.state.get_value() == OcppRole::Server
    }

    /// Advance focus, skipping the hidden URL-path field when in server role.
    pub fn focus_step(&mut self, forward: bool) {
        if forward {
            self.focus_next();
        } else {
            self.focus_previous();
        }
        if self.path_hidden() && matches!(self.focus, OcppSetupDialogFocus::Path) {
            if forward {
                self.focus_next();
            } else {
                self.focus_previous();
            }
        }
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        match self.resolve() {
            Ok(_) => self.error.state.clear(),
            Err(e) => self.error.state = e,
        }

        let has_error = !self.error.state.is_empty();
        // border(2) + inner margin(2) + name(3) + config path(3) + version|role(3)
        // + protocol|ip|port|path(3) + keybinds(1), plus the error box (3) only when there is a message.
        let box_height = if has_error { 20 } else { 17 };
        let box_width = 80;

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
            Constraint::Length(3),            // config path
            Constraint::Length(3),            // version | role
            Constraint::Length(3),            // protocol | ip | port
            Constraint::Length(error_height), // error (hidden when empty)
            Constraint::Length(1),            // keybinds
        ])
        .split(inner);

        StatefulWidget::render(&self.name.widget, rows[0], buf, &mut self.name.state);
        StatefulWidget::render(
            &self.config_path.widget,
            rows[1],
            buf,
            &mut self.config_path.state,
        );

        let [vl, vr] = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
            .areas(rows[2]);
        StatefulWidget::render(&self.version.widget, vl, buf, &mut self.version.state);
        StatefulWidget::render(&self.role.widget, vr, buf, &mut self.role.state);

        if self.path_hidden() {
            // No URL path for the server role — let ip take the freed space.
            let [proto, ip, port] = Layout::horizontal([
                Constraint::Length(12),
                Constraint::Min(1),
                Constraint::Length(13),
            ])
            .areas(rows[3]);
            StatefulWidget::render(&self.protocol.widget, proto, buf, &mut self.protocol.state);
            StatefulWidget::render(&self.ip.widget, ip, buf, &mut self.ip.state);
            StatefulWidget::render(&self.port.widget, port, buf, &mut self.port.state);
        } else {
            let [proto, ip, port, path] = Layout::horizontal([
                Constraint::Length(12),
                Constraint::Min(1),
                Constraint::Length(13),
                Constraint::Length(24),
            ])
            .areas(rows[3]);
            StatefulWidget::render(&self.protocol.widget, proto, buf, &mut self.protocol.state);
            StatefulWidget::render(&self.ip.widget, ip, buf, &mut self.ip.state);
            StatefulWidget::render(&self.port.widget, port, buf, &mut self.port.state);
            StatefulWidget::render(&self.path.widget, path, buf, &mut self.path.state);
        }

        if has_error {
            StatefulWidget::render(&self.error.widget, rows[4], buf, &mut self.error.state);
        }
        StatefulWidget::render(
            &self.keybinds.widget,
            rows[5],
            buf,
            &mut self.keybinds.state,
        );

        // Must be called after every sibling widget above has been rendered, so the popup
        // paints on top rather than being overwritten (painter's-algorithm buffer model).
        self.config_path
            .widget
            .render_overlay(area, buf, &mut self.config_path.state);
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

fn suggest_input<T: Validate + Clone>(
    title: &str,
    placeholder: &str,
    style: &InputFieldStyle,
    provider: FsPathProvider,
) -> Widget<SuggestInputState<FsPathProvider>, SuggestInput<T, FsPathProvider>> {
    let mut state = SuggestInputStateBuilder::default()
        .provider(provider)
        .build()
        .unwrap();
    state.set_placeholder(Some(placeholder.to_string()));

    Widget {
        state,
        widget: SuggestInputBuilder::default()
            .input_field(
                InputFieldBuilder::default()
                    .border(Border::Full(Margin::new(1, 0)))
                    .title(Some((title, HorizontalAlignment::Left).into()))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(style.clone())
                    .build()
                    .unwrap(),
            )
            .popup_style(SuggestInputStyle::default())
            .build()
            .unwrap(),
    }
}

fn set_suggest_text(
    w: &mut Widget<SuggestInputState<FsPathProvider>, SuggestInput<ConfigPath, FsPathProvider>>,
    value: &str,
) {
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
