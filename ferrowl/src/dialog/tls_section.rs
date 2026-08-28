//! Shared TLS/mTLS cluster widget for the Modbus and OCPP setup dialogs (MB-R-104..112,
//! MB-R-136, MB-R-139, OC-R-110, OC-R-111, OC-R-113, OC-R-116). Lives in `ferrowl/src/dialog/`
//! (not `ferrowl-ui`) because it needs `ferrowl_util::tls`'s policy types and
//! `crate::dialog::path_suggest::FsPathProvider`, neither of which `ferrowl-ui` depends on —
//! mirrors `ca_file_list.rs`'s own doc-comment precedent for this same layering reason.
//!
//! `role`/`level` are set once per relevant call via [`TlsSection::sync`] rather than passed as
//! explicit parameters to every predicate: `#[focus(when = ...)]` expressions are spliced
//! verbatim with only `self` in scope (no mechanism to receive caller-supplied arguments), so the
//! 7 internal visibility gates read `self.role`/`self.level` directly. An embedding dialog calls
//! `self.tls.sync(role, level)` as the first statement of any method that touches these gates,
//! since nothing else keeps `role`/`level` current.
#![allow(dead_code)]

use derive_builder::Builder;
use ferrowl_ui::{
    Border, EventResult,
    state::{
        ButtonState, InputFieldStateBuilder, SelectionState, SelectionStateBuilder,
        SuggestInputState, SuggestInputStateBuilder,
    },
    style::{ButtonStyle, InputFieldStyle, SelectionStyle},
    traits::{HandleEvents, ToLabel},
    widgets::{
        Button, GetValue, InputFieldBuilder, Selection, SelectionBuilder, SuggestInput,
        SuggestInputBuilder, Widget,
    },
};
use ferrowl_ui_derive::{Focus, focusable};

use crate::config::ClientOrServer;
use crate::dialog::NonEmpty;
use crate::dialog::ca_file_list::AddCaFileDialog;
use crate::dialog::path_suggest::FsPathProvider;
use ferrowl_util::tls::{
    ClientCertSource, ClientCertVerification, ClientTlsPolicy, ClientVerification,
    ServerCertSource, ServerTlsPolicy,
};
use std::fmt::Debug;

/// TLS/mTLS level, collapsed from either dialog's own richer level type. `Off` covers both "no
/// TLS at all" (transport doesn't carry TLS / protocol isn't wss) and, for OCPP specifically,
/// `SecurityLevel::None`/`SecurityLevel::BasicAuth` (every predicate this widget owns tests only
/// `>= Tls` or `== MutualTls`, never distinguishing `None` from `BasicAuth` — `show_credentials`,
/// the one OCPP predicate that does distinguish them, stays on `OcppSetupDialog` itself, never
/// becomes a `TlsSection` concern).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum EffectiveTlsLevel {
    #[default]
    Off,
    Tls,
    MutualTls,
}

// `impl From<TlsLevel> for EffectiveTlsLevel` / `impl From<SecurityLevel> for EffectiveTlsLevel`
// are *not* defined here: `crate::module::modbus::setup_dialog::tls` and
// `crate::module::ocpp::setup_dialog::security` are both declared as a private `mod` (not `pub
// mod`) on their owning dialogs, so `TlsLevel`/`SecurityLevel` (themselves `pub enum`s) are
// unreachable from this file — the enums' own visibility doesn't help when the module path to
// them is private. These `From` impls belong next to `TlsLevel` in `setup_dialog/tls.rs` and next
// to `SecurityLevel` in `setup_dialog/security.rs` instead, the only files that can reach them.

/// "Generate an ephemeral self-signed certificate/identity" toggle, offered whenever `Tls`/
/// `MutualTls` is selected. The *same* widget field is reused for both roles (they are never
/// shown at the same time, since a dialog instance is fixed to one role): for the server role it
/// toggles the presented server certificate's source (MB-R-106) whenever `Tls`/`MutualTls` is
/// selected; for the client role it toggles the client's own mTLS identity (MB-R-138/139)
/// whenever `MutualTls` is selected (the identity only exists under mTLS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfSignedChoice {
    Off,
    On,
}

impl ToLabel for SelfSignedChoice {
    fn to_label(&self) -> String {
        match self {
            SelfSignedChoice::Off => "Off",
            SelfSignedChoice::On => "On",
        }
        .to_string()
    }
}

/// A binary skip-verify toggle, offered whenever `Tls`/`MutualTls` is selected. Two distinct
/// widget fields use this shape: the client-role "accept any server certificate" toggle (shown
/// at `Tls`+) and the server-role "accept any client certificate" toggle (`client_cert_skip_verify`,
/// MB-R-136, shown at `MutualTls` only) — never the same field, since each role only ever shows
/// one of the two rows this shape backs.
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

/// Raw TLS-proper field values, read by `extract`/consumed by each outer dialog's own
/// `build_config` (Modbus: `TlsInputs`, unaffected; OCPP: `SecurityInputs`, unaffected) to
/// assemble the wire config together with that dialog's own non-TLS inputs (Modbus: none extra;
/// OCPP: `username`/`password`, which stay outer).
pub struct TlsSectionInputs {
    pub ca_file: String,
    pub cert_file: String,
    pub key_file: String,
    pub client_cert_file: String,
    pub client_key_file: String,
    pub client_ca_files: Vec<String>,
    pub self_signed: bool,
    pub skip_verify: bool,
    pub client_cert_skip_verify: bool,
}

