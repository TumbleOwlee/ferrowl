//! Simulation + backend glue: the Lua sim lifecycle, the per-tick `refresh` (deferred send/setup,
//! Lua-queued actions, auto-Heartbeat/MeterValues, message log sync), `:` command execution, and
//! payload send/dispatch.

use crate::app::Level;
use crate::module::ocpp::client::backend::{DEFAULT_HEARTBEAT_SECS, TICKS_PER_SEC};
use crate::module::ocpp::client::build_client_view;
use crate::module::ocpp::client::lua_sim::{merge_overrides, run_client_sim};
use crate::module::ocpp::config::device::{ConfigKeyDef, OcppDeviceConfig};
use crate::module::ocpp::config::session::OcppRole;
use crate::module::ocpp::lock::{HasState, with_state, with_state_mut};
use crate::module::ocpp::scope::Scope;
use crate::module::ocpp::server::build_server_view;
use crate::module::view::{CommandFuture, CommandResult, RefreshFuture};

use super::{ClientState, ClientVersion, ClientView, config_rows, conn_rows, msg_row, nv_rows};

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
        )
    }

    /// Write the device config (reconciled with the live spec, scripts + connectors preserved).
    fn save_device_to(&self, path: &str) -> CommandResult {
        use ferrowl_util::convert::{Converter, FileType};
        let Some(ty) = FileType::from_path(path) else {
            return CommandResult::Handled(Some(format!(
                "unknown format for '{path}' (use .toml or .json)"
            )));
        };
        let mut device = OcppDeviceConfig::from_spec(&self.spec, self.device.scripts.clone());
        device.version = Some(crate::config::VERSION.to_string());
        device.log_file = self.device.log_file.clone();
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
        match Converter::save(&device, path, ty) {
            Ok(()) => CommandResult::Handled(Some(format!("Saved device config to {path}"))),
            Err(e) => CommandResult::Handled(Some(format!("Save failed: {e:?}"))),
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
        tokio::spawn(async move {
            match V::decode_call(&name, payload) {
                Ok(action) => match sender.send_scoped(action, scope).await {
                    Ok(response) => {
                        with_state_mut(&state, |s| {
                            V::apply_post_send(s, &name, scope, started_tx.as_deref(), &response);
                        });
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
            if let Some((spec, path)) = self.deferred.setup.take() {
                let mut device = OcppDeviceConfig::from_spec(&spec, self.device.scripts.clone());
                device.log_file = self.device.log_file.clone();
                device.connectors = self.device.connectors.clone();
                device.config = self.device.config.clone();
                if spec.role == OcppRole::Server {
                    let _ = self.backend.stop().await;
                    self.deferred.replacement = Some(build_server_view(spec, path, device));
                    return;
                }
                if spec.version != self.spec.version {
                    let _ = self.backend.stop().await;
                    if !device.scripts.is_empty() {
                        self.log.write().await.write(
                            Level::Warning,
                            "Version switched: scripts kept but may call actions the new version lacks",
                        );
                    }
                    self.deferred.replacement = Some(build_client_view(spec, path, device));
                    return;
                } else {
                    let was_online = self.backend.is_online();
                    let _ = self.backend.stop().await;
                    self.spec = spec;
                    self.device = device;
                    self.device_path = path;
                    self.log
                        .write()
                        .await
                        .write(Level::Info, "Settings updated");
                    if was_online {
                        let handler = self.make_handler();
                        let _ = self.backend.start(&self.spec, handler).await;
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
                    .with_state(|s| s.heartbeat_interval_secs())
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
                self.runtime.applied_log_file = self.device.log_file.clone();
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
        match cmd.trim() {
            "start" => Box::pin(async move {
                let handler = self.make_handler();
                match self.backend.start(&self.spec, handler).await {
                    Ok(()) => {
                        CommandResult::Handled(Some(format!("Connecting to {}", self.spec.url())))
                    }
                    Err(e) => CommandResult::Handled(Some(format!("Connect failed: {e}"))),
                }
            }),
            "stop" => Box::pin(async move {
                match self.backend.stop().await {
                    Ok(()) => CommandResult::Handled(Some("Disconnected".into())),
                    Err(e) => CommandResult::Handled(Some(format!("Disconnect failed: {e}"))),
                }
            }),
            "restart" => Box::pin(async move {
                let _ = self.backend.stop().await;
                let handler = self.make_handler();
                match self.backend.start(&self.spec, handler).await {
                    Ok(()) => CommandResult::Handled(Some("Reconnecting".into())),
                    Err(e) => CommandResult::Handled(Some(format!("Reconnect failed: {e}"))),
                }
            }),
            "edit" | "e" => {
                self.overlay = super::ClientOverlay::Setup(Box::new(
                    crate::module::ocpp::setup_dialog::OcppSetupDialog::edit(
                        &self.spec,
                        &self.device_path,
                    ),
                ));
                Box::pin(std::future::ready(CommandResult::Handled(None)))
            }
            "compact" => {
                self.set_compact(!self.compact);
                Box::pin(std::future::ready(CommandResult::Handled(None)))
            }
            "wd" => {
                let result = if self.device_path.is_empty() {
                    CommandResult::Handled(Some("No configuration file path configured.".into()))
                } else {
                    self.save_device_to(&self.device_path.clone())
                };
                Box::pin(std::future::ready(result))
            }
            cmd if cmd.starts_with("wd ") => {
                let path = cmd["wd ".len()..].trim().to_string();
                let result = self.save_device_to(&path);
                Box::pin(std::future::ready(result))
            }
            "log" => {
                self.device.log_file = None;
                Box::pin(std::future::ready(CommandResult::Handled(Some(
                    "File logging disabled".into(),
                ))))
            }
            cmd if cmd.starts_with("log ") => {
                let path = cmd["log ".len()..].trim().to_string();
                let msg = if path.is_empty() {
                    self.device.log_file = None;
                    "File logging disabled".to_string()
                } else {
                    self.device.log_file = Some(path.clone());
                    format!("Logging to {path}")
                };
                Box::pin(std::future::ready(CommandResult::Handled(Some(msg))))
            }
            _ => Box::pin(std::future::ready(CommandResult::Unhandled)),
        }
    }
}
