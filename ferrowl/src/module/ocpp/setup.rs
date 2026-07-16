//! OCPP creation dialog wrapper. Implements [`SetupView`] over [`OcppSetupDialog`] and, on
//! confirm, builds the matching view for the chosen role (client → full CS view, server →
//! placeholder).

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::EventResult;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::module::ocpp::client::build_client_view;
use crate::module::ocpp::config::device::OcppDeviceConfig;
use crate::module::ocpp::config::session::{OcppModuleSpec, OcppProtocol, OcppRole, OcppSpec};
use crate::module::ocpp::server::build_server_view;
use crate::module::ocpp::setup_dialog::OcppSetupDialog;
use crate::module::type_descriptor::{ModuleViewFactory, SetupView};

/// Setup dialog for the OCPP module type.
pub struct OcppSetupView {
    dialog: OcppSetupDialog,
}

impl OcppSetupView {
    pub fn new() -> Self {
        Self {
            dialog: OcppSetupDialog::new(),
        }
    }
}

impl SetupView for OcppSetupView {
    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        self.dialog.render(area, buf);
    }

    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        self.dialog.handle_events(modifiers, code)
    }

    fn focus_next(&mut self) {
        self.dialog.focus_next();
    }

    fn focus_previous(&mut self) {
        self.dialog.focus_previous();
    }

    fn close_requested(&mut self) -> bool {
        self.dialog.take_close_request()
    }

    fn confirm(&self) -> Option<(String, ModuleViewFactory)> {
        let spec = self.dialog.resolve().ok()?;
        let path = self.dialog.config_path();
        let name = spec.name.clone();

        // Assemble the device config: an existing file at `path` is authoritative (its scripts,
        // and — to avoid clobbering — its version/role/timeout); otherwise build it from the
        // dialog's selections with no scripts yet.
        let device = if path.is_empty() {
            OcppDeviceConfig::from_spec(&spec, Vec::new())
        } else {
            match crate::config::load_ocpp_device(&path) {
                Ok(mut loaded) => {
                    apply_security_precedence(&mut loaded, &spec);
                    loaded
                }
                Err(_) => OcppDeviceConfig::from_spec(&spec, Vec::new()),
            }
        };
        // Reconcile the runtime spec with the (possibly file-sourced) device fields + endpoint.
        let module = OcppModuleSpec::from_spec(&spec, &path);
        let spec = OcppSpec::from_parts(&module, &device);

        let factory: ModuleViewFactory = match device.role {
            OcppRole::Client => Box::new(move || build_client_view(spec, path, device)),
            OcppRole::Server => Box::new(move || build_server_view(spec, path, device)),
        };
        Some((name, factory))
    }
}

/// Decide which security section wins when merging a loaded device config with the dialog's
/// resolved spec. The dialog only exposes security controls for `wss://`, so a `ws://` selection
/// must not silently wipe out a security section already present in the loaded file: the file's
/// section is left untouched. For `wss://` the dialog is authoritative and overwrites it.
fn apply_security_precedence(loaded: &mut OcppDeviceConfig, spec: &OcppSpec) {
    if spec.protocol == OcppProtocol::Wss {
        loaded.security = spec.security.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::ocpp::config::device::OcppSecurityConfig;
    use crate::module::ocpp::config::session::OcppVersion;
    use crossterm::event::{KeyCode, KeyModifiers};

    // Regression: the `SetupView::close_requested` default trait method must be overridden here
    // to delegate to the dialog's close-confirm popup, or the creation overlay's Esc→confirm
    // flow would silently do nothing for an OCPP module setup.
    #[test]
    /// UI-R-023 — the OCPP setup delegates close-requested to the dialog's close-request flag.
    fn ut_close_requested_delegates_to_dialog_take_close_request() {
        let mut sv = OcppSetupView::new();
        assert!(!sv.close_requested());
        sv.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        sv.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(sv.close_requested());
        assert!(!sv.close_requested(), "flag must clear after take");
    }

    fn base_spec(protocol: OcppProtocol) -> OcppSpec {
        OcppSpec {
            name: "cs-1".into(),
            version: OcppVersion::V1_6,
            role: OcppRole::Client,
            protocol,
            ip: "127.0.0.1".into(),
            port: 9000,
            path: String::new(),
            timeout_ms: None,
            security: OcppSecurityConfig {
                username: Some("dialog-user".into()),
                ..Default::default()
            },
        }
    }

    #[test]
    /// UI-R-024 — a ws setup preserves the loaded security config on resolve.
    fn ut_ws_preserves_loaded_security() {
        let mut loaded = OcppDeviceConfig {
            security: OcppSecurityConfig {
                ca_file: Some("existing-ca.pem".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let spec = base_spec(OcppProtocol::Ws);
        apply_security_precedence(&mut loaded, &spec);
        assert_eq!(loaded.security.ca_file.as_deref(), Some("existing-ca.pem"));
        assert_eq!(loaded.security.username, None);
    }

    #[test]
    /// UI-R-024 — a wss setup resolves the dialog's security config over the loaded one.
    fn ut_wss_overwrites_loaded_security_with_dialog() {
        let mut loaded = OcppDeviceConfig {
            security: OcppSecurityConfig {
                ca_file: Some("existing-ca.pem".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let spec = base_spec(OcppProtocol::Wss);
        apply_security_precedence(&mut loaded, &spec);
        assert_eq!(loaded.security, spec.security);
    }
}
