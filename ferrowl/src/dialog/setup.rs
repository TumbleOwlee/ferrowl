//! Module setup dialog. In **Edit** mode (`:e`) it edits the current tab's per-instance
//! settings (name, transport + endpoint, role). In **New** mode (`:n`/`:new`) it additionally
//! takes an optional device-config path: empty creates an empty module, otherwise the path is
//! validated live and must point at a loadable config. While any field is invalid the dialog
//! cannot be confirmed (only cancelled with Esc).

use derive_builder::Builder;
use ferrowl_derive::{Focus, focusable};
use ferrowl_ui::{
    COLOR_SCHEME,
    state::{InputFieldState, InputFieldStateBuilder, SelectionState, SelectionStateBuilder},
    style::{InputFieldStyle, SelectionStyle, TextStyle},
    traits::ToLabel,
    types::Border,
    widgets::{
        GetValue, InputField, InputFieldBuilder, Selection, SelectionBuilder, Text, TextBuilder,
        Widget,
    },
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, Clear, StatefulWidget, Widget as UiWidget},
};

use crate::config::{DeviceConfig, Endpoint, Role};

/// Edit an existing instance, or create a new module (with an optional config path).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogMode {
    Edit,
    New,
}

/// Transport selection value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transport {
    Tcp,
    Rtu,
}

impl ToLabel for Transport {
    fn to_label(&self) -> String {
        match self {
            Transport::Tcp => "TCP",
            Transport::Rtu => "RTU",
        }
        .to_string()
    }
}

impl ToLabel for Role {
    fn to_label(&self) -> String {
        match self {
            Role::Client => "Client",
            Role::Server => "Server",
        }
        .to_string()
    }
}

/// Serial parity selection value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parity {
    None,
    Odd,
    Even,
}

impl ToLabel for Parity {
    fn to_label(&self) -> String {
        match self {
            Parity::None => "None",
            Parity::Odd => "Odd",
            Parity::Even => "Even",
        }
        .to_string()
    }
}

impl Parity {
    /// Map to the `Endpoint`/`rtu::Config` representation (`None` = no parity).
    fn to_config(&self) -> Option<String> {
        match self {
            Parity::None => None,
            Parity::Odd => Some("odd".to_string()),
            Parity::Even => Some("even".to_string()),
        }
    }

    fn from_config(value: Option<&str>) -> Parity {
        match value.map(|s| s.to_ascii_lowercase()).as_deref() {
            Some("odd") => Parity::Odd,
            Some("even") => Parity::Even,
            _ => Parity::None,
        }
    }

    fn index(&self) -> usize {
        match self {
            Parity::None => 0,
            Parity::Odd => 1,
            Parity::Even => 2,
        }
    }
}

/// A numeric serial choice (data/stop bits) rendered as a selection label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct U8Choice(pub u8);

impl ToLabel for U8Choice {
    fn to_label(&self) -> String {
        self.0.to_string()
    }
}

/// The validated per-instance settings.
pub struct SetupValues {
    pub name: String,
    pub role: Role,
    pub endpoint: Endpoint,
    /// Optional per-instance timing overrides (ms); `None` falls back to device/app config.
    pub timeout_ms: Option<usize>,
    pub delay_ms: Option<usize>,
    pub interval_ms: Option<usize>,
}

/// The full validated dialog result. `device` is set in New mode: the config path (or
/// `"<new>"`) and the loaded (or empty) device config.
pub struct SetupOutcome {
    pub values: SetupValues,
    pub device: Option<(String, DeviceConfig)>,
}

