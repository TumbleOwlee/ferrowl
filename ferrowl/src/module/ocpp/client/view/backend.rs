//! Simulation + backend glue: the Lua sim lifecycle, the per-tick `refresh` (deferred send/setup,
//! Lua-queued actions, auto-Heartbeat/MeterValues, message log sync), `:` command execution, and
//! payload send/dispatch.

use crate::app::Level;
use crate::module::ocpp::client::backend::{DEFAULT_HEARTBEAT_SECS, TICKS_PER_SEC};
use crate::module::ocpp::client::build_client_view;
use crate::module::ocpp::client::lua_sim::{
    ClientFields, merge_overrides, run_client_script_once, run_client_sim,
};
use crate::module::ocpp::config::device::{ConfigKeyDef, OcppDeviceConfig};
use crate::module::ocpp::config::session::OcppRole;
use crate::module::ocpp::lock::{HasState, with_state, with_state_mut};
use crate::module::ocpp::scope::Scope;
use crate::module::ocpp::server::build_server_view;
use crate::module::view::{CommandFuture, CommandResult, RefreshFuture, parse_command};

use super::{
    ClientState, ClientVersion, ClientView, OCPP_CLIENT_COMMAND_SPECS, OcppClientCmd,
    PendingLifecycle, SetupFollowUp, config_rows, conn_rows, msg_row, nv_rows,
};

/// Read a CS-level string field (boot identity) by its `ClientFields` name, for persisting on
/// `:wd` (OC-R-103). `None` if the field is missing or not a string.
fn cs_string_field<S: ClientFields>(s: &S, name: &str) -> Option<String> {
    match s.cs_get(name) {
        Some(ferrowl_lua::module::ValueType::String(v)) => Some(v),
        _ => None,
    }
}

impl<V: ClientVersion> ClientView<V> {
    pub(super) fn start_sim(&mut self) {
        self.stop_sim();
        self.runtime.handle = run_client_sim(
            self.state.clone(),
            self.runtime.action_queue.clone(),
            self.enabled_scripts(),
            self.device.script_interval_duration(),
            self.script_log.clone(),
        );
    }

    fn stop_sim(&mut self) {
        if let Some(mut sim) = self.runtime.handle.take() {
            sim.stop();
        }
    }

    /// Execute one script once against this station's state, outside the sim (SC-R-035). The sim
    /// thread is left alone, and the script runs whether or not it is enabled.
    pub(super) fn run_script_once(&mut self, name: String, code: String) {
        run_client_script_once(
            self.state.clone(),
            self.runtime.action_queue.clone(),
            name,
            code,
            self.script_log.clone(),
        );
    }

    /// Drain and send one Lua-enqueued action. The transaction shortcuts map to a TransactionEvent
    /// for the action's connector; state-driven and other actions build their payload then merge.
    fn dispatch_lua_action(&mut self, scope: Scope, name: &str, overrides: serde_json::Value) {
        let (send_name, mut payload) = match name {
            "StartTransaction" if V::has_tx_shortcuts() => {
                ("TransactionEvent".to_string(), self.start_event(scope))
            }
            "StopTransaction" if V::has_tx_shortcuts() => match self.stop_event(scope) {
                Some(payload) => ("TransactionEvent".to_string(), payload),
                None => return,
            },
            n if V::state_driven().contains(&n) => (name.to_string(), self.state_payload(n, scope)),
            _ => {
                let template = V::default_action(name)
                    .and_then(|a| V::encode_action(&a).ok())
                    .unwrap_or_else(|| serde_json::json!({}));
                (name.to_string(), template)
            }
        };
        merge_overrides(&mut payload, overrides);
        self.send_payload(&send_name, payload, scope);
    }

    fn make_handler(&self) -> V::Handler {
        V::handler(
            self.backend.online_handle(),
            self.backend.messages_handle(),
            self.state.clone(),
            self.backend.sender(),
        )
    }

    /// UI-R-350 — signal the stop an applied configuration edit needs and defer `follow_up` to the
    /// settle block, following the same overwrite/nothing-was-running rules as `:stop`/`:restart`
    /// (`OcppClientCmd::Stop`). Returns `true` if the follow-up was deferred (caller must not fall
    /// through to code that assumes it already ran), `false` if it ran inline because nothing was
    /// running.
    async fn apply_setup_follow_up(&mut self, follow_up: SetupFollowUp) -> bool {
        if self.pending_lifecycle.is_some() {
            self.pending_lifecycle = Some(PendingLifecycle::ApplySetup(Box::new(follow_up)));
            return true;
        }
        match self.backend.request_stop().await {
            Ok(()) => {
                self.pending_lifecycle = Some(PendingLifecycle::ApplySetup(Box::new(follow_up)));
                true
            }
            // Nothing was running: no in-flight task for `poll_stop()` to ever resolve, so the
            // follow-up runs inline right here.
            Err(_) => {
                match follow_up {
                    SetupFollowUp::Replace(view) => {
                        self.deferred.replacement = Some(view);
                    }
                    SetupFollowUp::InPlace {
                        spec,
                        path,
                        device,
                        was_online,
                    } => {
                        self.spec = *spec;
                        self.device = *device;
                        self.device_path = path;
                        self.log
                            .write()
                            .await
                            .write(Level::Info, "Settings updated");
                        if was_online {
                            let handler = self.make_handler();
                            if let Err(e) = self
                                .backend
                                .start(&self.spec, &self.device, &self.log, handler)
                                .await
                            {
                                self.log.write().await.write(
                                    Level::Error,
                                    &format!("Restart after settings update failed: {e}"),
                                );
                            }
                        }
                    }
                }
                false
            }
        }
    }

