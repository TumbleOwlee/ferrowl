//! OCPP module setup dialog (`:new`). Collects name, version, role, protocol, the websocket
//! endpoint (ip/port), and — for `wss://` — a security level (Basic Auth / TLS / mTLS) with its
//! credential/certificate fields, validating live like the Modbus dialog.

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

use crate::dialog::NonEmpty;
use crate::dialog::path_suggest::FsPathProvider;
use crate::module::ocpp::config::device::OcppSecurityConfig;
use crate::module::ocpp::config::session::{OcppProtocol, OcppRole, OcppSpec, OcppVersion};

/// Websocket transport security level, offered only when the protocol is `wss://`. Cumulative:
/// each level's fields are a superset of the one below it (`BasicAuth` fields are also shown, and
/// still apply, at `Tls` and `MutualTls`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SecurityLevel {
    None,
    BasicAuth,
    Tls,
    MutualTls,
}

impl ToLabel for SecurityLevel {
    fn to_label(&self) -> String {
        match self {
            SecurityLevel::None => "None",
            SecurityLevel::BasicAuth => "Basic Auth",
            SecurityLevel::Tls => "TLS",
            SecurityLevel::MutualTls => "mTLS",
        }
        .to_string()
    }
}

/// Client-only "accept any server certificate" toggle, offered whenever `wss://` is selected
/// (orthogonal to the security level — even a Basic-Auth-only connection may need it against a
/// self-signed CSMS). Mirrors `ReconnectChoice` in the Modbus dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipVerifyChoice {
    Off,
    On,
}

impl ToLabel for SkipVerifyChoice {
    fn to_label(&self) -> String {
        match self {
            SkipVerifyChoice::Off => "Off",
            SkipVerifyChoice::On => "On",
        }
        .to_string()
    }
}

/// Raw text of every security input field, passed by name so the many look-alike path fields
/// cannot be transposed at a call site (a swapped positional pair would compile and only fail at
/// TLS-handshake time).
pub struct SecurityInputs<'a> {
    pub username: &'a str,
    pub password: &'a str,
    pub ca_file: &'a str,
    pub cert_file: &'a str,
    pub key_file: &'a str,
    pub client_cert_file: &'a str,
    pub client_key_file: &'a str,
    pub client_ca_file: &'a str,
}

impl SecurityLevel {
    /// Infer the level an existing [`OcppSecurityConfig`] represents, by role. Precedence (highest
    /// first): client cert (client) / require-client-cert or client CA (server) → `MutualTls`;
    /// cert+key (server) / CA file (client) → `Tls`; username → `BasicAuth`; else `None`.
    pub fn from_config(cfg: &OcppSecurityConfig, role: OcppRole) -> SecurityLevel {
        match role {
            OcppRole::Client => {
                if cfg.client_cert_file.is_some() {
                    SecurityLevel::MutualTls
                } else if cfg.ca_file.is_some() {
                    SecurityLevel::Tls
                } else if cfg.username.is_some() {
                    SecurityLevel::BasicAuth
                } else {
                    SecurityLevel::None
                }
            }
            OcppRole::Server => {
                if cfg.require_client_cert || cfg.client_ca_file.is_some() {
                    SecurityLevel::MutualTls
                } else if cfg.cert_file.is_some() || cfg.key_file.is_some() {
                    SecurityLevel::Tls
                } else if cfg.username.is_some() {
                    SecurityLevel::BasicAuth
                } else {
                    SecurityLevel::None
                }
            }
        }
    }

