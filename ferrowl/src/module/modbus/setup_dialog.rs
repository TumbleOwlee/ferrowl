//! Module setup dialog. In **Edit** mode (`:e`) it edits the current tab's per-instance
//! settings (name, transport + endpoint, role). In **New** mode (`:n`/`:new`) it additionally
//! takes an optional device-config path: empty creates an empty module, otherwise the path is
//! validated live and must point at a loadable config. While any field is invalid the dialog
//! cannot be confirmed (only cancelled with Esc).

use crossterm::event::{KeyCode, KeyModifiers};
use derive_builder::Builder;
use ferrowl_ui::{
    Border, COLOR_SCHEME, EventResult,
    state::{
        InputFieldState, InputFieldStateBuilder, SelectionState, SelectionStateBuilder,
        SuggestInputState, SuggestInputStateBuilder,
    },
    style::{InputFieldStyle, SelectionStyle, TextStyle},
    traits::{HandleEvents, ToLabel},
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

use crate::config::device::ReadRanges;
use crate::config::{DeviceConfig, Endpoint, Role};
use crate::dialog::NonEmpty;
use crate::dialog::close_confirm::{CloseConfirmDialog, CloseConfirmOutcome, route_close_confirm};
use crate::dialog::path_suggest::FsPathProvider;

use super::build::Timing;

mod choices;
use choices::{DialogMode, Parity, ReconnectChoice, Transport, U8Choice};

/// The validated per-instance settings.
pub struct SetupValues {
    pub name: String,
    pub config_path: String,
    pub role: Role,
    pub endpoint: Endpoint,
    /// Optional per-instance timing overrides (ms); `None` falls back to device/app config.
    pub timeout_ms: Option<usize>,
    pub delay_ms: Option<usize>,
    pub interval_ms: Option<usize>,
    /// Client-only auto-reconnect setting; always explicit after a dialog save.
    pub reconnect: Option<bool>,
    /// Explicit per-function-code read ranges (client only), applied to the device config.
    pub read_ranges: ReadRanges,
}

/// The full validated dialog result. `device` is set in New mode: the config path (or
/// `""`) and the loaded (or empty) device config.
pub struct SetupOutcome {
    pub values: SetupValues,
    pub device: Option<(String, DeviceConfig)>,
}

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
                match crate::config::load_device(input) {
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
pub struct SetupDialog {
    #[focus]
    pub name: Widget<InputFieldState, InputField<NonEmpty>>,
    #[focus]
    pub config_path:
        Widget<SuggestInputState<FsPathProvider>, SuggestInput<ConfigPath, FsPathProvider>>,
    #[focus]
    pub transport: Widget<SelectionState<Transport>, Selection<Transport>>,
    #[focus]
    pub role: Widget<SelectionState<Role>, Selection<Role>>,
    #[focus(when = {self.transport.get_value() == Transport::Tcp})]
    pub ip: Widget<InputFieldState, InputField<String>>,
    #[focus(when = {self.transport.get_value() == Transport::Tcp})]
    pub port: Widget<InputFieldState, InputField<String>>,
    #[focus(when = {self.transport.get_value() == Transport::Rtu})]
    pub path: Widget<SuggestInputState<FsPathProvider>, SuggestInput<String, FsPathProvider>>,
    #[focus(when = {self.transport.get_value() == Transport::Rtu})]
    pub baud: Widget<InputFieldState, InputField<String>>,
    #[focus(when = {self.transport.get_value() == Transport::Rtu})]
    pub parity: Widget<SelectionState<Parity>, Selection<Parity>>,
    #[focus(when = {self.transport.get_value() == Transport::Rtu})]
    pub data_bits: Widget<SelectionState<U8Choice>, Selection<U8Choice>>,
    #[focus(when = {self.transport.get_value() == Transport::Rtu})]
    pub stop_bits: Widget<SelectionState<U8Choice>, Selection<U8Choice>>,
    #[focus]
    pub timeout: Widget<InputFieldState, InputField<String>>,
    #[focus]
    pub delay: Widget<InputFieldState, InputField<String>>,
    #[focus]
    pub interval: Widget<InputFieldState, InputField<String>>,
    #[focus(when = {self.role.get_value() == Role::Client})]
    pub reconnect: Widget<SelectionState<ReconnectChoice>, Selection<ReconnectChoice>>,
    #[focus]
    pub holding_ranges: Widget<InputFieldState, InputField<String>>,
    #[focus]
    pub input_ranges: Widget<InputFieldState, InputField<String>>,
    #[focus]
    pub coil_ranges: Widget<InputFieldState, InputField<String>>,
    #[focus]
    pub discrete_ranges: Widget<InputFieldState, InputField<String>>,
    pub error: Widget<String, Text>,
    pub keybinds: Widget<String, Text>,
    pub mode: DialogMode,
    /// Confirm-close popup, opened with Esc.
    #[builder(default)]
    pub close_confirm: Option<CloseConfirmDialog>,
    /// Set once the close-confirm popup is confirmed; the host checks this via
    /// `take_close_request` and closes the dialog.
    #[builder(default)]
    close_requested: bool,
}

impl SetupDialog {
    /// Edit an existing instance (`:e`). `timing` is the effective (resolved) timeout/delay/
    /// interval/reconnect settings used to prefill the inputs.
    pub fn edit(
        name: &str,
        config_path: &str,
        role: Role,
        endpoint: &Endpoint,
        timing: Timing,
        ranges: &ReadRanges,
    ) -> Self {
        let mut dialog = Self::build(name, config_path, DialogMode::Edit, timing, ranges);
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
                set_suggest_input(&mut dialog.path, path);
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
    /// the timeout/delay/interval/reconnect inputs with the global app defaults.
    pub fn create(timing: Timing) -> Self {
        Self::build("", "", DialogMode::New, timing, &ReadRanges::default())
    }

    fn build(
        name: &str,
        config_path: &str,
        mode: DialogMode,
        timing: Timing,
        ranges: &ReadRanges,
    ) -> Self {
        let selection_style = SelectionStyle::default();
        let input_style = InputFieldStyle::default();
        let error_style = TextStyle {
            general: ratatui::style::Style::default()
                .fg(COLOR_SCHEME.error)
                .bg(COLOR_SCHEME.bg),
        };

        let mut name_field = input("Name", None, "module name", &input_style, true);
        set_input(&mut name_field, name);
        let mut config_path_field = suggest_input(
            "Config Path [TOML/JSON] (optional)",
            None,
            "device.toml",
            &input_style,
            false,
            FsPathProvider::with_extensions(&["toml", "json"]),
        );
        set_suggest_input(&mut config_path_field, config_path);

        let mut dialog = SetupDialogBuilder::default()
            .name(name_field)
            .config_path(config_path_field)
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
            .path(suggest_input(
                "Serial Path",
                None,
                "/dev/ttyUSB0",
                &input_style,
                false,
                FsPathProvider::default(),
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
            .reconnect(selection(
                "Reconnect",
                None,
                vec![ReconnectChoice::On, ReconnectChoice::Off],
                &selection_style,
            ))
            .holding_ranges(input(
                "Holding ranges",
                None,
                "0-100,140-160",
                &input_style,
                false,
            ))
            .input_ranges(input("Input ranges", None, "0-9", &input_style, false))
            .coil_ranges(input("Coil ranges", None, "0-31", &input_style, false))
            .discrete_ranges(input("Discrete ranges", None, "0-31", &input_style, false))
            .error(Widget {
                state: String::new(),
                widget: TextBuilder::default()
                    .title(Some("Error".into()))
                    .border(Border::Full(Margin::new(1, 0)))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .multiline(true)
                    .style(error_style)
                    .build()
                    .expect("all required builder fields are set"),
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
                    .expect("all required builder fields are set"),
            })
            .mode(mode)
            .focus(SetupDialogFocus::Name)
            .build()
            .expect("all required builder fields are set");

        // Prefill timing inputs with the resolved defaults so clients always show a value.
        set_input(&mut dialog.timeout, &timing.timeout_ms.to_string());
        set_input(&mut dialog.delay, &timing.delay_ms.to_string());
        set_input(&mut dialog.interval, &timing.interval_ms.to_string());
        dialog
            .reconnect
            .state
            .set_selection(if timing.reconnect { 0 } else { 1 });

        // Prefill explicit read ranges from the device config.
        for (field, value) in [
            (&mut dialog.holding_ranges, &ranges.holding),
            (&mut dialog.input_ranges, &ranges.input),
            (&mut dialog.coil_ranges, &ranges.coils),
            (&mut dialog.discrete_ranges, &ranges.discrete),
        ] {
            if let Some(v) = value {
                set_input(field, v);
            }
        }
        dialog
    }

    /// Route a key: the close-confirm popup captures all keys while open; Esc opens it;
    /// everything else falls through to the derived per-field routing.
    pub fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        match route_close_confirm(&mut self.close_confirm, modifiers, code) {
            CloseConfirmOutcome::NotActive => {}
            CloseConfirmOutcome::Close => {
                self.close_requested = true;
                return EventResult::Consumed;
            }
            CloseConfirmOutcome::Consumed => return EventResult::Consumed,
        }

        if modifiers == KeyModifiers::NONE && code == KeyCode::Esc {
            self.close_confirm = Some(CloseConfirmDialog::new());
            return EventResult::Consumed;
        }

        <Self as HandleEvents>::handle_events(self, modifiers, code)
    }

    /// Whether the close-confirm popup was confirmed since the last call; clears the flag.
    pub fn take_close_request(&mut self) -> bool {
        std::mem::take(&mut self.close_requested)
    }

    /// Validate everything and produce the outcome. In New mode the (optional) config path is
    /// loaded/validated here, so an invalid path is reported as an error.
    pub fn resolve(&self) -> Result<SetupOutcome, String> {
        let values = self.values()?;
        let device = if self.mode == DialogMode::New {
            let path = self.config_path.state.input().trim().to_string();
            if path.is_empty() || !std::path::Path::new(&path).exists() {
                Some((path, DeviceConfig::default()))
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
        let config_path = self.config_path.state.input().trim().to_string();
        if !config_path.is_empty() && FileType::from_path(&config_path).is_none() {
            return Err(format!(
                "Unknown format for '{config_path}' (use .toml or .json)"
            ));
        }
        let role = self.role.state.get_value();
        let endpoint = match self.transport.state.get_value() {
            Transport::Tcp => {
                let mut ip = self.ip.state.input().trim().to_string();
                if ip.is_empty() {
                    ip = "127.0.0.1".to_string();
                }
                let port = self.port.state.input();
                let port = if !port.is_empty() {
                    port.trim()
                        .parse::<u16>()
                        .map_err(|_| "Port must be a number (0-65535).".to_string())?
                } else {
                    502
                };
                Endpoint::Tcp { ip, port }
            }
            Transport::Rtu => {
                let mut path = self.path.state.input().trim().to_string();
                if path.is_empty() {
                    path = "/dev/ttyUSB0".to_string();
                }
                let baud_rate = self.baud.state.input();
                let baud_rate = if !baud_rate.is_empty() {
                    baud_rate
                        .trim()
                        .parse::<u32>()
                        .map_err(|_| "Baud rate must be a number.".to_string())?
                } else {
                    19200
                };
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
        // Timing and explicit read ranges are shown and captured for all roles.
        let opt = |s: &str| {
            let t = s.trim();
            (!t.is_empty()).then(|| t.to_string())
        };
        let timeout_ms = parse_ms(self.timeout.state.input(), "Timeout")?;
        let delay_ms = parse_ms(self.delay.state.input(), "Delay")?;
        let interval_ms = parse_ms(self.interval.state.input(), "Interval")?;
        // Reconnect is client-only and hidden for servers; don't report a value for a setting
        // the user never saw, so a server-role save can't clobber it in the device config.
        let reconnect =
            (role == Role::Client).then(|| self.reconnect.state.get_value() == ReconnectChoice::On);
        let read_ranges = ReadRanges {
            holding: opt(self.holding_ranges.state.input()),
            input: opt(self.input_ranges.state.input()),
            coils: opt(self.coil_ranges.state.input()),
            discrete: opt(self.discrete_ranges.state.input()),
        };

        Ok(SetupValues {
            name,
            config_path,
            role,
            endpoint,
            timeout_ms,
            delay_ms,
            interval_ms,
            reconnect,
            read_ranges,
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
        // RTU needs three endpoint rows (path/baud, parity/data-bits, stop-bits); TCP one.
        let endpoint_rows: u16 = if is_rtu { 3 } else { 1 };
        // border(2) + inner margin(2) + name(3) + device(3) + select(3) + endpoint + timing(3) + ranges(6)
        // + error(4) + keybinds(1) + optional config-path row (New mode).
        let box_height = 27 + endpoint_rows * 3;

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
        let block_inner = block.inner(vcenter);
        let inner = block_inner.inner(Margin::new(2, 1));
        ratatui::prelude::Widget::render(&ratatui::widgets::Clear, vcenter, buf);
        block.render(vcenter, buf);

        let constraints = vec![
            Constraint::Length(3),                 // name
            Constraint::Length(3),                 // config path
            Constraint::Length(3),                 // transport + role
            Constraint::Length(endpoint_rows * 3), // endpoint
            Constraint::Length(3),                 // timeout + delay + interval
            Constraint::Length(3),                 // holding + input ranges
            Constraint::Length(3),                 // coil + discrete ranges
            Constraint::Length(4),                 // error
            Constraint::Length(1),                 // keybinds
        ];
        let rows = Layout::vertical(constraints).split(inner);

        let mut idx = 0;
        StatefulWidget::render(&self.name.widget, rows[idx], buf, &mut self.name.state);
        idx += 1;

        StatefulWidget::render(
            &self.config_path.widget,
            rows[idx],
            buf,
            &mut self.config_path.state,
        );
        idx += 1;

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
            let [path_area, baud_area] =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .areas(row0);
            StatefulWidget::render(&self.path.widget, path_area, buf, &mut self.path.state);
            StatefulWidget::render(&self.baud.widget, baud_area, buf, &mut self.baud.state);
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

        {
            // Client: timeout | delay | interval | reconnect. Server hides reconnect, so the
            // remaining three widen to thirds instead of leaving a blank quarter.
            let is_client = self.role.state.get_value() == Role::Client;
            let (timeout_area, delay_area, interval_area, reconnect_area) = if is_client {
                let [t, d, i, r] = Layout::horizontal([
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                ])
                .areas(rows[idx]);
                (t, d, i, Some(r))
            } else {
                let [t, d, i] = Layout::horizontal([
                    Constraint::Percentage(34),
                    Constraint::Percentage(33),
                    Constraint::Percentage(33),
                ])
                .areas(rows[idx]);
                (t, d, i, None)
            };
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
            if let Some(reconnect_area) = reconnect_area {
                StatefulWidget::render(
                    &self.reconnect.widget,
                    reconnect_area,
                    buf,
                    &mut self.reconnect.state,
                );
            }

            render_pair(
                &mut self.holding_ranges,
                &mut self.input_ranges,
                rows[idx],
                buf,
            );
            idx += 1;
            render_pair(
                &mut self.coil_ranges,
                &mut self.discrete_ranges,
                rows[idx],
                buf,
            );
            idx += 1;
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

        // Suggestion popups draw last, over everything else in the dialog (and may overflow
        // the dialog box itself), so both must be rendered after all sibling widgets above.
        self.config_path
            .widget
            .render_overlay(area, buf, &mut self.config_path.state);
        self.path
            .widget
            .render_overlay(area, buf, &mut self.path.state);

        if let Some(d) = self.close_confirm.as_mut() {
            d.render(vcenter, buf);
        }
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

fn input<T: Validate + Clone>(
    title: &str,
    title_alignment: Option<HorizontalAlignment>,
    placeholder: &str,
    style: &InputFieldStyle,
    focused: bool,
) -> Widget<InputFieldState, InputField<T>> {
    Widget {
        state: InputFieldStateBuilder::default()
            .focused(focused)
            .disabled(false)
            .placeholder(Some(placeholder.to_string()))
            .allowed_for::<T>()
            .build()
            .expect("all required builder fields are set"),
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
            .expect("all required builder fields are set"),
    }
}

/// Build a [`SuggestInput`] field with the same title/border/margin/style defaults as
/// [`input`], backed by `provider` for the completion popup.
fn suggest_input<T: Validate + Clone, P: ferrowl_ui::traits::SuggestionProvider + Clone>(
    title: &str,
    title_alignment: Option<HorizontalAlignment>,
    placeholder: &str,
    style: &InputFieldStyle,
    focused: bool,
    provider: P,
) -> Widget<SuggestInputState<P>, SuggestInput<T, P>> {
    Widget {
        state: SuggestInputStateBuilder::default()
            .field(
                InputFieldStateBuilder::default()
                    .focused(focused)
                    .disabled(false)
                    .placeholder(Some(placeholder.to_string()))
                    .allowed_for::<T>()
                    .build()
                    .expect("all required builder fields are set"),
            )
            .provider(provider)
            .build()
            .expect("all required builder fields are set"),
        widget: SuggestInputBuilder::default()
            .input_field(
                InputFieldBuilder::default()
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
                    .expect("all required builder fields are set"),
            )
            .build()
            .expect("all required builder fields are set"),
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
            .expect("all required builder fields are set"),
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
            .expect("all required builder fields are set"),
    }
}

fn set_input<T: Validate + Clone>(
    widget: &mut Widget<InputFieldState, InputField<T>>,
    value: &str,
) {
    widget.state.set_input(value.to_string());
    widget.state.set_cursor(value.chars().count());
}

fn set_suggest_input<T: Validate + Clone, P: ferrowl_ui::traits::SuggestionProvider + Clone>(
    widget: &mut Widget<SuggestInputState<P>, SuggestInput<T, P>>,
    value: &str,
) {
    widget.state.set_input(value.to_string());
    widget.state.set_cursor(value.chars().count());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};
    use ferrowl_ui::traits::{HandleEvents, IsFocus, SetFocus};

    fn buffer_text(buf: &Buffer) -> String {
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    /// Typing into the config-path field opens the filesystem suggestion popup, and the
    /// popup is drawn on top of the dialog by the trailing `render_overlay` calls in `render`.
    #[test]
    fn ut_render_config_path_field_shows_suggestion_popup() {
        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        dialog.config_path.state.set_focused(true);
        dialog
            .config_path
            .state
            .handle_events(KeyModifiers::NONE, KeyCode::Char('s'));
        assert!(dialog.config_path.state.suggestions_open());

        let area = Rect::new(0, 0, 80, 60);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
        let text = buffer_text(&buf);
        assert!(text.contains("src"), "missing suggestion popup:\n{text}");
    }

    #[test]
    fn ut_resolve_reconnect_off_maps_to_some_false() {
        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        set_input(&mut dialog.name, "dev");
        dialog.role.state.set_selection(1); // Client
        dialog.reconnect.state.set_selection(1); // Off
        let outcome = dialog.resolve().unwrap();
        assert_eq!(outcome.values.reconnect, Some(false));
    }

    #[test]
    fn ut_resolve_server_role_reports_no_reconnect() {
        // Reconnect is hidden for servers; resolving must not report a value for a setting the
        // user never saw, so applying it can't clobber the device config's existing setting.
        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        set_input(&mut dialog.name, "dev");
        // Default role is Server; reconnect selection is irrelevant/unseen.
        let outcome = dialog.resolve().unwrap();
        assert_eq!(outcome.values.reconnect, None);
    }

    #[test]
    fn ut_edit_prefills_reconnect_off() {
        let timing = Timing {
            timeout_ms: 100,
            delay_ms: 10,
            interval_ms: 50,
            reconnect: false,
        };
        let endpoint = Endpoint::Tcp {
            ip: "127.0.0.1".to_string(),
            port: 502,
        };
        let dialog = SetupDialog::edit(
            "dev",
            "",
            Role::Client,
            &endpoint,
            timing,
            &ReadRanges::default(),
        );
        assert_eq!(dialog.reconnect.state.get_value(), ReconnectChoice::Off);
        let outcome = dialog.resolve().unwrap();
        assert_eq!(outcome.values.reconnect, Some(false));
    }

    #[test]
    fn ut_focus_next_skips_reconnect_for_server_role() {
        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        // Default role is Server, so reconnect is gated off and traversal must skip it.
        dialog.focus = SetupDialogFocus::Interval;
        dialog.interval.state.set_focused(true);
        dialog.focus_next();
        assert!(dialog.holding_ranges.state.is_focused());
        assert!(!dialog.reconnect.state.is_focused());
    }

    fn default_timing() -> Timing {
        Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        }
    }

    #[test]
    fn ut_take_close_request_set_via_esc_enter_and_cleared_after_take() {
        let mut dialog = SetupDialog::create(default_timing());
        assert!(!dialog.take_close_request());
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(dialog.close_confirm.is_some());
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(dialog.take_close_request());
        assert!(!dialog.take_close_request(), "flag must clear after take");
    }

    #[test]
    fn ut_colon_in_text_input_types() {
        let mut dialog = SetupDialog::create(default_timing());
        // Default focus is Name, a free-text field; `:` is typed as ordinary text.
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Char(':'));
        assert_eq!(dialog.name.state.input(), ":");
    }

    #[test]
    fn ut_esc_in_confirm_keeps_setup_open() {
        let mut dialog = SetupDialog::create(default_timing());
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(dialog.close_confirm.is_some());
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(dialog.close_confirm.is_none());
        assert!(!dialog.take_close_request());
    }

    #[test]
    fn ut_space_in_confirm_closes() {
        let mut dialog = SetupDialog::create(default_timing());
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(dialog.close_confirm.is_some());
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Char(' '));
        assert!(dialog.take_close_request());
    }
}