#[focusable]
#[derive(Builder, Focus)]
pub struct SetupDialog {
    #[focus]
    pub name: Widget<InputFieldState, InputField<String>>,
    #[focus(when = {self.mode == DialogMode::New})]
    pub config_path: Widget<InputFieldState, InputField<String>>,
    #[focus]
    pub transport: Widget<SelectionState<Transport>, Selection<Transport>>,
    #[focus]
    pub role: Widget<SelectionState<Role>, Selection<Role>>,
    #[focus(when = {self.transport.get_value() == Transport::Tcp})]
    pub ip: Widget<InputFieldState, InputField<String>>,
    #[focus(when = {self.transport.get_value() == Transport::Tcp})]
    pub port: Widget<InputFieldState, InputField<String>>,
    #[focus(when = {self.transport.get_value() == Transport::Rtu})]
    pub path: Widget<InputFieldState, InputField<String>>,
    #[focus(when = {self.transport.get_value() == Transport::Rtu})]
    pub baud: Widget<InputFieldState, InputField<String>>,
    #[focus(when = {self.transport.get_value() == Transport::Rtu})]
    pub parity: Widget<SelectionState<Parity>, Selection<Parity>>,
    #[focus(when = {self.transport.get_value() == Transport::Rtu})]
    pub data_bits: Widget<SelectionState<U8Choice>, Selection<U8Choice>>,
    #[focus(when = {self.transport.get_value() == Transport::Rtu})]
    pub stop_bits: Widget<SelectionState<U8Choice>, Selection<U8Choice>>,
    #[focus(when = {self.role.get_value() == Role::Client})]
    pub timeout: Widget<InputFieldState, InputField<String>>,
    #[focus(when = {self.role.get_value() == Role::Client})]
    pub delay: Widget<InputFieldState, InputField<String>>,
    #[focus(when = {self.role.get_value() == Role::Client})]
    pub interval: Widget<InputFieldState, InputField<String>>,
    pub error: Widget<String, Text>,
    pub keybinds: Widget<String, Text>,
    pub mode: DialogMode,
}

impl SetupDialog {
    /// Edit an existing instance (`:e`). `timing` is the effective (resolved) timeout/delay/
    /// interval in ms used to prefill the inputs (shown only for clients).
    pub fn edit(name: &str, role: Role, endpoint: &Endpoint, timing: (usize, usize, usize)) -> Self {
        let mut dialog = Self::build(name, DialogMode::Edit, timing);
        dialog
            .role
            .state
            .set_selection(if role == Role::Client { 1 } else { 0 });
        match endpoint {
            Endpoint::Tcp { ip, port } => {
                dialog.transport.state.set_selection(0);
                set_input(&mut dialog.ip, ip);
                set_input(&mut dialog.port, &port.to_string());
            }
            Endpoint::Rtu {
                path,
                baud_rate,
                parity,
                data_bits,
                stop_bits,
            } => {
                dialog.transport.state.set_selection(1);
                set_input(&mut dialog.path, path);
                set_input(&mut dialog.baud, &baud_rate.to_string());
                dialog
                    .parity
                    .state
                    .set_selection(Parity::from_config(parity.as_deref()).index());
                select_u8(&mut dialog.data_bits.state, *data_bits);
                select_u8(&mut dialog.stop_bits.state, *stop_bits);
            }
        }
        dialog
    }

    /// Create a new module (`:n`/`:new`), with an optional device-config path. `timing` prefills
    /// the (client-only) timeout/delay/interval inputs with the global app defaults.
    pub fn create(timing: (usize, usize, usize)) -> Self {
        Self::build("", DialogMode::New, timing)
    }

