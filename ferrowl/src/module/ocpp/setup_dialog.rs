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
    #[allow(clippy::too_many_arguments)]
    pub fn build_config(
        self,
        role: OcppRole,
        username: &str,
        password: &str,
        ca_file: &str,
        cert_file: &str,
        key_file: &str,
        client_cert_file: &str,
        client_key_file: &str,
        client_ca_file: &str,
    ) -> OcppSecurityConfig {
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
    #[focus(when = {self.role.get_value() == OcppRole::Client})]
    pub path: Widget<InputFieldState, InputField<String>>,
    /// Transport security level, offered only for `wss://`.
    #[focus(when = {self.wss()})]
    pub security: Widget<SelectionState<SecurityLevel>, Selection<SecurityLevel>>,
    /// Basic Auth username. Note: rendered as plain text — no masked-input widget exists yet.
    #[focus(when = {self.wss() && self.level() >= SecurityLevel::BasicAuth})]
    pub username: Widget<InputFieldState, InputField<String>>,
    /// Basic Auth password. Note: rendered as plain text (no masking) — same limitation as
    /// `username`; the field is not obscured on screen.
    #[focus(when = {self.wss() && self.level() >= SecurityLevel::BasicAuth})]
    pub password: Widget<InputFieldState, InputField<String>>,
    /// Client role only: extra trust anchor for a self-signed CSMS certificate.
    #[focus(when = {self.wss() && self.level() >= SecurityLevel::Tls && self.role.get_value() == OcppRole::Client})]
    pub ca_file: Widget<SuggestInputState<FsPathProvider>, SuggestInput<String, FsPathProvider>>,
    /// Server role only: certificate chain presented to connecting clients.
    #[focus(when = {self.wss() && self.level() >= SecurityLevel::Tls && self.role.get_value() == OcppRole::Server})]
    pub cert_file: Widget<SuggestInputState<FsPathProvider>, SuggestInput<String, FsPathProvider>>,
    /// Server role only: private key matching `cert_file`.
    #[focus(when = {self.wss() && self.level() >= SecurityLevel::Tls && self.role.get_value() == OcppRole::Server})]
    pub key_file: Widget<SuggestInputState<FsPathProvider>, SuggestInput<String, FsPathProvider>>,
    /// Client role only: client certificate presented for mutual TLS.
    #[focus(when = {self.wss() && self.level() == SecurityLevel::MutualTls && self.role.get_value() == OcppRole::Client})]
    pub client_cert_file:
        Widget<SuggestInputState<FsPathProvider>, SuggestInput<String, FsPathProvider>>,
    /// Client role only: private key matching `client_cert_file`.
    #[focus(when = {self.wss() && self.level() == SecurityLevel::MutualTls && self.role.get_value() == OcppRole::Client})]
    pub client_key_file:
        Widget<SuggestInputState<FsPathProvider>, SuggestInput<String, FsPathProvider>>,
    /// Server role only: CA used to verify client certificates (selecting mTLS as server implies
    /// `require_client_cert = true` in the resolved config).
    #[focus(when = {self.wss() && self.level() == SecurityLevel::MutualTls && self.role.get_value() == OcppRole::Server})]
    pub client_ca_file:
        Widget<SuggestInputState<FsPathProvider>, SuggestInput<String, FsPathProvider>>,
    pub error: Widget<String, Text>,
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
            // A wss server without at least TLS would silently bind plain TCP; refuse instead.
            if role == OcppRole::Server && level < SecurityLevel::Tls {
                return Err("wss requires TLS certificates (choose TLS or mTLS)".into());
            }
            let cfg = level.build_config(
                role,
                self.username.state.input(),
                self.password.state.input(),
                self.ca_file.state.input(),
                self.cert_file.state.input(),
                self.key_file.state.input(),
                self.client_cert_file.state.input(),
                self.client_key_file.state.input(),
                self.client_ca_file.state.input(),
            );
            validate_security(&cfg, role, level)?;
            cfg
        } else {
            OcppSecurityConfig::default()
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

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        match self.resolve() {
            Ok(_) => self.error.state.clear(),
            Err(e) => self.error.state = e,
        }

        let has_error = !self.error.state.is_empty();
        let role = self.role.get_value();
        let wss = self.wss();
        let level = self.level();
        let show_security_row = wss;
        let show_cert_a = wss && level >= SecurityLevel::Tls;
        let show_cert_b = wss && level == SecurityLevel::MutualTls;

        // border(2) + inner margin(2) + name(3) + config path(3) + version|role(3)
        // + protocol|ip|port|path(3) + keybinds(1), plus the error box (3) and the security rows
        // (3 each), only when applicable.
        let box_height = 17
            + if has_error { 3 } else { 0 }
            + if show_security_row { 3 } else { 0 }
            + if show_cert_a { 3 } else { 0 }
            + if show_cert_b { 3 } else { 0 };
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
        let rows = Layout::vertical([
            Constraint::Length(3),               // name
            Constraint::Length(3),               // config path
            Constraint::Length(3),               // version | role
            Constraint::Length(3),               // protocol | ip | port | path
            Constraint::Length(security_height), // security | username | password
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

        if show_security_row {
            if level >= SecurityLevel::BasicAuth {
                let [sec, user, pass] = Layout::horizontal([
                    Constraint::Percentage(34),
                    Constraint::Percentage(33),
                    Constraint::Percentage(33),
                ])
                .areas(rows[4]);
                StatefulWidget::render(&self.security.widget, sec, buf, &mut self.security.state);
                StatefulWidget::render(&self.username.widget, user, buf, &mut self.username.state);
                StatefulWidget::render(&self.password.widget, pass, buf, &mut self.password.state);
            } else {
                let [sec, _] =
                    Layout::horizontal([Constraint::Percentage(34), Constraint::Percentage(66)])
                        .areas(rows[4]);
                StatefulWidget::render(&self.security.widget, sec, buf, &mut self.security.state);
            }
        }

        if show_cert_a {
            match role {
                OcppRole::Server => {
                    let [left, right] = Layout::horizontal([
                        Constraint::Percentage(50),
                        Constraint::Percentage(50),
                    ])
                    .areas(rows[5]);
                    StatefulWidget::render(
                        &self.cert_file.widget,
                        left,
                        buf,
                        &mut self.cert_file.state,
                    );
                    StatefulWidget::render(
                        &self.key_file.widget,
                        right,
                        buf,
                        &mut self.key_file.state,
                    );
                }
                OcppRole::Client => {
                    StatefulWidget::render(
                        &self.ca_file.widget,
                        rows[5],
                        buf,
                        &mut self.ca_file.state,
                    );
                }
            }
        }

        if show_cert_b {
            match role {
                OcppRole::Client => {
                    let [left, right] = Layout::horizontal([
                        Constraint::Percentage(50),
                        Constraint::Percentage(50),
                    ])
                    .areas(rows[6]);
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
                OcppRole::Server => {
                    StatefulWidget::render(
                        &self.client_ca_file.widget,
                        rows[6],
                        buf,
                        &mut self.client_ca_file.state,
                    );
                }
            }
        }

        if has_error {
            StatefulWidget::render(&self.error.widget, rows[7], buf, &mut self.error.state);
        }
        StatefulWidget::render(
            &self.keybinds.widget,
            rows[8],
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
) -> Result<(), String> {
    let exists = |label: &str, path: &str| -> Result<(), String> {
        if !std::path::Path::new(path).exists() {
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
            "u",
            "p",
            "ca",
            "cert",
            "key",
            "ccert",
            "ckey",
            "cca",
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
            "",
            "",
            "ca",
            "cert",
            "key",
            "ccert",
            "ckey",
            "cca",
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
            "",
            "",
            "",
            "cert",
            "key",
            "",
            "",
            "cca",
        );
        assert_eq!(cfg.client_ca_file.as_deref(), Some("cca"));
        assert!(cfg.require_client_cert);
        assert_eq!(cfg.client_cert_file, None); // client-only field
    }

    #[test]
    fn ut_build_config_mutual_tls_client_keeps_client_cert_key() {
        let cfg = SecurityLevel::MutualTls.build_config(
            OcppRole::Client,
            "",
            "",
            "ca",
            "",
            "",
            "ccert",
            "ckey",
            "",
        );
        assert_eq!(cfg.ca_file.as_deref(), Some("ca"));
        assert_eq!(cfg.client_cert_file.as_deref(), Some("ccert"));
        assert_eq!(cfg.client_key_file.as_deref(), Some("ckey"));
        assert_eq!(cfg.client_ca_file, None); // server-only field
        assert!(!cfg.require_client_cert);
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
    fn ut_server_wss_below_tls_is_rejected() {
        let d = wss_dialog(1); // Server, security level defaults to None
        let err = d.resolve().unwrap_err();
        assert!(
            err.contains("wss requires TLS certificates"),
            "unexpected error: {err}"
        );
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
    fn ut_focus_wss_none_shows_only_security_selection() {
        let mut d = wss_dialog(0); // Client, wss, level None
        d.set_focused(true);
        let mut visited = Vec::new();
        for _ in 0..20 {
            d.focus_next();
            visited.push(d.focus);
        }
        assert!(visited.contains(&OcppSetupDialogFocus::Security));
        assert!(!visited.contains(&OcppSetupDialogFocus::Username));
        assert!(!visited.contains(&OcppSetupDialogFocus::CaFile));
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