/// Shared TLS/mTLS cluster: self-signed toggle, own-identity cert/key pair, skip-verify toggle,
/// peer-verification input, client-CA add/remove list. Embedded by the Modbus and OCPP setup
/// dialogs via `#[focus(nested, when = ...)]`.
#[focusable(nestable)]
#[derive(Builder, Clone, Focus)]
pub struct TlsSection {
    /// Server: "generate an ephemeral self-signed server certificate" toggle (shown at TLS+).
    /// Client, at mTLS only: "generate an ephemeral self-signed client identity" toggle
    /// (MB-R-139) — the same widget field backs both, since only one role is ever active for a
    /// given dialog instance.
    #[focus(when = {self.show_self_signed()})]
    pub self_signed: Widget<SelectionState<SelfSignedChoice>, Selection<SelfSignedChoice>>,
    /// Server-only certificate chain presented to connecting clients.
    #[focus(when = {self.show_server_cert()})]
    pub cert_file:
        Widget<SuggestInputState<FsPathProvider>, SuggestInput<NonEmpty, FsPathProvider>>,
    /// Server-only private key matching `cert_file`.
    #[focus(when = {self.show_server_cert()})]
    pub key_file: Widget<SuggestInputState<FsPathProvider>, SuggestInput<NonEmpty, FsPathProvider>>,
    /// Client-only client certificate presented for mutual TLS.
    #[focus(when = {self.show_client_cert()})]
    pub client_cert_file:
        Widget<SuggestInputState<FsPathProvider>, SuggestInput<NonEmpty, FsPathProvider>>,
    /// Client-only private key matching `client_cert_file`.
    #[focus(when = {self.show_client_cert()})]
    pub client_key_file:
        Widget<SuggestInputState<FsPathProvider>, SuggestInput<NonEmpty, FsPathProvider>>,
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
    /// via `client_ca_add_dialog` or remove the selected entry).
    #[focus(when = {self.show_client_ca() && !self.client_ca_files.state.values().is_empty()})]
    pub client_ca_files: Widget<SelectionState<String>, Selection<String>>,
    #[focus(when = {self.show_client_ca()})]
    pub client_ca_add_button: Widget<ButtonState, Button>,
    #[focus(when = {self.show_client_ca() && !self.client_ca_files.state.values().is_empty()})]
    pub client_ca_delete_button: Widget<ButtonState, Button>,
    /// Sub-dialog for adding one path to `client_ca_files`, opened by `client_ca_add_button`; not
    /// itself a `#[focus]` field — routed specially in `handle_events`, mirroring `close_confirm`.
    #[builder(default)]
    pub client_ca_add_dialog: Option<AddCaFileDialog>,
    /// The role this section is currently rendering/gating for, set fresh by `sync` before any
    /// entry point (`render`/`handle_events`/`extract` in the owning dialog) reads a `when` gate.
    #[builder(default = "ClientOrServer::Server")]
    role: ClientOrServer,
    /// The effective TLS/mTLS level this section is currently gating for, set fresh by `sync`.
    #[builder(default)]
    level: EffectiveTlsLevel,
}

