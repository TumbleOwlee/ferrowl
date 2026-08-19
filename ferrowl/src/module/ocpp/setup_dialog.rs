//! OCPP module setup dialog (`:new`). Collects name, version, role, protocol, the websocket
//! endpoint (ip/port), and — for `wss://` — a security level (Basic Auth / TLS / mTLS) with its
//! credential/certificate fields, validating live like the Modbus dialog.

use crossterm::event::{KeyCode, KeyModifiers};
use derive_builder::Builder;
use ferrowl_ui::{
    Border, COLOR_SCHEME, EventResult, render_field, render_row,
    state::{
        ButtonState, InputFieldState, InputFieldStateBuilder, SelectionState,
        SelectionStateBuilder, SuggestInputState, SuggestInputStateBuilder,
    },
    style::{ButtonStyle, InputFieldStyle, SelectionStyle, SuggestInputStyle, TextStyle},
    traits::{HandleEvents, ToLabel},
    widgets::{
        Button, GetValue, InputField, InputFieldBuilder, Selection, SelectionBuilder, SuggestInput,
        SuggestInputBuilder, Text, TextBuilder, Validate, ValidateResult, Widget,
    },
};
use ferrowl_ui_derive::{Focus, focusable};
use ferrowl_util::convert::FileType;
use ferrowl_util::tls::{
    ClientCertSource, ClientCertVerification, ClientTlsPolicy, ClientVerification,
    ServerCertSource, ServerTlsPolicy,
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, Clear, Widget as UiWidget},
};

use crate::dialog::NonEmpty;
use crate::dialog::close_confirm::{CloseConfirmDialog, CloseConfirmOutcome, route_close_confirm};
use crate::dialog::path_suggest::FsPathProvider;
use crate::module::ocpp::config::device::OcppSecurityConfig;
use crate::module::ocpp::config::session::{OcppProtocol, OcppRole, OcppSpec, OcppVersion};

mod security;
use security::{
    SecurityInputs, SecurityLevel, SelfSignedChoice, SkipVerifyChoice, validate_security,
};

/// Live validator for the device-config path field: empty is allowed (a fresh empty config),
/// otherwise the path must be a TOML/JSON file, and — if it exists — a loadable OCPP device
/// config. Mirrors the Modbus dialog's `ConfigPath`.
#[derive(Debug, Clone)]
pub struct ConfigPath;

/// Auto-reconnect toggle: client redial (OC-R-048, OC-R-107) or server bind retry
/// (OC-R-083, OC-R-108–109). Mirrors Modbus's own `setup_dialog::choices::ReconnectChoice`,
/// duplicated rather than shared: the two setup dialogs are independent module types with no
/// shared UI-choices module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconnectChoice {
    On,
    Off,
}

impl ToLabel for ReconnectChoice {
    fn to_label(&self) -> String {
        match self {
            ReconnectChoice::On => "On",
            ReconnectChoice::Off => "Off",
        }
        .to_string()
    }
}

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
    pub name: Widget<InputFieldState, InputField<NonEmpty>>,
    /// Path to the OCPP device-config file (empty = a fresh, empty device config).
    #[focus]
    pub config_path:
        Widget<SuggestInputState<FsPathProvider>, SuggestInput<ConfigPath, FsPathProvider>>,
    #[focus]
    pub version: Widget<SelectionState<OcppVersion>, Selection<OcppVersion>>,
    #[focus]
    pub role: Widget<SelectionState<OcppRole>, Selection<OcppRole>>,
    /// Automatically reconnect (with backoff) instead of ending the module task on failure:
    /// client redial (OC-R-048) or server bind retry (OC-R-083). Shown for every role.
    #[focus]
    pub reconnect: Widget<SelectionState<ReconnectChoice>, Selection<ReconnectChoice>>,
    #[focus]
    pub protocol: Widget<SelectionState<OcppProtocol>, Selection<OcppProtocol>>,
    #[focus]
    pub ip: Widget<InputFieldState, InputField<String>>,
    #[focus]
    pub port: Widget<InputFieldState, InputField<u16>>,
    /// Optional URL path appended after the endpoint, e.g. `/ocpp/cp001`.
    #[focus(when = {self.role.get_value() == OcppRole::Client})]
    pub path: Widget<InputFieldState, InputField<String>>,
    /// Transport security level, offered only for `wss://`.
    #[focus(when = {self.show_security()})]
    pub security: Widget<SelectionState<SecurityLevel>, Selection<SecurityLevel>>,
    /// Basic Auth username. Note: rendered as plain text — no masked-input widget exists yet.
    #[focus(when = {self.show_credentials()})]
    pub username: Widget<InputFieldState, InputField<String>>,
    /// Basic Auth password. Note: rendered as plain text (no masking) — same limitation as
    /// `username`; the field is not obscured on screen.
    #[focus(when = {self.show_credentials()})]
    pub password: Widget<InputFieldState, InputField<String>>,
    /// Server: "generate an ephemeral self-signed certificate" toggle (OC-R-110, shown at TLS+).
    /// Client, at mTLS only: "generate an ephemeral self-signed client identity" toggle
    /// (OC-R-116) — the same widget field backs both, since only one role is ever active for a
    /// given dialog instance. Mirrors the Modbus dialog's `self_signed` field exactly.
    #[focus(when = {self.show_self_signed()})]
    pub self_signed: Widget<SelectionState<SelfSignedChoice>, Selection<SelfSignedChoice>>,
    /// Server role only: certificate chain presented to connecting clients.
    #[focus(when = {self.show_server_cert()})]
    pub cert_file:
        Widget<SuggestInputState<FsPathProvider>, SuggestInput<NonEmpty, FsPathProvider>>,
    /// Server role only: private key matching `cert_file`.
    #[focus(when = {self.show_server_cert()})]
    pub key_file: Widget<SuggestInputState<FsPathProvider>, SuggestInput<NonEmpty, FsPathProvider>>,
    /// Server-only, at mTLS only: "accept any client certificate" toggle (OC-R-113). On hides the
    /// client-CA list below and excludes it from the resolved config; the list's own text is
    /// preserved (never cleared) so toggling back Off restores it.
    #[focus(when = {self.show_client_cert_skip_verify()})]
    pub client_cert_skip_verify:
        Widget<SelectionState<SkipVerifyChoice>, Selection<SkipVerifyChoice>>,
    /// Client role only: client certificate presented for mutual TLS.
    #[focus(when = {self.show_client_cert()})]
    pub client_cert_file:
        Widget<SuggestInputState<FsPathProvider>, SuggestInput<NonEmpty, FsPathProvider>>,
    /// Client role only: private key matching `client_cert_file`.
    #[focus(when = {self.show_client_cert()})]
    pub client_key_file:
        Widget<SuggestInputState<FsPathProvider>, SuggestInput<NonEmpty, FsPathProvider>>,
    /// Server-only list of CAs used to verify client certificates under mTLS (OC-R-113) — a
    /// certificate signed by any one is sufficient. An add/remove list (mirrors the Modbus
    /// dialog's `client_ca_files` cluster exactly, sharing `AddCaFileDialog`): `Selection<String>`
    /// browses/selects the current entries; `client_ca_add_button`/`client_ca_delete_button` add
    /// via `client_ca_add_dialog` or remove the selected entry. Selecting mTLS as server implies
    /// `ServerTlsPolicy::MutualTls` in the resolved config (unless `client_cert_skip_verify` is
    /// on, in which case this list is ignored).
    #[focus(when = {self.show_client_ca() && !self.client_ca_files.state.values().is_empty()})]
    pub client_ca_files: Widget<SelectionState<String>, Selection<String>>,
    #[focus(when = {self.show_client_ca()})]
    pub client_ca_add_button: Widget<ButtonState, Button>,
    #[focus(when = {self.show_client_ca() && !self.client_ca_files.state.values().is_empty()})]
    pub client_ca_delete_button: Widget<ButtonState, Button>,
    /// Client role only: accept any server certificate without authenticating it. **OC-R-111**:
    /// shown only at `Tls`/`MutualTls` (not at every wss level) — Basic Auth alone has nothing to
    /// do with certificate verification.
    #[focus(when = {self.show_skip_verify()})]
    pub skip_verify: Widget<SelectionState<SkipVerifyChoice>, Selection<SkipVerifyChoice>>,
    /// Client role only: extra trust anchor for a self-signed CSMS certificate.
    #[focus(when = {self.show_ca_file()})]
    pub ca_file: Widget<SuggestInputState<FsPathProvider>, SuggestInput<String, FsPathProvider>>,
    /// Sub-dialog for adding one path to `client_ca_files`, opened by `client_ca_add_button`; not
    /// itself a `#[focus]` field — routed specially in `handle_events`, mirroring `close_confirm`.
    #[builder(default)]
    pub client_ca_add_dialog: Option<crate::dialog::ca_file_list::AddCaFileDialog>,
    /// Security section the dialog was opened with (`edit`; `Default` for a fresh dialog).
    /// [`resolve`](Self::resolve) returns it untouched while the protocol is `ws`: the security
    /// UI is hidden then, and a hidden section must never clobber a config-file-only setup
    /// (Basic Auth over plain ws is valid and file-only). Also the source for stitching the
    /// *inactive* role's half back into the resolved config under `wss`, so a role toggle
    /// preserves the other role's previously-saved settings instead of resetting them to
    /// [`OcppSecurityConfig::default`]'s placeholder (mirrors the Modbus dialog's `original_tls`).
    pub preserved_security: OcppSecurityConfig,
    /// `Path::exists` results with a timestamp, so the per-tick live validation does not stat
    /// the filesystem on every redraw (see [`path_exists`](Self::path_exists)).
    pub fs_cache: std::cell::RefCell<std::collections::HashMap<String, (bool, std::time::Instant)>>,
    pub error: Widget<String, Text>,
    /// One-line info hint shown when a server-role `wss://` instance is below the TLS level (an
    /// ephemeral self-signed certificate will be generated at each start). Not a focusable field.
    pub hint: Widget<String, Text>,
    pub keybinds: Widget<String, Text>,
    /// Close-confirm popup, opened by Esc.
    #[builder(default)]
    pub close_confirm: Option<CloseConfirmDialog>,
    /// Set on confirmed close; the host checks this via `take_close_request` and closes the dialog.
    #[builder(default)]
    close_requested: bool,
}