    /// Build the resolved [`OcppSecurityConfig`] for this level/role from raw field text, so a
    /// field not visible at this level/role (e.g. `client_cert_file` at `Tls`) is dropped rather
    /// than smuggled through from a stale input.
    pub fn build_config(self, role: OcppRole, inputs: SecurityInputs<'_>) -> OcppSecurityConfig {
        let SecurityInputs {
            username,
            password,
            ca_file,
            cert_file,
            key_file,
            client_cert_file,
            client_key_file,
            client_ca_file,
        } = inputs;
        let opt = |s: &str| {
            let t = s.trim();
            (!t.is_empty()).then(|| t.to_string())
        };
        let basic = self >= SecurityLevel::BasicAuth;
        let tls = self >= SecurityLevel::Tls;
        let mtls = self == SecurityLevel::MutualTls;
        let is_client = role == OcppRole::Client;
        let is_server = role == OcppRole::Server;
        OcppSecurityConfig {
            username: if basic { opt(username) } else { None },
            password: if basic { opt(password) } else { None },
            ca_file: if tls && is_client { opt(ca_file) } else { None },
            cert_file: if tls && is_server {
                opt(cert_file)
            } else {
                None
            },
            key_file: if tls && is_server {
                opt(key_file)
            } else {
                None
            },
            client_cert_file: if mtls && is_client {
                opt(client_cert_file)
            } else {
                None
            },
            client_key_file: if mtls && is_client {
                opt(client_key_file)
            } else {
                None
            },
            client_ca_file: if mtls && is_server {
                opt(client_ca_file)
            } else {
                None
            },
            require_client_cert: mtls && is_server,
            // Set by the caller (`resolve`), which knows the role/level rule for `self_signed`
            // and reads the dialog's `skip_verify` toggle for `insecure_skip_verify` — neither is
            // derivable from the raw field text this function works from.
            self_signed: false,
            insecure_skip_verify: false,
        }
    }