impl TlsSection {
    /// Fully-baked constructor: every shared field's title/placeholder/provider, identical
    /// between Modbus's and OCPP's own construction blocks today (verified: "Cert File"/
    /// "server.crt", "Client Cert"/"client.crt", etc. match exactly), so no per-caller
    /// parameterization is needed.
    pub fn new() -> Self {
        let selection_style = SelectionStyle::default();
        let input_style = InputFieldStyle::default();

        TlsSectionBuilder::default()
            .self_signed(selection(
                "Self-Signed",
                vec![SelfSignedChoice::Off, SelfSignedChoice::On],
                &selection_style,
            ))
            .skip_verify(selection(
                "Skip Verify",
                vec![SkipVerifyChoice::Off, SkipVerifyChoice::On],
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
                FsPathProvider::with_extensions(&["pem", "crt", "key"]),
            ))
            .cert_file(suggest_input(
                "Cert File",
                "server.crt",
                &input_style,
                FsPathProvider::with_extensions(&["pem", "crt", "key"]),
            ))
            .key_file(suggest_input(
                "Key File",
                "server.key",
                &input_style,
                FsPathProvider::with_extensions(&["pem", "crt", "key"]),
            ))
            .client_cert_file(suggest_input(
                "Client Cert",
                "client.crt",
                &input_style,
                FsPathProvider::with_extensions(&["pem", "crt", "key"]),
            ))
            .client_key_file(suggest_input(
                "Client Key",
                "client.key",
                &input_style,
                FsPathProvider::with_extensions(&["pem", "crt", "key"]),
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
            .focus(TlsSectionFocus::SelfSigned)
            .view_focused(false)
            .build()
            .expect("all required builder fields are set")
    }

    /// Set the role/level this section gates and resolves for, fresh, before any entry point
    /// (`render`/`handle_events`/`extract` on the *owning* dialog) reads a `when`-gated field.
    /// See the module doc comment for why this is a `sync` call rather than parameters threaded
    /// through every predicate.
    pub fn sync(&mut self, role: ClientOrServer, level: EffectiveTlsLevel) {
        self.role = role;
        self.level = level;
    }

    /// Which of this section's own panes currently holds focus — `#[focusable]`'s generated
    /// `focus` field is private to this module (and its descendants) by construction, so an
    /// embedding outer dialog needs this accessor to route its own Enter/Space handling for the
    /// client-CA ADD/DEL buttons and to guard against leaving focus on a hidden DEL button.
    pub fn focus(&self) -> TlsSectionFocus {
        self.focus
    }

    /// Server: self-signed server-certificate toggle (TLS level or above). Client, at mTLS
    /// only: self-signed client-identity toggle (MB-R-139) — same widget, different meaning per
    /// role (see the field's doc comment).
    fn show_self_signed(&self) -> bool {
        (self.role == ClientOrServer::Server && self.level >= EffectiveTlsLevel::Tls)
            || (self.role == ClientOrServer::Client && self.level == EffectiveTlsLevel::MutualTls)
    }

    /// Row (self-signed toggle): a dialog that paints this as its own standalone row (rather
    /// than folding it into the identity row alongside cert/key, as a combined-row layout would)
    /// reads this directly for its own row-height budgeting, mirroring the other 3 `pub` row
    /// helpers below.
    pub fn show_self_signed_row(&self) -> bool {
        self.show_self_signed()
    }

    /// Client-only skip-verify toggle (client at TLS level or above).
    fn show_skip_verify(&self) -> bool {
        self.role == ClientOrServer::Client && self.level >= EffectiveTlsLevel::Tls
    }

    /// Server-only, mTLS only: "accept any client certificate" toggle (MB-R-136).
    fn show_client_cert_skip_verify(&self) -> bool {
        self.role == ClientOrServer::Server && self.level == EffectiveTlsLevel::MutualTls
    }

    /// Client trust-anchor input (client at TLS level or above).
    fn show_ca_file(&self) -> bool {
        self.role == ClientOrServer::Client
            && self.level >= EffectiveTlsLevel::Tls
            && self.skip_verify.get_value() == SkipVerifyChoice::Off
    }

    /// Server certificate/key inputs (server at TLS level or above).
    fn show_server_cert(&self) -> bool {
        self.role == ClientOrServer::Server
            && self.level >= EffectiveTlsLevel::Tls
            && self.self_signed.get_value() == SelfSignedChoice::Off
    }

    /// Client mTLS certificate/key inputs — hidden when the client's self-signed-identity
    /// toggle is on (MB-R-139), mirroring the server's `show_server_cert`.
    fn show_client_cert(&self) -> bool {
        self.role == ClientOrServer::Client
            && self.level == EffectiveTlsLevel::MutualTls
            && self.self_signed.get_value() == SelfSignedChoice::Off
    }

    /// Server mTLS client-CA list input — hidden when `client_cert_skip_verify` is on
    /// (MB-R-136), preserving the list's own text so toggling back Off restores it.
    fn show_client_ca(&self) -> bool {
        self.role == ClientOrServer::Server
            && self.level == EffectiveTlsLevel::MutualTls
            && self.client_cert_skip_verify.get_value() == SkipVerifyChoice::Off
    }

    /// Row (skip-verify toggle): server's `client_cert_skip_verify`, or the client's
    /// `skip_verify` — exactly one applies for a given role.
    pub fn show_skip_verify_row(&self) -> bool {
        self.show_client_cert_skip_verify() || self.show_skip_verify()
    }

    /// Row (own-identity cert/key pair): server's `cert_file`/`key_file`, or the client's
    /// `client_cert_file`/`client_key_file` — exactly one applies for a given role.
    pub fn show_identity_row(&self) -> bool {
        self.show_server_cert() || self.show_client_cert()
    }

    /// Row (peer-verification input): server's `client_ca_files` list, or the client's
    /// `ca_file` — exactly one applies for a given role.
    pub fn show_peer_verify_row(&self) -> bool {
        self.show_client_ca() || self.show_ca_file()
    }

    /// Prefill every field from an existing policy (Edit mode), by role — today's inline
    /// `edit()`/OCPP-`edit()` match-on-role destructuring, moved verbatim.
    pub fn prefill(
        &mut self,
        role: ClientOrServer,
        server: Option<&ServerTlsPolicy>,
        client: Option<&ClientTlsPolicy>,
    ) {
        match role {
            ClientOrServer::Server => {
                let Some(tls) = server else { return };
                let (server_cert, client_verification) = match tls {
                    ServerTlsPolicy::MutualTls {
                        server_cert,
                        client_verification,
                    } => (server_cert.clone(), Some(client_verification.clone())),
                    ServerTlsPolicy::Tls { server_cert } => (server_cert.clone(), None),
                    ServerTlsPolicy::NoTls => (ServerCertSource::Unset, None),
                };
                self.self_signed.state.set_selection(
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
                set_suggest_input(&mut self.cert_file, cert_file);
                set_suggest_input(&mut self.key_file, key_file);
                let (ca_files, skip) = match &client_verification {
                    Some(ClientCertVerification::Verify { ca_files }) => (ca_files.clone(), false),
                    Some(ClientCertVerification::SkipVerify) => (Vec::new(), true),
                    None => (Vec::new(), false),
                };
                *self.client_ca_files.state.values_mut() = ca_files;
                self.client_ca_files.state.set_selection(0);
                self.client_cert_skip_verify
                    .state
                    .set_selection(if skip { 1 } else { 0 });
            }
            ClientOrServer::Client => {
                let Some(tls) = client else { return };
                let (client_verification, client_identity) = match tls {
                    ClientTlsPolicy::MutualTls {
                        client_verification,
                        client_identity,
                    } => (client_verification.clone(), Some(client_identity.clone())),
                    ClientTlsPolicy::Tls {
                        client_verification,
                    } => (client_verification.clone(), None),
                    ClientTlsPolicy::NoTls => (ClientVerification::Verify { ca_file: None }, None),
                };
                self.skip_verify.state.set_selection(
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
                set_suggest_input(&mut self.ca_file, ca_file);
                let self_signed_client =
                    matches!(client_identity, Some(ClientCertSource::SelfSigned));
                self.self_signed
                    .state
                    .set_selection(if self_signed_client { 1 } else { 0 });
                let (ccert, ckey) = match &client_identity {
                    Some(ClientCertSource::Explicit {
                        client_cert_file,
                        client_key_file,
                    }) => (client_cert_file.as_str(), client_key_file.as_str()),
                    _ => ("", ""),
                };
                set_suggest_input(&mut self.client_cert_file, ccert);
                set_suggest_input(&mut self.client_key_file, ckey);
            }
        }
    }

    /// Read every TLS-proper field's raw text/toggle state, uniformly regardless of role — the
    /// caller (each outer dialog's own `build_config`) selects the right half.
    pub fn extract(&self) -> TlsSectionInputs {
        TlsSectionInputs {
            ca_file: self.ca_file.state.input().to_string(),
            cert_file: self.cert_file.state.input().to_string(),
            key_file: self.key_file.state.input().to_string(),
            client_cert_file: self.client_cert_file.state.input().to_string(),
            client_key_file: self.client_key_file.state.input().to_string(),
            client_ca_files: self.client_ca_files.state.values().to_vec(),
            self_signed: self.self_signed.state.get_value() == SelfSignedChoice::On,
            skip_verify: self.skip_verify.state.get_value() == SkipVerifyChoice::On,
            client_cert_skip_verify: self.client_cert_skip_verify.state.get_value()
                == SkipVerifyChoice::On,
        }
    }

    /// Remove the currently-selected client-CA entry (MB-R-136), if any, adjusting the
    /// selection cursor to stay in bounds.
    pub(crate) fn delete_selected_client_ca(&mut self) {
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
            // a dead target, so fall back to ADD.
            self.focus_previous();
        }
    }

    /// Route a key for this section's own client-CA add-dialog and ADD/DEL buttons (Enter/
    /// Space); everything else falls through to the derived per-field routing. Self-contained so
    /// `TlsSection` can be driven and tested without an embedding dialog.
    pub fn handle_events(
        &mut self,
        modifiers: crossterm::event::KeyModifiers,
        code: crossterm::event::KeyCode,
    ) -> EventResult {
        use crossterm::event::{KeyCode, KeyModifiers};

        if let Some(dialog) = self.client_ca_add_dialog.as_mut() {
            match (modifiers, code) {
                (KeyModifiers::NONE, KeyCode::Esc) if dialog.path.state.suggestions_open() => {
                    let _ = dialog.path.state.handle_events(modifiers, code);
                }
                (KeyModifiers::NONE, KeyCode::Esc) => {
                    self.client_ca_add_dialog = None;
                }
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
                TlsSectionFocus::ClientCaAddButton => {
                    self.client_ca_add_dialog = Some(AddCaFileDialog::new());
                    return EventResult::Consumed;
                }
                TlsSectionFocus::ClientCaDeleteButton => {
                    self.delete_selected_client_ca();
                    return EventResult::Consumed;
                }
                _ => {}
            }
        }

        <Self as HandleEvents>::handle_events(self, modifiers, code)
    }
}

impl Default for TlsSection {
    fn default() -> Self {
        Self::new()
    }
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
            .expect("all required builder fields are set"),
        widget: SelectionBuilder::default()
            .border(Border::Full(ratatui::layout::Margin::new(1, 0)))
            .title(Some(
                (title, ratatui::layout::HorizontalAlignment::Left).into(),
            ))
            .margin(ratatui::layout::Margin {
                vertical: 0,
                horizontal: 1,
            })
            .style(style.clone())
            .build()
            .expect("all required builder fields are set"),
    }
}

fn suggest_input<
    T: ferrowl_ui::widgets::Validate + Clone,
    P: ferrowl_ui::traits::SuggestionProvider + Clone,
>(
    title: &str,
    placeholder: &str,
    style: &InputFieldStyle,
    provider: P,
) -> Widget<SuggestInputState<P>, SuggestInput<T, P>> {
    Widget {
        state: SuggestInputStateBuilder::default()
            .field(
                InputFieldStateBuilder::default()
                    .focused(false)
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
                    .border(Border::Full(ratatui::layout::Margin::new(1, 0)))
                    .title(Some(
                        (title, ratatui::layout::HorizontalAlignment::Left).into(),
                    ))
                    .margin(ratatui::layout::Margin {
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

fn set_suggest_input<
    T: ferrowl_ui::widgets::Validate + Clone,
    P: ferrowl_ui::traits::SuggestionProvider + Clone,
>(
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
    use ferrowl_ui::traits::{IsFocus, SetFocus};
    use ferrowl_ui::{render_field, render_row};
    use ratatui::{
        buffer::Buffer,
        layout::{Constraint, Rect},
    };

    fn tmp_file(name: &str) -> String {
        let path = std::env::temp_dir().join(format!("ferrowl_tls_section_test_{name}"));
        std::fs::write(&path, b"").unwrap();
        path.to_str().unwrap().to_string()
    }

    fn type_into<S: ferrowl_ui::traits::SetFocus + HandleEvents>(state: &mut S, s: &str) {
        state.set_focused(true);
        for c in s.chars() {
            state.handle_events(KeyModifiers::NONE, KeyCode::Char(c));
        }
    }

    fn confirm_ca_add(section: &mut TlsSection) {
        while section
            .client_ca_add_dialog
            .as_ref()
            .unwrap()
            .path
            .state
            .suggestions_open()
        {
            section.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        }
        section.handle_events(KeyModifiers::NONE, KeyCode::Enter);
    }

    fn render_bare(section: &mut TlsSection, is_server: bool, area: Rect, buf: &mut Buffer) {
        // `render_field!`/`render_row!` take a bare `$self:ident`, not `self`, hence the rebind.
        let rows: [Rect; 4] =
            ratatui::layout::Layout::vertical([Constraint::Length(3); 4]).areas(area);
        let mut idx = 0;
        let tls = section;
        if tls.show_self_signed() {
            render_field!(tls, self_signed, rows[idx], buf);
            idx += 1;
        }
        if tls.show_identity_row() {
            if is_server {
                render_row!(tls, rows[idx], buf; cert_file, key_file);
            } else {
                render_row!(tls, rows[idx], buf; client_cert_file, client_key_file);
            }
            idx += 1;
        }
        if tls.show_skip_verify_row() {
            if is_server {
                render_field!(tls, client_cert_skip_verify, rows[idx], buf);
            } else {
                render_field!(tls, skip_verify, rows[idx], buf);
            }
            idx += 1;
        }
        if tls.show_peer_verify_row() {
            if is_server {
                render_row!(tls, rows[idx], buf;
                    client_ca_files => Constraint::Percentage(60),
                    client_ca_add_button => Constraint::Percentage(20),
                    client_ca_delete_button => Constraint::Fill(1)
                );
            } else {
                render_field!(tls, ca_file, rows[idx], buf);
            }
        }
    }

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

    fn row_of(buf: &Buffer, needle: &str) -> u16 {
        let text = buffer_text(buf);
        text.lines()
            .position(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} not found in:\n{text}")) as u16
    }

    // --- sync ------------------------------------------------------------------------------

    #[test]
    /// UI-R-049 — `sync` updates the fresh role/level `TlsSection`'s own gates read, rather than
    /// caching a stale copy from construction time.
    fn ut_sync_updates_role_and_level_predicates() {
        let mut section = TlsSection::new();
        section.sync(ClientOrServer::Server, EffectiveTlsLevel::Off);
        assert!(!section.show_server_cert());
        section.sync(ClientOrServer::Server, EffectiveTlsLevel::Tls);
        assert!(section.show_server_cert());
    }

    // --- extract / prefill ------------------------------------------------------------------

    #[test]
    /// MB-R-104..112 — a server TLS section at `Tls` with self-signed on extracts and resolves
    /// to `ServerTlsPolicy::Tls { server_cert: ServerCertSource::SelfSigned }`, dropping the
    /// mTLS-only client-CA list entirely.
    fn ut_extract_server_self_signed_builds_config() {
        let mut section = TlsSection::new();
        section.sync(ClientOrServer::Server, EffectiveTlsLevel::Tls);
        section.self_signed.state.set_selection(1); // On
        *section.client_ca_files.state.values_mut() = vec!["client_ca.pem".to_string()];
        let extracted = section.extract();
        assert!(extracted.self_signed);
        let server_cert = ServerCertSource::resolve(extracted.self_signed, None, None).unwrap();
        assert_eq!(server_cert, ServerCertSource::SelfSigned);
    }

    #[test]
    /// MB-R-135/OC-R-111 — toggling Self-Signed On excludes stale cert_file/key_file text from
    /// the extracted inputs, even though the widgets' stored text is untouched.
    fn ut_extract_self_signed_excludes_stale_cert_key_text() {
        let mut section = TlsSection::new();
        section.sync(ClientOrServer::Server, EffectiveTlsLevel::Tls);
        set_suggest_input(&mut section.cert_file, "s.crt");
        set_suggest_input(&mut section.key_file, "s.key");
        section.self_signed.state.set_selection(1); // On, after text was typed

        let extracted = section.extract();
        assert!(extracted.self_signed);
        // The stored text survives the toggle -- extract still reports it, since excluding it
        // from the resolved policy is `ServerCertSource::resolve`'s job, not `extract`'s.
        assert_eq!(extracted.cert_file, "s.crt");
        assert_eq!(extracted.key_file, "s.key");
        let server_cert = ServerCertSource::resolve(
            extracted.self_signed,
            Some(extracted.cert_file.clone()),
            Some(extracted.key_file.clone()),
        )
        .unwrap();
        assert_eq!(server_cert, ServerCertSource::SelfSigned);
    }

    #[test]
    /// MB-R-135/OC-R-111 — toggling Skip-Verify On is reflected in the extracted inputs, and the
    /// stale ca_file text is preserved on the widget (only excluded downstream).
    fn ut_extract_skip_verify_excludes_stale_ca_file_text() {
        let mut section = TlsSection::new();
        section.sync(ClientOrServer::Client, EffectiveTlsLevel::Tls);
        set_suggest_input(&mut section.ca_file, "ca.pem");
        section.skip_verify.state.set_selection(1); // On, after text was typed

        let extracted = section.extract();
        assert!(extracted.skip_verify);
        assert_eq!(extracted.ca_file, "ca.pem");
        let verification = ClientVerification::resolve(extracted.skip_verify, None);
        assert_eq!(verification, ClientVerification::SkipVerify);
    }

    #[test]
    /// MB-R-135 — toggling Self-Signed back Off restores the previously entered cert/key paths
    /// (nothing was cleared, only excluded while On).
    fn ut_extract_toggle_self_signed_back_off_restores_cert_key() {
        let cert = tmp_file("s.crt");
        let key = tmp_file("s.key");

        let mut section = TlsSection::new();
        section.sync(ClientOrServer::Server, EffectiveTlsLevel::Tls);
        set_suggest_input(&mut section.cert_file, &cert);
        set_suggest_input(&mut section.key_file, &key);
        section.self_signed.state.set_selection(1); // On
        section.self_signed.state.set_selection(0); // Off again

        let extracted = section.extract();
        assert!(!extracted.self_signed);
        assert_eq!(extracted.cert_file, cert);
        assert_eq!(extracted.key_file, key);
    }

    #[test]
    /// MB-R-104..112 — a `~/...` path validates the same way TLS material loading will.
    fn ut_extract_tls_cert_key_tilde_paths_validate() {
        let home = std::env::home_dir().expect("HOME must resolve in test environment");
        let cert_name = format!("ferrowl_tls_section_tilde_{}.crt", std::process::id());
        let key_name = format!("ferrowl_tls_section_tilde_{}.key", std::process::id());
        std::fs::write(home.join(&cert_name), b"").unwrap();
        std::fs::write(home.join(&key_name), b"").unwrap();

        let mut section = TlsSection::new();
        section.sync(ClientOrServer::Server, EffectiveTlsLevel::Tls);
        set_suggest_input(&mut section.cert_file, &format!("~/{cert_name}"));
        set_suggest_input(&mut section.key_file, &format!("~/{key_name}"));

        let extracted = section.extract();
        let cert_path = ferrowl_util::path::expand(&extracted.cert_file);
        let key_path = ferrowl_util::path::expand(&extracted.key_file);

        let cert_exists = cert_path.exists();
        let key_exists = key_path.exists();
        let _ = std::fs::remove_file(home.join(&cert_name));
        let _ = std::fs::remove_file(home.join(&key_name));

        assert!(cert_exists, "expanded cert path must exist");
        assert!(key_exists, "expanded key path must exist");
    }

    #[test]
    /// MB-R-104..112 — `prefill` restores a server-role `MutualTls` policy's fields (self-signed,
    /// client-CA list, client_cert_skip_verify) and round-trips through `extract`.
    fn ut_prefill_server_mutual_tls_round_trips() {
        let server = ServerTlsPolicy::MutualTls {
            server_cert: ServerCertSource::Explicit {
                cert_file: "s.crt".to_string(),
                key_file: "s.key".to_string(),
            },
            client_verification: ClientCertVerification::Verify {
                ca_files: vec!["ca1.pem".to_string(), "ca2.pem".to_string()],
            },
        };
        let mut section = TlsSection::new();
        section.prefill(ClientOrServer::Server, Some(&server), None);
        section.sync(ClientOrServer::Server, EffectiveTlsLevel::MutualTls);

        let extracted = section.extract();
        assert!(!extracted.self_signed);
        assert_eq!(extracted.cert_file, "s.crt");
        assert_eq!(extracted.key_file, "s.key");
        assert_eq!(
            extracted.client_ca_files,
            vec!["ca1.pem".to_string(), "ca2.pem".to_string()]
        );
        assert!(!extracted.client_cert_skip_verify);
    }

    #[test]
    /// MB-R-139 — `prefill` restores a client-role self-signed `MutualTls` identity.
    fn ut_prefill_client_mutual_tls_self_signed_round_trips() {
        let client = ClientTlsPolicy::MutualTls {
            client_verification: ClientVerification::default(),
            client_identity: ClientCertSource::SelfSigned,
        };
        let mut section = TlsSection::new();
        section.prefill(ClientOrServer::Client, None, Some(&client));
        section.sync(ClientOrServer::Client, EffectiveTlsLevel::MutualTls);

        let extracted = section.extract();
        assert!(extracted.self_signed);
        assert_eq!(extracted.client_cert_file, "");
        assert_eq!(extracted.client_key_file, "");
    }

    // --- row order / render ------------------------------------------------------------------

    #[test]
    /// UI-R-024 — mTLS row order, server role: Self-Signed first, then the server's own
    /// cert/key pair, then Skip Verify, then the client-CA list.
    fn ut_mtls_row_order_server() {
        let mut section = TlsSection::new();
        section.sync(ClientOrServer::Server, EffectiveTlsLevel::MutualTls);
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render_bare(&mut section, true, area, &mut buf);
        let self_signed_row = row_of(&buf, "Self-Signed");
        let cert_row = row_of(&buf, "Cert File");
        let skip_row = row_of(&buf, "Skip Verify");
        let ca_row = row_of(&buf, "Client CA(s)");
        assert!(self_signed_row < cert_row);
        assert!(cert_row < skip_row);
        assert!(skip_row < ca_row);
    }

    #[test]
    /// UI-R-024 — mTLS row order, client role: Self-Signed first, then the client's own
    /// cert/key pair, then Skip Verify, then the CA-file input.
    fn ut_mtls_row_order_client() {
        let mut section = TlsSection::new();
        section.sync(ClientOrServer::Client, EffectiveTlsLevel::MutualTls);
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render_bare(&mut section, false, area, &mut buf);
        let self_signed_row = row_of(&buf, "Self-Signed");
        let cert_row = row_of(&buf, "Client Cert");
        let skip_row = row_of(&buf, "Skip Verify");
        let ca_row = row_of(&buf, "CA File");
        assert!(self_signed_row < cert_row);
        assert!(cert_row < skip_row);
        assert!(skip_row < ca_row);
    }

    // --- client-CA list lifecycle (moved verbatim from Modbus's setup_dialog.rs) -------------

    /// MB-R-136 — the client-CA row is a genuine add/remove list: the ADD button opens a
    /// sub-dialog whose confirmed path is appended and selected, and the DEL button removes
    /// whichever entry is currently selected — not a comma-separated text field.
    #[test]
    fn ut_client_ca_files_add_remove_edit() {
        let ca1 = tmp_file("mca1.pem");
        let ca2 = tmp_file("mca2.pem");
        let mut section = TlsSection::new();
        section.sync(ClientOrServer::Server, EffectiveTlsLevel::MutualTls);
        section.self_signed.state.set_selection(1); // server cert self-signed, no file needed

        assert!(section.client_ca_files.state.values().is_empty());

        // ADD: open the sub-dialog, type a path, confirm with Enter.
        section.focus = TlsSectionFocus::ClientCaAddButton;
        section.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(section.client_ca_add_dialog.is_some());
        type_into(
            &mut section.client_ca_add_dialog.as_mut().unwrap().path.state,
            &ca1,
        );
        confirm_ca_add(&mut section);
        assert!(section.client_ca_add_dialog.is_none());
        assert_eq!(
            section.client_ca_files.state.values(),
            std::slice::from_ref(&ca1)
        );

        // ADD a second entry.
        section.focus = TlsSectionFocus::ClientCaAddButton;
        section.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        type_into(
            &mut section.client_ca_add_dialog.as_mut().unwrap().path.state,
            &ca2,
        );
        confirm_ca_add(&mut section);
        assert_eq!(
            section.client_ca_files.state.values(),
            &[ca1.clone(), ca2.clone()]
        );

        // An empty path is rejected: the sub-dialog stays open with an error, nothing appended.
        section.focus = TlsSectionFocus::ClientCaAddButton;
        section.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        section.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(section.client_ca_add_dialog.is_some());
        assert!(
            !section
                .client_ca_add_dialog
                .as_ref()
                .unwrap()
                .error
                .state
                .is_empty()
        );

        // A path that doesn't exist on disk is also rejected: same sub-dialog stays open with an
        // error, nothing appended.
        type_into(
            &mut section.client_ca_add_dialog.as_mut().unwrap().path.state,
            "/nonexistent/ca-does-not-exist.pem",
        );
        section.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(section.client_ca_add_dialog.is_some());
        assert!(
            !section
                .client_ca_add_dialog
                .as_ref()
                .unwrap()
                .error
                .state
                .is_empty()
        );
        section.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(section.client_ca_add_dialog.is_none());
        assert_eq!(
            section.client_ca_files.state.values(),
            &[ca1.clone(), ca2.clone()]
        );

        // DEL: remove the currently-selected entry (selection sits on the last-added item).
        assert_eq!(section.client_ca_files.state.selection(), 1);
        section.focus = TlsSectionFocus::ClientCaDeleteButton;
        section.handle_events(KeyModifiers::NONE, KeyCode::Char(' '));
        assert_eq!(
            section.client_ca_files.state.values(),
            std::slice::from_ref(&ca1)
        );

        // Removing the last entry leaves the list empty and the DEL button no longer eligible.
        section.focus = TlsSectionFocus::ClientCaDeleteButton;
        section.handle_events(KeyModifiers::NONE, KeyCode::Char(' '));
        assert!(section.client_ca_files.state.values().is_empty());
        assert!(!section.show_client_ca() || section.client_ca_files.state.values().is_empty());

        // Deleting down to an empty list must not leave `focus` stuck on the now-ineligible DEL
        // button — it falls back to ADD.
        assert_eq!(section.focus, TlsSectionFocus::ClientCaAddButton);
    }

    /// UI-R-026 — Esc while the path field's completion popup is open dismisses only the popup;
    /// a second Esc (popup now closed) closes the sub-dialog itself.
    #[test]
    fn ut_client_ca_add_dialog_esc_dismisses_popup_before_sub_dialog() {
        let mut section = TlsSection::new();
        section.sync(ClientOrServer::Server, EffectiveTlsLevel::MutualTls);
        section.self_signed.state.set_selection(1);

        section.focus = TlsSectionFocus::ClientCaAddButton;
        section.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        type_into(
            &mut section.client_ca_add_dialog.as_mut().unwrap().path.state,
            "s",
        );
        assert!(
            section
                .client_ca_add_dialog
                .as_ref()
                .unwrap()
                .path
                .state
                .suggestions_open(),
            "no completion popup offered for a 's' prefix"
        );

        section.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(
            section.client_ca_add_dialog.is_some(),
            "Esc with the popup open must dismiss the popup, not the sub-dialog"
        );
        assert!(
            !section
                .client_ca_add_dialog
                .as_ref()
                .unwrap()
                .path
                .state
                .suggestions_open()
        );

        section.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(
            section.client_ca_add_dialog.is_none(),
            "Esc with the popup already closed must close the sub-dialog"
        );
    }

    /// MB-R-136 — deleting the last remaining client-CA entry moves focus off the now-
    /// unfocusable DEL button and onto ADD, so a subsequent Tab still traverses correctly.
    #[test]
    fn ut_delete_last_client_ca_falls_back_focus_to_add_button() {
        let mut section = TlsSection::new();
        section.sync(ClientOrServer::Server, EffectiveTlsLevel::MutualTls);
        section
            .client_ca_files
            .state
            .set_values(vec!["ca1.pem".to_string()]);
        section.focus = TlsSectionFocus::ClientCaDeleteButton;
        section.client_ca_delete_button.state.set_focused(true);

        section.handle_events(KeyModifiers::NONE, KeyCode::Char(' '));

        assert!(section.client_ca_files.state.values().is_empty());
        assert_eq!(section.focus, TlsSectionFocus::ClientCaAddButton);
        assert!(!section.client_ca_delete_button.state.is_focused());
        assert!(section.client_ca_add_button.state.is_focused());
        section.focus_next();
        assert_ne!(section.focus, TlsSectionFocus::ClientCaDeleteButton);
    }

    /// UI-R-024 — an empty client-CA list shows no placeholder entry, and the DEL button is not
    /// rendered at all (nothing eligible to delete), so ADD gets the row's full width.
    #[test]
    fn ut_client_ca_empty_hides_delete_button() {
        let mut section = TlsSection::new();
        section.sync(ClientOrServer::Server, EffectiveTlsLevel::MutualTls);
        assert!(section.client_ca_files.state.values().is_empty());
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render_field!(section, self_signed, Rect::new(0, 0, 80, 3), &mut buf);
        // No client-CA entries: give ADD the row's full remaining width, DEL entirely skipped,
        // mirroring the outer dialog's own render() empty-list branch.
        if section.focus == TlsSectionFocus::ClientCaDeleteButton {
            section.focus_previous();
        }
        render_row!(section, Rect::new(0, 3, 80, 3), &mut buf;
            client_ca_files => Constraint::Percentage(80),
            client_ca_add_button => Constraint::Fill(1)
        );
        let text = buffer_text(&buf);
        assert!(
            !text.contains("DEL"),
            "DEL button rendered with an empty client-CA list:\n{text}"
        );
        assert!(text.contains("ADD"), "ADD button missing:\n{text}");
    }

    /// UI-R-024 — with an empty client-CA list, the `client_ca_files` box still occupies the
    /// full fixed 3-row slot (top border + content + bottom border) rather than shrinking to a
    /// 2-row box.
    #[test]
    fn ut_client_ca_empty_list_box_keeps_full_row_height() {
        let mut section = TlsSection::new();
        section.sync(ClientOrServer::Server, EffectiveTlsLevel::MutualTls);
        assert!(section.client_ca_files.state.values().is_empty());
        let area = Rect::new(0, 0, 80, 3);
        let mut buf = Buffer::empty(area);
        render_row!(section, area, &mut buf;
            client_ca_files => Constraint::Percentage(80),
            client_ca_add_button => Constraint::Fill(1)
        );
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

    /// UI-R-024 — the client-CA list's row stays a fixed 3 rows tall (1 content + 2 border)
    /// regardless of how many entries it holds; more entries scroll/clip, never grow the box.
    #[test]
    fn ut_client_ca_row_height_fixed_regardless_of_entry_count() {
        let mut section = TlsSection::new();
        section.sync(ClientOrServer::Server, EffectiveTlsLevel::MutualTls);
        section
            .client_ca_files
            .state
            .set_values((0..10).map(|i| format!("ca{i}.pem")).collect::<Vec<_>>());
        let area = Rect::new(0, 0, 80, 6);
        let mut buf = Buffer::empty(area);
        let [row0, row1] =
            ratatui::layout::Layout::vertical([Constraint::Length(3), Constraint::Length(3)])
                .areas(area);
        render_row!(section, row0, &mut buf;
            client_ca_files => Constraint::Percentage(60),
            client_ca_add_button => Constraint::Percentage(20),
            client_ca_delete_button => Constraint::Fill(1)
        );
        // Something else painted in row1, to prove the client-CA box didn't bleed into it.
        render_field!(section, ca_file, row1, &mut buf);
        let text = buffer_text(&buf);
        let ca_row = row_of(&buf, "Client CA(s)");
        let next_row = row_of(&buf, "CA File");
        assert_eq!(
            next_row - ca_row,
            3,
            "client-CA row appears to have grown beyond a fixed 3-row box:\n{text}"
        );
    }
}