    /// Write the device config (reconciled with the live spec, scripts + connectors preserved).
    fn save_device_to(&self, path: &str) -> CommandResult {
        use crate::convert::{Converter, FileType};
        let Some(ty) = FileType::from_path(path) else {
            return CommandResult::Handled(Some((
                Level::Warning,
                format!("unknown format for '{path}' (use .toml or .json)"),
            )));
        };
        let mut device = OcppDeviceConfig::from_spec(&self.spec, self.device.scripts.clone());
        device.version = Some(crate::config::VERSION.to_string());
        device.connectors = self.with_state(|s| {
            (0..s.connector_count())
                .map(|i| V::connector_ref(s, i))
                .collect()
        });
        // Persist the client's config keys (server config is transient, never written).
        device.config = self.with_state(|s| {
            s.config()
                .iter()
                .map(|c| ConfigKeyDef {
                    key: c.key.clone(),
                    value: c.value.clone(),
                    readonly: c.readonly,
                })
                .collect()
        });
        // Persist CS boot identity (OC-R-103).
        device.model = self.with_state(|s| cs_string_field(s, "Model"));
        device.vendor = self.with_state(|s| cs_string_field(s, "Vendor"));
        device.firmware_version = self.with_state(|s| cs_string_field(s, "FirmwareVersion"));
        device.serial_number = self.with_state(|s| cs_string_field(s, "SerialNumber"));
        // Persist the 1.6-only meter/modem identity fields (OC-R-104); a no-op on 2.0.1/2.1.
        device.iccid = self.with_state(|s| cs_string_field(s, "Iccid"));
        device.imsi = self.with_state(|s| cs_string_field(s, "Imsi"));
        device.meter_serial_number = self.with_state(|s| cs_string_field(s, "MeterSerialNumber"));
        device.meter_type = self.with_state(|s| cs_string_field(s, "MeterType"));
        let base = ferrowl_util::path::base_dir_of(path);
        device.security.tls.server.relativize_paths(&base);
        device.security.tls.client.relativize_paths(&base);
        match Converter::save(&device, path, ty) {
            Ok(()) => CommandResult::Handled(Some((
                Level::Info,
                format!("Saved device config to {path}"),
            ))),
            Err(e) => CommandResult::Handled(Some((Level::Error, format!("Save failed: {e:?}")))),
        }
    }

    pub(super) fn set_compact(&mut self, compact: bool) {
        self.compact = compact;
        let margin = ratatui::layout::Margin {
            vertical: if compact { 0 } else { 1 },
            horizontal: 0,
        };
        // The connector table stays compact (no vertical margin) to save space.
        self.state_table.widget.set_row_margin(margin);
        self.config_table.widget.set_row_margin(margin);
        self.msg_table.widget.set_row_margin(margin);
    }

    pub(super) fn start_event(&mut self, scope: Scope) -> serde_json::Value {
        let payload = self.with_state_mut(|s| V::start_event(s, scope));
        // 2.0.1 resets the meter tick eagerly on a started transaction.
        if V::has_tx_shortcuts() {
            self.runtime.meter_tick = 0;
        }
        payload
    }

    pub(super) fn stop_event(&mut self, scope: Scope) -> Option<serde_json::Value> {
        self.with_state_mut(|s| V::stop_event(s, scope))
    }

    pub(super) fn state_payload(&self, name: &str, scope: Scope) -> serde_json::Value {
        self.with_state(|s| V::state_payload(s, name, scope))
    }