    /// Index into the `security` selection's value list (declaration order above).
    fn index(self) -> usize {
        match self {
            SecurityLevel::None => 0,
            SecurityLevel::BasicAuth => 1,
            SecurityLevel::Tls => 2,
            SecurityLevel::MutualTls => 3,
        }
    }
}

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
    pub name: Widget<InputFieldState, InputField<NonEmpty>>,
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
    /// Client role only: accept any server certificate without authenticating it. Orthogonal to
    /// `security` (shown at every level once `wss://` + client are selected) — needed to talk to
    /// a server-role CSMS whose certificate is regenerated (and thus unpinnable) at each start.
    #[focus(when = {self.show_skip_verify()})]
    pub skip_verify: Widget<SelectionState<SkipVerifyChoice>, Selection<SkipVerifyChoice>>,
    /// Client role only: extra trust anchor for a self-signed CSMS certificate.
    #[focus(when = {self.show_ca_file()})]
    pub ca_file: Widget<SuggestInputState<FsPathProvider>, SuggestInput<String, FsPathProvider>>,
    /// Server role only: certificate chain presented to connecting clients.
    #[focus(when = {self.show_server_cert()})]
    pub cert_file:
        Widget<SuggestInputState<FsPathProvider>, SuggestInput<NonEmpty, FsPathProvider>>,
    /// Server role only: private key matching `cert_file`.
    #[focus(when = {self.show_server_cert()})]
    pub key_file: Widget<SuggestInputState<FsPathProvider>, SuggestInput<NonEmpty, FsPathProvider>>,
    /// Client role only: client certificate presented for mutual TLS.
    #[focus(when = {self.show_client_cert()})]
    pub client_cert_file:
        Widget<SuggestInputState<FsPathProvider>, SuggestInput<NonEmpty, FsPathProvider>>,
    /// Client role only: private key matching `client_cert_file`.
    #[focus(when = {self.show_client_cert()})]
    pub client_key_file:
        Widget<SuggestInputState<FsPathProvider>, SuggestInput<NonEmpty, FsPathProvider>>,
    /// Server role only: CA used to verify client certificates (selecting mTLS as server implies
    /// `require_client_cert = true` in the resolved config).
    #[focus(when = {self.show_client_ca()})]
    pub client_ca_file:
        Widget<SuggestInputState<FsPathProvider>, SuggestInput<NonEmpty, FsPathProvider>>,
    /// Security section the dialog was opened with (`edit`; `Default` for a fresh dialog).
    /// [`resolve`](Self::resolve) returns it untouched while the protocol is `ws`: the security
    /// UI is hidden then, and a hidden section must never clobber a config-file-only setup
    /// (Basic Auth over plain ws is valid and file-only).
    pub preserved_security: OcppSecurityConfig,
    /// `Path::exists` results with a timestamp, so the per-tick live validation does not stat
    /// the filesystem on every redraw (see [`path_exists`](Self::path_exists)).
    pub fs_cache: std::cell::RefCell<std::collections::HashMap<String, (bool, std::time::Instant)>>,
    pub error: Widget<String, Text>,
    /// One-line info hint shown when a server-role `wss://` instance is below the TLS level (an
    /// ephemeral self-signed certificate will be generated at each start). Not a focusable field.
    pub hint: Widget<String, Text>,
    pub keybinds: Widget<String, Text>,
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
            .client_ca_file(suggest_input(
                "Client CA",
                "client_ca.pem",
                &input_style,
                cert_provider(),
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

        let level = SecurityLevel::from_config(&spec.security, spec.role);
        d.security.state.set_selection(level.index());
        d.skip_verify
            .state
            .set_selection(if spec.security.insecure_skip_verify {
                1
            } else {
                0
            });
        set_text(
            &mut d.username,
            spec.security.username.as_deref().unwrap_or(""),
        );
        set_text(
            &mut d.password,
            spec.security.password.as_deref().unwrap_or(""),
        );
        set_suggest_text(
            &mut d.ca_file,
            spec.security.ca_file.as_deref().unwrap_or(""),
        );
        set_suggest_text(
            &mut d.cert_file,
            spec.security.cert_file.as_deref().unwrap_or(""),
        );
        set_suggest_text(
            &mut d.key_file,
            spec.security.key_file.as_deref().unwrap_or(""),
        );
        set_suggest_text(
            &mut d.client_cert_file,
            spec.security.client_cert_file.as_deref().unwrap_or(""),
        );
        set_suggest_text(
            &mut d.client_key_file,
            spec.security.client_key_file.as_deref().unwrap_or(""),
        );
        set_suggest_text(
            &mut d.client_ca_file,
            spec.security.client_ca_file.as_deref().unwrap_or(""),
        );
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
        let protocol = self.protocol.get_value();
        let security = if protocol == OcppProtocol::Wss {
            let level = self.security.get_value();
            let mut cfg = level.build_config(
                role,
                SecurityInputs {
                    username: self.username.state.input(),
                    password: self.password.state.input(),
                    ca_file: self.ca_file.state.input(),
                    cert_file: self.cert_file.state.input(),
                    key_file: self.key_file.state.input(),
                    client_cert_file: self.client_cert_file.state.input(),
                    client_key_file: self.client_key_file.state.input(),
                    client_ca_file: self.client_ca_file.state.input(),
                },
            );
            // Below TLS, a wss server generates an ephemeral self-signed certificate at each
            // start rather than binding plain TCP; Tls/mTLS still require real cert/key files
            // (checked below).
            if role == OcppRole::Server && level < SecurityLevel::Tls {
                cfg.self_signed = true;
            }
            if role == OcppRole::Client {
                cfg.insecure_skip_verify = self.skip_verify.get_value() == SkipVerifyChoice::On;
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
            security,
        })
    }

    /// The entered device-config path (trimmed; empty when none).
    pub fn config_path(&self) -> String {
        self.config_path.state.input().trim().to_string()
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

    /// The client-side skip-verify toggle (any wss client, orthogonal to the level).
    fn show_skip_verify(&self) -> bool {
        self.wss() && self.role.get_value() == OcppRole::Client
    }

    /// Client trust-anchor input (wss client at TLS level or above).
    fn show_ca_file(&self) -> bool {
        self.wss()
            && self.level() >= SecurityLevel::Tls
            && self.role.get_value() == OcppRole::Client
    }

    /// Server certificate/key inputs (wss server at TLS level or above).
    fn show_server_cert(&self) -> bool {
        self.wss()
            && self.level() >= SecurityLevel::Tls
            && self.role.get_value() == OcppRole::Server
    }

    /// Client mTLS certificate/key inputs.
    fn show_client_cert(&self) -> bool {
        self.wss()
            && self.level() == SecurityLevel::MutualTls
            && self.role.get_value() == OcppRole::Client
    }

    /// Server mTLS client-CA input.
    fn show_client_ca(&self) -> bool {
        self.wss()
            && self.level() == SecurityLevel::MutualTls
            && self.role.get_value() == OcppRole::Server
    }

    /// First certificate row: server cert/key, or the client trust anchor.
    fn show_cert_row_a(&self) -> bool {
        self.show_ca_file() || self.show_server_cert()
    }

    /// Second certificate row: client mTLS cert/key, or the server client-CA.
    fn show_cert_row_b(&self) -> bool {
        self.show_client_cert() || self.show_client_ca()
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
        let show_security_row = self.show_security();
        let show_credentials = self.show_credentials();
        let show_server_cert = self.show_server_cert();
        let show_client_ca = self.show_client_ca();
        let show_cert_a = self.show_cert_row_a();
        let show_cert_b = self.show_cert_row_b();
        let show_hint = self.show_hint();

        // border(2) + inner margin(2) + name(3) + config path(3) + version|role(3)
        // + protocol|ip|port|path(3) + keybinds(1), plus the error box (3), the security rows
        // (3 each), and the hint line (1), only when applicable.
        let box_height = 17
            + if has_error { 3 } else { 0 }
            + if show_security_row { 3 } else { 0 }
            + if show_cert_a { 3 } else { 0 }
            + if show_cert_b { 3 } else { 0 }
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
        let inner = block.inner(vcenter).inner(Margin::new(2, 1));
        UiWidget::render(&Clear, vcenter, buf);
        block.render(vcenter, buf);

        let error_height = if has_error { 3 } else { 0 };
        let security_height = if show_security_row { 3 } else { 0 };
        let cert_a_height = if show_cert_a { 3 } else { 0 };
        let cert_b_height = if show_cert_b { 3 } else { 0 };
        let hint_height = if show_hint { 1 } else { 0 };
        let rows = Layout::vertical([
            Constraint::Length(3),               // name
            Constraint::Length(3),               // config path
            Constraint::Length(3),               // version | role
            Constraint::Length(3),               // protocol | ip | port | path
            Constraint::Length(security_height), // security | username | password | skip-verify
            Constraint::Length(hint_height),     // self-signed hint (server, below TLS)
            Constraint::Length(cert_a_height),   // cert_file|key_file or ca_file
            Constraint::Length(cert_b_height),   // client_cert|client_key or client_ca_file
            Constraint::Length(error_height),    // error (hidden when empty)
            Constraint::Length(1),               // keybinds
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

        let is_client = role == OcppRole::Client;
        if show_security_row {
            if show_credentials {
                if is_client {
                    let [sec, user, pass, skip] = Layout::horizontal([
                        Constraint::Percentage(25),
                        Constraint::Percentage(25),
                        Constraint::Percentage(25),
                        Constraint::Percentage(25),
                    ])
                    .areas(rows[4]);
                    StatefulWidget::render(
                        &self.security.widget,
                        sec,
                        buf,
                        &mut self.security.state,
                    );
                    StatefulWidget::render(
                        &self.username.widget,
                        user,
                        buf,
                        &mut self.username.state,
                    );
                    StatefulWidget::render(
                        &self.password.widget,
                        pass,
                        buf,
                        &mut self.password.state,
                    );
                    StatefulWidget::render(
                        &self.skip_verify.widget,
                        skip,
                        buf,
                        &mut self.skip_verify.state,
                    );
                } else {
                    let [sec, user, pass] = Layout::horizontal([
                        Constraint::Percentage(34),
                        Constraint::Percentage(33),
                        Constraint::Percentage(33),
                    ])
                    .areas(rows[4]);
                    StatefulWidget::render(
                        &self.security.widget,
                        sec,
                        buf,
                        &mut self.security.state,
                    );
                    StatefulWidget::render(
                        &self.username.widget,
                        user,
                        buf,
                        &mut self.username.state,
                    );
                    StatefulWidget::render(
                        &self.password.widget,
                        pass,
                        buf,
                        &mut self.password.state,
                    );
                }
            } else if is_client {
                let [sec, skip] =
                    Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .areas(rows[4]);
                StatefulWidget::render(&self.security.widget, sec, buf, &mut self.security.state);
                StatefulWidget::render(
                    &self.skip_verify.widget,
                    skip,
                    buf,
                    &mut self.skip_verify.state,
                );
            } else {
                let [sec, _] =
                    Layout::horizontal([Constraint::Percentage(34), Constraint::Percentage(66)])
                        .areas(rows[4]);
                StatefulWidget::render(&self.security.widget, sec, buf, &mut self.security.state);
            }
        }

        if show_hint {
            self.hint.state = "Self-signed certificate is generated at each start (clients: skip-verify or pinned certs)".to_string();
            StatefulWidget::render(&self.hint.widget, rows[5], buf, &mut self.hint.state);
        }

        if show_cert_a {
            if show_server_cert {
                let [left, right] =
                    Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .areas(rows[6]);
                StatefulWidget::render(
                    &self.cert_file.widget,
                    left,
                    buf,
                    &mut self.cert_file.state,
                );
                StatefulWidget::render(&self.key_file.widget, right, buf, &mut self.key_file.state);
            } else {
                StatefulWidget::render(&self.ca_file.widget, rows[6], buf, &mut self.ca_file.state);
            }
        }

        if show_cert_b {
            if show_client_ca {
                StatefulWidget::render(
                    &self.client_ca_file.widget,
                    rows[7],
                    buf,
                    &mut self.client_ca_file.state,
                );
            } else {
                let [left, right] =
                    Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .areas(rows[7]);
                StatefulWidget::render(
                    &self.client_cert_file.widget,
                    left,
                    buf,
                    &mut self.client_cert_file.state,
                );
                StatefulWidget::render(
                    &self.client_key_file.widget,
                    right,
                    buf,
                    &mut self.client_key_file.state,
                );
            }
        }

        if has_error {
            StatefulWidget::render(&self.error.widget, rows[8], buf, &mut self.error.state);
        }
        StatefulWidget::render(
            &self.keybinds.widget,
            rows[9],
            buf,
            &mut self.keybinds.state,
        );

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
        self.client_ca_file
            .widget
            .render_overlay(area, buf, &mut self.client_ca_file.state);
    }
}

/// Check every required credential/certificate file is present and, for path fields, exists on
/// disk. `level` has already been checked `>= Tls` for the server role by the caller.
fn validate_security(
    cfg: &OcppSecurityConfig,
    role: OcppRole,
    level: SecurityLevel,
    file_exists: &dyn Fn(&str) -> bool,
) -> Result<(), String> {
    let exists = |label: &str, path: &str| -> Result<(), String> {
        if !file_exists(path) {
            return Err(format!("{label} not found: {path}"));
        }
        Ok(())
    };

    match role {
        OcppRole::Server => {
            if level >= SecurityLevel::Tls {
                let cert = cfg
                    .cert_file
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .ok_or("Certificate file is required for TLS.")?;
                exists("Certificate file", cert)?;
                let key = cfg
                    .key_file
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .ok_or("Key file is required for TLS.")?;
                exists("Key file", key)?;
            }
            if level == SecurityLevel::MutualTls {
                let ca = cfg
                    .client_ca_file
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .ok_or("Client CA file is required for mTLS.")?;
                exists("Client CA file", ca)?;
            }
        }
        OcppRole::Client => {
            if let Some(ca) = cfg.ca_file.as_deref()
                && !ca.is_empty()
            {
                exists("CA file", ca)?;
            }
            if level == SecurityLevel::MutualTls {
                let cert = cfg
                    .client_cert_file
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .ok_or("Client certificate file is required for mTLS.")?;
                exists("Client certificate file", cert)?;
                let key = cfg
                    .client_key_file
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .ok_or("Client key file is required for mTLS.")?;
                exists("Client key file", key)?;
            }
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};
    use ferrowl_ui::traits::{HandleEvents, SetFocus};

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

    // --- SecurityLevel::from_config -----------------------------------------------------------

    #[test]
    fn ut_from_config_none_both_roles() {
        let cfg = OcppSecurityConfig::default();
        assert_eq!(
            SecurityLevel::from_config(&cfg, OcppRole::Client),
            SecurityLevel::None
        );
        assert_eq!(
            SecurityLevel::from_config(&cfg, OcppRole::Server),
            SecurityLevel::None
        );
    }

    #[test]
    fn ut_from_config_basic_auth_both_roles() {
        let cfg = OcppSecurityConfig {
            username: Some("u".into()),
            password: Some("p".into()),
            ..Default::default()
        };
        assert_eq!(
            SecurityLevel::from_config(&cfg, OcppRole::Client),
            SecurityLevel::BasicAuth
        );
        assert_eq!(
            SecurityLevel::from_config(&cfg, OcppRole::Server),
            SecurityLevel::BasicAuth
        );
    }

    #[test]
    fn ut_from_config_tls_client_is_ca_file() {
        let cfg = OcppSecurityConfig {
            ca_file: Some("ca.pem".into()),
            ..Default::default()
        };
        assert_eq!(
            SecurityLevel::from_config(&cfg, OcppRole::Client),
            SecurityLevel::Tls
        );
    }

    #[test]
    fn ut_from_config_tls_server_is_cert_and_key() {
        let cfg = OcppSecurityConfig {
            cert_file: Some("s.crt".into()),
            key_file: Some("s.key".into()),
            ..Default::default()
        };
        assert_eq!(
            SecurityLevel::from_config(&cfg, OcppRole::Server),
            SecurityLevel::Tls
        );
    }

    #[test]
    fn ut_from_config_mutual_tls_client_is_client_cert() {
        let cfg = OcppSecurityConfig {
            client_cert_file: Some("c.crt".into()),
            ..Default::default()
        };
        assert_eq!(
            SecurityLevel::from_config(&cfg, OcppRole::Client),
            SecurityLevel::MutualTls
        );
    }

    #[test]
    fn ut_from_config_mutual_tls_server_is_require_flag_or_client_ca() {
        let by_flag = OcppSecurityConfig {
            require_client_cert: true,
            ..Default::default()
        };
        assert_eq!(
            SecurityLevel::from_config(&by_flag, OcppRole::Server),
            SecurityLevel::MutualTls
        );
        let by_ca = OcppSecurityConfig {
            client_ca_file: Some("ca.pem".into()),
            ..Default::default()
        };
        assert_eq!(
            SecurityLevel::from_config(&by_ca, OcppRole::Server),
            SecurityLevel::MutualTls
        );
    }

    // --- SecurityLevel::build_config -----------------------------------------------------------

    #[test]
    fn ut_build_config_drops_fields_not_visible_at_level() {
        let cfg = SecurityLevel::BasicAuth.build_config(
            OcppRole::Server,
            SecurityInputs {
                username: "u",
                password: "p",
                ca_file: "ca",
                cert_file: "cert",
                key_file: "key",
                client_cert_file: "ccert",
                client_key_file: "ckey",
                client_ca_file: "cca",
            },
        );
        assert_eq!(cfg.username.as_deref(), Some("u"));
        assert_eq!(cfg.password.as_deref(), Some("p"));
        assert_eq!(cfg.cert_file, None);
        assert_eq!(cfg.key_file, None);
        assert_eq!(cfg.client_ca_file, None);
        assert!(!cfg.require_client_cert);
    }

    #[test]
    fn ut_build_config_tls_server_keeps_cert_key_not_client_fields() {
        let cfg = SecurityLevel::Tls.build_config(
            OcppRole::Server,
            SecurityInputs {
                username: "",
                password: "",
                ca_file: "ca",
                cert_file: "cert",
                key_file: "key",
                client_cert_file: "ccert",
                client_key_file: "ckey",
                client_ca_file: "cca",
            },
        );
        assert_eq!(cfg.cert_file.as_deref(), Some("cert"));
        assert_eq!(cfg.key_file.as_deref(), Some("key"));
        assert_eq!(cfg.ca_file, None); // client-only field
        assert_eq!(cfg.client_ca_file, None);
    }

    #[test]
    fn ut_build_config_mutual_tls_server_sets_require_client_cert() {
        let cfg = SecurityLevel::MutualTls.build_config(
            OcppRole::Server,
            SecurityInputs {
                username: "",
                password: "",
                ca_file: "",
                cert_file: "cert",
                key_file: "key",
                client_cert_file: "",
                client_key_file: "",
                client_ca_file: "cca",
            },
        );
        assert_eq!(cfg.client_ca_file.as_deref(), Some("cca"));
        assert!(cfg.require_client_cert);
        assert_eq!(cfg.client_cert_file, None); // client-only field
    }

    #[test]
    fn ut_build_config_mutual_tls_client_keeps_client_cert_key() {
        let cfg = SecurityLevel::MutualTls.build_config(
            OcppRole::Client,
            SecurityInputs {
                username: "",
                password: "",
                ca_file: "ca",
                cert_file: "",
                key_file: "",
                client_cert_file: "ccert",
                client_key_file: "ckey",
                client_ca_file: "",
            },
        );
        assert_eq!(cfg.ca_file.as_deref(), Some("ca"));
        assert_eq!(cfg.client_cert_file.as_deref(), Some("ccert"));
        assert_eq!(cfg.client_key_file.as_deref(), Some("ckey"));
        assert_eq!(cfg.client_ca_file, None); // server-only field
        assert!(!cfg.require_client_cert);
    }

    // Regression: editing a ws module whose device file carries a security section (Basic Auth
    // over plain ws is valid, config-file-only) must hand that section back unchanged — the
    // security UI is hidden for ws, and a hidden section must never clobber the file.
    #[test]
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

    #[test]
    fn ut_server_wss_none_resolves_self_signed_no_cert_error() {
        let d = wss_dialog(1); // Server, security level defaults to None
        let spec = d
            .resolve()
            .expect("below-TLS server should self-sign, not error");
        assert!(spec.security.self_signed);
        assert_eq!(spec.security.cert_file, None);
        assert_eq!(spec.security.key_file, None);
    }

    #[test]
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
        assert!(spec.security.self_signed);
        assert_eq!(spec.security.username.as_deref(), Some("cp001"));
    }

    #[test]
    fn ut_server_tls_missing_cert_is_rejected() {
        let mut d = wss_dialog(1);
        d.security.state.set_selection(SecurityLevel::Tls.index());
        let err = d.resolve().unwrap_err();
        assert!(err.contains("Certificate file is required"), "{err}");
    }

    #[test]
    fn ut_server_tls_nonexistent_cert_is_rejected() {
        let mut d = wss_dialog(1);
        d.security.state.set_selection(SecurityLevel::Tls.index());
        set_suggest_text(&mut d.cert_file, "/no/such/cert.crt");
        set_suggest_text(&mut d.key_file, "/no/such/key.key");
        let err = d.resolve().unwrap_err();
        assert!(err.contains("Certificate file not found"), "{err}");
    }

    #[test]
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
        assert!(err.contains("Client CA file is required"), "{err}");
    }

    #[test]
    fn ut_client_mutual_tls_missing_cert_key_is_rejected() {
        let mut d = wss_dialog(0); // Client
        d.security
            .state
            .set_selection(SecurityLevel::MutualTls.index());
        let err = d.resolve().unwrap_err();
        assert!(err.contains("Client certificate file is required"), "{err}");
    }

    #[test]
    fn ut_client_ca_file_when_set_must_exist() {
        let mut d = wss_dialog(0);
        d.security.state.set_selection(SecurityLevel::Tls.index());
        set_suggest_text(&mut d.ca_file, "/no/such/ca.pem");
        let err = d.resolve().unwrap_err();
        assert!(err.contains("CA file not found"), "{err}");
    }

    #[test]
    fn ut_client_wss_none_is_allowed() {
        let d = wss_dialog(0); // Client, level defaults to None
        assert!(d.resolve().is_ok());
    }

    #[test]
    fn ut_ws_never_requires_security() {
        let mut d = OcppSetupDialog::new(); // Ws, Client by default
        set_text(&mut d.name, "cs-1");
        let spec = d.resolve().unwrap();
        assert_eq!(spec.security, OcppSecurityConfig::default());
    }

    // --- edit -> resolve round trip ------------------------------------------------------------

    #[test]
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
            security: OcppSecurityConfig {
                cert_file: Some(cert),
                key_file: Some(key),
                client_ca_file: Some(cca),
                require_client_cert: true,
                ..Default::default()
            },
        };
        let dialog = OcppSetupDialog::edit(&spec, "device.toml");
        let resolved = dialog.resolve().expect("valid mTLS server config");
        assert_eq!(resolved.security, spec.security);
    }

    #[test]
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
            security: OcppSecurityConfig {
                insecure_skip_verify: true,
                ..Default::default()
            },
        };
        let dialog = OcppSetupDialog::edit(&spec, "device.toml");
        assert_eq!(dialog.skip_verify.state.get_value(), SkipVerifyChoice::On);
        let resolved = dialog.resolve().expect("valid client config");
        assert!(resolved.security.insecure_skip_verify);
    }

    // --- render height -----------------------------------------------------------------------

    #[test]
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
                    | OcppSetupDialogFocus::ClientCaFile
            ));
        }
    }

    #[test]
    fn ut_focus_wss_none_shows_security_selection_and_skip_verify_for_client() {
        let mut d = wss_dialog(0); // Client, wss, level None
        d.set_focused(true);
        let mut visited = Vec::new();
        for _ in 0..20 {
            d.focus_next();
            visited.push(d.focus);
        }
        assert!(visited.contains(&OcppSetupDialogFocus::Security));
        assert!(visited.contains(&OcppSetupDialogFocus::SkipVerify));
        assert!(!visited.contains(&OcppSetupDialogFocus::Username));
        assert!(!visited.contains(&OcppSetupDialogFocus::CaFile));
    }

    #[test]
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
    fn ut_focus_wss_mutual_tls_server_reaches_client_ca_file() {
        let mut d = wss_dialog(1); // Server
        d.security
            .state
            .set_selection(SecurityLevel::MutualTls.index());
        d.set_focused(true);
        let mut visited = Vec::new();
        for _ in 0..20 {
            d.focus_next();
            visited.push(d.focus);
        }
        assert!(visited.contains(&OcppSetupDialogFocus::ClientCaFile));
        assert!(visited.contains(&OcppSetupDialogFocus::CertFile));
        assert!(visited.contains(&OcppSetupDialogFocus::KeyFile));
        assert!(!visited.contains(&OcppSetupDialogFocus::ClientCertFile));
    }

    /// Typing into the config-path field opens the filesystem suggestion popup, and the popup is
    /// drawn on top of the dialog by the trailing `render_overlay` calls in `render`.
    #[test]
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
}