    fn build(name: &str, mode: DialogMode, timing: (usize, usize, usize)) -> Self {
        let selection_style = SelectionStyle::default();
        let input_style = InputFieldStyle::default();
        let error_style = TextStyle {
            general: ratatui::style::Style::default()
                .fg(COLOR_SCHEME.error)
                .bg(COLOR_SCHEME.bg),
        };

        let mut name_field = input("Name", None, "module name", &input_style, true);
        set_input(&mut name_field, name);

        let mut dialog = SetupDialogBuilder::default()
            .name(name_field)
            .config_path(input(
                "Config Path (optional)",
                None,
                "configs/device.toml",
                &input_style,
                false,
            ))
            .transport(selection(
                "Transport",
                None,
                vec![Transport::Tcp, Transport::Rtu],
                &selection_style,
            ))
            .role(selection(
                "Role",
                Some(HorizontalAlignment::Right),
                vec![Role::Server, Role::Client],
                &selection_style,
            ))
            .ip(input("IP", None, "127.0.0.1", &input_style, false))
            .port(input(
                "Port",
                Some(HorizontalAlignment::Right),
                "502",
                &input_style,
                false,
            ))
            .path(input(
                "Serial Path",
                None,
                "/dev/ttyUSB0",
                &input_style,
                false,
            ))
            .baud(input(
                "Baud",
                Some(HorizontalAlignment::Right),
                "19200",
                &input_style,
                false,
            ))
            .parity(selection(
                "Parity",
                None,
                vec![Parity::None, Parity::Odd, Parity::Even],
                &selection_style,
            ))
            .data_bits(selection(
                "Data Bits",
                Some(HorizontalAlignment::Right),
                vec![U8Choice(8), U8Choice(7), U8Choice(6), U8Choice(5)],
                &selection_style,
            ))
            .stop_bits(selection(
                "Stop Bits",
                None,
                vec![U8Choice(1), U8Choice(2)],
                &selection_style,
            ))
            .timeout(input("Timeout ms", None, "", &input_style, false))
            .delay(input("Delay ms", None, "", &input_style, false))
            .interval(input("Interval ms", None, "", &input_style, false))
            .error(Widget {
                state: String::new(),
                widget: TextBuilder::default()
                    .title(Some("Error".into()))
                    .border(Border::Full(Margin::new(1, 0)))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(error_style)
                    .build()
                    .unwrap(),
            })
            .keybinds(Widget {
                state: "<Tab> next   <Enter> confirm   <Esc> cancel".to_string(),
                widget: TextBuilder::default()
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .style(TextStyle::default())
                    .build()
                    .unwrap(),
            })
            .mode(mode)
            .focus(SetupDialogFocus::Name)
            .build()
            .unwrap();

        // Prefill timing inputs with the resolved defaults so clients always show a value.
        let (timeout_ms, delay_ms, interval_ms) = timing;
        set_input(&mut dialog.timeout, &timeout_ms.to_string());
        set_input(&mut dialog.delay, &delay_ms.to_string());
        set_input(&mut dialog.interval, &interval_ms.to_string());
        dialog
    }

    /// Validate everything and produce the outcome. In New mode the (optional) config path is
    /// loaded/validated here, so an invalid path is reported as an error.
    pub fn resolve(&self) -> Result<SetupOutcome, String> {
        let values = self.values()?;
        let device = if self.mode == DialogMode::New {
            let path = self.config_path.state.input().trim().to_string();
            if path.is_empty() {
                Some(("<new>".to_string(), DeviceConfig::default()))
            } else {
                let device =
                    crate::config::load_device(&path).map_err(|e| format!("Config: {e}"))?;
                Some((path, device))
            }
        } else {
            None
        };
        Ok(SetupOutcome { values, device })
    }

