//! Builds the view that replaces an OCPP module after a role/version-switching edit, honouring a
//! lifecycle command merged into the apply (UI-R-354).

use crate::app::Level;
use crate::module::ocpp::client::build_client_view;
use crate::module::ocpp::config::device::OcppDeviceConfig;
use crate::module::ocpp::config::session::{OcppRole, OcppSpec};
use crate::module::ocpp::server::{build_server_view, build_server_view_unbound};
use crate::module::view::{CommandResult, ModuleView};

/// A lifecycle command that arrived while an edit apply was pending (UI-R-354).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum MergedCommand {
    Start,
    Stop,
    Restart,
}

/// Build the replacement view for `role`/`spec`. With a merged `then`, the apply line is written
/// to the replacement's own log (the tab adopts it) followed by the command's line (UI-R-358); a
/// merged `Stop` leaves a CSMS unbound (UI-R-356) and reports `stop_error` at Error (UI-R-315).
pub(crate) async fn build_replacement(
    role: OcppRole,
    spec: OcppSpec,
    path: String,
    device: OcppDeviceConfig,
    then: Option<MergedCommand>,
    stop_error: Option<String>,
) -> Box<dyn ModuleView> {
    let mut view = match role {
        OcppRole::Client => build_client_view(spec, path, device),
        OcppRole::Server if then == Some(MergedCommand::Stop) => {
            build_server_view_unbound(spec, path, device)
        }
        OcppRole::Server => build_server_view(spec, path, device),
    };
    let Some(cmd) = then else {
        return view;
    };
    let log = view.log();
    log.write().await.write(Level::Info, "Settings updated");
    let result = match cmd {
        MergedCommand::Start => view.handle_command("start").await,
        MergedCommand::Restart => view.handle_command("restart").await,
        MergedCommand::Stop => {
            let (level, msg) = match (role, stop_error) {
                (OcppRole::Client, None) => (Level::Info, "Disconnected".to_string()),
                (OcppRole::Server, None) => (Level::Info, "CSMS server stopped".to_string()),
                (OcppRole::Client, Some(e)) => (Level::Error, format!("Disconnect failed: {e}")),
                (OcppRole::Server, Some(e)) => {
                    (Level::Error, format!("CSMS server stop failed: {e}"))
                }
            };
            log.write().await.write(level, &msg);
            return view;
        }
    };
    if let CommandResult::Handled(Some((level, msg))) = result {
        log.write().await.write(level, &msg);
    }
    view
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::ocpp::config::session::{OcppProtocol, OcppVersion};

    fn spec(role: OcppRole) -> OcppSpec {
        OcppSpec {
            name: "mod".into(),
            version: OcppVersion::V1_6,
            role,
            protocol: OcppProtocol::Ws,
            ip: "127.0.0.1".into(),
            port: 0,
            path: String::new(),
            timeout_ms: None,
            reconnect: None,
            security: Default::default(),
        }
    }

    async fn stop_lines(role: OcppRole, err: Option<&str>) -> Vec<(Level, String)> {
        let view = build_replacement(
            role,
            spec(role),
            String::new(),
            OcppDeviceConfig::default(),
            Some(MergedCommand::Stop),
            err.map(str::to_string),
        )
        .await;
        view.log()
            .read()
            .await
            .peek_n(crate::app::LOG_SIZE)
            .into_iter()
            .map(|(_, level, l)| (level, l))
            .collect()
    }

    /// UI-R-315, UI-R-358 — a failed stop merged into a replacing apply is reported at Error with
    /// the same text the module's own stop uses, never as a clean stop.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ut_merged_stop_failure_is_logged_at_error() {
        let client = stop_lines(OcppRole::Client, Some("boom")).await;
        assert!(client.contains(&(Level::Error, "Disconnect failed: boom".to_string())));
        assert!(!client.iter().any(|(_, l)| l == "Disconnected"));
        let server = stop_lines(OcppRole::Server, Some("boom")).await;
        assert!(server.contains(&(Level::Error, "CSMS server stop failed: boom".to_string())));
        assert!(!server.iter().any(|(_, l)| l == "CSMS server stopped"));
    }
}
