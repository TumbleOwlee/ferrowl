//! Module setup dialog. In **Edit** mode (`:e`) it edits the current tab's per-instance
//! settings (name, transport + endpoint, role). In **New** mode (`:n`/`:new`) it additionally
//! takes an optional device-config path: empty creates an empty module, otherwise the path is
//! validated live and must point at a loadable config. While any field is invalid the dialog
//! cannot be confirmed (only cancelled with Esc).

use crossterm::event::{KeyCode, KeyModifiers};
use derive_builder::Builder;
use ferrowl_ui::{
    Border, COLOR_SCHEME, EventResult, render_field, render_row,
    state::{
        ButtonState, InputFieldState, InputFieldStateBuilder, SelectionState,
        SelectionStateBuilder, SuggestInputState, SuggestInputStateBuilder,
    },
    style::{ButtonStyle, InputFieldStyle, SelectionStyle, TextStyle},
    traits::{HandleEvents, SetFocus, ToLabel},
    widgets::{
        Button, GetValue, InputField, InputFieldBuilder, Selection, SelectionBuilder, SuggestInput,
        SuggestInputBuilder, Text, TextBuilder, Validate, ValidateResult, Widget,
    },
};
use ferrowl_ui_derive::{Focus, focusable};
use ferrowl_util::convert::FileType;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, Clear, Widget as UiWidget},
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
    /// Auto-reconnect setting (MB-R-050–MB-R-055 client, MB-R-130–MB-R-134 server); always
    /// explicit after a dialog save.
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
    /// TLS level, offered for TCP, RtuOverTcp (MB-R-115), and AsciiOverTcp (MB-R-127) — not
    /// RTU or Ascii (MB-R-112/121: neither carries a `tls` field at all).
    #[focus(when = {matches!(self.transport.get_value(), Transport::Tcp | Transport::RtuOverTcp | Transport::AsciiOverTcp)})]
    pub tls_level: Widget<SelectionState<TlsLevel>, Selection<TlsLevel>>,
    #[focus]
    pub role: Widget<SelectionState<Role>, Selection<Role>>,
    /// Server: "generate an ephemeral self-signed server certificate" toggle (shown at TLS+).
    /// Client, at mTLS only: "generate an ephemeral self-signed client identity" toggle
    /// (MB-R-139) — the same widget field backs both, since only one role is ever active for a
    /// given dialog instance.
    #[focus(when = {self.show_self_signed()})]
    pub self_signed: Widget<SelectionState<SelfSignedChoice>, Selection<SelfSignedChoice>>,
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
    /// Server-only, at mTLS only: "accept any client certificate" toggle (MB-R-136). On hides
    /// the client-CA list below and excludes it from the resolved config; the list's own text is
    /// preserved (never cleared) so toggling back Off restores it.
    #[focus(when = {self.show_client_cert_skip_verify()})]
    pub client_cert_skip_verify:
        Widget<SelectionState<SkipVerifyChoice>, Selection<SkipVerifyChoice>>,
    /// Client-only "accept any server certificate" toggle.
    #[focus(when = {self.show_skip_verify()})]
    pub skip_verify: Widget<SelectionState<SkipVerifyChoice>, Selection<SkipVerifyChoice>>,
    /// Client-only extra trust anchor for a self-signed server certificate.
    #[focus(when = {self.show_ca_file()})]
    pub ca_file: Widget<SuggestInputState<FsPathProvider>, SuggestInput<String, FsPathProvider>>,
    /// Server-only list of CAs used to verify client certificates under mTLS (MB-R-136) — a
    /// certificate signed by any one is sufficient. An add/remove list (`Selection<String>`
    /// browses/selects the current entries; `client_ca_add_button`/`client_ca_delete_button` add
    /// via `client_ca_add_dialog` or remove the selected entry), mirroring the register named-
    /// value editor's add/remove list interaction. Selecting mTLS as server implies
    /// `ServerTlsPolicy::MutualTls` in the resolved config (unless `client_cert_skip_verify` is
    /// on, in which case this list is ignored).
    #[focus(when = {self.show_client_ca() && !self.client_ca_files.state.values().is_empty()})]
    pub client_ca_files: Widget<SelectionState<String>, Selection<String>>,
    #[focus(when = {self.show_client_ca()})]
    pub client_ca_add_button: Widget<ButtonState, Button>,
    #[focus(when = {self.show_client_ca() && !self.client_ca_files.state.values().is_empty()})]
    pub client_ca_delete_button: Widget<ButtonState, Button>,
    /// Sub-dialog for adding one path to `client_ca_files`, opened by `client_ca_add_button`;
    /// not itself a `#[focus]` field — routed specially in `handle_events` while open, mirroring
    /// `close_confirm`.
    #[builder(default)]
    pub client_ca_add_dialog: Option<crate::dialog::ca_file_list::AddCaFileDialog>,
    #[focus(when = {matches!(self.transport.get_value(), Transport::Tcp | Transport::RtuOverTcp | Transport::Udp | Transport::AsciiOverTcp)})]
    pub ip: Widget<InputFieldState, InputField<String>>,
    #[focus(when = {matches!(self.transport.get_value(), Transport::Tcp | Transport::RtuOverTcp | Transport::Udp | Transport::AsciiOverTcp)})]
    pub port: Widget<InputFieldState, InputField<String>>,
    #[focus(when = {matches!(self.transport.get_value(), Transport::Rtu | Transport::Ascii)})]
    pub path: Widget<SuggestInputState<FsPathProvider>, SuggestInput<String, FsPathProvider>>,
    #[focus(when = {matches!(self.transport.get_value(), Transport::Rtu | Transport::Ascii)})]
    pub baud: Widget<InputFieldState, InputField<String>>,
    /// Governs client redial (MB-R-050–055) and server bind/serial-open/mid-serve retry
    /// (MB-R-130–134); shown for every role and transport, next to Port or Baud.
    #[focus]
    pub reconnect: Widget<SelectionState<ReconnectChoice>, Selection<ReconnectChoice>>,
    #[focus(when = {matches!(self.transport.get_value(), Transport::Rtu | Transport::Ascii)})]
    pub parity: Widget<SelectionState<Parity>, Selection<Parity>>,
    #[focus(when = {matches!(self.transport.get_value(), Transport::Rtu | Transport::Ascii)})]
    pub data_bits: Widget<SelectionState<U8Choice>, Selection<U8Choice>>,
    #[focus(when = {matches!(self.transport.get_value(), Transport::Rtu | Transport::Ascii)})]
    pub stop_bits: Widget<SelectionState<U8Choice>, Selection<U8Choice>>,
    #[focus]
    pub timeout: Widget<InputFieldState, InputField<String>>,
    #[focus]
    pub delay: Widget<InputFieldState, InputField<String>>,
    #[focus]
    pub interval: Widget<InputFieldState, InputField<String>>,
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
    /// The original device's `tls` config, if any (Edit mode only). On save, the half of
    /// `ModbusTlsConfig` belonging to the *inactive* role is stitched back in from here, so a
    /// role toggle preserves the other role's previously-saved TLS settings instead of resetting
    /// them to `ModbusTlsConfig::default()`'s placeholder.
    #[builder(default)]
    original_tls: Option<ModbusTlsConfig>,
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
        // Tcp=0, Rtu=1, RtuOverTcp=2, Udp=3, Ascii=4, AsciiOverTcp=5
        match endpoint {
            Endpoint::Tcp { ip, port } => {
                dialog.transport.state.set_selection(0);
                set_input(&mut dialog.ip, ip);
                set_input(&mut dialog.port, &port.to_string());
            }
            Endpoint::RtuOverTcp { ip, port } => {
                dialog.transport.state.set_selection(2);
                set_input(&mut dialog.ip, ip);
                set_input(&mut dialog.port, &port.to_string());
            }
            Endpoint::Udp { ip, port } => {
                dialog.transport.state.set_selection(3);
                set_input(&mut dialog.ip, ip);
                set_input(&mut dialog.port, &port.to_string());
            }
            Endpoint::AsciiOverTcp { ip, port } => {
                dialog.transport.state.set_selection(5);
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
            Endpoint::Ascii {
                path,
                baud_rate,
                parity,
                data_bits,
                stop_bits,
            } => {
                dialog.transport.state.set_selection(4);
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
            use ferrowl_util::tls::{
                ClientCertSource, ClientCertVerification, ClientTlsPolicy, ClientVerification,
                ServerCertSource, ServerTlsPolicy,
            };

            let level = TlsLevel::from_config(tls, role);
            dialog.tls_level.state.set_selection(level.index());
            dialog.original_tls = Some(tls.clone());

            match role {
                Role::Server => {
                    let (server_cert, client_verification) = match &tls.server {
                        ServerTlsPolicy::MutualTls {
                            server_cert,
                            client_verification,
                        } => (server_cert.clone(), Some(client_verification.clone())),
                        ServerTlsPolicy::Tls { server_cert } => (server_cert.clone(), None),
                        ServerTlsPolicy::NoTls => (ServerCertSource::Unset, None),
                    };
                    dialog.self_signed.state.set_selection(
                        if server_cert == ServerCertSource::SelfSigned {
                            1
                        } else {
                            0
                        },
                    );
                    let (cert_file, key_file) = match &server_cert {
                        ServerCertSource::Explicit {
                            cert_file,
                            key_file,
                        } => (cert_file.as_str(), key_file.as_str()),
                        _ => ("", ""),
                    };
                    set_suggest_input(&mut dialog.cert_file, cert_file);
                    set_suggest_input(&mut dialog.key_file, key_file);
                    let (ca_files, skip) = match &client_verification {
                        Some(ClientCertVerification::Verify { ca_files }) => {
                            (ca_files.clone(), false)
                        }
                        Some(ClientCertVerification::SkipVerify) => (Vec::new(), true),
                        None => (Vec::new(), false),
                    };
                    *dialog.client_ca_files.state.values_mut() = ca_files;
                    dialog.client_ca_files.state.set_selection(0);
                    dialog
                        .client_cert_skip_verify
                        .state
                        .set_selection(if skip { 1 } else { 0 });
                }
                Role::Client => {
                    let (client_verification, client_identity) = match &tls.client {
                        ClientTlsPolicy::MutualTls {
                            client_verification,
                            client_identity,
                        } => (client_verification.clone(), Some(client_identity.clone())),
                        ClientTlsPolicy::Tls {
                            client_verification,
                        } => (client_verification.clone(), None),
                        ClientTlsPolicy::NoTls => {
                            (ClientVerification::Verify { ca_file: None }, None)
                        }
                    };
                    dialog.skip_verify.state.set_selection(
                        if client_verification == ClientVerification::SkipVerify {
                            1
                        } else {
                            0
                        },
                    );
                    let ca_file = match &client_verification {
                        ClientVerification::Verify { ca_file } => ca_file.as_deref().unwrap_or(""),
                        ClientVerification::SkipVerify => "",
                    };
                    set_suggest_input(&mut dialog.ca_file, ca_file);
                    let self_signed_client =
                        matches!(client_identity, Some(ClientCertSource::SelfSigned));
                    dialog
                        .self_signed
                        .state
                        .set_selection(if self_signed_client { 1 } else { 0 });
                    let (ccert, ckey) = match &client_identity {
                        Some(ClientCertSource::Explicit {
                            client_cert_file,
                            client_key_file,
                        }) => (client_cert_file.as_str(), client_key_file.as_str()),
                        _ => ("", ""),
                    };
                    set_suggest_input(&mut dialog.client_cert_file, ccert);
                    set_suggest_input(&mut dialog.client_key_file, ckey);
                }
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
                // Tcp=0, Rtu=1, RtuOverTcp=2, Udp=3, Ascii=4, AsciiOverTcp=5 — each appended
                // last to keep prior indices stable rather than reordering around them.
                vec![
                    Transport::Tcp,
                    Transport::Rtu,
                    Transport::RtuOverTcp,
                    Transport::Udp,
                    Transport::Ascii,
                    Transport::AsciiOverTcp,
                ],
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
                Some(HorizontalAlignment::Center),
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
                Some(HorizontalAlignment::Center),
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
                Some(HorizontalAlignment::Center),
                vec![U8Choice(8), U8Choice(7), U8Choice(6), U8Choice(5)],
                &selection_style,
            ))
            .stop_bits(selection(
                "Stop Bits",
                Some(HorizontalAlignment::Right),
                vec![U8Choice(1), U8Choice(2)],
                &selection_style,
            ))
            .tls_level(selection(
                "TLS",
                Some(HorizontalAlignment::Center),
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
            .client_cert_skip_verify(selection(
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
            .client_ca_files(selection(
                "Client CA(s)",
                None,
                Vec::<String>::new(),
                &selection_style,
            ))
            .client_ca_add_button(ferrowl_ui::widgets::button(
                "ADD",
                ButtonStyle::default(),
                1,
            ))
            .client_ca_delete_button(ferrowl_ui::widgets::button(
                "DEL",
                ButtonStyle::default(),
                1,
            ))
            .timeout(input("Timeout ms", None, "", &input_style, false))
            .delay(input("Delay ms", None, "", &input_style, false))
            .interval(input("Interval ms", None, "", &input_style, false))
            .reconnect(selection(
                "Reconnect",
                Some(HorizontalAlignment::Right),
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

    /// The TLS level selection row (TCP, RtuOverTcp (MB-R-115), or AsciiOverTcp (MB-R-127);
    /// never RTU or Ascii, MB-R-112/121).
    fn tls_shown(&self) -> bool {
        matches!(
            self.transport.get_value(),
            Transport::Tcp | Transport::RtuOverTcp | Transport::AsciiOverTcp
        ) && self.tls_level.get_value() != TlsLevel::Off
    }

    /// The currently selected TLS level.
    fn tls_level(&self) -> TlsLevel {
        self.tls_level.get_value()
    }

    /// Server: self-signed server-certificate toggle (TLS level or above). Client, at mTLS
    /// only: self-signed client-identity toggle (MB-R-139) — same widget, different meaning per
    /// role (see the field's doc comment).
    fn show_self_signed(&self) -> bool {
        self.tls_shown()
            && ((self.role.get_value() == Role::Server && self.tls_level() >= TlsLevel::Tls)
                || (self.role.get_value() == Role::Client
                    && self.tls_level() == TlsLevel::MutualTls))
    }

    /// Client-only skip-verify toggle (TCP client at TLS level or above).
    fn show_skip_verify(&self) -> bool {
        self.tls_shown()
            && self.role.get_value() == Role::Client
            && self.tls_level() >= TlsLevel::Tls
    }

    /// Server-only, mTLS only: "accept any client certificate" toggle (MB-R-136).
    fn show_client_cert_skip_verify(&self) -> bool {
        self.tls_shown()
            && self.role.get_value() == Role::Server
            && self.tls_level() == TlsLevel::MutualTls
    }

    /// Client trust-anchor input (TCP client at TLS level or above).
    fn show_ca_file(&self) -> bool {
        self.tls_shown()
            && self.role.get_value() == Role::Client
            && self.tls_level() >= TlsLevel::Tls
            && self.skip_verify.get_value() == SkipVerifyChoice::Off
    }

    /// Server certificate/key inputs (TCP server at TLS level or above).
    fn show_server_cert(&self) -> bool {
        self.tls_shown()
            && self.role.get_value() == Role::Server
            && self.tls_level() >= TlsLevel::Tls
            && self.self_signed.get_value() == SelfSignedChoice::Off
    }

    /// Client mTLS certificate/key inputs — hidden when the client's self-signed-identity
    /// toggle is on (MB-R-139), mirroring the server's `show_server_cert`.
    fn show_client_cert(&self) -> bool {
        self.tls_shown()
            && self.role.get_value() == Role::Client
            && self.tls_level() == TlsLevel::MutualTls
            && self.self_signed.get_value() == SelfSignedChoice::Off
    }

    /// Server mTLS client-CA list input — hidden when `client_cert_skip_verify` is on
    /// (MB-R-136), preserving the list's own text so toggling back Off restores it.
    fn show_client_ca(&self) -> bool {
        self.tls_shown()
            && self.role.get_value() == Role::Server
            && self.tls_level() == TlsLevel::MutualTls
            && self.client_cert_skip_verify.get_value() == SkipVerifyChoice::Off
    }

    /// Row 3 (skip-verify toggle): server's `client_cert_skip_verify`, or the client's
    /// `skip_verify` — exactly one applies for a given role.
    fn show_skip_verify_row(&self) -> bool {
        self.show_client_cert_skip_verify() || self.show_skip_verify()
    }

    /// Row 2 (own-identity cert/key pair): server's `cert_file`/`key_file`, or the client's
    /// `client_cert_file`/`client_key_file` — exactly one applies for a given role.
    fn show_identity_row(&self) -> bool {
        self.show_server_cert() || self.show_client_cert()
    }

    /// Row 4 (peer-verification input): server's `client_ca_files` list, or the client's
    /// `ca_file` — exactly one applies for a given role.
    fn show_peer_verify_row(&self) -> bool {
        self.show_client_ca() || self.show_ca_file()
    }

    /// Route a key: the close-confirm popup captures all keys while open; then the client-CA
    /// add-dialog (MB-R-136), if open; then the client-CA ADD/DEL buttons (Enter/Space); Esc
    /// (with nothing else open) opens the close-confirm popup; everything else falls through to
    /// the derived per-field routing.
    pub fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        match route_close_confirm(&mut self.close_confirm, modifiers, code) {
            CloseConfirmOutcome::NotActive => {}
            CloseConfirmOutcome::Close => {
                self.close_requested = true;
                return EventResult::Consumed;
            }
            CloseConfirmOutcome::Consumed => return EventResult::Consumed,
        }

        if let Some(dialog) = self.client_ca_add_dialog.as_mut() {
            match (modifiers, code) {
                (KeyModifiers::NONE, KeyCode::Esc) => {
                    self.client_ca_add_dialog = None;
                }
                // While the path field's completion popup is open, Enter accepts the
                // highlighted suggestion (mirroring every other suggest-input field) rather
                // than submitting the sub-dialog.
                (KeyModifiers::NONE, KeyCode::Enter) if dialog.path.state.suggestions_open() => {
                    let _ = dialog.path.state.handle_events(modifiers, code);
                }
                (KeyModifiers::NONE, KeyCode::Enter) => match dialog.apply() {
                    Ok(path) => {
                        self.client_ca_files.state.values_mut().push(path);
                        let idx = self.client_ca_files.state.values().len() - 1;
                        self.client_ca_files.state.set_selection(idx);
                        self.client_ca_add_dialog = None;
                    }
                    Err(e) => dialog.error.state = e,
                },
                _ => {
                    let _ = dialog.path.state.handle_events(modifiers, code);
                }
            }
            return EventResult::Consumed;
        }

        if modifiers == KeyModifiers::NONE && matches!(code, KeyCode::Enter | KeyCode::Char(' ')) {
            match self.focus {
                SetupDialogFocus::ClientCaAddButton => {
                    self.client_ca_add_dialog =
                        Some(crate::dialog::ca_file_list::AddCaFileDialog::new());
                    return EventResult::Consumed;
                }
                SetupDialogFocus::ClientCaDeleteButton => {
                    self.delete_selected_client_ca();
                    return EventResult::Consumed;
                }
                _ => {}
            }
        }

        if modifiers == KeyModifiers::NONE && code == KeyCode::Esc {
            self.close_confirm = Some(CloseConfirmDialog::new());
            return EventResult::Consumed;
        }

        <Self as HandleEvents>::handle_events(self, modifiers, code)
    }

    /// Remove the currently-selected client-CA entry (MB-R-136), if any, adjusting the
    /// selection cursor to stay in bounds.
    fn delete_selected_client_ca(&mut self) {
        let idx = self.client_ca_files.state.selection();
        let vals = self.client_ca_files.state.values_mut();
        if vals.is_empty() {
            return;
        }
        vals.remove(idx);
        let new_len = self.client_ca_files.state.values().len();
        let new_idx = if new_len == 0 {
            0
        } else {
            idx.min(new_len - 1)
        };
        self.client_ca_files.state.set_selection(new_idx);
        if new_len == 0 {
            // DEL is no longer eligible (`#[focus(when = ...)]` excludes it once the list is
            // empty) — leaving `self.focus` pointed at it would strand further Tab navigation on
            // a dead target, so fall back to ADD. Must also move the widget-level highlight (not
            // just the tracking enum), mirroring what `focus_next`/`focus_previous` do on every
            // other transition — otherwise DEL stays visually focused (though hidden) and ADD
            // stays unhighlighted until the next real Tab press.
            self.client_ca_delete_button.state.set_focused(false);
            self.focus = SetupDialogFocus::ClientCaAddButton;
            self.client_ca_add_button.state.set_focused(true);
        }
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
            Transport::RtuOverTcp => {
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
                Endpoint::RtuOverTcp { ip, port }
            }
            Transport::Udp => {
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
                Endpoint::Udp { ip, port }
            }
            Transport::AsciiOverTcp => {
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
                Endpoint::AsciiOverTcp { ip, port }
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
            Transport::Ascii => {
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
                Endpoint::Ascii {
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
        // Reconnect is shown for every role: it governs client redial (MB-R-050–055) and, since
        // the shared backoff driver, server bind/serial-open/mid-serve retry (MB-R-130–134).
        let reconnect = Some(self.reconnect.state.get_value() == ReconnectChoice::On);
        let read_ranges = ReadRanges {
            holding: opt(self.holding_ranges.state.input()),
            input: opt(self.input_ranges.state.input()),
            coils: opt(self.coil_ranges.state.input()),
            discrete: opt(self.discrete_ranges.state.input()),
        };

        // TLS is hidden entirely for RTU (MB-R-112) and Ascii (MB-R-121): report no value at
        // all, so a save on an RTU/Ascii instance never clobbers a device config's existing
        // `tls` setting — neither carries a `tls` field. RtuOverTcp carries TLS exactly like
        // Tcp (MB-R-115), and so does AsciiOverTcp (MB-R-127).
        let tls = if matches!(
            endpoint,
            Endpoint::Tcp { .. } | Endpoint::RtuOverTcp { .. } | Endpoint::AsciiOverTcp { .. }
        ) {
            let level = self.tls_level.state.get_value();
            if level == TlsLevel::Off {
                Some(None)
            } else {
                // MB-R-135/136/139: `build_config` resolves the active role's policy directly
                // from the raw text together with the toggle widgets (self_signed/skip_verify/
                // client_cert_skip_verify), so a toggle excludes stale text from the resolved
                // config rather than layering a flag on top of whatever raw text happened to be
                // present.
                let mut cfg = level.build_config(
                    role,
                    TlsInputs {
                        ca_file: self.ca_file.state.input(),
                        cert_file: self.cert_file.state.input(),
                        key_file: self.key_file.state.input(),
                        client_cert_file: self.client_cert_file.state.input(),
                        client_key_file: self.client_key_file.state.input(),
                        client_ca_files: self.client_ca_files.state.values(),
                        self_signed: self.self_signed.state.get_value() == SelfSignedChoice::On,
                        skip_verify: self.skip_verify.state.get_value() == SkipVerifyChoice::On,
                        client_cert_skip_verify: self.client_cert_skip_verify.state.get_value()
                            == SkipVerifyChoice::On,
                    },
                )?;
                // Stitch the inactive role's half back in from the original config (if any), so
                // a role toggle preserves the other role's previously-saved TLS settings instead
                // of resetting them to `ModbusTlsConfig::default()`'s placeholder.
                if let Some(orig) = &self.original_tls {
                    match role {
                        Role::Server => cfg.client = orig.client.clone(),
                        Role::Client => cfg.server = orig.server.clone(),
                    }
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
        // Deliberately not `!= Transport::Tcp`: RtuOverTcp and AsciiOverTcp use the 1-row
        // TCP-shaped ip/port layout, not RTU's/Ascii's 2-row serial layout, so both must stay
        // excluded from `is_rtu` here, while Ascii must be included alongside Rtu.
        let is_rtu = matches!(
            self.transport.state.get_value(),
            Transport::Rtu | Transport::Ascii
        );
        let is_udp = matches!(self.transport.state.get_value(), Transport::Udp);
        // RTU needs two endpoint rows (path/baud, parity/data-bits/stop-bits); TCP one.
        let endpoint_rows: u16 = if is_rtu { 2 } else { 1 };
        let show_tls = self.tls_shown();
        // Fixed 4-slot TLS row order (both roles): Self-Signed, own-identity cert/key pair,
        // Skip Verify, peer-verification input — each flag already folds in `tls_shown()`, so a
        // row's absence (e.g. the client's Self-Signed row outside mTLS) simply isn't budgeted.
        let show_self_signed_row = self.show_self_signed();
        let show_identity_row = self.show_identity_row();
        let show_skip_verify_row = self.show_skip_verify_row();
        let show_peer_verify_row = self.show_peer_verify_row();
        let tls_rows: u16 = show_self_signed_row as u16
            + show_identity_row as u16
            + show_skip_verify_row as u16
            + show_peer_verify_row as u16;
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
        if show_self_signed_row {
            constraints.push(Constraint::Length(3)); // Self-Signed
        }
        if show_identity_row {
            constraints.push(Constraint::Length(3)); // own-identity cert/key pair
        }
        if show_skip_verify_row {
            constraints.push(Constraint::Length(3)); // Skip Verify
        }
        if show_peer_verify_row {
            constraints.push(Constraint::Length(3)); // peer-verification input (CA file, or client-CA list)
        }
        constraints.push(Constraint::Length(4)); // error
        constraints.push(Constraint::Length(1)); // keybinds
        let rows = Layout::vertical(constraints).split(inner);

        let mut idx = 0;
        render_field!(self, name, rows[idx], buf);
        idx += 1;

        render_field!(self, config_path, rows[idx], buf);
        idx += 1;

        if is_rtu || is_udp {
            render_row!(self, rows[idx], buf; transport, role);
        } else {
            render_row!(self, rows[idx], buf;
                transport => Constraint::Percentage(40),
                tls_level => Constraint::Percentage(20),
                role => Constraint::Percentage(40)
            );
        }
        idx += 1;

        if show_tls {
            let is_server = self.role.state.get_value() == Role::Server;

            // Row 1: Self-Signed (both roles).
            if show_self_signed_row {
                render_field!(self, self_signed, rows[idx], buf);
                idx += 1;
            }

            // Row 2: own-identity cert/key pair.
            if show_identity_row {
                if is_server {
                    render_row!(self, rows[idx], buf; cert_file, key_file);
                } else {
                    render_row!(self, rows[idx], buf; client_cert_file, client_key_file);
                }
                idx += 1;
            }

            // Row 3: Skip Verify.
            if show_skip_verify_row {
                if is_server {
                    render_field!(self, client_cert_skip_verify, rows[idx], buf);
                } else {
                    render_field!(self, skip_verify, rows[idx], buf);
                }
                idx += 1;
            }

            // Row 4: peer-verification input.
            if show_peer_verify_row {
                if is_server {
                    // No client-CA entries yet: give ADD the row's full remaining width and
                    // skip DEL entirely rather than paint an empty, nothing-to-delete button.
                    if self.client_ca_files.state.values().is_empty() {
                        // Hidden button shouldn't be focused.
                        if self.focus == SetupDialogFocus::ClientCaDeleteButton {
                            self.focus_previous();
                        }
                        render_row!(self, rows[idx], buf;
                            client_ca_files => Constraint::Percentage(80),
                            client_ca_add_button => Constraint::Fill(1)
                        );
                    } else {
                        render_row!(self, rows[idx], buf;
                            client_ca_files => Constraint::Percentage(60),
                            client_ca_add_button => Constraint::Percentage(20),
                            client_ca_delete_button => Constraint::Fill(1)
                        );
                    }
                } else {
                    render_field!(self, ca_file, rows[idx], buf);
                }
                idx += 1;
            }
        }

        let endpoint_area = rows[idx];
        idx += 1;
        if is_rtu {
            let [row0, row1] = Layout::vertical([Constraint::Length(3), Constraint::Length(3)])
                .areas(endpoint_area);
            render_row!(self, row0, buf; path, baud, reconnect);
            render_row!(self, row1, buf;
                parity => Constraint::Percentage(35),
                data_bits => Constraint::Percentage(30),
                stop_bits => Constraint::Percentage(35)
            );
        } else {
            render_row!(self, endpoint_area, buf; ip, port, reconnect);
        }

        render_row!(self, rows[idx], buf; timeout, delay, interval);
        idx += 1;

        render_row!(self, rows[idx], buf; holding_ranges, input_ranges);
        idx += 1;
        render_row!(self, rows[idx], buf; coil_ranges, discrete_ranges);
        idx += 1;

        let error_area = rows[idx];
        idx += 1;
        if !self.error.state.is_empty() {
            render_field!(self, error, error_area, buf);
        }

        render_field!(self, keybinds, rows[idx], buf);

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
        // `client_ca_files` is a `Selection`, not a `SuggestInput` — no completion overlay.

        if let Some(d) = self.client_ca_add_dialog.as_mut() {
            d.render(area, buf);
        }

        if let Some(d) = self.close_confirm.as_mut() {
            d.render(vcenter, buf);
        }
    }
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
    use ferrowl_ui::traits::IsFocus;

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
    /// MB-R-130, MB-R-134 — a server role also reports a reconnect setting: server-side
    /// reconnect (bind/serial-open/mid-serve retry) is governed by the same config field as
    /// the client's, so the dialog must resolve a value for it regardless of role.
    fn ut_resolve_server_role_reports_reconnect() {
        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        set_input(&mut dialog.name, "dev");
        // Default role is Server; default reconnect selection is On.
        let outcome = dialog.resolve().unwrap();
        assert_eq!(outcome.values.reconnect, Some(true));

        dialog.reconnect.state.set_selection(1); // Off
        let outcome = dialog.resolve().unwrap();
        assert_eq!(outcome.values.reconnect, Some(false));
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
    /// UI-R-024 — a TCP setup dialog always offers the TLS level selector, but the detail
    /// section (self-signed/cert/etc.) only appears once a level above Off is actually picked
    /// — the level selector alone must never imply the rest of the section is showing.
    fn ut_tcp_dialog_shows_tls_level_selector_but_not_detail_section_at_off() {
        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        assert_eq!(dialog.transport.state.get_value(), Transport::Tcp);
        assert_eq!(dialog.tls_level.state.get_value(), TlsLevel::Off);
        // Level selector itself: always rendered for TCP, regardless of the chosen level.
        let area = Rect::new(0, 0, 80, 60);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
        let text = buffer_text(&buf);
        assert!(text.contains("TLS"), "missing TLS level selector:\n{text}");
        // Detail section: hidden at Off, so the toggle/cert rows aren't allocated or drawn.
        assert!(!dialog.tls_shown());
        assert!(
            !text.contains("Self-Signed"),
            "self-signed toggle shown at TLS level Off:\n{text}"
        );
    }

    #[test]
    /// UI-R-024 — picking a TLS level above Off reveals the detail section (MB-R-104's fields
    /// become settable only once the user has actually opted into TLS).
    fn ut_tls_shown_once_level_selected_above_off() {
        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        assert!(!dialog.tls_shown());
        dialog.tls_level.state.set_selection(TlsLevel::Tls.index());
        assert!(dialog.tls_shown());
    }

    #[test]
    /// UI-R-024 — a server that turns on Self-Signed no longer needs (or shows) the
    /// cert/key file row; toggling it back off restores the row.
    fn ut_self_signed_hides_server_cert_row() {
        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        // Default role is Server.
        dialog.tls_level.state.set_selection(TlsLevel::Tls.index());
        assert!(dialog.show_identity_row());
        dialog.self_signed.state.set_selection(1); // On
        assert!(!dialog.show_identity_row());
        dialog.self_signed.state.set_selection(0); // Off
        assert!(dialog.show_identity_row());
    }

    #[test]
    /// UI-R-024 — a client that turns on Skip Verify no longer needs (or shows) the CA-file
    /// row; toggling it back off restores the row.
    fn ut_skip_verify_hides_ca_file_row() {
        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        dialog.role.state.set_selection(1); // Client
        dialog.tls_level.state.set_selection(TlsLevel::Tls.index());
        assert!(dialog.show_peer_verify_row());
        dialog.skip_verify.state.set_selection(1); // On
        assert!(!dialog.show_peer_verify_row());
        dialog.skip_verify.state.set_selection(0); // Off
        assert!(dialog.show_peer_verify_row());
    }

    fn row_of(buf: &Buffer, needle: &str) -> u16 {
        let text = buffer_text(buf);
        text.lines()
            .position(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} not found in:\n{text}")) as u16
    }

    #[test]
    /// UI-R-024 — with an empty client-CA list, the `client_ca_files` box still occupies the
    /// full fixed 3-row slot (top border + content + bottom border) rather than shrinking to a
    /// 2-row box (the `Selection` widget's default self-sizing collapses a 0-entry list to
    /// `height = entries + border`, i.e. 2 rows, leaving its bottom border misaligned with the
    /// ADD button's own 3-row box beside it).
    fn ut_client_ca_empty_list_box_keeps_full_row_height() {
        let mut dialog = SetupDialog::create(default_timing());
        set_input(&mut dialog.name, "dev");
        dialog.role.state.set_selection(0); // Server
        dialog
            .tls_level
            .state
            .set_selection(TlsLevel::MutualTls.index());
        assert!(dialog.client_ca_files.state.values().is_empty());
        let area = Rect::new(0, 0, 80, 60);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
        let ca_row = row_of(&buf, "Client CA(s)");
        let bottom_line: String = (0..buf.area.width)
            .map(|x| buf[(x, ca_row + 2)].symbol().to_string())
            .collect();
        assert!(
            bottom_line.contains('└'),
            "empty client-CA box's bottom border is missing from its third row \
             (box collapsed to 2 rows instead of the fixed 3):\n{bottom_line}"
        );
    }

    #[test]
    /// UI-R-024 — mTLS row order, server role: Self-Signed first, then the server's own
    /// cert/key pair, then Skip Verify, then the client-CA list (post-gate3 layout refinement).
    fn ut_mtls_row_order_server() {
        let mut dialog = SetupDialog::create(default_timing());
        dialog.role.state.set_selection(0); // Server
        dialog
            .tls_level
            .state
            .set_selection(TlsLevel::MutualTls.index());
        let area = Rect::new(0, 0, 80, 60);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
        let self_signed_row = row_of(&buf, "Self-Signed");
        let cert_row = row_of(&buf, "Cert File");
        let skip_row = row_of(&buf, "Skip Verify");
        let ca_row = row_of(&buf, "Client CA(s)");
        assert!(
            self_signed_row < cert_row,
            "self-signed must render before cert/key"
        );
        assert!(
            cert_row < skip_row,
            "cert/key must render before skip-verify"
        );
        assert!(
            skip_row < ca_row,
            "skip-verify must render before client-CA list"
        );
    }

    #[test]
    /// UI-R-024 — mTLS row order, client role: Self-Signed first, then the client's own
    /// cert/key pair, then Skip Verify, then the CA-file input (post-gate3 layout refinement).
    fn ut_mtls_row_order_client() {
        let mut dialog = SetupDialog::create(default_timing());
        dialog.role.state.set_selection(1); // Client
        dialog
            .tls_level
            .state
            .set_selection(TlsLevel::MutualTls.index());
        let area = Rect::new(0, 0, 80, 60);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
        let self_signed_row = row_of(&buf, "Self-Signed");
        let cert_row = row_of(&buf, "Client Cert");
        let skip_row = row_of(&buf, "Skip Verify");
        let ca_row = row_of(&buf, "CA File");
        assert!(
            self_signed_row < cert_row,
            "self-signed must render before client cert/key"
        );
        assert!(
            cert_row < skip_row,
            "client cert/key must render before skip-verify"
        );
        assert!(
            skip_row < ca_row,
            "skip-verify must render before the CA-file input"
        );
    }

    #[test]
    /// UI-R-024 — an empty client-CA list shows no placeholder entry, and the DEL button is
    /// not rendered at all (nothing eligible to delete), so ADD gets the row's full width.
    fn ut_client_ca_empty_hides_delete_button() {
        let mut dialog = SetupDialog::create(default_timing());
        dialog.role.state.set_selection(0); // Server
        dialog
            .tls_level
            .state
            .set_selection(TlsLevel::MutualTls.index());
        assert!(dialog.client_ca_files.state.values().is_empty());
        let area = Rect::new(0, 0, 80, 60);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
        let text = buffer_text(&buf);
        assert!(
            !text.contains("DEL"),
            "DEL button rendered with an empty client-CA list:\n{text}"
        );
        assert!(text.contains("ADD"), "ADD button missing:\n{text}");
    }

    #[test]
    /// UI-R-024 — the client-CA row's ADD/DEL buttons hug the dialog's right inner edge with
    /// no trailing dead space, matching every other full-width row in the dialog.
    fn ut_client_ca_delete_button_hugs_right_edge() {
        let mut dialog = SetupDialog::create(default_timing());
        set_input(&mut dialog.name, "dev");
        dialog.role.state.set_selection(0); // Server
        dialog
            .tls_level
            .state
            .set_selection(TlsLevel::MutualTls.index());
        dialog
            .client_ca_files
            .state
            .set_values(vec!["ca1.pem".to_string()]);
        let area = Rect::new(0, 0, 80, 60);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);

        fn rightmost_non_space(buf: &Buffer, y: u16) -> u16 {
            (0..buf.area.width)
                .rev()
                .find(|&x| buf[(x, y)].symbol() != " ")
                .unwrap_or(0)
        }

        let name_row = row_of(&buf, "Name");
        let ca_row = row_of(&buf, "Client CA(s)");
        assert_eq!(
            rightmost_non_space(&buf, name_row),
            rightmost_non_space(&buf, ca_row),
            "DEL button leaves trailing dead space vs. the dialog's other full-width rows"
        );
    }

    #[test]
    /// UI-R-024 — the client-CA list's row stays a fixed 3 rows tall (1 content + 2 border)
    /// regardless of how many entries it holds; more entries scroll/clip, never grow the box.
    fn ut_client_ca_row_height_fixed_regardless_of_entry_count() {
        let mut dialog = SetupDialog::create(default_timing());
        set_input(&mut dialog.name, "dev");
        dialog.role.state.set_selection(0); // Server
        dialog
            .tls_level
            .state
            .set_selection(TlsLevel::MutualTls.index());
        dialog
            .client_ca_files
            .state
            .set_values((0..10).map(|i| format!("ca{i}.pem")).collect::<Vec<_>>());
        let area = Rect::new(0, 0, 80, 60);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
        let text = buffer_text(&buf);
        let ca_row = row_of(&buf, "Client CA(s)");
        let next_row = row_of(&buf, "IP");
        // The client-CA box is 1 content row + 2 border rows; with >3 entries the extras
        // scroll/clip, they must never push the following row further down.
        assert_eq!(
            next_row - ca_row,
            3,
            "client-CA row appears to have grown beyond a fixed 3-row box:\n{text}"
        );
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
    /// UI-R-024 — an RtuOverTcp setup dialog shows the same fields as TCP (ip/port,
    /// TLS level selector) and none of RTU's serial fields (MB-R-113).
    fn ut_rtu_over_tcp_dialog_shows_tcp_like_fields() {
        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        dialog.transport.state.set_selection(2); // Tcp=0, Rtu=1, RtuOverTcp=2
        assert_eq!(dialog.transport.state.get_value(), Transport::RtuOverTcp);
        let area = Rect::new(0, 0, 80, 60);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
        let text = buffer_text(&buf);
        assert!(
            text.contains("IP"),
            "missing IP field for RtuOverTcp:\n{text}"
        );
        assert!(
            text.contains("TLS"),
            "missing TLS selector for RtuOverTcp:\n{text}"
        );
        assert!(
            !text.contains("Baud"),
            "RTU serial field leaked into RtuOverTcp:\n{text}"
        );
    }

    #[test]
    /// MB-R-113 — resolving an RtuOverTcp dialog produces `Endpoint::RtuOverTcp`
    /// with the entered ip/port.
    fn ut_resolve_rtu_over_tcp_endpoint() {
        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        set_input(&mut dialog.name, "dev");
        dialog.transport.state.set_selection(2);
        set_input(&mut dialog.ip, "10.0.0.9");
        set_input(&mut dialog.port, "1502");
        let outcome = dialog.resolve().unwrap();
        assert_eq!(
            outcome.values.endpoint,
            Endpoint::RtuOverTcp {
                ip: "10.0.0.9".into(),
                port: 1502
            }
        );
    }

    #[test]
    /// MB-R-116 — resolving a Udp dialog produces `Endpoint::Udp` with the entered ip/port,
    /// for either role.
    fn ut_resolve_udp_endpoint() {
        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        set_input(&mut dialog.name, "dev");
        dialog.transport.state.set_selection(3); // Tcp=0, Rtu=1, RtuOverTcp=2, Udp=3
        set_input(&mut dialog.ip, "10.0.0.9");
        set_input(&mut dialog.port, "1502");
        let outcome = dialog.resolve().unwrap();
        assert_eq!(
            outcome.values.endpoint,
            Endpoint::Udp {
                ip: "10.0.0.9".into(),
                port: 1502
            }
        );

        dialog.role.state.set_selection(1); // Role::Server=0, Role::Client=1
        let outcome = dialog.resolve().unwrap();
        assert_eq!(outcome.values.role, Role::Client);
    }

    #[test]
    /// MB-R-121 — selecting an Ascii endpoint sets the transport selector to index 4.
    fn ut_edit_ascii_sets_transport_selection() {
        let timing = Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        };
        let endpoint = Endpoint::Ascii {
            path: "/dev/ttyUSB0".to_string(),
            baud_rate: 9600,
            parity: None,
            data_bits: None,
            stop_bits: None,
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
        assert_eq!(dialog.transport.state.get_value(), Transport::Ascii);
    }

    #[test]
    /// MB-R-125 — selecting an AsciiOverTcp endpoint sets the transport selector to index 5.
    fn ut_edit_ascii_over_tcp_sets_transport_selection() {
        let timing = Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        };
        let endpoint = Endpoint::AsciiOverTcp {
            ip: "10.0.0.9".to_string(),
            port: 1502,
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
        assert_eq!(dialog.transport.state.get_value(), Transport::AsciiOverTcp);
    }

    #[test]
    /// MB-R-121 — resolving an Ascii dialog produces `Endpoint::Ascii` with the entered
    /// path/baud/parity/data_bits/stop_bits, mirroring the Rtu resolve test.
    fn ut_values_ascii_produces_endpoint() {
        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        set_input(&mut dialog.name, "dev");
        dialog.transport.state.set_selection(4); // Tcp=0, Rtu=1, RtuOverTcp=2, Udp=3, Ascii=4
        assert_eq!(dialog.transport.state.get_value(), Transport::Ascii);
        set_suggest_input(&mut dialog.path, "/dev/ttyUSB1");
        set_input(&mut dialog.baud, "9600");
        let outcome = dialog.resolve().unwrap();
        assert_eq!(
            outcome.values.endpoint,
            Endpoint::Ascii {
                path: "/dev/ttyUSB1".into(),
                baud_rate: 9600,
                parity: dialog.parity.state.get_value().to_config(),
                data_bits: Some(dialog.data_bits.state.get_value().0),
                stop_bits: Some(dialog.stop_bits.state.get_value().0),
            }
        );
    }

    #[test]
    /// MB-R-125 — resolving an AsciiOverTcp dialog produces `Endpoint::AsciiOverTcp` with the
    /// entered ip/port, mirroring the RtuOverTcp/Udp resolve tests.
    fn ut_values_ascii_over_tcp_produces_endpoint() {
        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        set_input(&mut dialog.name, "dev");
        dialog.transport.state.set_selection(5); // ..Udp=3, Ascii=4, AsciiOverTcp=5
        assert_eq!(dialog.transport.state.get_value(), Transport::AsciiOverTcp);
        set_input(&mut dialog.ip, "10.0.0.9");
        set_input(&mut dialog.port, "1502");
        let outcome = dialog.resolve().unwrap();
        assert_eq!(
            outcome.values.endpoint,
            Endpoint::AsciiOverTcp {
                ip: "10.0.0.9".into(),
                port: 1502
            }
        );
    }

    #[test]
    /// MB-R-127 — TLS is offered (tls_shown() true once a level is picked) for AsciiOverTcp
    /// exactly as for Tcp/RtuOverTcp, never for Ascii (MB-R-121: rtu::Config carries no tls
    /// field).
    fn ut_tls_shown_for_ascii_over_tcp_not_ascii() {
        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        dialog.transport.state.set_selection(5); // AsciiOverTcp
        assert_eq!(dialog.transport.state.get_value(), Transport::AsciiOverTcp);
        assert!(!dialog.tls_shown());
        dialog.tls_level.state.set_selection(TlsLevel::Tls.index());
        assert!(dialog.tls_shown());

        dialog.transport.state.set_selection(4); // Ascii
        assert_eq!(dialog.transport.state.get_value(), Transport::Ascii);
        assert!(!dialog.tls_shown());
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
        *dialog.client_ca_files.state.values_mut() = vec!["client_ca.pem".to_string()];
        let outcome = dialog.resolve().unwrap();
        let cfg = outcome.values.tls.unwrap().unwrap();
        assert_eq!(
            cfg.server,
            ferrowl_util::tls::ServerTlsPolicy::Tls {
                server_cert: ferrowl_util::tls::ServerCertSource::SelfSigned
            }
        );
    }

    #[test]
    /// MB-R-135 — toggling Self-Signed On excludes stale cert_file/key_file text from the
    /// resolved config, even though the widgets' stored text is untouched.
    fn ut_resolve_self_signed_excludes_stale_cert_key_text() {
        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        set_input(&mut dialog.name, "dev");
        dialog.tls_level.state.set_selection(TlsLevel::Tls.index());
        set_suggest_input(&mut dialog.cert_file, "s.crt");
        set_suggest_input(&mut dialog.key_file, "s.key");
        dialog.self_signed.state.set_selection(1); // On, after the text was typed

        let outcome = dialog.resolve().unwrap();
        let cfg = outcome.values.tls.unwrap().unwrap();
        assert_eq!(
            cfg.server,
            ferrowl_util::tls::ServerTlsPolicy::Tls {
                server_cert: ferrowl_util::tls::ServerCertSource::SelfSigned
            }
        );
        // The stored text survives the toggle -- only the resolved config excludes it.
        assert_eq!(dialog.cert_file.state.input(), "s.crt");
        assert_eq!(dialog.key_file.state.input(), "s.key");
    }

    #[test]
    /// MB-R-135 — toggling Skip-Verify On excludes stale ca_file text from the resolved config,
    /// even though the widget's stored text is untouched.
    fn ut_resolve_skip_verify_excludes_stale_ca_file_text() {
        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        set_input(&mut dialog.name, "dev");
        dialog.role.state.set_selection(1); // Role::Server=0, Role::Client=1
        dialog.tls_level.state.set_selection(TlsLevel::Tls.index());
        set_suggest_input(&mut dialog.ca_file, "ca.pem");
        dialog.skip_verify.state.set_selection(1); // On, after the text was typed

        let outcome = dialog.resolve().unwrap();
        let cfg = outcome.values.tls.unwrap().unwrap();
        assert_eq!(
            cfg.client,
            ferrowl_util::tls::ClientTlsPolicy::Tls {
                client_verification: ferrowl_util::tls::ClientVerification::SkipVerify
            }
        );
        assert_eq!(dialog.ca_file.state.input(), "ca.pem");
    }

    #[test]
    /// MB-R-135 — toggling Self-Signed back Off restores the previously entered cert/key paths
    /// (nothing was cleared, only excluded while On).
    fn ut_resolve_toggle_self_signed_back_off_restores_cert_key() {
        let cert = std::env::temp_dir().join("ferrowl_modbus_setup_test_s.crt");
        let key = std::env::temp_dir().join("ferrowl_modbus_setup_test_s.key");
        std::fs::write(&cert, b"").unwrap();
        std::fs::write(&key, b"").unwrap();
        let cert = cert.to_str().unwrap().to_string();
        let key = key.to_str().unwrap().to_string();

        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        set_input(&mut dialog.name, "dev");
        dialog.tls_level.state.set_selection(TlsLevel::Tls.index());
        set_suggest_input(&mut dialog.cert_file, &cert);
        set_suggest_input(&mut dialog.key_file, &key);
        dialog.self_signed.state.set_selection(1); // On
        dialog.self_signed.state.set_selection(0); // Off again

        let outcome = dialog.resolve().unwrap();
        let cfg = outcome.values.tls.unwrap().unwrap();
        assert_eq!(
            cfg.server,
            ferrowl_util::tls::ServerTlsPolicy::Tls {
                server_cert: ferrowl_util::tls::ServerCertSource::Explicit {
                    cert_file: cert,
                    key_file: key,
                }
            }
        );
    }

    #[test]
    /// UI-R-022 — the focus cycle visits the reconnect field for every role, since server-side
    /// reconnect (MB-R-130–134) makes it applicable regardless of role.
    fn ut_focus_next_reaches_reconnect_for_server_role() {
        let mut dialog = SetupDialog::create(Timing {
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        });
        // Default role is Server, and default transport is Tcp, so `baud` is gated off and
        // traversal moves straight from `port` to `reconnect`.
        dialog.focus = SetupDialogFocus::Port;
        dialog.port.state.set_focused(true);
        dialog.focus_next();
        assert!(dialog.reconnect.state.is_focused());
    }

    /// Drive `focus_next()` from `SetupDialogFocus::Role` and collect the sequence of foci
    /// visited up to and including `SetupDialogFocus::Ip`.
    fn focus_sequence_from_role(dialog: &mut SetupDialog) -> Vec<SetupDialogFocus> {
        dialog.focus = SetupDialogFocus::Role;
        let mut seq = vec![dialog.focus];
        loop {
            dialog.focus_next();
            seq.push(dialog.focus);
            if dialog.focus == SetupDialogFocus::Ip {
                break;
            }
        }
        seq
    }

    #[test]
    /// MB-R-136 — the mTLS server-role Tab order matches the dialog's visual row order (post-
    /// s9 layout): Role, Self-Signed, own cert/key, Skip Verify, then the client-CA list and its
    /// ADD/DEL buttons, then IP.
    fn ut_tab_order_server_mtls() {
        let mut dialog = SetupDialog::create(default_timing());
        dialog.role.state.set_selection(0); // Server
        dialog
            .tls_level
            .state
            .set_selection(TlsLevel::MutualTls.index());
        dialog
            .client_ca_files
            .state
            .set_values(vec!["ca1.pem".to_string()]); // non-empty, so DEL is eligible
        let seq = focus_sequence_from_role(&mut dialog);
        assert_eq!(
            seq,
            vec![
                SetupDialogFocus::Role,
                SetupDialogFocus::SelfSigned,
                SetupDialogFocus::CertFile,
                SetupDialogFocus::KeyFile,
                SetupDialogFocus::ClientCertSkipVerify,
                SetupDialogFocus::ClientCaFiles,
                SetupDialogFocus::ClientCaAddButton,
                SetupDialogFocus::ClientCaDeleteButton,
                SetupDialogFocus::Ip,
            ]
        );
    }

    #[test]
    /// MB-R-136 — the mTLS client-role Tab order: Role, Self-Signed, own cert/key, Skip Verify,
    /// then the CA-file trust-anchor input, then IP (no ADD/DEL — client role never shows the
    /// client-CA list).
    fn ut_tab_order_client_mtls() {
        let mut dialog = SetupDialog::create(default_timing());
        dialog.role.state.set_selection(1); // Client
        dialog
            .tls_level
            .state
            .set_selection(TlsLevel::MutualTls.index());
        let seq = focus_sequence_from_role(&mut dialog);
        assert_eq!(
            seq,
            vec![
                SetupDialogFocus::Role,
                SetupDialogFocus::SelfSigned,
                SetupDialogFocus::ClientCertFile,
                SetupDialogFocus::ClientKeyFile,
                SetupDialogFocus::SkipVerify,
                SetupDialogFocus::CaFile,
                SetupDialogFocus::Ip,
            ]
        );
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

    fn type_into<S: SetFocus + HandleEvents>(state: &mut S, s: &str) {
        state.set_focused(true);
        for c in s.chars() {
            state.handle_events(KeyModifiers::NONE, KeyCode::Char(c));
        }
    }

    /// MB-R-136 — the client-CA row is a genuine add/remove list: the ADD button opens a
    /// sub-dialog whose confirmed path is appended and selected, and the DEL button removes
    /// whichever entry is currently selected — not a comma-separated text field.
    #[test]
    fn ut_client_ca_files_add_remove_edit() {
        let mut dialog = SetupDialog::create(default_timing());
        set_input(&mut dialog.name, "dev");
        dialog.role.state.set_selection(0); // Role::Server
        dialog
            .tls_level
            .state
            .set_selection(TlsLevel::MutualTls.index());
        dialog.self_signed.state.set_selection(1); // server cert self-signed, no file needed

        assert!(dialog.client_ca_files.state.values().is_empty());

        // ADD: open the sub-dialog, type a path, confirm with Enter.
        dialog.focus = SetupDialogFocus::ClientCaAddButton;
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(dialog.client_ca_add_dialog.is_some());
        type_into(
            &mut dialog.client_ca_add_dialog.as_mut().unwrap().path.state,
            "ca1.pem",
        );
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(dialog.client_ca_add_dialog.is_none());
        assert_eq!(
            dialog.client_ca_files.state.values(),
            &["ca1.pem".to_string()]
        );

        // ADD a second entry.
        dialog.focus = SetupDialogFocus::ClientCaAddButton;
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        type_into(
            &mut dialog.client_ca_add_dialog.as_mut().unwrap().path.state,
            "ca2.pem",
        );
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert_eq!(
            dialog.client_ca_files.state.values(),
            &["ca1.pem".to_string(), "ca2.pem".to_string()]
        );

        // An empty path is rejected: the sub-dialog stays open with an error, nothing appended.
        dialog.focus = SetupDialogFocus::ClientCaAddButton;
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(dialog.client_ca_add_dialog.is_some());
        assert!(
            !dialog
                .client_ca_add_dialog
                .as_ref()
                .unwrap()
                .error
                .state
                .is_empty()
        );
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(dialog.client_ca_add_dialog.is_none());
        assert_eq!(
            dialog.client_ca_files.state.values(),
            &["ca1.pem".to_string(), "ca2.pem".to_string()]
        );

        // DEL: remove the currently-selected entry (selection sits on the last-added item).
        assert_eq!(dialog.client_ca_files.state.selection(), 1);
        dialog.focus = SetupDialogFocus::ClientCaDeleteButton;
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Char(' '));
        assert_eq!(
            dialog.client_ca_files.state.values(),
            &["ca1.pem".to_string()]
        );

        // MB-R-136 — the ADD sub-dialog's path field offers filesystem completions, and
        // accepting one (Enter with the popup open) fills the field without submitting the
        // sub-dialog (Enter without a popup open is what submits).
        {
            dialog.focus = SetupDialogFocus::ClientCaAddButton;
            dialog.handle_events(KeyModifiers::NONE, KeyCode::Enter);
            let sub = dialog.client_ca_add_dialog.as_mut().unwrap();
            type_into(&mut sub.path.state, "s");
            assert!(
                sub.path.state.suggestions_open(),
                "no completion popup offered for a 's' prefix (expects to match e.g. 'src')"
            );
            dialog.handle_events(KeyModifiers::NONE, KeyCode::Enter);
            assert!(
                dialog.client_ca_add_dialog.is_some(),
                "Enter with the completion popup open must accept the suggestion, not submit \
                 the sub-dialog"
            );
            dialog.client_ca_add_dialog = None;
            // Abandoning the sub-dialog above doesn't move focus off the ADD button that opened
            // it; point back at DEL so the next step actually deletes rather than reopening ADD.
            dialog.focus = SetupDialogFocus::ClientCaDeleteButton;
        }

        // Removing the last entry leaves the list empty and the DEL button no longer eligible.
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Char(' '));
        assert!(dialog.client_ca_files.state.values().is_empty());
        assert!(!dialog.show_client_ca() || dialog.client_ca_files.state.values().is_empty());

        // Deleting down to an empty list must not leave `focus` stuck on the now-ineligible DEL
        // button — it falls back to ADD, so Tab from there keeps working.
        assert_eq!(dialog.focus, SetupDialogFocus::ClientCaAddButton);

        // Resolving with an empty list and skip-verify off is a validation error (MB-R-136).
        assert!(dialog.resolve().is_err());
    }

    #[test]
    /// MB-R-136 — deleting the last remaining client-CA entry moves focus off the now-
    /// unfocusable DEL button (its `#[focus(when = ...)]` excludes it once the list is empty) and
    /// onto ADD, so a subsequent Tab still traverses correctly instead of getting stuck.
    fn ut_delete_last_client_ca_falls_back_focus_to_add_button() {
        let mut dialog = SetupDialog::create(default_timing());
        dialog.role.state.set_selection(0); // Server
        dialog
            .tls_level
            .state
            .set_selection(TlsLevel::MutualTls.index());
        dialog
            .client_ca_files
            .state
            .set_values(vec!["ca1.pem".to_string()]);
        dialog.focus = SetupDialogFocus::ClientCaDeleteButton;
        dialog.client_ca_delete_button.state.set_focused(true);

        dialog.handle_events(KeyModifiers::NONE, KeyCode::Char(' '));

        assert!(dialog.client_ca_files.state.values().is_empty());
        assert_eq!(dialog.focus, SetupDialogFocus::ClientCaAddButton);
        // The fallback must also move the *widget-level* highlight, not just the tracking enum
        // — otherwise DEL stays visually focused (though hidden) and ADD stays unhighlighted
        // until the next real Tab press.
        assert!(!dialog.client_ca_delete_button.state.is_focused());
        assert!(dialog.client_ca_add_button.state.is_focused());
        // Tab from the fallback proceeds normally rather than looping on a dead target.
        dialog.focus_next();
        assert_ne!(dialog.focus, SetupDialogFocus::ClientCaDeleteButton);
    }
}