    fn values(&self) -> Result<SetupValues, String> {
        let name = self.name.state.input().trim().to_string();
        if name.is_empty() {
            return Err("Name is required.".into());
        }
        let role = self.role.state.get_value();
        let endpoint = match self.transport.state.get_value() {
            Transport::Tcp => {
                let ip = self.ip.state.input().trim().to_string();
                if ip.is_empty() {
                    return Err("IP address is required.".into());
                }
                let port = self
                    .port
                    .state
                    .input()
                    .trim()
                    .parse::<u16>()
                    .map_err(|_| "Port must be a number (0-65535).".to_string())?;
                Endpoint::Tcp { ip, port }
            }
            Transport::Rtu => {
                let path = self.path.state.input().trim().to_string();
                if path.is_empty() {
                    return Err("Serial path is required.".into());
                }
                let baud_rate = self
                    .baud
                    .state
                    .input()
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| "Baud rate must be a number.".to_string())?;
                Endpoint::Rtu {
                    path,
                    baud_rate,
                    parity: self.parity.state.get_value().to_config(),
                    data_bits: Some(self.data_bits.state.get_value().0),
                    stop_bits: Some(self.stop_bits.state.get_value().0),
                }
            }
        };
        let parse_ms = |input: &str, label: &str| -> Result<Option<usize>, String> {
            let t = input.trim();
            if t.is_empty() {
                Ok(None)
            } else {
                t.parse::<usize>()
                    .map(Some)
                    .map_err(|_| format!("{label} must be a whole number of milliseconds."))
            }
        };
        // Timing applies to polling clients only; servers never poll.
        let (timeout_ms, delay_ms, interval_ms) = if role == Role::Client {
            (
                parse_ms(self.timeout.state.input(), "Timeout")?,
                parse_ms(self.delay.state.input(), "Delay")?,
                parse_ms(self.interval.state.input(), "Interval")?,
            )
        } else {
            (None, None, None)
        };

        Ok(SetupValues {
            name,
            role,
            endpoint,
            timeout_ms,
            delay_ms,
            interval_ms,
        })
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        // Reflect validation state in the error field.
        match self.resolve() {
            Ok(_) => self.error.state.clear(),
            Err(e) => self.error.state = e,
        }

        let is_new = self.mode == DialogMode::New;
        let is_rtu = self.transport.state.get_value() == Transport::Rtu;
        // Timing only applies to (and is shown for) polling clients.
        let is_client = self.role.state.get_value() == Role::Client;
        // RTU needs three endpoint rows (path/baud, parity/data-bits, stop-bits); TCP one.
        let endpoint_rows: u16 = if is_rtu { 3 } else { 1 };
        // border(2) + inner margin(2) + name(3) + select(3) + endpoint + error(3) + keybinds(1)
        // + optional config-path row (New mode) + optional timing row (client only).
        let box_height = 14
            + endpoint_rows * 3
            + if is_new { 3 } else { 0 }
            + if is_client { 3 } else { 0 };