impl OcppSetupDialog {
    pub fn new() -> Self {
        let input_style = InputFieldStyle::default();
        let selection_style = SelectionStyle::default();
        let cert_provider = || FsPathProvider::with_extensions(&["pem", "crt", "key"]);

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
            .role(aligned_selection(
                "Role",
                Some(HorizontalAlignment::Center),
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
            .reconnect(aligned_selection(
                "Reconnect",
                Some(HorizontalAlignment::Right),
                vec![ReconnectChoice::On, ReconnectChoice::Off],
                &selection_style,
            ))
            .security(selection(
                "Security",
                vec![
                    SecurityLevel::None,
                    SecurityLevel::BasicAuth,
                    SecurityLevel::Tls,
                    SecurityLevel::MutualTls,
                ],
                &selection_style,
            ))
            .username(input("Username", "cp001", &input_style, false))
            .password(input("Password", "", &input_style, false))
            .skip_verify(selection(
                "Skip Verify",
                vec![SkipVerifyChoice::Off, SkipVerifyChoice::On],
                &selection_style,
            ))
            .self_signed(selection(
                "Self-Signed",
                vec![SelfSignedChoice::Off, SelfSignedChoice::On],
                &selection_style,
            ))
            .client_cert_skip_verify(selection(
                "Skip Verify",
                vec![SkipVerifyChoice::Off, SkipVerifyChoice::On],
                &selection_style,
            ))
            .ca_file(suggest_input(
                "CA File",
                "ca.pem",
                &input_style,
                cert_provider(),
            ))
            .cert_file(suggest_input(
                "Cert File",
                "server.crt",
                &input_style,
                cert_provider(),
            ))
            .key_file(suggest_input(
                "Key File",
                "server.key",
                &input_style,
                cert_provider(),
            ))
            .client_cert_file(suggest_input(
                "Client Cert",
                "client.crt",
                &input_style,
                cert_provider(),
            ))
            .client_key_file(suggest_input(
                "Client Key",
                "client.key",
                &input_style,
                cert_provider(),
            ))
            .client_ca_files(selection(
                "Client CA(s)",
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
            .preserved_security(OcppSecurityConfig::default())
            .fs_cache(Default::default())
            .error(text(TextStyle {
                general: ratatui::prelude::Style::default()
                    .fg(COLOR_SCHEME.error)
                    .bg(COLOR_SCHEME.bg),
            }))
            .hint(hint_text())
            .keybinds(keybinds_text())
            .focus(OcppSetupDialogFocus::Name)
            .build()
            .expect("all required builder fields are set")
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
        d.reconnect
            .state
            .set_selection(if spec.reconnect.unwrap_or(true) { 0 } else { 1 });

        let level = SecurityLevel::from_config(&spec.security, spec.role);
        d.security.state.set_selection(level.index());
        set_text(
            &mut d.username,
            spec.security.username.as_deref().unwrap_or(""),
        );
        set_text(
            &mut d.password,
            spec.security.password.as_deref().unwrap_or(""),
        );

        match spec.role {
            OcppRole::Server => {
                let (server_cert, client_verification) = match &spec.security.server {
                    ServerTlsPolicy::MutualTls {
                        server_cert,
                        client_verification,
                    } => (server_cert.clone(), Some(client_verification.clone())),
                    ServerTlsPolicy::Tls { server_cert } => (server_cert.clone(), None),
                    ServerTlsPolicy::NoTls => (ServerCertSource::Unset, None),
                };
                d.self_signed
                    .state
                    .set_selection(if server_cert == ServerCertSource::SelfSigned {
                        1
                    } else {
                        0
                    });
                let (cert_file, key_file) = match &server_cert {
                    ServerCertSource::Explicit {
                        cert_file,
                        key_file,
                    } => (cert_file.as_str(), key_file.as_str()),
                    _ => ("", ""),
                };
                set_suggest_text(&mut d.cert_file, cert_file);
                set_suggest_text(&mut d.key_file, key_file);
                let (ca_files, skip) = match &client_verification {
                    Some(ClientCertVerification::Verify { ca_files }) => (ca_files.clone(), false),
                    Some(ClientCertVerification::SkipVerify) => (Vec::new(), true),
                    None => (Vec::new(), false),
                };
                *d.client_ca_files.state.values_mut() = ca_files;
                d.client_ca_files.state.set_selection(0);
                d.client_cert_skip_verify
                    .state
                    .set_selection(if skip { 1 } else { 0 });
            }
            OcppRole::Client => {
                let (client_verification, client_identity) = match &spec.security.client {
                    ClientTlsPolicy::MutualTls {
                        client_verification,
                        client_identity,
                    } => (client_verification.clone(), Some(client_identity.clone())),
                    ClientTlsPolicy::Tls {
                        client_verification,
                    } => (client_verification.clone(), None),
                    ClientTlsPolicy::NoTls => (ClientVerification::Verify { ca_file: None }, None),
                };
                d.skip_verify.state.set_selection(
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
                set_suggest_text(&mut d.ca_file, ca_file);
                let self_signed_client =
                    matches!(client_identity, Some(ClientCertSource::SelfSigned));
                d.self_signed
                    .state
                    .set_selection(if self_signed_client { 1 } else { 0 });
                let (ccert, ckey) = match &client_identity {
                    Some(ClientCertSource::Explicit {
                        client_cert_file,
                        client_key_file,
                    }) => (client_cert_file.as_str(), client_key_file.as_str()),
                    _ => ("", ""),
                };
                set_suggest_text(&mut d.client_cert_file, ccert);
                set_suggest_text(&mut d.client_key_file, ckey);
            }
        }
        d.preserved_security = spec.security.clone();
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

        let role = self.role.get_value();
        let reconnect = Some(self.reconnect.state.get_value() == ReconnectChoice::On);
        let protocol = self.protocol.get_value();
        let security = if protocol == OcppProtocol::Wss {
            let level = self.security.get_value();
            let is_client = role == OcppRole::Client;
            let is_server = role == OcppRole::Server;
            let tls = level >= SecurityLevel::Tls;
            // Below TLS, a wss server still generates an ephemeral self-signed certificate at
            // each start rather than binding plain TCP (OC-R-095's fallback) -- folded into the
            // `self_signed` input passed to `build_config` rather than a post-processing
            // override, so `ServerCertSource::resolve` alone decides the outcome. A field hidden
            // below TLS (the client's `ca_file` trust anchor) is blanked here so stale text never
            // leaks into the resolved config once the level drops back down.
            fn blank_below_tls(tls: bool, s: &str) -> &str {
                if tls { s } else { "" }
            }
            let effective_self_signed = if is_server {
                level < SecurityLevel::Tls || self.self_signed.get_value() == SelfSignedChoice::On
            } else {
                self.self_signed.get_value() == SelfSignedChoice::On
            };
            let mut cfg = level.build_config(
                role,
                SecurityInputs {
                    username: self.username.state.input(),
                    password: self.password.state.input(),
                    ca_file: blank_below_tls(tls, self.ca_file.state.input()),
                    cert_file: blank_below_tls(tls, self.cert_file.state.input()),
                    key_file: blank_below_tls(tls, self.key_file.state.input()),
                    client_cert_file: self.client_cert_file.state.input(),
                    client_key_file: self.client_key_file.state.input(),
                    client_ca_files: self.client_ca_files.state.values(),
                    self_signed: effective_self_signed,
                    skip_verify: is_client && self.skip_verify.get_value() == SkipVerifyChoice::On,
                    client_cert_skip_verify: is_server
                        && self.client_cert_skip_verify.get_value() == SkipVerifyChoice::On,
                },
            )?;
            // Stitch the inactive role's half back in from the config the dialog was opened
            // with (if any), so a role toggle preserves the other role's previously-saved
            // security settings instead of resetting them to `OcppSecurityConfig::default`'s
            // placeholder (mirrors the Modbus dialog's `original_tls` stitching).
            match role {
                OcppRole::Server => cfg.client = self.preserved_security.client.clone(),
                OcppRole::Client => cfg.server = self.preserved_security.server.clone(),
            }
            validate_security(&cfg, role, level, &|p| self.path_exists(p))?;
            cfg
        } else {
            // The security UI is hidden for ws, so hand back whatever the dialog was opened
            // with: an edit round-trip must not wipe a config-file-only security section.
            self.preserved_security.clone()
        };

        Ok(OcppSpec {
            name,
            version: self.version.state.get_value(),
            role,
            protocol,
            ip,
            port,
            path,
            timeout_ms: None,
            reconnect,
            security,
        })
    }

    /// The entered device-config path (trimmed; empty when none).
    pub fn config_path(&self) -> String {
        self.config_path.state.input().trim().to_string()
    }

    /// Route a key: the close-confirm popup captures all keys while open; then the client-CA
    /// add-dialog (OC-R-113), if open; then the client-CA ADD/DEL buttons (Enter/Space); Esc
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
                OcppSetupDialogFocus::ClientCaAddButton => {
                    self.client_ca_add_dialog =
                        Some(crate::dialog::ca_file_list::AddCaFileDialog::new());
                    return EventResult::Consumed;
                }
                OcppSetupDialogFocus::ClientCaDeleteButton => {
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

    /// Remove the currently-selected client-CA entry (OC-R-113), if any, adjusting the
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
            // a dead target, so fall back to ADD (mirrors the Modbus setup dialog's fix).
            // `focus_previous()` correctly pairs the widget-level blur/focus with the enum
            // update, unlike a raw enum assignment. Callers only reach here with
            // `self.focus == ClientCaDeleteButton` (the Enter/Space handler guards on that), so
            // "previous" always lands on ADD.
            self.focus_previous();
        }
    }

    /// Whether the close-confirm popup was confirmed since the last call; clears the flag.
    pub fn take_close_request(&mut self) -> bool {
        std::mem::take(&mut self.close_requested)
    }

    /// The URL `path` field is only meaningful for the client (CS) role — the CSMS server binds a
    /// host:port and ignores it — so it is hidden (and skipped by focus) when the role is Server.
    fn path_hidden(&self) -> bool {
        self.role.get_value() == OcppRole::Server
    }

    /// Whether the protocol is `wss://` (gates every security-related field).
    fn wss(&self) -> bool {
        self.protocol.get_value() == OcppProtocol::Wss
    }

    /// The currently selected security level.
    fn level(&self) -> SecurityLevel {
        self.security.get_value()
    }

    // --- Security-field visibility -----------------------------------------------------------
    // Single source of truth consumed by the `#[focus(when)]` gates, the render branches and the
    // dialog-height computation, so keyboard focus, painting and layout can never disagree about
    // which fields exist.

    /// The security-level selection row (any wss endpoint).
    fn show_security(&self) -> bool {
        self.wss()
    }

    /// Basic Auth credential inputs (wss at Basic Auth level or above).
    fn show_credentials(&self) -> bool {
        self.wss() && self.level() >= SecurityLevel::BasicAuth
    }

    /// The client-side skip-verify toggle. **OC-R-111**: shown only at `Tls`/`MutualTls` (not at
    /// every wss level as before this spec diff) — a Basic-Auth-only connection has nothing to do
    /// with certificate verification.
    fn show_skip_verify(&self) -> bool {
        self.wss()
            && self.role.get_value() == OcppRole::Client
            && self.level() >= SecurityLevel::Tls
    }

    /// Client trust-anchor input (wss client at TLS level or above, Skip-Verify Off — OC-R-111).
    fn show_ca_file(&self) -> bool {
        self.wss()
            && self.level() >= SecurityLevel::Tls
            && self.role.get_value() == OcppRole::Client
            && self.skip_verify.get_value() == SkipVerifyChoice::Off
    }

    /// Server: self-signed server-certificate toggle (TLS level or above, OC-R-110). Client, at
    /// mTLS only: self-signed client-identity toggle (OC-R-116) — same widget, different meaning
    /// per role (see the field's doc comment).
    fn show_self_signed(&self) -> bool {
        self.wss()
            && ((self.role.get_value() == OcppRole::Server && self.level() >= SecurityLevel::Tls)
                || (self.role.get_value() == OcppRole::Client
                    && self.level() == SecurityLevel::MutualTls))
    }

    /// Server-only, mTLS only: "accept any client certificate" toggle (OC-R-113).
    fn show_client_cert_skip_verify(&self) -> bool {
        self.wss()
            && self.role.get_value() == OcppRole::Server
            && self.level() == SecurityLevel::MutualTls
    }

    /// Row: the Skip Verify toggle — server's `client_cert_skip_verify` (mTLS only), or the
    /// client's `skip_verify` (TLS level or above) — exactly one applies for a given role.
    fn show_skip_verify_row(&self) -> bool {
        self.show_client_cert_skip_verify() || self.show_skip_verify()
    }

    /// Server certificate/key inputs (wss server at TLS level or above, Self-Signed Off).
    fn show_server_cert(&self) -> bool {
        self.wss()
            && self.level() >= SecurityLevel::Tls
            && self.role.get_value() == OcppRole::Server
            && self.self_signed.get_value() == SelfSignedChoice::Off
    }

    /// Client mTLS certificate/key inputs — hidden when the client's self-signed-identity toggle
    /// is on (OC-R-116), mirroring the server's `show_server_cert`.
    fn show_client_cert(&self) -> bool {
        self.wss()
            && self.level() == SecurityLevel::MutualTls
            && self.role.get_value() == OcppRole::Client
            && self.self_signed.get_value() == SelfSignedChoice::Off
    }

    /// Server mTLS client-CA list input — hidden when `client_cert_skip_verify` is on
    /// (OC-R-113), preserving the list's own text so toggling back Off restores it.
    fn show_client_ca(&self) -> bool {
        self.wss()
            && self.level() == SecurityLevel::MutualTls
            && self.role.get_value() == OcppRole::Server
            && self.client_cert_skip_verify.get_value() == SkipVerifyChoice::Off
    }

    /// Row: the own-identity cert/key pair sharing the Self-Signed row — server's
    /// `cert_file`/`key_file`, or the client's `client_cert_file`/`client_key_file`.
    fn show_identity_row(&self) -> bool {
        self.show_server_cert() || self.show_client_cert()
    }

    /// Row: the peer-verification input sharing the Skip Verify row — server's
    /// `client_ca_files` list, or the client's `ca_file`.
    fn show_peer_verify_row(&self) -> bool {
        self.show_client_ca() || self.show_ca_file()
    }

    /// Cached `Path::exists` with a short TTL: `render` re-runs `resolve` (and so the security
    /// validation) on every 100ms tick, and stat-ing configured certificate paths each tick is
    /// wasted I/O — and visibly laggy on network filesystems. One second of staleness is
    /// imperceptible next to typing latency.
    fn path_exists(&self, path: &str) -> bool {
        const TTL: std::time::Duration = std::time::Duration::from_secs(1);
        let now = std::time::Instant::now();
        let mut cache = self.fs_cache.borrow_mut();
        if let Some((hit, at)) = cache.get(path)
            && now.duration_since(*at) < TTL
        {
            return *hit;
        }
        let exists = std::path::Path::new(path).exists();
        cache.insert(path.to_string(), (exists, now));
        exists
    }

    /// The self-signed hint line (wss server below TLS level).
    fn show_hint(&self) -> bool {
        self.wss() && self.role.get_value() == OcppRole::Server && self.level() < SecurityLevel::Tls
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        match self.resolve() {
            Ok(_) => self.error.state.clear(),
            Err(e) => self.error.state = e,
        }

        let has_error = !self.error.state.is_empty();
        let role = self.role.get_value();
        let is_server = role == OcppRole::Server;
        let show_security_row = self.show_security();
        let show_credentials = self.show_credentials();
        // Fixed 3-row TLS layout (both roles): Security (no side toggle), a Self-Signed row that
        // also carries the own-identity cert/key pair, and a Skip Verify row that also carries
        // the peer-verification input — each combined row's existence is governed by its toggle
        // field's own gate, since the paired file field's gate is always a strict subset of it.
        let show_self_signed_row = self.show_self_signed();
        let show_identity_row = self.show_identity_row();
        let show_skip_verify_row = self.show_skip_verify_row();
        let show_peer_verify_row = self.show_peer_verify_row();
        let show_hint = self.show_hint();

        // border(2) + inner margin(2) + name(3) + config path(3) + version|role|reconnect(3)
        // + protocol|ip|port|path(3) + keybinds(1), plus the error box (3), the security row
        // (3), the self-signed row (3, mTLS only for client), the skip-verify row (3), and the
        // hint line (1), only when applicable.
        let box_height = 17
            + if has_error { 3 } else { 0 }
            + if show_security_row { 3 } else { 0 }
            + if show_self_signed_row { 3 } else { 0 }
            + if show_skip_verify_row { 3 } else { 0 }
            + if show_hint { 1 } else { 0 };
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
        let block_inner = block.inner(vcenter);
        let inner = block_inner.inner(Margin::new(2, 1));
        UiWidget::render(&Clear, vcenter, buf);
        block.render(vcenter, buf);

        let error_height = if has_error { 3 } else { 0 };
        let security_height = if show_security_row { 3 } else { 0 };
        let self_signed_height = if show_self_signed_row { 3 } else { 0 };
        let skip_verify_height = if show_skip_verify_row { 3 } else { 0 };
        let hint_height = if show_hint { 1 } else { 0 };
        let rows = Layout::vertical([
            Constraint::Length(3),                  // name
            Constraint::Length(3),                  // config path
            Constraint::Length(3),                  // version | role | reconnect (client only)
            Constraint::Length(3),                  // protocol | ip | port | path
            Constraint::Length(security_height),    // security | username | password
            Constraint::Length(self_signed_height), // self_signed (+ own-identity cert/key pair)
            Constraint::Length(hint_height),        // self-signed hint (server, below TLS)
            Constraint::Length(skip_verify_height), // skip-verify toggle (+ peer-verification input)
            Constraint::Length(error_height),       // error (hidden when empty)
            Constraint::Length(1),                  // keybinds
        ])
        .split(inner);

        render_field!(self, name, rows[0], buf);
        render_field!(self, config_path, rows[1], buf);
        render_row!(self, rows[2], buf; version, role, reconnect);

        if self.path_hidden() {
            // No URL path for the server role — let ip take the freed space.
            render_row!(self, rows[3], buf;
                protocol => Constraint::Length(12),
                ip => Constraint::Min(1),
                port => Constraint::Length(13)
            );
        } else {
            render_row!(self, rows[3], buf;
                protocol => Constraint::Length(12),
                ip => Constraint::Min(1),
                port => Constraint::Length(13),
                path => Constraint::Length(24)
            );
        }

        if show_security_row {
            if show_credentials {
                render_row!(self, rows[4], buf;
                    security=> Constraint::Percentage(25),
                    username=> Constraint::Fill(1),
                    password=> Constraint::Fill(1)
                );
            } else {
                // No credential fields: the selection is the row's only widget, so it takes
                // the full width instead of leaving two thirds blank.
                render_field!(self, security, rows[4], buf);
            }
        }

        // Self-Signed row: always Self-Signed itself, plus (when applicable) the role's own
        // identity cert/key pair sharing the same row.
        if show_self_signed_row {
            if show_identity_row {
                if is_server {
                    render_row!(self, rows[5], buf;
                        self_signed => Constraint::Percentage(25),
                        cert_file => Constraint::Fill(1),
                        key_file => Constraint::Fill(1)
                    );
                } else {
                    render_row!(self, rows[5], buf;
                        self_signed => Constraint::Percentage(25),
                        client_cert_file => Constraint::Fill(1),
                        client_key_file => Constraint::Fill(1)
                    );
                }
            } else {
                render_field!(self, self_signed, rows[5], buf);
            }
        }

        if show_hint {
            self.hint.state = "Self-signed certificate is generated at each start (clients: skip-verify or pinned certs)".to_string();
            render_field!(self, hint, rows[6], buf);
        }

        // Skip Verify row: always the Skip Verify toggle itself, plus (when applicable) the
        // peer-verification input sharing the same row.
        if show_skip_verify_row {
            if show_peer_verify_row {
                if is_server {
                    // No client-CA entries yet: give ADD the remaining width and skip DEL
                    // entirely rather than paint an empty, nothing-to-delete button.
                    if self.client_ca_files.state.values().is_empty() {
                        render_row!(self, rows[7], buf;
                            client_cert_skip_verify => Constraint::Percentage(25),
                            client_ca_files => Constraint::Percentage(60),
                            client_ca_add_button => Constraint::Fill(1)
                        );
                    } else {
                        render_row!(self, rows[7], buf;
                            client_cert_skip_verify => Constraint::Percentage(25),
                            client_ca_files => Constraint::Percentage(45),
                            client_ca_add_button => Constraint::Percentage(15),
                            client_ca_delete_button => Constraint::Fill(1)
                        );
                    }
                } else {
                    render_row!(self, rows[7], buf;
                        skip_verify => Constraint::Percentage(25),
                        ca_file => Constraint::Fill(1)
                    );
                }
            } else if is_server {
                render_field!(self, client_cert_skip_verify, rows[7], buf);
            } else {
                render_field!(self, skip_verify, rows[7], buf);
            }
        }

        if has_error {
            render_field!(self, error, rows[8], buf);
        }
        render_field!(self, keybinds, rows[9], buf);

        // Must be called after every sibling widget above has been rendered, so a popup paints on
        // top rather than being overwritten (painter's-algorithm buffer model).
        self.config_path
            .widget
            .render_overlay(area, buf, &mut self.config_path.state);
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

        if let Some(confirm) = self.close_confirm.as_mut() {
            confirm.render(vcenter, buf);
        }
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
            .allowed_for::<T>()
            .build()
            .expect("all required builder fields are set"),
        widget: InputFieldBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .title(Some((title, HorizontalAlignment::Left).into()))
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .style(style.clone())
            .build()
            .expect("all required builder fields are set"),
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
        .expect("all required builder fields are set");
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
                    .expect("all required builder fields are set"),
            )
            .popup_style(SuggestInputStyle::default())
            .build()
            .expect("all required builder fields are set"),
    }
}

fn set_suggest_text<T: Validate + Clone>(
    w: &mut Widget<SuggestInputState<FsPathProvider>, SuggestInput<T, FsPathProvider>>,
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
    aligned_selection(title, None, values, style)
}

fn aligned_selection<T: ToLabel + Clone>(
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
            .expect("all required builder fields are set"),
    }
}

/// One-line info hint (normal text style, no border) shown when a server-role `wss://` instance
/// is below the TLS level. Content is filled in at render time (see [`OcppSetupDialog::render`]).
fn hint_text() -> Widget<String, Text> {
    Widget {
        state: String::new(),
        widget: TextBuilder::default()
            .margin(Margin {
                vertical: 0,
                horizontal: 1,
            })
            .horizontal_alignment(HorizontalAlignment::Left)
            .style(TextStyle::default())
            .build()
            .expect("all required builder fields are set"),
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
            .expect("all required builder fields are set"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};
    use ferrowl_ui::traits::{IsFocus, SetFocus};

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

    fn tmp_file(name: &str) -> String {
        let path = std::env::temp_dir().join(format!("ferrowl_ocpp_setup_test_{name}"));
        std::fs::write(&path, b"").unwrap();
        path.to_str().unwrap().to_string()
    }

    #[test]
    /// OC-R-048, OC-R-107 — the setup dialog resolves a reconnect-off selection into the spec.
    fn ut_resolve_reconnect_off_maps_to_some_false() {
        let mut d = OcppSetupDialog::new(); // Client by default
        set_text(&mut d.name, "cs-1");
        d.reconnect.state.set_selection(1); // Off
        let spec = d.resolve().expect("valid client config");
        assert_eq!(spec.reconnect, Some(false));
    }

    #[test]
    /// OC-R-083 — a server-role setup reports its own reconnect setting (default On), same as
    /// the client role.
    fn ut_resolve_server_role_reports_reconnect() {
        let mut d = OcppSetupDialog::new();
        set_text(&mut d.name, "csms-1");
        d.role.state.set_selection(1); // Server
        let spec = d.resolve().expect("valid server config");
        assert_eq!(spec.reconnect, Some(true));
        d.reconnect.state.set_selection(1); // Off
        let spec = d.resolve().expect("valid server config");
        assert_eq!(spec.reconnect, Some(false));
    }

    #[test]
    /// OC-R-107 — editing an existing client spec prefills the reconnect toggle from the spec.
    fn ut_edit_prefills_reconnect_off() {
        let spec = OcppSpec {
            name: "cs-1".into(),
            version: OcppVersion::V1_6,
            role: OcppRole::Client,
            protocol: OcppProtocol::Ws,
            ip: "127.0.0.1".into(),
            port: 9000,
            path: String::new(),
            timeout_ms: None,
            reconnect: Some(false),
            security: OcppSecurityConfig::default(),
        };
        let dialog = OcppSetupDialog::edit(&spec, "device.toml");
        assert_eq!(dialog.reconnect.state.get_value(), ReconnectChoice::Off);
        let resolved = dialog.resolve().expect("valid client config");
        assert_eq!(resolved.reconnect, Some(false));
    }

    #[test]
    /// UI-R-022 — the focus cycle reaches the reconnect field for a server role too (OC-R-083).
    fn ut_focus_next_reaches_reconnect_for_server_role() {
        let mut d = OcppSetupDialog::new();
        d.role.state.set_selection(1); // Server
        d.set_focused(true);
        let mut visited = false;
        for _ in 0..20 {
            d.focus_next();
            if d.reconnect.state.is_focused() {
                visited = true;
                break;
            }
        }
        assert!(
            visited,
            "reconnect field must be reachable for a server role"
        );
    }

    // Regression: editing a ws module whose device file carries a security section (Basic Auth
    // over plain ws is valid, config-file-only) must hand that section back unchanged — the
    // security UI is hidden for ws, and a hidden section must never clobber the file.
    #[test]
    /// UI-R-024 — a ws setup resolves preserving the prefilled security.
    fn ut_resolve_ws_preserves_prefilled_security() {
        let security = OcppSecurityConfig {
            username: Some("cp001".into()),
            password: Some("secret".into()),
            ..Default::default()
        };
        let spec = OcppSpec {
            name: "cs-1".into(),
            version: OcppVersion::V1_6,
            role: OcppRole::Client,
            protocol: OcppProtocol::Ws,
            ip: "127.0.0.1".into(),
            port: 9000,
            path: String::new(),
            timeout_ms: None,
            reconnect: None,
            security: security.clone(),
        };
        let d = OcppSetupDialog::edit(&spec, "");
        let resolved = d.resolve().expect("ws edit resolves");
        assert_eq!(resolved.security, security);
    }

    // --- dialog-level validation ---------------------------------------------------------------

    fn wss_dialog(role_idx: usize) -> OcppSetupDialog {
        let mut d = OcppSetupDialog::new();
        set_text(&mut d.name, "cs-1");
        d.protocol.state.set_selection(1); // Wss
        d.role.state.set_selection(role_idx);
        d
    }

    // --- OC-R-110/OC-R-111: Self-Signed / Skip-Verify toggle parity with the Modbus dialog -----

    #[test]
    /// OC-R-110 — the Self-Signed toggle is shown only for a wss server at TLS level or above.
    fn ut_show_self_signed_only_at_tls_and_above_for_server() {
        let mut d = wss_dialog(1); // Server
        d.security.state.set_selection(SecurityLevel::Tls.index());
        assert!(d.show_self_signed());

        d.security
            .state
            .set_selection(SecurityLevel::BasicAuth.index());
        assert!(!d.show_self_signed());
    }

    #[test]
    /// OC-R-110 — toggling Self-Signed On hides the server cert/key row.
    fn ut_self_signed_hides_server_cert_row() {
        let mut d = wss_dialog(1); // Server
        d.security.state.set_selection(SecurityLevel::Tls.index());
        assert!(d.show_server_cert());
        d.self_signed.state.set_selection(1); // On
        assert!(!d.show_server_cert());
        d.self_signed.state.set_selection(0); // Off again
        assert!(d.show_server_cert());
    }

    #[test]
    /// OC-R-110 — toggling Self-Signed On excludes stale cert_file/key_file text from the
    /// resolved config, even though the widgets' stored text is untouched (mirrors the Modbus
    /// dialog's MB-R-135 fix).
    fn ut_resolve_self_signed_excludes_stale_cert_key_text() {
        let mut d = wss_dialog(1); // Server
        d.security.state.set_selection(SecurityLevel::Tls.index());
        set_suggest_text(&mut d.cert_file, "s.crt");
        set_suggest_text(&mut d.key_file, "s.key");
        d.self_signed.state.set_selection(1); // On, after the text was typed

        let spec = d.resolve().expect("self-signed needs no cert/key files");
        assert_eq!(
            spec.security.server,
            ServerTlsPolicy::Tls {
                server_cert: ServerCertSource::SelfSigned
            }
        );
        // The stored text survives the toggle -- only the resolved config excludes it.
        assert_eq!(d.cert_file.state.input(), "s.crt");
        assert_eq!(d.key_file.state.input(), "s.key");
    }

    #[test]
    /// OC-R-110 — Self-Signed On at TLS level needs no cert/key files to resolve successfully
    /// (closes the gap `validate_security` used to require them unconditionally at Tls+).
    fn ut_validate_security_self_signed_needs_no_cert_files() {
        let mut d = wss_dialog(1); // Server
        d.security.state.set_selection(SecurityLevel::Tls.index());
        d.self_signed.state.set_selection(1); // On
        assert!(d.resolve().is_ok());
    }

    #[test]
    /// OC-R-111 — toggling Skip-Verify On hides the CA-file row.
    fn ut_skip_verify_hides_ca_file_row() {
        let mut d = wss_dialog(0); // Client
        d.security.state.set_selection(SecurityLevel::Tls.index());
        assert!(d.show_ca_file());
        d.skip_verify.state.set_selection(1); // On
        assert!(!d.show_ca_file());
        d.skip_verify.state.set_selection(0); // Off again
        assert!(d.show_ca_file());
    }

    #[test]
    /// OC-R-111 — toggling Skip-Verify On excludes stale ca_file text from the resolved config,
    /// even though the widget's stored text is untouched.
    fn ut_resolve_skip_verify_excludes_stale_ca_file_text() {
        let mut d = wss_dialog(0); // Client
        d.security.state.set_selection(SecurityLevel::Tls.index());
        set_suggest_text(&mut d.ca_file, "ca.pem");
        d.skip_verify.state.set_selection(1); // On, after the text was typed

        let spec = d.resolve().expect("skip-verify needs no ca file");
        assert_eq!(
            spec.security.client,
            ClientTlsPolicy::Tls {
                client_verification: ClientVerification::SkipVerify
            }
        );
        assert_eq!(d.ca_file.state.input(), "ca.pem");
    }

    #[test]
    /// UI-R-024 — a wss server with no TLS material resolves (self-signed) without a validation error.
    fn ut_server_wss_none_resolves_self_signed_no_cert_error() {
        let d = wss_dialog(1); // Server, security level defaults to None
        let spec = d
            .resolve()
            .expect("below-TLS server should self-sign, not error");
        assert_eq!(
            spec.security.server,
            ServerTlsPolicy::Tls {
                server_cert: ServerCertSource::SelfSigned
            }
        );
    }

    #[test]
    /// UI-R-024 — a wss server with basic auth resolves without a validation error.
    fn ut_server_wss_basic_auth_resolves_self_signed_no_cert_error() {
        let mut d = wss_dialog(1); // Server
        d.security
            .state
            .set_selection(SecurityLevel::BasicAuth.index());
        set_text(&mut d.username, "cp001");
        set_text(&mut d.password, "s3cret");
        let spec = d
            .resolve()
            .expect("below-TLS server should self-sign, not error");
        assert_eq!(
            spec.security.server,
            ServerTlsPolicy::Tls {
                server_cert: ServerCertSource::SelfSigned
            }
        );
        assert_eq!(spec.security.username.as_deref(), Some("cp001"));
    }

    #[test]
    /// UI-R-024 — a server TLS setup missing its cert fails validation and keeps the dialog open.
    fn ut_server_tls_missing_cert_is_rejected() {
        let mut d = wss_dialog(1);
        d.security.state.set_selection(SecurityLevel::Tls.index());
        let err = d.resolve().unwrap_err();
        assert!(err.contains("Certificate file is required"), "{err}");
    }

    #[test]
    /// UI-R-024 — a server TLS setup with a nonexistent cert file fails validation.
    fn ut_server_tls_nonexistent_cert_is_rejected() {
        let mut d = wss_dialog(1);
        d.security.state.set_selection(SecurityLevel::Tls.index());
        set_suggest_text(&mut d.cert_file, "/no/such/cert.crt");
        set_suggest_text(&mut d.key_file, "/no/such/key.key");
        let err = d.resolve().unwrap_err();
        assert!(err.contains("Certificate file not found"), "{err}");
    }

    #[test]
    /// UI-R-024 — a server TLS setup with valid files passes validation.
    fn ut_server_tls_valid_files_pass() {
        let cert = tmp_file("cert.crt");
        let key = tmp_file("key.key");
        let mut d = wss_dialog(1);
        d.security.state.set_selection(SecurityLevel::Tls.index());
        set_suggest_text(&mut d.cert_file, &cert);
        set_suggest_text(&mut d.key_file, &key);
        assert!(d.resolve().is_ok());
    }

    #[test]
    /// UI-R-024 — a mutual-TLS server missing its client CA fails validation.
    fn ut_server_mutual_tls_missing_client_ca_is_rejected() {
        let cert = tmp_file("cert2.crt");
        let key = tmp_file("key2.key");
        let mut d = wss_dialog(1);
        d.security
            .state
            .set_selection(SecurityLevel::MutualTls.index());
        set_suggest_text(&mut d.cert_file, &cert);
        set_suggest_text(&mut d.key_file, &key);
        let err = d.resolve().unwrap_err();
        assert!(err.contains("Client CA list is required"), "{err}");
    }

    fn type_into(state: &mut ferrowl_ui::state::InputFieldState, s: &str) {
        state.set_focused(true);
        for c in s.chars() {
            state.handle_events(KeyModifiers::NONE, KeyCode::Char(c));
        }
    }

    #[test]
    /// OC-R-113 — the client-CA row is a genuine add/remove list: the ADD button opens a
    /// sub-dialog whose confirmed path is appended and selected, and the DEL button removes
    /// whichever entry is currently selected — not a comma-separated text field.
    fn ut_client_ca_files_add_remove_edit() {
        let cert = tmp_file("mca_cert.crt");
        let key = tmp_file("mca_key.key");
        let ca1 = tmp_file("mca_ca1.pem");
        let ca2 = tmp_file("mca_ca2.pem");
        let mut d = wss_dialog(1); // Server
        d.security
            .state
            .set_selection(SecurityLevel::MutualTls.index());
        set_suggest_text(&mut d.cert_file, &cert);
        set_suggest_text(&mut d.key_file, &key);

        assert!(d.client_ca_files.state.values().is_empty());

        // ADD: open the sub-dialog, type a path, confirm with Enter.
        d.focus = OcppSetupDialogFocus::ClientCaAddButton;
        d.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(d.client_ca_add_dialog.is_some());
        type_into(
            &mut d.client_ca_add_dialog.as_mut().unwrap().path.state,
            &ca1,
        );
        d.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(d.client_ca_add_dialog.is_none());
        assert_eq!(d.client_ca_files.state.values(), &[ca1.clone()]);

        // ADD a second entry.
        d.focus = OcppSetupDialogFocus::ClientCaAddButton;
        d.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        type_into(
            &mut d.client_ca_add_dialog.as_mut().unwrap().path.state,
            &ca2,
        );
        d.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert_eq!(
            d.client_ca_files.state.values(),
            &[ca1.clone(), ca2.clone()]
        );

        // A path that doesn't exist on disk is rejected: the sub-dialog stays open with an
        // error, nothing appended (only a file present on disk can be confirmed).
        d.focus = OcppSetupDialogFocus::ClientCaAddButton;
        d.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        type_into(
            &mut d.client_ca_add_dialog.as_mut().unwrap().path.state,
            "/nonexistent/ca-does-not-exist.pem",
        );
        d.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(d.client_ca_add_dialog.is_some());
        assert!(
            !d.client_ca_add_dialog
                .as_ref()
                .unwrap()
                .error
                .state
                .is_empty()
        );
        d.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(d.client_ca_add_dialog.is_none());
        assert_eq!(
            d.client_ca_files.state.values(),
            &[ca1.clone(), ca2.clone()]
        );

        let spec = d.resolve().expect("two CAs resolve");
        match spec.security.server {
            ServerTlsPolicy::MutualTls {
                client_verification: ClientCertVerification::Verify { ca_files },
                ..
            } => assert_eq!(ca_files, vec![ca1.clone(), ca2.clone()]),
            other => panic!("expected MutualTls with Verify, got {other:?}"),
        }

        // DEL: remove the currently-selected entry (selection sits on the last-added item).
        assert_eq!(d.client_ca_files.state.selection(), 1);
        d.focus = OcppSetupDialogFocus::ClientCaDeleteButton;
        d.handle_events(KeyModifiers::NONE, KeyCode::Char(' '));
        assert_eq!(d.client_ca_files.state.values(), &[ca1.clone()]);

        let spec = d.resolve().expect("one CA resolves");
        match spec.security.server {
            ServerTlsPolicy::MutualTls {
                client_verification: ClientCertVerification::Verify { ca_files },
                ..
            } => assert_eq!(ca_files, vec![ca1]),
            other => panic!("expected MutualTls with Verify, got {other:?}"),
        }

        // Remove all: needs Skip Verify on to resolve without error.
        d.client_ca_delete_button.state.set_focused(true);
        d.handle_events(KeyModifiers::NONE, KeyCode::Char(' '));
        assert!(d.client_ca_files.state.values().is_empty());
        // Deleting down to an empty list must not leave `focus` stuck on the now-ineligible DEL
        // button — it falls back to ADD, so Tab from there keeps working. The fallback must also
        // move the widget-level highlight, not just the tracking enum, or DEL stays visually
        // focused (though hidden) and ADD stays unhighlighted until the next real Tab press.
        assert_eq!(d.focus, OcppSetupDialogFocus::ClientCaAddButton);
        assert!(!d.client_ca_delete_button.state.is_focused());
        assert!(d.client_ca_add_button.state.is_focused());
        assert!(d.resolve().is_err());
        d.client_cert_skip_verify.state.set_selection(1); // On
        let spec = d.resolve().expect("skip-verify needs no CA list");
        assert_eq!(
            spec.security.server,
            ServerTlsPolicy::MutualTls {
                server_cert: ServerCertSource::Explicit {
                    cert_file: cert,
                    key_file: key,
                },
                client_verification: ClientCertVerification::SkipVerify,
            }
        );
    }

    #[test]
    /// OC-R-116 — the client role's Self-Signed toggle is shown only at MutualTls, and excludes
    /// stale client-cert/key text from the resolved config when on.
    fn ut_client_self_signed_shown_only_at_mutual_tls_and_excludes_stale_cert_key() {
        let mut d = wss_dialog(0); // Client
        d.security.state.set_selection(SecurityLevel::Tls.index());
        assert!(!d.show_self_signed());

        d.security
            .state
            .set_selection(SecurityLevel::MutualTls.index());
        assert!(d.show_self_signed());
        set_suggest_text(&mut d.client_cert_file, "stale.crt");
        set_suggest_text(&mut d.client_key_file, "stale.key");
        d.self_signed.state.set_selection(1); // On, after the text was typed

        let spec = d
            .resolve()
            .expect("self-signed client needs no cert/key files");
        assert_eq!(
            spec.security.client,
            ClientTlsPolicy::MutualTls {
                client_verification: ClientVerification::default(),
                client_identity: ClientCertSource::SelfSigned,
            }
        );
        assert_eq!(d.client_cert_file.state.input(), "stale.crt");
        assert_eq!(d.client_key_file.state.input(), "stale.key");
    }

    #[test]
    /// OC-R-113 — the server-role client-cert-skip-verify toggle is shown only at MutualTls and
    /// hides the client-CA list row when on.
    fn ut_server_client_cert_skip_verify_shown_only_at_mutual_tls_hides_ca_list() {
        let mut d = wss_dialog(1); // Server
        d.security.state.set_selection(SecurityLevel::Tls.index());
        assert!(!d.show_client_cert_skip_verify());

        d.security
            .state
            .set_selection(SecurityLevel::MutualTls.index());
        assert!(d.show_client_cert_skip_verify());
        assert!(d.show_client_ca());
        d.client_cert_skip_verify.state.set_selection(1); // On
        assert!(!d.show_client_ca());
    }

    #[test]
    /// UI-R-024 — a mutual-TLS client missing its cert/key fails validation.
    fn ut_client_mutual_tls_missing_cert_key_is_rejected() {
        let mut d = wss_dialog(0); // Client
        d.security
            .state
            .set_selection(SecurityLevel::MutualTls.index());
        let err = d.resolve().unwrap_err();
        // `ClientCertSource::resolve` itself rejects "neither cert nor key nor self-signed" before
        // `validate_security` ever runs (mirrors the Modbus dialog's `build_config`, which resolves
        // the client identity the same way) — the raw resolver message, not `validate_security`'s
        // own (now unreachable for this exact case) "Client certificate file is required" text.
        assert!(
            err.contains("client_cert_file and client_key_file must both be set"),
            "{err}"
        );
    }

    #[test]
    /// UI-R-024 — a client CA file, when set, must exist to pass validation.
    fn ut_client_ca_file_when_set_must_exist() {
        let mut d = wss_dialog(0);
        d.security.state.set_selection(SecurityLevel::Tls.index());
        set_suggest_text(&mut d.ca_file, "/no/such/ca.pem");
        let err = d.resolve().unwrap_err();
        assert!(err.contains("CA file not found"), "{err}");
    }

    #[test]
    /// UI-R-024 — a wss client with no TLS material passes validation.
    fn ut_client_wss_none_is_allowed() {
        let d = wss_dialog(0); // Client, level defaults to None
        assert!(d.resolve().is_ok());
    }

    #[test]
    /// UI-R-024 — a ws setup never requires security material.
    fn ut_ws_never_requires_security() {
        let mut d = OcppSetupDialog::new(); // Ws, Client by default
        set_text(&mut d.name, "cs-1");
        let spec = d.resolve().unwrap();
        assert_eq!(spec.security, OcppSecurityConfig::default());
    }

    // --- edit -> resolve round trip ------------------------------------------------------------

    #[test]
    /// UI-R-024 — Edit mode round-trips a mutual-TLS server config through the dialog.
    fn ut_edit_resolve_roundtrip_mutual_tls_server() {
        let cert = tmp_file("rt_cert.crt");
        let key = tmp_file("rt_key.key");
        let cca = tmp_file("rt_cca.pem");
        let spec = OcppSpec {
            name: "csms-1".into(),
            version: OcppVersion::V2_0_1,
            role: OcppRole::Server,
            protocol: OcppProtocol::Wss,
            ip: "127.0.0.1".into(),
            port: 9443,
            path: String::new(),
            timeout_ms: None,
            reconnect: None,
            security: OcppSecurityConfig {
                server: ServerTlsPolicy::MutualTls {
                    server_cert: ServerCertSource::Explicit {
                        cert_file: cert,
                        key_file: key,
                    },
                    client_verification: ClientCertVerification::Verify {
                        ca_files: vec![cca],
                    },
                },
                ..Default::default()
            },
        };
        let dialog = OcppSetupDialog::edit(&spec, "device.toml");
        let resolved = dialog.resolve().expect("valid mTLS server config");
        assert_eq!(resolved.security, spec.security);
    }

    #[test]
    /// UI-R-024 — Edit mode round-trips a skip-verify client config through the dialog.
    fn ut_edit_resolve_roundtrip_client_skip_verify() {
        let spec = OcppSpec {
            name: "cp-1".into(),
            version: OcppVersion::V1_6,
            role: OcppRole::Client,
            protocol: OcppProtocol::Wss,
            ip: "127.0.0.1".into(),
            port: 9000,
            path: "/ocpp/cp001".into(),
            timeout_ms: None,
            reconnect: None,
            security: OcppSecurityConfig {
                client: ClientTlsPolicy::Tls {
                    client_verification: ClientVerification::SkipVerify,
                },
                ..Default::default()
            },
        };
        let dialog = OcppSetupDialog::edit(&spec, "device.toml");
        assert_eq!(dialog.skip_verify.state.get_value(), SkipVerifyChoice::On);
        let resolved = dialog.resolve().expect("valid client config");
        assert_eq!(
            resolved.security.client,
            ClientTlsPolicy::Tls {
                client_verification: ClientVerification::SkipVerify
            }
        );
    }

    // --- render height -----------------------------------------------------------------------

    #[test]
    /// UI-R-024 — the TLS hint row renders only for the server role.
    fn ut_render_hint_row_only_for_server_below_tls() {
        let area = Rect::new(0, 0, 80, 60);

        // Server, wss, below TLS: hint row present.
        let mut with_hint = wss_dialog(1);
        let mut buf = Buffer::empty(area);
        with_hint.render(area, &mut buf);
        let with_hint_text = buffer_text(&buf);
        assert!(
            with_hint_text.contains("Self-signed certificate is generated at each start"),
            "missing hint line:\n{with_hint_text}"
        );

        // Server, wss, Tls: no hint row (real cert/key required instead).
        let cert = tmp_file("hint_cert.crt");
        let key = tmp_file("hint_key.key");
        let mut without_hint = wss_dialog(1);
        without_hint
            .security
            .state
            .set_selection(SecurityLevel::Tls.index());
        set_suggest_text(&mut without_hint.cert_file, &cert);
        set_suggest_text(&mut without_hint.key_file, &key);
        let mut buf2 = Buffer::empty(area);
        without_hint.render(area, &mut buf2);
        let without_hint_text = buffer_text(&buf2);
        assert!(!without_hint_text.contains("Self-signed certificate is generated"));

        // Client, wss, below TLS: no hint row (hint is server-only).
        let mut client = wss_dialog(0);
        let mut buf3 = Buffer::empty(area);
        client.render(area, &mut buf3);
        let client_text = buffer_text(&buf3);
        assert!(!client_text.contains("Self-signed certificate is generated"));
    }

    // --- focus traversal ------------------------------------------------------------------------

    #[test]
    /// UI-R-022 — a ws selection hides (skips) all security fields in the focus cycle.
    fn ut_focus_ws_hides_all_security_fields() {
        let mut d = OcppSetupDialog::new(); // Ws by default
        d.set_focused(true);
        assert_eq!(d.focus, OcppSetupDialogFocus::Name);
        // Cycle through every focusable slot; none should land on a security field while Ws.
        for _ in 0..20 {
            d.focus_next();
            assert!(!matches!(
                d.focus,
                OcppSetupDialogFocus::Security
                    | OcppSetupDialogFocus::Username
                    | OcppSetupDialogFocus::Password
                    | OcppSetupDialogFocus::SkipVerify
                    | OcppSetupDialogFocus::CaFile
                    | OcppSetupDialogFocus::CertFile
                    | OcppSetupDialogFocus::KeyFile
                    | OcppSetupDialogFocus::ClientCertFile
                    | OcppSetupDialogFocus::ClientKeyFile
                    | OcppSetupDialogFocus::ClientCaFiles
            ));
        }
    }

    #[test]
    /// UI-R-022 — a wss client focus cycle includes the security selection at level None, but
    /// not the skip-verify field (OC-R-111 hides it below TLS).
    fn ut_focus_wss_none_shows_security_selection_for_client() {
        let mut d = wss_dialog(0); // Client, wss, level None
        d.set_focused(true);
        let mut visited = Vec::new();
        for _ in 0..20 {
            d.focus_next();
            visited.push(d.focus);
        }
        assert!(visited.contains(&OcppSetupDialogFocus::Security));
        assert!(!visited.contains(&OcppSetupDialogFocus::SkipVerify));
        assert!(!visited.contains(&OcppSetupDialogFocus::Username));
        assert!(!visited.contains(&OcppSetupDialogFocus::CaFile));
    }

    #[test]
    /// OC-R-111 — the client Skip-Verify toggle is hidden under `None`/`BasicAuth` and shown at
    /// `Tls`/`MutualTls`.
    fn ut_skip_verify_toggle_hidden_under_none_and_basic_auth_shown_at_tls_and_above() {
        let mut d = wss_dialog(0); // Client

        d.security.state.set_selection(SecurityLevel::None.index());
        assert!(!d.show_skip_verify());

        d.security
            .state
            .set_selection(SecurityLevel::BasicAuth.index());
        assert!(!d.show_skip_verify());

        d.security.state.set_selection(SecurityLevel::Tls.index());
        assert!(d.show_skip_verify());

        d.security
            .state
            .set_selection(SecurityLevel::MutualTls.index());
        assert!(d.show_skip_verify());
    }

    #[test]
    /// UI-R-022 — a wss server focus cycle omits the skip-verify field.
    fn ut_focus_wss_none_server_has_no_skip_verify() {
        let mut d = wss_dialog(1); // Server, wss, level None
        d.set_focused(true);
        let mut visited = Vec::new();
        for _ in 0..20 {
            d.focus_next();
            visited.push(d.focus);
        }
        assert!(visited.contains(&OcppSetupDialogFocus::Security));
        assert!(!visited.contains(&OcppSetupDialogFocus::SkipVerify));
    }

    #[test]
    /// UI-R-022 — a mutual-TLS server focus cycle reaches the client-CA field.
    fn ut_focus_wss_mutual_tls_server_reaches_client_ca_file() {
        let mut d = wss_dialog(1); // Server
        d.security
            .state
            .set_selection(SecurityLevel::MutualTls.index());
        // `client_ca_files` is focus-eligible only when non-empty (post-s11: an empty list hides
        // the field, matching its no-longer-rendered DEL button) — populate it so this cycle
        // actually reaches it.
        d.client_ca_files
            .state
            .set_values(vec!["ca1.pem".to_string()]);
        d.set_focused(true);
        let mut visited = Vec::new();
        for _ in 0..20 {
            d.focus_next();
            visited.push(d.focus);
        }
        assert!(visited.contains(&OcppSetupDialogFocus::ClientCaFiles));
        assert!(visited.contains(&OcppSetupDialogFocus::CertFile));
        assert!(visited.contains(&OcppSetupDialogFocus::KeyFile));
        assert!(!visited.contains(&OcppSetupDialogFocus::ClientCertFile));
    }

    /// Typing into the config-path field opens the filesystem suggestion popup, and the popup is
    /// drawn on top of the dialog by the trailing `render_overlay` calls in `render`.
    #[test]
    /// UI-R-026 — the config-path field shows a completion popup.
    fn ut_render_config_path_field_shows_suggestion_popup() {
        let mut dialog = OcppSetupDialog::new();
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

    // --- close-confirm --------------------------------------------------------------------------

    #[test]
    /// UI-R-023 — Esc-then-Enter sets the close request, which clears after being taken.
    fn ut_take_close_request_set_via_esc_enter_and_cleared_after_take() {
        let mut dialog = OcppSetupDialog::new();
        assert!(!dialog.take_close_request());
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(dialog.close_confirm.is_some());
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(dialog.take_close_request());
        assert!(!dialog.take_close_request(), "flag must clear after take");
    }

    #[test]
    /// UI-R-023 — Esc in the close-confirm keeps the setup dialog open.
    fn ut_esc_in_confirm_keeps_open() {
        let mut dialog = OcppSetupDialog::new();
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(dialog.close_confirm.is_some());
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(dialog.close_confirm.is_none());
        assert!(!dialog.take_close_request());
    }

    #[test]
    /// UI-R-014 — `:` types into a setup text field rather than entering command mode.
    fn ut_colon_in_text_input_types() {
        let mut dialog = OcppSetupDialog::new();
        // Default focus is Name, a free-text field; `:` must be typed as ordinary text.
        dialog.handle_events(KeyModifiers::NONE, KeyCode::Char(':'));
        assert_eq!(dialog.name.state.input(), ":");
        assert!(dialog.close_confirm.is_none());
    }

    // --- post-gate3 row-layout refinement ---------------------------------------------------

    fn row_of(buf: &Buffer, needle: &str) -> u16 {
        let text = buffer_text(buf);
        text.lines()
            .position(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} not found in:\n{text}")) as u16
    }

    #[test]
    /// OC-R-110, OC-R-113, OC-R-116 — mTLS row order, server role: the security row carries only
    /// security/username/password (no side toggle); Self-Signed shares a row with the server's
    /// own cert/key pair; Skip Verify shares a row with the client-CA list.
    fn ut_mtls_row_order_server() {
        let mut d = wss_dialog(1); // Server
        d.security
            .state
            .set_selection(SecurityLevel::MutualTls.index());
        let area = Rect::new(0, 0, 80, 60);
        let mut buf = Buffer::empty(area);
        d.render(area, &mut buf);
        let text = buffer_text(&buf);
        let security_row = row_of(&buf, "Security");
        let self_signed_row = row_of(&buf, "Self-Signed");
        let cert_row = row_of(&buf, "Cert File");
        let skip_row = row_of(&buf, "Skip Verify");
        let ca_row = row_of(&buf, "Client CA(s)");
        assert!(
            security_row < self_signed_row,
            "security must render before self-signed:\n{text}"
        );
        assert_eq!(
            self_signed_row, cert_row,
            "self-signed and the own-identity cert/key pair must share a row:\n{text}"
        );
        assert!(
            self_signed_row < skip_row,
            "self-signed must render before skip-verify:\n{text}"
        );
        assert_eq!(
            skip_row, ca_row,
            "skip-verify and the client-CA list must share a row:\n{text}"
        );
    }

    #[test]
    /// OC-R-110, OC-R-111, OC-R-116 — mTLS row order, client role: the security row carries only
    /// security/username/password; Self-Signed shares a row with the client's own cert/key pair;
    /// Skip Verify shares a row with the CA-file input.
    fn ut_mtls_row_order_client() {
        let mut d = wss_dialog(0); // Client
        d.security
            .state
            .set_selection(SecurityLevel::MutualTls.index());
        let area = Rect::new(0, 0, 80, 60);
        let mut buf = Buffer::empty(area);
        d.render(area, &mut buf);
        let text = buffer_text(&buf);
        let security_row = row_of(&buf, "Security");
        let self_signed_row = row_of(&buf, "Self-Signed");
        let cert_row = row_of(&buf, "Client Cert");
        let skip_row = row_of(&buf, "Skip Verify");
        let ca_row = row_of(&buf, "CA File");
        assert!(
            security_row < self_signed_row,
            "security must render before self-signed:\n{text}"
        );
        assert_eq!(
            self_signed_row, cert_row,
            "self-signed and the client's own cert/key pair must share a row:\n{text}"
        );
        assert!(
            self_signed_row < skip_row,
            "self-signed must render before skip-verify:\n{text}"
        );
        assert_eq!(
            skip_row, ca_row,
            "skip-verify and the CA-file input must share a row:\n{text}"
        );
    }

    #[test]
    /// UI-R-024 — the security row no longer carries a side toggle: Self-Signed/Skip Verify
    /// never appear on the same line as Username/Password.
    fn ut_security_row_has_no_side_toggle() {
        let mut d = wss_dialog(1); // Server
        d.security.state.set_selection(SecurityLevel::Tls.index());
        let area = Rect::new(0, 0, 80, 60);
        let mut buf = Buffer::empty(area);
        d.render(area, &mut buf);
        let text = buffer_text(&buf);
        let security_line = text
            .lines()
            .find(|l| l.contains("Security"))
            .expect("security row present");
        assert!(
            !security_line.contains("Self-Signed"),
            "security row still carries the side toggle:\n{text}"
        );
    }

    #[test]
    /// UI-R-024 — an empty client-CA list shows no placeholder entry, and the DEL button is not
    /// rendered at all, so ADD gets the row's full width (mirrors the Modbus dialog).
    fn ut_client_ca_empty_hides_delete_button() {
        let mut d = wss_dialog(1); // Server
        d.security
            .state
            .set_selection(SecurityLevel::MutualTls.index());
        assert!(d.client_ca_files.state.values().is_empty());
        let area = Rect::new(0, 0, 80, 60);
        let mut buf = Buffer::empty(area);
        d.render(area, &mut buf);
        let text = buffer_text(&buf);
        assert!(
            !text.contains("DEL"),
            "DEL button rendered with an empty client-CA list:\n{text}"
        );
        assert!(text.contains("ADD"), "ADD button missing:\n{text}");
    }

    #[test]
    /// UI-R-024 — the client-CA row's DEL button hugs the dialog's right inner edge with no
    /// trailing dead space, matching every other full-width row (mirrors the Modbus dialog).
    fn ut_client_ca_delete_button_hugs_right_edge() {
        let mut d = wss_dialog(1); // Server
        d.security
            .state
            .set_selection(SecurityLevel::MutualTls.index());
        d.client_ca_files
            .state
            .set_values(vec!["ca1.pem".to_string()]);
        let area = Rect::new(0, 0, 80, 60);
        let mut buf = Buffer::empty(area);
        d.render(area, &mut buf);

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
    /// UI-R-024 — the client-CA list's row stays a fixed 3 rows tall regardless of entry count;
    /// more entries scroll/clip, never grow the box (mirrors the Modbus dialog).
    fn ut_client_ca_row_height_fixed_regardless_of_entry_count() {
        let mut d = wss_dialog(1); // Server
        d.security
            .state
            .set_selection(SecurityLevel::MutualTls.index());
        d.client_ca_files
            .state
            .set_values((0..10).map(|i| format!("ca{i}.pem")).collect::<Vec<_>>());
        let area = Rect::new(0, 0, 80, 60);
        let mut buf = Buffer::empty(area);
        d.render(area, &mut buf);
        let text = buffer_text(&buf);
        let ca_row = row_of(&buf, "Client CA(s)");
        let keybinds_row = row_of(&buf, "Esc");
        // The client-CA box is 1 content row + 2 border rows, immediately followed by the
        // error row (0 or 3 lines) and keybinds; with >3 entries the extras scroll/clip, they
        // must never push keybinds further down than a fixed 3-row box would.
        assert!(
            keybinds_row - ca_row <= 6,
            "client-CA row appears to have grown beyond a fixed 3-row box:\n{text}"
        );
    }
}
