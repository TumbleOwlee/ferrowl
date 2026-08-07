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
use ferrowl_modbus::tcp::ModbusTlsConfig;

use super::build::Timing;

mod choices;
use choices::{DialogMode, Parity, ReconnectChoice, Transport, U8Choice};
mod tls;
use tls::{SelfSignedChoice, SkipVerifyChoice, TlsInputs, TlsLevel, validate_tls};

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
    /// The device config's `tls` setting (MB-R-104). `None` when the TLS section is hidden
    /// (RTU transport, MB-R-112) — a hidden section must never clobber a device config's
    /// existing setting. `Some(None)` means shown-and-explicitly-off; `Some(Some(cfg))` means
    /// shown at a level above Off. Mirrors `reconnect`'s hidden-vs-explicit shape one level
    /// deeper (the value itself, not just whether to touch it, is optional).
    pub tls: Option<Option<ModbusTlsConfig>>,
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
    /// TLS level, offered only for TCP (MB-R-112: RTU carries no `tls` field at all).
    #[focus(when = {self.transport.get_value() == Transport::Tcp})]
    pub tls_level: Widget<SelectionState<TlsLevel>, Selection<TlsLevel>>,
    /// Server-only "generate an ephemeral self-signed certificate" toggle.
    #[focus(when = {self.show_self_signed()})]
    pub self_signed: Widget<SelectionState<SelfSignedChoice>, Selection<SelfSignedChoice>>,
    /// Client-only "accept any server certificate" toggle.
    #[focus(when = {self.show_skip_verify()})]
    pub skip_verify: Widget<SelectionState<SkipVerifyChoice>, Selection<SkipVerifyChoice>>,
    /// Client-only extra trust anchor for a self-signed server certificate.
    #[focus(when = {self.show_ca_file()})]
    pub ca_file: Widget<SuggestInputState<FsPathProvider>, SuggestInput<String, FsPathProvider>>,
    /// Server-only certificate chain presented to connecting clients.
    #[focus(when = {self.show_server_cert()})]
    pub cert_file: Widget<SuggestInputState<FsPathProvider>, SuggestInput<String, FsPathProvider>>,
    /// Server-only private key matching `cert_file`.
    #[focus(when = {self.show_server_cert()})]
    pub key_file: Widget<SuggestInputState<FsPathProvider>, SuggestInput<String, FsPathProvider>>,
    /// Client-only client certificate presented for mutual TLS.
    #[focus(when = {self.show_client_cert()})]
    pub client_cert_file:
        Widget<SuggestInputState<FsPathProvider>, SuggestInput<String, FsPathProvider>>,
    /// Client-only private key matching `client_cert_file`.
    #[focus(when = {self.show_client_cert()})]
    pub client_key_file:
        Widget<SuggestInputState<FsPathProvider>, SuggestInput<String, FsPathProvider>>,
    /// Server-only CA used to verify client certificates (selecting mTLS as server implies
    /// `require_client_cert = true` in the resolved config).
    #[focus(when = {self.show_client_ca()})]
    pub client_ca_file:
        Widget<SuggestInputState<FsPathProvider>, SuggestInput<String, FsPathProvider>>,
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
        tls: Option<&ModbusTlsConfig>,
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
        if let Some(tls) = tls {
            let level = TlsLevel::from_config(tls, role);
            dialog.tls_level.state.set_selection(level.index());
            dialog
                .self_signed
                .state
                .set_selection(if tls.self_signed { 1 } else { 0 });
            dialog
                .skip_verify
                .state
                .set_selection(if tls.insecure_skip_verify { 1 } else { 0 });
            set_suggest_input(&mut dialog.ca_file, tls.ca_file.as_deref().unwrap_or(""));
            set_suggest_input(
                &mut dialog.cert_file,
                tls.cert_file.as_deref().unwrap_or(""),
            );
            set_suggest_input(&mut dialog.key_file, tls.key_file.as_deref().unwrap_or(""));
            set_suggest_input(
                &mut dialog.client_cert_file,
                tls.client_cert_file.as_deref().unwrap_or(""),
            );
            set_suggest_input(
                &mut dialog.client_key_file,
                tls.client_key_file.as_deref().unwrap_or(""),
            );
            set_suggest_input(
                &mut dialog.client_ca_file,
                tls.client_ca_file.as_deref().unwrap_or(""),
            );
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
            .tls_level(selection(
                "TLS",
                None,
                vec![TlsLevel::Off, TlsLevel::Tls, TlsLevel::MutualTls],
                &selection_style,
            ))
            .self_signed(selection(
                "Self-Signed",
                None,
                vec![SelfSignedChoice::Off, SelfSignedChoice::On],
                &selection_style,
            ))
            .skip_verify(selection(
                "Skip Verify",
                None,
                vec![SkipVerifyChoice::Off, SkipVerifyChoice::On],
                &selection_style,
            ))
            .ca_file(suggest_input(
                "CA File",
                None,
                "ca.pem",
                &input_style,
                false,
                FsPathProvider::with_extensions(&["pem", "crt", "key"]),
            ))
            .cert_file(suggest_input(
                "Cert File",
                None,
                "server.crt",
                &input_style,
                false,
                FsPathProvider::with_extensions(&["pem", "crt", "key"]),
            ))
            .key_file(suggest_input(
                "Key File",
                None,
                "server.key",
                &input_style,
                false,
                FsPathProvider::with_extensions(&["pem", "crt", "key"]),
            ))
            .client_cert_file(suggest_input(
                "Client Cert",
                None,
                "client.crt",
                &input_style,
                false,
                FsPathProvider::with_extensions(&["pem", "crt", "key"]),
            ))
            .client_key_file(suggest_input(
                "Client Key",
                None,
                "client.key",
                &input_style,
                false,
                FsPathProvider::with_extensions(&["pem", "crt", "key"]),
            ))
            .client_ca_file(suggest_input(
                "Client CA",
                None,
                "client_ca.pem",
                &input_style,
                false,
                FsPathProvider::with_extensions(&["pem", "crt", "key"]),
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

    // --- TLS-field visibility ------------------------------------------------------------------
    // Single source of truth consumed by the `#[focus(when)]` gates, the render branches and the
    // dialog-height computation, so keyboard focus, painting and layout can never disagree about
    // which fields exist. Mirrors `ferrowl::module::ocpp::setup_dialog`'s own `show_*` methods.

    /// The TLS level selection row (any TCP endpoint; never RTU, MB-R-112).
    fn tls_shown(&self) -> bool {
        self.transport.get_value() == Transport::Tcp
    }

    /// The currently selected TLS level.
    fn tls_level(&self) -> TlsLevel {
        self.tls_level.get_value()
    }

    /// Server-only self-signed toggle (TCP server at TLS level or above).
    fn show_self_signed(&self) -> bool {
        self.tls_shown()
            && self.role.get_value() == Role::Server
            && self.tls_level() >= TlsLevel::Tls
    }

    /// Client-only skip-verify toggle (TCP client at TLS level or above).
    fn show_skip_verify(&self) -> bool {
        self.tls_shown()
            && self.role.get_value() == Role::Client
            && self.tls_level() >= TlsLevel::Tls
    }

    /// Client trust-anchor input (TCP client at TLS level or above).
    fn show_ca_file(&self) -> bool {
        self.tls_shown()
            && self.role.get_value() == Role::Client
            && self.tls_level() >= TlsLevel::Tls
    }

    /// Server certificate/key inputs (TCP server at TLS level or above).
    fn show_server_cert(&self) -> bool {
        self.tls_shown()
            && self.role.get_value() == Role::Server
            && self.tls_level() >= TlsLevel::Tls
    }

    /// Client mTLS certificate/key inputs.
    fn show_client_cert(&self) -> bool {
        self.tls_shown()
            && self.role.get_value() == Role::Client
            && self.tls_level() == TlsLevel::MutualTls
    }

    /// Server mTLS client-CA input.
    fn show_client_ca(&self) -> bool {
        self.tls_shown()
            && self.role.get_value() == Role::Server
            && self.tls_level() == TlsLevel::MutualTls
    }

    /// First certificate row: server cert/key, or the client trust anchor.
    fn show_cert_row_a(&self) -> bool {
        self.show_ca_file() || self.show_server_cert()
    }

    /// Second certificate row: client mTLS cert/key, or the server client-CA.
    fn show_cert_row_b(&self) -> bool {
        self.show_client_cert() || self.show_client_ca()
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

        // TLS is hidden entirely for RTU (MB-R-112): report no value at all, so a save on an
        // RTU instance never clobbers a device config's existing `tls` setting.
        let tls = if matches!(endpoint, Endpoint::Tcp { .. }) {
            let level = self.tls_level.state.get_value();
            if level == TlsLevel::Off {
                Some(None)
            } else {
                let mut cfg = level.build_config(
                    role,
                    TlsInputs {
                        ca_file: self.ca_file.state.input(),
                        cert_file: self.cert_file.state.input(),
                        key_file: self.key_file.state.input(),
                        client_cert_file: self.client_cert_file.state.input(),
                        client_key_file: self.client_key_file.state.input(),
                        client_ca_file: self.client_ca_file.state.input(),
                    },
                );
                if role == Role::Server {
                    cfg.self_signed = self.self_signed.state.get_value() == SelfSignedChoice::On;
                }
                if role == Role::Client {
                    cfg.insecure_skip_verify =
                        self.skip_verify.state.get_value() == SkipVerifyChoice::On;
                }
                validate_tls(&cfg, role, level, &|p| std::path::Path::new(p).exists())?;
                Some(Some(cfg))
            }
        } else {
            None
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
            tls,
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
        let show_tls = self.tls_shown();
        let show_cert_row_a = self.show_cert_row_a();
        let show_cert_row_b = self.show_cert_row_b();
        let tls_rows: u16 = show_tls as u16 + show_cert_row_a as u16 + show_cert_row_b as u16;
        // border(2) + inner margin(2) + name(3) + device(3) + select(3) + endpoint + timing(3) + ranges(6)
        // + error(4) + keybinds(1) + optional config-path row (New mode) + optional TLS rows.
        let box_height = 27 + endpoint_rows * 3 + tls_rows * 3;

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

        let mut constraints = vec![
            Constraint::Length(3),                 // name
            Constraint::Length(3),                 // config path
            Constraint::Length(3),                 // transport + role
            Constraint::Length(endpoint_rows * 3), // endpoint
            Constraint::Length(3),                 // timeout + delay + interval
            Constraint::Length(3),                 // holding + input ranges
            Constraint::Length(3),                 // coil + discrete ranges
        ];
        if show_tls {
            constraints.push(Constraint::Length(3)); // TLS level (+ self-signed/skip-verify)
        }
        if show_cert_row_a {
            constraints.push(Constraint::Length(3)); // ca_file, or cert_file + key_file
        }
        if show_cert_row_b {
            constraints.push(Constraint::Length(3)); // client_cert_file + client_key_file, or client_ca_file
        }
        constraints.push(Constraint::Length(4)); // error
        constraints.push(Constraint::Length(1)); // keybinds
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

        if show_tls {
            let is_server = self.role.state.get_value() == Role::Server;
            let show_side = if is_server {
                self.show_self_signed()
            } else {
                self.show_skip_verify()
            };
            if show_side {
                let [level_area, side_area] =
                    Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .areas(rows[idx]);
                StatefulWidget::render(
                    &self.tls_level.widget,
                    level_area,
                    buf,
                    &mut self.tls_level.state,
                );
                if is_server {
                    StatefulWidget::render(
                        &self.self_signed.widget,
                        side_area,
                        buf,
                        &mut self.self_signed.state,
                    );
                } else {
                    StatefulWidget::render(
                        &self.skip_verify.widget,
                        side_area,
                        buf,
                        &mut self.skip_verify.state,
                    );
                }
            } else {
                StatefulWidget::render(
                    &self.tls_level.widget,
                    rows[idx],
                    buf,
                    &mut self.tls_level.state,
                );
            }
            idx += 1;
        }

        if show_cert_row_a {
            if self.show_ca_file() {
                StatefulWidget::render(
                    &self.ca_file.widget,
                    rows[idx],
                    buf,
                    &mut self.ca_file.state,
                );
            } else {
                render_suggest_pair(&mut self.cert_file, &mut self.key_file, rows[idx], buf);
            }
            idx += 1;
        }

        if show_cert_row_b {
            if self.show_client_cert() {
                render_suggest_pair(
                    &mut self.client_cert_file,
                    &mut self.client_key_file,
                    rows[idx],
                    buf,
                );
            } else {
                StatefulWidget::render(
                    &self.client_ca_file.widget,
                    rows[idx],
                    buf,
                    &mut self.client_ca_file.state,
                );
            }
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
        self.ca_file
            .widget
            .render_overlay(area, buf, &mut self.ca_file.state);
        self.cert_file
            .widget
            .render_overlay(area, buf, &mut self.cert_file.state);
        self.key_file
            .widget
            .render_overlay(area, buf, &mut self.key_file.state);
        self.client_cert_file
            .widget
            .render_overlay(area, buf, &mut self.client_cert_file.state);
        self.client_key_file
            .widget
            .render_overlay(area, buf, &mut self.client_key_file.state);
        self.client_ca_file
            .widget
            .render_overlay(area, buf, &mut self.client_ca_file.state);

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

/// Render two path-suggesting input fields side by side in `area`.
fn render_suggest_pair(
    left: &mut Widget<SuggestInputState<FsPathProvider>, SuggestInput<String, FsPathProvider>>,
    right: &mut Widget<SuggestInputState<FsPathProvider>, SuggestInput<String, FsPathProvider>>,
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
    /// UI-R-026 — the config-path field shows a completion popup.
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
    /// UI-R-024 — the setup dialog resolves a reconnect-off selection into the config value.
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
    /// UI-R-024 — a server-role setup resolves to no reconnect setting.
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
    /// UI-R-024 — Edit mode pre-fills the dialog from the existing config.
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
            None,
        );
        assert_eq!(dialog.reconnect.state.get_value(), ReconnectChoice::Off);
        let outcome = dialog.resolve().unwrap();
        assert_eq!(outcome.values.reconnect, Some(false));
    }

    #[test]
    /// UI-R-024 — a TCP setup dialog exposes the TLS section.
    fn ut_tcp_dialog_shows_tls_section() {
        let dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        assert_eq!(dialog.transport.state.get_value(), Transport::Tcp);
        assert!(dialog.tls_shown());
    }

    #[test]
    /// UI-R-024 — an RTU setup dialog never shows the TLS section (MB-R-112).
    fn ut_rtu_dialog_hides_tls_section() {
        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        dialog.transport.state.set_selection(1); // Rtu
        assert_eq!(dialog.transport.state.get_value(), Transport::Rtu);
        assert!(!dialog.tls_shown());
    }

    #[test]
    /// UI-R-024 — resolving an RTU dialog reports no TLS value at all, so applying it can never
    /// clobber a device config's existing `tls` setting (MB-R-112).
    fn ut_resolve_rtu_reports_no_tls() {
        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        set_input(&mut dialog.name, "dev");
        dialog.transport.state.set_selection(1); // Rtu
        set_suggest_input(&mut dialog.path, "/dev/ttyUSB0");
        let outcome = dialog.resolve().unwrap();
        assert_eq!(outcome.values.tls, None);
    }

    #[test]
    /// UI-R-024 — resolving a TCP dialog at TLS level Off reports an explicit no-TLS setting.
    fn ut_resolve_tcp_tls_off_reports_some_none() {
        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        set_input(&mut dialog.name, "dev");
        let outcome = dialog.resolve().unwrap();
        assert_eq!(outcome.values.tls, Some(None));
    }

    #[test]
    /// UI-R-024 — resolving a TCP dialog at TLS level Tls (server, self-signed) builds a config
    /// with `self_signed` set and drops the mTLS-only client-CA field.
    fn ut_resolve_tcp_tls_server_self_signed_builds_config() {
        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        set_input(&mut dialog.name, "dev");
        dialog.tls_level.state.set_selection(TlsLevel::Tls.index());
        dialog.self_signed.state.set_selection(1); // On
        set_suggest_input(&mut dialog.client_ca_file, "client_ca.pem");
        let outcome = dialog.resolve().unwrap();
        let cfg = outcome.values.tls.unwrap().unwrap();
        assert!(cfg.self_signed);
        assert_eq!(cfg.client_ca_file, None);
        assert!(!cfg.require_client_cert);
    }

    #[test]
    /// UI-R-022 — the focus cycle skips the reconnect field when it is disabled for a server role.
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
    /// UI-R-023 — Esc-then-Enter sets the close request, which clears after being taken.
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
    /// UI-R-014 — `:` types into a setup text field rather than entering command mode.
    fn ut_colon_in_text_input_types() {
        let mut dialog = SetupDialog::create(default_timing());
        // Default focus is Name, a free-text field; `:` is typed as ordinary text.
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Char(':'));
        assert_eq!(dialog.name.state.input(), ":");
    }

    #[test]
    /// UI-R-023 — Esc in the close-confirm keeps the setup dialog open.
    fn ut_esc_in_confirm_keeps_setup_open() {
        let mut dialog = SetupDialog::create(default_timing());
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(dialog.close_confirm.is_some());
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(dialog.close_confirm.is_none());
        assert!(!dialog.take_close_request());
    }

    #[test]
    /// UI-R-023 — Space in the close-confirm closes the setup dialog.
    fn ut_space_in_confirm_closes() {
        let mut dialog = SetupDialog::create(default_timing());
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(dialog.close_confirm.is_some());
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Char(' '));
        assert!(dialog.take_close_request());
    }
}