        let [_, hcenter, _] = Layout::horizontal([
            Constraint::Min(1),
            Constraint::Length(60),
            Constraint::Min(1),
        ])
        .areas(area);
        let [_, vcenter, _] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(box_height),
            Constraint::Min(1),
        ])
        .areas(hcenter);

        Clear.render(vcenter, buf);
        let title = if is_new { "New Module" } else { "Module Setup" };
        let block = Block::bordered()
            .style(
                ratatui::style::Style::default()
                    .fg(COLOR_SCHEME.hi)
                    .bg(COLOR_SCHEME.bg),
            )
            .title_alignment(HorizontalAlignment::Center)
            .title(title);
        let inner = block.inner(vcenter).inner(Margin::new(2, 1));
        ratatui::prelude::Widget::render(&ratatui::widgets::Clear, vcenter, buf);
        block.render(vcenter, buf);

        let mut constraints = vec![Constraint::Length(3)]; // name
        if is_new {
            constraints.push(Constraint::Length(3)); // config path
        }
        constraints.push(Constraint::Length(3)); // transport + role
        constraints.push(Constraint::Length(endpoint_rows * 3)); // endpoint
        if is_client {
            constraints.push(Constraint::Length(3)); // timeout + delay + interval
        }
        constraints.push(Constraint::Length(3)); // error
        constraints.push(Constraint::Length(1)); // keybinds
        let rows = Layout::vertical(constraints).split(inner);

        let mut idx = 0;
        StatefulWidget::render(&self.name.widget, rows[idx], buf, &mut self.name.state);
        idx += 1;

        if is_new {
            StatefulWidget::render(
                &self.config_path.widget,
                rows[idx],
                buf,
                &mut self.config_path.state,
            );
            idx += 1;
        }

        let [transport_area, role_area] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .areas(rows[idx]);
        idx += 1;
        StatefulWidget::render(
            &self.transport.widget,
            transport_area,
            buf,
            &mut self.transport.state,
        );
        StatefulWidget::render(&self.role.widget, role_area, buf, &mut self.role.state);

        let endpoint_area = rows[idx];
        idx += 1;
        if is_rtu {
            let [row0, row1, row2] = Layout::vertical([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
            ])
            .areas(endpoint_area);
            render_pair(&mut self.path, &mut self.baud, row0, buf);
            let [parity_area, data_area] =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .areas(row1);
            StatefulWidget::render(
                &self.parity.widget,
                parity_area,
                buf,
                &mut self.parity.state,
            );
            StatefulWidget::render(
                &self.data_bits.widget,
                data_area,
                buf,
                &mut self.data_bits.state,
            );
            let [left, _] =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .areas(row2);
            StatefulWidget::render(&self.stop_bits.widget, left, buf, &mut self.stop_bits.state);
        } else {
            render_pair(&mut self.ip, &mut self.port, endpoint_area, buf);
        }

        if is_client {
            let [timeout_area, delay_area, interval_area] = Layout::horizontal([
                Constraint::Percentage(34),
                Constraint::Percentage(33),
                Constraint::Percentage(33),
            ])
            .areas(rows[idx]);
            idx += 1;
            StatefulWidget::render(
                &self.timeout.widget,
                timeout_area,
                buf,
                &mut self.timeout.state,
            );
            StatefulWidget::render(&self.delay.widget, delay_area, buf, &mut self.delay.state);
            StatefulWidget::render(
                &self.interval.widget,
                interval_area,
                buf,
                &mut self.interval.state,
            );
        }

        let error_area = rows[idx];
        idx += 1;
        if !self.error.state.is_empty() {
            StatefulWidget::render(&self.error.widget, error_area, buf, &mut self.error.state);
        }

        StatefulWidget::render(
            &self.keybinds.widget,
            rows[idx],
            buf,
            &mut self.keybinds.state,
        );
    }
}

/// Render two input fields side by side in `area`.
fn render_pair(
    left: &mut Widget<InputFieldState, InputField<String>>,
    right: &mut Widget<InputFieldState, InputField<String>>,
    area: Rect,
    buf: &mut Buffer,
) {
    let [left_area, right_area] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);
    StatefulWidget::render(&left.widget, left_area, buf, &mut left.state);
    StatefulWidget::render(&right.widget, right_area, buf, &mut right.state);
}

/// Select the entry matching `current` (if present) in a numeric choice selection.
fn select_u8(state: &mut SelectionState<U8Choice>, current: Option<u8>) {
    if let Some(value) = current
        && let Some(index) = state.values().iter().position(|c| c.0 == value)
    {
        state.set_selection(index);
    }
}

fn input(
    title: &str,
    title_alignment: Option<HorizontalAlignment>,
    placeholder: &str,
    style: &InputFieldStyle,
    focused: bool,
) -> Widget<InputFieldState, InputField<String>> {
    Widget {
        state: InputFieldStateBuilder::default()
            .focused(focused)
            .disabled(false)
            .placeholder(Some(placeholder.to_string()))
            .build()
            .unwrap(),
        widget: InputFieldBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some(
                (title, title_alignment.unwrap_or(HorizontalAlignment::Left)).into(),
            ))
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .style(style.clone())
            .build()
            .unwrap(),
    }
}

fn selection<T: ToLabel + Clone>(
    title: &str,
    title_alignment: Option<HorizontalAlignment>,
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
            .title(Some(
                (title, title_alignment.unwrap_or(HorizontalAlignment::Left)).into(),
            ))
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .style(style.clone())
            .build()
            .unwrap(),
    }
}

fn set_input(widget: &mut Widget<InputFieldState, InputField<String>>, value: &str) {
    widget.state.set_input(value.to_string());
    widget.state.set_cursor(value.chars().count());
}