    /// Decode + send a (name, payload) at `scope` without blocking the UI loop. A transaction start
    /// mints its id eagerly (carried in the payload, 2.0.1); confirm or roll it back on the response
    /// so auto-MeterValues only fire once the start is acknowledged.
    fn send_payload(&mut self, name: &str, payload: serde_json::Value, scope: Scope) {
        let sender = self.backend.sender();
        let state = self.state.clone();
        let log = self.log.clone();
        let name = name.to_string();
        let started_tx = (name == "TransactionEvent"
            && payload.get("eventType").and_then(|v| v.as_str()) == Some("Started"))
        .then(|| {
            payload
                .pointer("/transactionInfo/transactionId")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .flatten();
        // OC-R-122: a transaction-start message (1.6's `StartTransaction`, or 2.x's
        // `TransactionEvent(Started)`) is followed by a coupled `StatusNotification` once the
        // start is acknowledged.
        let is_tx_start = name == "StartTransaction" || started_tx.is_some();
        tokio::spawn(async move {
            match V::decode_call(&name, payload) {
                Ok(action) => match sender.clone().send_scoped(action, scope).await {
                    Ok(response) => {
                        with_state_mut(&state, |s| {
                            V::apply_post_send(s, &name, scope, started_tx.as_deref(), &response);
                        });
                        if is_tx_start {
                            let _ = crate::module::ocpp::client::backend::send_status_notification(
                                sender, &state, scope,
                            )
                            .await;
                        }
                    }
                    Err(e) => {
                        with_state_mut(&state, |s| {
                            V::rollback_tx(s, scope, started_tx.as_deref());
                        });
                        log.write()
                            .await
                            .write(Level::Error, &format!("{name} failed: {e}"));
                    }
                },
                Err(e) => log
                    .write()
                    .await
                    .write(Level::Error, &format!("{name} invalid payload: {e}")),
            }
        });
    }

    pub(super) fn refresh_impl<'a>(&'a mut self) -> RefreshFuture<'a> {
        Box::pin(async move {
            // UI-R-314/UI-R-315/UI-R-350 — a deferred stop-bearing lifecycle command or applied
            // configuration edit only signalled `request_stop()`; drain its outcome (and run any
            // follow-up) once the task actually ends.
            if self.pending_lifecycle.is_some()
                && let Some(stop_result) = self.backend.poll_stop().await
            {
                match self.pending_lifecycle.take() {
                    Some(PendingLifecycle::Stop) => {
                        let (level, msg) = match stop_result {
                            Ok(()) => (Level::Info, "Disconnected".to_string()),
                            // OC-R-102 — a stop failure logs at Error.
                            Err(e) => (Level::Error, format!("Disconnect failed: {e}")),
                        };
                        self.log.write().await.write(level, &msg);
                        self.last_stop_outcome = Some(match level {
                            Level::Error => crate::module::view::StopOutcome::Failed(msg),
                            _ => crate::module::view::StopOutcome::Clean,
                        });
                    }
                    Some(PendingLifecycle::Restart) => {
                        if let Err(e) = stop_result {
                            self.log
                                .write()
                                .await
                                .write(Level::Error, &format!("Reconnect: stop failed: {e}"));
                        }
                        let handler = self.make_handler();
                        let (level, msg) = match self
                            .backend
                            .start(&self.spec, &self.device, &self.log, handler)
                            .await
                        {
                            Ok(()) => (Level::Info, "Reconnecting".to_string()),
                            Err(e) => (Level::Error, format!("Reconnect failed: {e}")),
                        };
                        self.log.write().await.write(level, &msg);
                    }
                    Some(PendingLifecycle::ApplySetup(follow_up)) => {
                        // OC-R-102 — a stop failure logs at Error, same as the sibling `Stop` and
                        // `Restart` arms above; `Client<V>::join` has no path back to `Err` in the
                        // current fixtures, so this arm is untested there too.
                        if let Err(e) = stop_result {
                            self.log
                                .write()
                                .await
                                .write(Level::Error, &format!("Settings update: stop failed: {e}"));
                        }
                        match *follow_up {
                            SetupFollowUp::Replace(view) => {
                                self.deferred.replacement = Some(view);
                            }
                            SetupFollowUp::InPlace {
                                spec,
                                path,
                                device,
                                was_online,
                            } => {
                                self.spec = *spec;
                                self.device = *device;
                                self.device_path = path;
                                self.log
                                    .write()
                                    .await
                                    .write(Level::Info, "Settings updated");
                                if was_online {
                                    let handler = self.make_handler();
                                    if let Err(e) = self
                                        .backend
                                        .start(&self.spec, &self.device, &self.log, handler)
                                        .await
                                    {
                                        self.log.write().await.write(
                                            Level::Error,
                                            &format!("Restart after settings update failed: {e}"),
                                        );
                                    }
                                }
                            }
                        }
                    }
                    None => unreachable!("outer condition checked pending_lifecycle.is_some()"),
                }
            }

            if let Some((spec, path, extra_headers)) = self.deferred.setup.take() {
                let mut device = OcppDeviceConfig::from_spec(&spec, self.device.scripts.clone());
                device.log_file = self.device.log_file.clone();
                device.connectors = self.device.connectors.clone();
                device.config = self.device.config.clone();
                device.model = self.device.model.clone();
                device.vendor = self.device.vendor.clone();
                device.firmware_version = self.device.firmware_version.clone();
                device.serial_number = self.device.serial_number.clone();
                device.iccid = self.device.iccid.clone();
                device.imsi = self.device.imsi.clone();
                device.meter_serial_number = self.device.meter_serial_number.clone();
                device.meter_type = self.device.meter_type.clone();
                // Not carried forward from `self.device` like the fields above: the setup
                // dialog's headers table is authoritative for a confirmed edit, unlike device
                // metadata the dialog never exposes.
                device.extra_headers = extra_headers;
                if spec.role == OcppRole::Server {
                    let replacement = build_server_view(spec, path, device);
                    self.apply_setup_follow_up(SetupFollowUp::Replace(replacement))
                        .await;
                    return;
                }
                if spec.version != self.spec.version {
                    if !device.scripts.is_empty() {
                        self.log.write().await.write(
                            Level::Warning,
                            "Version switched: scripts kept but may call actions the new version lacks",
                        );
                    }
                    let replacement = build_client_view(spec, path, device);
                    self.apply_setup_follow_up(SetupFollowUp::Replace(replacement))
                        .await;
                    return;
                } else {
                    let was_online = self.backend.is_online();
                    let follow_up = SetupFollowUp::InPlace {
                        spec: Box::new(spec),
                        path,
                        device: Box::new(device),
                        was_online,
                    };
                    if self.apply_setup_follow_up(follow_up).await {
                        return;
                    }
                }
            }

            if let Some((name, payload, scope)) = self.deferred.send.take() {
                self.send_payload(&name, payload, scope);
            }

            // Drain Lua-enqueued actions (each with its scope) and send them.
            let queued: Vec<(Scope, String, serde_json::Value)> =
                self.runtime.action_queue.lock().drain(..).collect();
            for (scope, name, overrides) in queued {
                self.dispatch_lua_action(scope, &name, overrides);
            }

            let online = self.backend.is_online();
            if self.runtime.was_online && !online {
                self.log
                    .write()
                    .await
                    .write(Level::Warning, "Connection lost — auto-transmit halted");
                self.runtime.heartbeat_tick = 0;
            }
            self.runtime.was_online = online;

            // Auto-Heartbeat (CS-level) at the BootNotification-supplied cadence while connected.
            if online {
                let interval_secs = self
                    .with_state(super::ClientState::heartbeat_interval_secs)
                    .unwrap_or(DEFAULT_HEARTBEAT_SECS)
                    .max(1);
                self.runtime.heartbeat_tick = self.runtime.heartbeat_tick.wrapping_add(1);
                if self.runtime.heartbeat_tick >= interval_secs as u32 * TICKS_PER_SEC {
                    self.runtime.heartbeat_tick = 0;
                    self.send_payload("Heartbeat", serde_json::json!({}), Scope::CS);
                }
            }

            // Auto-MeterValues per connector with a live transaction (~every 5s), gated online.
            let active: Vec<Scope> = with_state(&self.state, |s| V::active_meter_scopes(s));
            with_state(&self.state, |s| {
                V::track_meter_reset(
                    s,
                    &mut self.runtime.tx_was_active,
                    &mut self.runtime.meter_tick,
                );
            });
            if !active.is_empty() && online {
                self.runtime.meter_tick = self.runtime.meter_tick.wrapping_add(1);
                if self.runtime.meter_tick.is_multiple_of(50) {
                    for scope in active {
                        let payload = self.state_payload("MeterValues", scope);
                        self.send_payload("MeterValues", payload, scope);
                    }
                }
            }

            if self.runtime.applied_log_file != self.device.log_file {
                let name = self.spec.name.clone();
                self.log
                    .write()
                    .await
                    .set_log_file(self.device.log_file.as_deref(), &name);
                self.runtime
                    .applied_log_file
                    .clone_from(&self.device.log_file);
            }

            // Refresh tables. Messages are teed to the persistent log (all scopes) then filtered to
            // the selected entry for display.
            self.messages = self.backend.messages_snapshot().await;
            let mut max_seq = self.runtime.logged_seq;
            let new_lines: Vec<String> = self
                .messages
                .iter()
                .filter(|m| m.seq > self.runtime.logged_seq)
                .map(|m| {
                    max_seq = max_seq.max(m.seq);
                    m.log_line()
                })
                .collect();
            if !new_lines.is_empty() {
                let mut log = self.log.write().await;
                for line in new_lines {
                    log.write(Level::Info, &line);
                }
                self.runtime.logged_seq = max_seq;
            }

            let scope = self.selected_scope();
            self.visible_messages = self
                .messages
                .iter()
                .filter(|m| m.scope == scope)
                .cloned()
                .collect();
            let rows: Vec<_> = self.visible_messages.iter().map(msg_row).collect();
            let at_bottom = super::render::msg_log_at_bottom(&self.msg_table.state);
            self.msg_table.state.set_values(rows);
            // Tail the log to the newest message so incoming traffic shows instantly, but never
            // while the user is reading it (Messages scrolled up) or scrolling the payload pane
            // (whose content is driven by the selected message row).
            let follow = match self.focus {
                super::ClientViewFocus::Code => false,
                super::ClientViewFocus::MsgTable => at_bottom,
                _ => true,
            };
            if follow {
                self.msg_table.state.move_to_bottom();
            }

            let cp = self.spec.name.clone();
            let (conn_rows, state_rows, config_rows) = self.with_state(|s| {
                let state_rows = match V::connector_index_for_state(s, scope) {
                    Some(i) => nv_rows(s.conn_state_rows(i)),
                    None => nv_rows(s.cs_state_rows()),
                };
                (conn_rows::<V>(&cp, s), state_rows, config_rows(s))
            });
            self.conn_table.state.set_values(conn_rows);
            self.state_table.state.set_values(state_rows);
            self.config_table.state.set_values(config_rows);
            self.sync_code();

            if let super::ClientOverlay::Scripts(dialog) = &mut self.overlay {
                let entries =
                    crate::dialog::scripts::snapshot_log(&self.script_log, crate::app::LOG_SIZE)
                        .await;
                dialog.set_log_entries(entries);
            }
        })
    }

    pub(super) fn handle_command_impl<'a>(&'a mut self, cmd: &'a str) -> CommandFuture<'a> {
        let Some(parsed) = parse_command(&OCPP_CLIENT_COMMAND_SPECS, cmd) else {
            return Box::pin(std::future::ready(CommandResult::Unhandled));
        };
        match parsed {
            OcppClientCmd::Start => Box::pin(async move {
                let handler = self.make_handler();
                match self
                    .backend
                    .start(&self.spec, &self.device, &self.log, handler)
                    .await
                {
                    Ok(()) => CommandResult::Handled(Some((
                        Level::Info,
                        format!("Connecting to {}", self.spec.url()),
                    ))),
                    Err(e) => {
                        CommandResult::Handled(Some((Level::Error, format!("Connect failed: {e}"))))
                    }
                }
            }),
            OcppClientCmd::Stop => Box::pin(async move {
                // A stop already in flight is overwritten with the new follow-up rather than
                // re-requested — `request_stop()` on an already-`Stopping` backend errors
                // `NotRunning`, which must never be mistaken for "nothing to stop" and drop the
                // earlier command's outcome (UI-R-315: never discarded).
                if self.pending_lifecycle.is_some() {
                    self.pending_lifecycle = Some(PendingLifecycle::Stop);
                    self.last_stop_outcome = None;
                    return CommandResult::Handled(None);
                }
                match self.backend.request_stop().await {
                    Ok(()) => {
                        self.pending_lifecycle = Some(PendingLifecycle::Stop);
                        self.last_stop_outcome = None;
                        CommandResult::Handled(None)
                    }
                    // Nothing was running: no deferred outcome to carry, and nothing for
                    // `refresh()` to ever observe — logged, never returned as `:stop`'s own
                    // immediate result (UI-R-315).
                    Err(e) => {
                        self.log
                            .write()
                            .await
                            .write(Level::Error, &format!("Disconnect failed: {e}"));
                        CommandResult::Handled(None)
                    }
                }
            }),
            OcppClientCmd::Restart => Box::pin(async move {
                // See `OcppClientCmd::Stop` above.
                if self.pending_lifecycle.is_some() {
                    self.pending_lifecycle = Some(PendingLifecycle::Restart);
                    return CommandResult::Handled(None);
                }
                match self.backend.request_stop().await {
                    Ok(()) => {
                        self.pending_lifecycle = Some(PendingLifecycle::Restart);
                        CommandResult::Handled(None)
                    }
                    // Nothing was running: run the follow-up start immediately — there is no
                    // in-flight task `poll_stop()` could ever resolve.
                    Err(ferrowl_ocpp::Error::NotRunning) => {
                        let handler = self.make_handler();
                        match self
                            .backend
                            .start(&self.spec, &self.device, &self.log, handler)
                            .await
                        {
                            Ok(()) => {
                                CommandResult::Handled(Some((Level::Info, "Reconnecting".into())))
                            }
                            Err(e) => CommandResult::Handled(Some((
                                Level::Error,
                                format!("Reconnect failed: {e}"),
                            ))),
                        }
                    }
                    Err(e) => {
                        CommandResult::Handled(Some((Level::Error, format!("Restart failed: {e}"))))
                    }
                }
            }),
            OcppClientCmd::Edit => {
                self.overlay = super::ClientOverlay::Setup(Box::new(
                    crate::module::ocpp::setup_dialog::OcppSetupDialog::edit(
                        &self.spec,
                        &self.device_path,
                        &self.device.extra_headers,
                    ),
                ));
                Box::pin(std::future::ready(CommandResult::Handled(None)))
            }
            OcppClientCmd::Compact => {
                self.set_compact(!self.compact);
                Box::pin(std::future::ready(CommandResult::Handled(None)))
            }
            OcppClientCmd::WriteDevice(None) => {
                let result = if self.device_path.is_empty() {
                    CommandResult::Handled(Some((
                        Level::Warning,
                        "No configuration file path configured.".into(),
                    )))
                } else {
                    self.save_device_to(&self.device_path.clone())
                };
                Box::pin(std::future::ready(result))
            }
            OcppClientCmd::WriteDevice(Some(path)) => {
                let result = self.save_device_to(&path);
                Box::pin(std::future::ready(result))
            }
            OcppClientCmd::Log(file) => {
                let msg = match file {
                    None => {
                        self.device.log_file = None;
                        "File logging disabled".to_string()
                    }
                    Some(path) => {
                        self.device.log_file = Some(path.clone());
                        format!("Logging to {path}")
                    }
                };
                Box::pin(std::future::ready(CommandResult::Handled(Some((
                    Level::Info,
                    msg,
                )))))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ferrowl_test_support::reserve_tcp_port;
    use parking_lot::Mutex;
    use tokio::sync::Notify;

    use super::*;
    use crate::module::ocpp::config::session::{OcppProtocol, OcppRole, OcppSpec, OcppVersion};
    use crate::module::view::ModuleView;
    use ferrowl_ocpp::csms::{self, CsmsActionHandler};

    /// No-op log sink for the CSMS side, mirroring `ferrowl-ocpp/tests/ws_loopback_v16.rs::sink`.
    fn sink() -> impl ferrowl_ocpp::LogFn + Clone {
        |_s: String| async move {}
    }

    /// Poll until the CSMS listener has bound (`spawn` retries the bind in the background).
    async fn bound_addr<V: ferrowl_ocpp::Version>(
        server: &csms::Server<V>,
    ) -> std::net::SocketAddr {
        for _ in 0..50 {
            if let Some(addr) = server.local_addr() {
                return addr;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("CSMS listener never bound");
    }

    /// Poll until `flag` is set (e.g. the CS backend reports `is_online()`).
    async fn wait_for(flag: impl Fn() -> bool) {
        for _ in 0..100 {
            if flag() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("condition never became true");
    }

    fn client_view<V: ClientVersion>(version: OcppVersion, port: u16) -> ClientView<V> {
        let spec = OcppSpec {
            name: "cs".into(),
            version,
            role: OcppRole::Client,
            protocol: OcppProtocol::Ws,
            ip: "127.0.0.1".into(),
            port,
            path: String::new(),
            timeout_ms: None,
            reconnect: None,
            security: Default::default(),
        };
        ClientView::<V>::new(spec, String::new(), OcppDeviceConfig::default())
    }

    /// CS-E-032 — a live `:log` sink is never written by `:write-device`: the saved file carries
    /// no `log_file` key, reloading yields `log_file == None`, and the live sink stays configured.
    #[test]
    fn ut_save_device_to_omits_log_file() {
        use crate::convert::{Converter, FileType};

        let mut v = client_view::<ferrowl_ocpp::V1_6>(OcppVersion::V1_6, 0);
        v.device.log_file = Some("/tmp/x.log".into());

        let dir = ferrowl_test_support::reserve_temp_dir("ferrowl_ocpp_save_device_client");
        let path = dir.join("device.toml");
        let path = path.to_str().unwrap();

        v.save_device_to(path);

        let text = std::fs::read_to_string(path).expect("read saved device file");
        assert!(
            !text.contains("log_file"),
            "saved device file unexpectedly carries log_file: {text}"
        );

        let back: OcppDeviceConfig = Converter::load(path, FileType::Toml).expect("load");
        assert_eq!(back.log_file, None);
        assert_eq!(v.device.log_file, Some("/tmp/x.log".into()));
    }

    /// NF-R-072 — `:write-device` re-relativises `security.tls` PEM paths under the target dir.
    #[test]
    fn ut_write_device_relativizes_security_tls_paths_client() {
        use crate::convert::{Converter, FileType};
        use ferrowl_util::tls::{CertSource, ServerTlsPolicy};

        let mut v = client_view::<ferrowl_ocpp::V1_6>(OcppVersion::V1_6, 0);
        let dir = ferrowl_test_support::reserve_temp_dir("ferrowl_ocpp_client_relativize");
        let cert = dir.join("cert.pem");
        let key = dir.join("key.pem");
        v.spec.security.tls.server = ServerTlsPolicy::Tls {
            identity: CertSource::Files {
                cert_file: cert.to_str().unwrap().to_string(),
                key_file: key.to_str().unwrap().to_string(),
            },
        };

        let path = dir.join("device.toml");
        let path = path.to_str().unwrap();
        v.save_device_to(path);

        let text = std::fs::read_to_string(path).expect("read saved device file");
        assert!(text.contains("cert_file = \"cert.pem\""));
        assert!(text.contains("key_file = \"key.pem\""));

        let _: OcppDeviceConfig = Converter::load(path, FileType::Toml).expect("load");
    }

    /// NF-R-072 — a client `verify = "ca-files"` list under the save target's directory is
    /// re-encoded relative to it.
    #[test]
    fn ut_write_device_relativizes_client_ca_files_under_target_dir() {
        use crate::convert::{Converter, FileType};
        use ferrowl_util::tls::{CertVerification, ClientTlsPolicy};

        let mut v = client_view::<ferrowl_ocpp::V1_6>(OcppVersion::V1_6, 0);
        let dir = ferrowl_test_support::reserve_temp_dir("ferrowl_ocpp_client_ca_files");
        let ca = dir.join("ca.pem");
        v.spec.security.tls.client = ClientTlsPolicy::Tls {
            verification: CertVerification::CaFiles {
                ca_files: vec![ca.to_str().unwrap().to_string()],
            },
        };

        let path = dir.join("device.toml");
        let path = path.to_str().unwrap();
        v.save_device_to(path);

        let text = std::fs::read_to_string(path).expect("read saved device file");
        assert!(text.contains("ca_files = [\"ca.pem\"]"));

        let _: OcppDeviceConfig = Converter::load(path, FileType::Toml).expect("load");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    /// OC-R-123 — a client view against an unreachable CSMS with reconnect enabled shows
    /// RECONNECTING (not DISCONNECTED) once its task starts backing off, via
    /// `render_status_bar`'s `COLOR_SCHEME.warning` background.
    async fn it_cs_view_shows_reconnecting_while_backing_off() {
        use crate::view::status_bar::ConnStatus;
        use ferrowl_ui::COLOR_SCHEME;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        // A reserved-then-released ephemeral port: nothing answers on it afterward.
        let port = reserve_tcp_port().release();
        let mut v = client_view::<ferrowl_ocpp::V1_6>(OcppVersion::V1_6, port);
        v.spec.timeout_ms = Some(200);
        v.spec.reconnect = Some(true);
        let handler = v.make_handler();
        v.backend
            .start(&v.spec, &v.device, &v.log, handler)
            .await
            .expect("start must not fail synchronously against an unreachable CSMS");

        for _ in 0..100 {
            if v.backend.connection_status() == ConnStatus::Reconnecting {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(
            v.backend.connection_status(),
            ConnStatus::Reconnecting,
            "task must be backing off, never Connected, against an unreachable CSMS"
        );

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal
            .draw(|frame| v.render(frame, frame.area()))
            .unwrap();
        let last_row = terminal.backend().buffer().area.height - 1;
        let contents: String = (0..120)
            .map(|x| {
                terminal.backend().buffer()[(x, last_row)]
                    .symbol()
                    .to_string()
            })
            .collect();
        assert!(
            contents.contains("RECONNECTING"),
            "backing-off client must show RECONNECTING: {contents:?}"
        );
        assert_eq!(
            terminal.backend().buffer()[(0, last_row)].bg,
            COLOR_SCHEME.warning,
            "RECONNECTING row must use the warning background"
        );

        v.backend.stop().await.expect("cleanup stop");
    }

    /// UI-R-314 — `:start` schedules the connect task and returns immediately with its existing
    /// `(Info, …)` message, because the dial happens inside the spawned task, not on this call.
    /// A characterisation test over unchanged behaviour: it must pass first try, since `:start`
    /// carries no deferred outcome and is outside UI-R-315's stop-bearing scope.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ut_start_command_returns_without_waiting() {
        let guard = reserve_tcp_port();
        let port = guard.port();
        let _listener = guard.into_listener();

        let mut v = client_view::<ferrowl_ocpp::V1_6>(OcppVersion::V1_6, port);
        v.spec.timeout_ms = Some(60_000);

        let before = std::time::Instant::now();
        let result = v.handle_command("start").await;
        assert!(
            before.elapsed() < std::time::Duration::from_millis(50),
            "handle_command(\"start\") took {:?}, expected to return immediately",
            before.elapsed()
        );
        assert!(
            matches!(result, CommandResult::Handled(Some((Level::Info, _)))),
            "expected an immediate Info result"
        );

        v.backend.stop().await.expect("cleanup stop");
    }

    /// UI-R-314 — `:stop` against a client whose task is genuinely alive (dialling a peer that
    /// never completes the handshake) signals termination and returns without waiting for the
    /// task to end.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ut_stop_command_returns_without_waiting() {
        let guard = reserve_tcp_port();
        let port = guard.port();
        let _listener = guard.into_listener();

        let mut v = client_view::<ferrowl_ocpp::V1_6>(OcppVersion::V1_6, port);
        v.spec.timeout_ms = Some(60_000);
        let handler = v.make_handler();
        v.backend
            .start(&v.spec, &v.device, &v.log, handler)
            .await
            .expect("start must not fail synchronously");

        let before = std::time::Instant::now();
        let result = v.handle_command("stop").await;
        assert!(
            before.elapsed() < std::time::Duration::from_millis(50),
            "handle_command(\"stop\") took {:?}, expected to return immediately",
            before.elapsed()
        );
        assert!(matches!(result, CommandResult::Handled(None)));
        assert!(v.lifecycle_pending());

        for _ in 0..200 {
            if !v.lifecycle_pending() {
                break;
            }
            v.refresh().await;
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(!v.lifecycle_pending());
    }

    /// UI-R-315 — once `refresh()` settles a deferred stop, the outcome lands in the log as an
    /// `Info "Disconnected"` line rather than riding the command's own immediate result.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ut_refresh_logs_stop_outcome() {
        let guard = reserve_tcp_port();
        let port = guard.port();
        let _listener = guard.into_listener();

        let mut v = client_view::<ferrowl_ocpp::V1_6>(OcppVersion::V1_6, port);
        v.spec.timeout_ms = Some(60_000);
        let handler = v.make_handler();
        v.backend
            .start(&v.spec, &v.device, &v.log, handler)
            .await
            .expect("start must not fail synchronously");

        let result = v.handle_command("stop").await;
        assert!(matches!(result, CommandResult::Handled(None)));

        for _ in 0..200 {
            if !v.lifecycle_pending() {
                break;
            }
            v.refresh().await;
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(!v.lifecycle_pending());

        let lines = v
            .log
            .read()
            .await
            .peek_n(crate::app::LOG_SIZE)
            .into_iter()
            .map(|(_, level, l)| (level, l))
            .collect::<Vec<_>>();
        assert!(
            lines
                .iter()
                .any(|(level, l)| *level == Level::Info && l == "Disconnected"),
            "missing 'Disconnected' Info line: {lines:?}"
        );
    }

    /// CL-R-057 — once `refresh()` settles a deferred stop, the outcome is available through
    /// `take_stop_outcome` (not just the log), so a caller like headless teardown can read the
    /// stop's own result without scanning for an `Error`-level line that might belong to
    /// something else entirely.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ut_take_stop_outcome_reports_a_settled_clean_stop() {
        let guard = reserve_tcp_port();
        let port = guard.port();
        let _listener = guard.into_listener();

        let mut v = client_view::<ferrowl_ocpp::V1_6>(OcppVersion::V1_6, port);
        v.spec.timeout_ms = Some(60_000);
        let handler = v.make_handler();
        v.backend
            .start(&v.spec, &v.device, &v.log, handler)
            .await
            .expect("start must not fail synchronously");

        let result = v.handle_command("stop").await;
        assert!(matches!(result, CommandResult::Handled(None)));

        for _ in 0..200 {
            if !v.lifecycle_pending() {
                break;
            }
            v.refresh().await;
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(!v.lifecycle_pending());

        assert!(
            matches!(
                v.take_stop_outcome(),
                Some(crate::module::view::StopOutcome::Clean)
            ),
            "expected the settled stop's own outcome via take_stop_outcome"
        );
    }

    /// UI-R-314/UI-R-315 — a `:restart` issued while a `:stop` is still pending overwrites the
    /// follow-up in place rather than re-requesting `request_stop()` against a backend already
    /// `Stopping` (which would fall into the idle fallback and, for the client, call the full
    /// blocking `stop()` from `start()`'s own guard).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ut_restart_while_stop_pending_overwrites_the_follow_up() {
        let guard = reserve_tcp_port();
        let port = guard.port();
        let _listener = guard.into_listener();

        let mut v = client_view::<ferrowl_ocpp::V1_6>(OcppVersion::V1_6, port);
        v.spec.timeout_ms = Some(60_000);
        let handler = v.make_handler();
        v.backend
            .start(&v.spec, &v.device, &v.log, handler)
            .await
            .expect("start must not fail synchronously");

        assert!(matches!(
            v.handle_command("stop").await,
            CommandResult::Handled(None)
        ));
        assert!(v.lifecycle_pending());

        assert!(matches!(
            v.handle_command("restart").await,
            CommandResult::Handled(None)
        ));
        assert!(
            v.lifecycle_pending(),
            "the follow-up must still be pending, not dropped"
        );

        for _ in 0..200 {
            if !v.lifecycle_pending() {
                break;
            }
            v.refresh().await;
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(
            !v.lifecycle_pending(),
            "a stop overwritten with a restart must still settle, not latch forever"
        );

        v.backend.stop().await.expect("cleanup stop");
    }

    /// UI-R-350, UI-E-161 — applying a module configuration edit signals the stop and returns
    /// without waiting; while the stop is still pending, `self.spec` still holds the pre-edit URL,
    /// and only once the settle loop drains it does `self.spec` adopt the edited URL and the log
    /// carry `Settings updated`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ut_edit_apply_returns_without_blocking() {
        let guard = reserve_tcp_port();
        let port = guard.port();
        let _listener = guard.into_listener();

        let mut v = client_view::<ferrowl_ocpp::V1_6>(OcppVersion::V1_6, port);
        v.spec.timeout_ms = Some(60_000);
        let handler = v.make_handler();
        v.backend
            .start(&v.spec, &v.device, &v.log, handler)
            .await
            .expect("start must not fail synchronously");

        let pre_edit_port = v.spec.port;
        let mut edited = v.spec.clone();
        edited.port = pre_edit_port + 1;
        v.deferred.setup = Some((edited.clone(), String::new(), Vec::new()));

        let before = std::time::Instant::now();
        v.refresh().await;
        assert!(
            before.elapsed() < std::time::Duration::from_millis(50),
            "refresh() took {:?}, expected to return immediately",
            before.elapsed()
        );
        assert!(v.lifecycle_pending());
        assert_eq!(
            v.spec.port, pre_edit_port,
            "the pre-edit spec must still render until the deferred stop settles"
        );

        for _ in 0..200 {
            if !v.lifecycle_pending() {
                break;
            }
            v.refresh().await;
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(!v.lifecycle_pending());
        assert_eq!(
            v.spec.port, edited.port,
            "the edited spec must be adopted at settle"
        );

        let lines = v
            .log
            .read()
            .await
            .peek_n(crate::app::LOG_SIZE)
            .into_iter()
            .map(|(_, level, l)| (level, l))
            .collect::<Vec<_>>();
        assert!(
            lines
                .iter()
                .any(|(level, l)| *level == Level::Info && l == "Settings updated"),
            "missing 'Settings updated' Info line: {lines:?}"
        );

        v.backend.stop().await.expect("cleanup stop");
    }

    /// UI-R-350 — applying an edit while the module's connection attempt is still in flight (a
    /// held listener that TCP-accepts but never completes the handshake) abandons that attempt
    /// (OC-R-175) once the deferred stop settles, and the new configuration is adopted (`v.spec`
    /// matches the edited spec). The dial never reached `is_online()`, so per OC-R-085 the station
    /// stays idle rather than reconnecting; UI-E-160's "reconnects" outcome needs a station that
    /// was actually connected before the edit, which this fixture does not produce.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ut_edit_apply_while_connect_in_flight_adopts_spec_after_settle() {
        let guard = reserve_tcp_port();
        let port = guard.port();
        let _listener = guard.into_listener();

        let mut v = client_view::<ferrowl_ocpp::V1_6>(OcppVersion::V1_6, port);
        v.spec.timeout_ms = Some(60_000);
        let handler = v.make_handler();
        v.backend
            .start(&v.spec, &v.device, &v.log, handler)
            .await
            .expect("start must not fail synchronously");

        let mut edited = v.spec.clone();
        edited.timeout_ms = Some(1500);
        v.deferred.setup = Some((edited.clone(), String::new(), Vec::new()));
        v.refresh().await;
        assert!(v.lifecycle_pending());

        for _ in 0..200 {
            if !v.lifecycle_pending() {
                break;
            }
            v.refresh().await;
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(!v.lifecycle_pending());
        assert_eq!(
            v.spec.timeout_ms, edited.timeout_ms,
            "the edited spec must be adopted once the abandoned in-flight attempt settles"
        );
        use crate::view::status_bar::ConnStatus;
        assert_eq!(
            v.backend.connection_status(),
            ConnStatus::Disconnected,
            "OC-R-085: the abandoned dial never connected, so the settle must not restart it"
        );

        v.backend.stop().await.expect("cleanup stop");
    }

    /// UI-R-350 — a role switch (client → server) confirmed against a running station signals the
    /// stop and defers `take_replacement()` until that deferred stop settles.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ut_edit_apply_role_switch_defers_replacement_until_stop_settles() {
        let guard = reserve_tcp_port();
        let port = guard.port();
        let _listener = guard.into_listener();

        let mut v = client_view::<ferrowl_ocpp::V1_6>(OcppVersion::V1_6, port);
        v.spec.timeout_ms = Some(60_000);
        let handler = v.make_handler();
        v.backend
            .start(&v.spec, &v.device, &v.log, handler)
            .await
            .expect("start must not fail synchronously");

        let mut edited = v.spec.clone();
        edited.role = OcppRole::Server;
        v.deferred.setup = Some((edited, String::new(), Vec::new()));
        v.refresh().await;
        assert!(v.lifecycle_pending());
        assert!(
            v.take_replacement().is_none(),
            "the replacement must not be installed while the stop is still pending"
        );

        for _ in 0..200 {
            if !v.lifecycle_pending() {
                break;
            }
            v.refresh().await;
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(!v.lifecycle_pending());
        assert!(
            v.take_replacement().is_some(),
            "the replacement must be installed once the deferred stop settles"
        );
    }

    /// UI-R-350 — applying an edit against a stopped station lands within the same `refresh()`
    /// call, since `request_stop()` errors `NotRunning` and there is no in-flight task for
    /// `poll_stop()` to ever resolve.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ut_edit_apply_on_stopped_station_applies_immediately() {
        let guard = reserve_tcp_port();
        let port = guard.port();
        let _listener = guard.into_listener();

        let mut v = client_view::<ferrowl_ocpp::V1_6>(OcppVersion::V1_6, port);

        let mut edited = v.spec.clone();
        edited.port = port + 1;
        v.deferred.setup = Some((edited.clone(), String::new(), Vec::new()));
        v.refresh().await;

        assert!(
            !v.lifecycle_pending(),
            "nothing was running, so the apply must land inline, not defer"
        );
        assert_eq!(v.spec.port, edited.port);
    }

    /// UI-R-350 — an edit confirmed while a `:stop` is already pending overwrites the follow-up
    /// in place rather than re-requesting `request_stop()` against a backend already `Stopping`,
    /// same as `ut_restart_while_stop_pending_overwrites_the_follow_up`: only the apply's own
    /// outcome (`Settings updated`) lands at settle, not `Disconnected`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ut_edit_apply_while_stop_pending_overwrites_the_follow_up() {
        let guard = reserve_tcp_port();
        let port = guard.port();
        let _listener = guard.into_listener();

        let mut v = client_view::<ferrowl_ocpp::V1_6>(OcppVersion::V1_6, port);
        v.spec.timeout_ms = Some(60_000);
        let handler = v.make_handler();
        v.backend
            .start(&v.spec, &v.device, &v.log, handler)
            .await
            .expect("start must not fail synchronously");

        assert!(matches!(
            v.handle_command("stop").await,
            CommandResult::Handled(None)
        ));
        assert!(v.lifecycle_pending());

        let mut edited = v.spec.clone();
        edited.timeout_ms = Some(1500);
        v.deferred.setup = Some((edited.clone(), String::new(), Vec::new()));
        v.refresh().await;
        assert!(
            v.lifecycle_pending(),
            "the follow-up must still be pending, not dropped"
        );

        for _ in 0..200 {
            if !v.lifecycle_pending() {
                break;
            }
            v.refresh().await;
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(
            !v.lifecycle_pending(),
            "a stop overwritten with an apply must still settle, not latch forever"
        );
        assert_eq!(
            v.spec.timeout_ms, edited.timeout_ms,
            "the overwritten follow-up must be the apply, not the original stop"
        );

        let lines = v
            .log
            .read()
            .await
            .peek_n(crate::app::LOG_SIZE)
            .into_iter()
            .map(|(_, level, l)| (level, l))
            .collect::<Vec<_>>();
        assert!(
            lines
                .iter()
                .any(|(level, l)| *level == Level::Info && l == "Settings updated"),
            "missing 'Settings updated' Info line: {lines:?}"
        );
        assert!(
            !lines
                .iter()
                .any(|(level, l)| *level == Level::Info && l == "Disconnected"),
            "the overwritten stop's own outcome must not be logged: {lines:?}"
        );
    }

    /// CSMS handler answering every action used by these tests and recording the ordered list of
    /// received action names into `calls`, notifying `notify` once `StatusNotification` lands.
    struct RecordingCsms16 {
        calls: Arc<Mutex<Vec<String>>>,
        notify: Arc<Notify>,
    }

    impl CsmsActionHandler<ferrowl_ocpp::V1_6> for RecordingCsms16 {
        async fn handle_call(
            &self,
            _conn: csms::ConnectionId,
            action: ferrowl_ocpp::Action16,
        ) -> Result<ferrowl_ocpp::Response16, ferrowl_ocpp::CallError> {
            use ferrowl_ocpp::{Action16, Response16};
            let name = match &action {
                Action16::StartTransaction(_) => "StartTransaction",
                Action16::StatusNotification(_) => "StatusNotification",
                _ => "Other",
            };
            self.calls.lock().push(name.to_string());
            if name == "StatusNotification" {
                self.notify.notify_one();
            }
            match action {
                Action16::StartTransaction(_) => Ok(Response16::StartTransaction(
                    serde_json::from_value(serde_json::json!({
                        "idTagInfo": { "status": "Accepted" },
                        "transactionId": 42,
                    }))
                    .unwrap(),
                )),
                Action16::StatusNotification(_) => Ok(Response16::StatusNotification(
                    serde_json::from_value(serde_json::json!({})).unwrap(),
                )),
                _ => Err(ferrowl_ocpp::CallError::new(
                    ferrowl_ocpp::CallErrorCode::NotImplemented,
                    "unsupported",
                )),
            }
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    /// OC-R-122 — a RFID/operator-triggered `StartTransaction` (1.6) is followed by a coupled
    /// `StatusNotification` reflecting the post-start connector status, in that order.
    async fn ut_rfid_start_couples_status_notification() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let notify = Arc::new(Notify::new());
        let server = csms::ServerBuilder::<ferrowl_ocpp::V1_6>::new(
            csms::Config {
                host: "127.0.0.1".to_owned(),
                port: 0,
                timeout_ms: 2000,
                reconnect: true,
                basic_auth: None,
                tls: Default::default(),
            },
            ferrowl_ocpp::new_self_signed_cache(),
        )
        .spawn(
            RecordingCsms16 {
                calls: calls.clone(),
                notify: notify.clone(),
            },
            sink(),
        )
        .await
        .expect("server failed to bind");
        let addr = bound_addr(&server).await;

        let mut v = client_view::<ferrowl_ocpp::V1_6>(OcppVersion::V1_6, addr.port());
        let handler = v.make_handler();
        v.backend
            .start(&v.spec, &v.device, &v.log, handler)
            .await
            .expect("start");
        wait_for(|| v.backend.is_online()).await;

        let scope = Scope::connector(1);
        let payload = v.state_payload("StartTransaction", scope);
        v.deferred.send = Some(("StartTransaction".to_string(), payload, scope));
        ModuleView::refresh(&mut v).await;

        tokio::time::timeout(std::time::Duration::from_secs(5), notify.notified())
            .await
            .expect("StatusNotification never sent");
        assert_eq!(
            *calls.lock(),
            vec![
                "StartTransaction".to_string(),
                "StatusNotification".to_string()
            ],
        );
    }

    /// CSMS handler answering every 2.0.1 action used by these tests and recording the ordered
    /// list of received action names into `calls`, notifying `notify` once `StatusNotification`
    /// lands.
    struct RecordingCsms201 {
        calls: Arc<Mutex<Vec<String>>>,
        notify: Arc<Notify>,
    }

    impl CsmsActionHandler<ferrowl_ocpp::V2_0_1> for RecordingCsms201 {
        async fn handle_call(
            &self,
            _conn: csms::ConnectionId,
            action: ferrowl_ocpp::Action201,
        ) -> Result<ferrowl_ocpp::Response201, ferrowl_ocpp::CallError> {
            use ferrowl_ocpp::{Action201, Response201};
            let name = match &action {
                Action201::TransactionEvent(_) => "TransactionEvent",
                Action201::StatusNotification(_) => "StatusNotification",
                _ => "Other",
            };
            self.calls.lock().push(name.to_string());
            if name == "StatusNotification" {
                self.notify.notify_one();
            }
            match action {
                Action201::TransactionEvent(_) => Ok(Response201::TransactionEvent(
                    serde_json::from_value(serde_json::json!({})).unwrap(),
                )),
                Action201::StatusNotification(_) => Ok(Response201::StatusNotification(
                    serde_json::from_value(serde_json::json!({})).unwrap(),
                )),
                _ => Err(ferrowl_ocpp::CallError::new(
                    ferrowl_ocpp::CallErrorCode::NotImplemented,
                    "unsupported",
                )),
            }
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    /// OC-R-122 — a RFID/operator-triggered `TransactionEvent(Started)` (2.0.1) is followed by a
    /// coupled `StatusNotification` reflecting the post-start connector status, in that order.
    async fn ut_transaction_event_started_couples_status_notification() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let notify = Arc::new(Notify::new());
        let server = csms::ServerBuilder::<ferrowl_ocpp::V2_0_1>::new(
            csms::Config {
                host: "127.0.0.1".to_owned(),
                port: 0,
                timeout_ms: 2000,
                reconnect: true,
                basic_auth: None,
                tls: Default::default(),
            },
            ferrowl_ocpp::new_self_signed_cache(),
        )
        .spawn(
            RecordingCsms201 {
                calls: calls.clone(),
                notify: notify.clone(),
            },
            sink(),
        )
        .await
        .expect("server failed to bind");
        let addr = bound_addr(&server).await;

        let mut v = client_view::<ferrowl_ocpp::V2_0_1>(OcppVersion::V2_0_1, addr.port());
        let handler = v.make_handler();
        v.backend
            .start(&v.spec, &v.device, &v.log, handler)
            .await
            .expect("start");
        wait_for(|| v.backend.is_online()).await;

        let scope = Scope::evse(1, None);
        v.dispatch_lua_action(scope, "StartTransaction", serde_json::json!({}));
        ModuleView::refresh(&mut v).await;

        tokio::time::timeout(std::time::Duration::from_secs(5), notify.notified())
            .await
            .expect("StatusNotification never sent");
        assert_eq!(
            *calls.lock(),
            vec![
                "TransactionEvent".to_string(),
                "StatusNotification".to_string()
            ],
        );
    }
}
