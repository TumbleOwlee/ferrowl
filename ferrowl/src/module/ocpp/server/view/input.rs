//! Key handling: overlay routing and the content-pane key dispatch, plus the overlay-opening pane
//! actions they trigger (detail / scripts / delete-confirm / send dialog).

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::EventResult;
use ferrowl_ui::traits::{HandleEvents, OverlayRoute};
use ferrowl_ui::widgets::GetValue;

use crate::dialog::scripts::ScriptDialog;
use crate::module::modbus::dialog::ConfirmDeleteDialog;
use crate::module::ocpp::action_dialog::{ActionDialog, ActionResult, gen_tx_id};
use crate::module::ocpp::server::backend::{Scope, with_rfids_mut};
use crate::module::ocpp::server::detail::{DetailOverlay, DetailRequest};

use super::{ServerOverlay, ServerVersion, ServerView, ServerViewFocus};

impl<V: ServerVersion> ServerView<V>
where
    V::Action: Clone,
{
    pub(super) fn handle_events_impl(
        &mut self,
        modifiers: KeyModifiers,
        code: KeyCode,
    ) -> EventResult {
        if self.overlay.is_active() {
            // Setup dialog: offer the key to the dialog before common routing, so a future
            // dialog-owned popup can consume Esc/Enter/Tab/BackTab while it is open.
            if let ServerOverlay::Setup(setup) = &mut self.overlay
                && let EventResult::Consumed = setup.handle_events(modifiers, code)
            {
                return EventResult::Consumed;
            }

            // Common keys first: `Esc` closes `esc_close` variants, `Tab`/`BackTab` cycle focus on
            // `focus_cycle` variants. Anything else falls through to per-variant `Enter`/inner keys.
            match self.overlay.route_keys(modifiers, code) {
                OverlayRoute::Closed | OverlayRoute::Cycled => return EventResult::Consumed,
                OverlayRoute::Unhandled => {}
            }

            match &mut self.overlay {
                // Detail overlay: routes every key through its own `input()`.
                ServerOverlay::Detail(detail) => {
                    let req = detail.input(modifiers, code);
                    let identity = detail.identity.clone();
                    let scope = detail.scope;
                    match req {
                        Some(DetailRequest::Close) => {
                            // Keep the (possibly edited) config rows in memory so reopening keeps
                            // them while the CS stays in the list.
                            if let ServerOverlay::Detail(d) = self.overlay.take()
                                && d.is_cs
                            {
                                self.cs_configs.insert(d.identity.clone(), d.config_rows());
                            }
                        }
                        Some(DetailRequest::Fetch(key)) => {
                            if let Some(conn) = self.conn_for(&identity) {
                                self.send_to(
                                    conn,
                                    Scope::CS,
                                    V::config_action(),
                                    V::config_request(&key),
                                );
                            }
                        }
                        Some(DetailRequest::Set(key, value)) => {
                            if let Some(conn) = self.conn_for(&identity) {
                                self.send_to(
                                    conn,
                                    Scope::CS,
                                    V::set_action(),
                                    V::set_request(&key, &value),
                                );
                            }
                        }
                        // RFID edits mutate the shared store live (gating takes effect
                        // immediately); the device file is written on `:wd`. CS-scoped overlays
                        // edit the CS list.
                        Some(DetailRequest::AddRfid(tag)) => {
                            with_rfids_mut(&self.rfids, |s| s.add(scope, tag));
                        }
                        Some(DetailRequest::DelRfid(tag)) => {
                            with_rfids_mut(&self.rfids, |s| s.remove(scope, &tag));
                        }
                        None => {}
                    }
                }

                // Delete-confirmation dialog: `Esc`/`Tab` already routed; `Enter`/`Space` resolves.
                ServerOverlay::Confirm(_) => {
                    if let (KeyModifiers::NONE, KeyCode::Enter | KeyCode::Char(' ')) =
                        (modifiers, code)
                    {
                        let ServerOverlay::Confirm(confirm) = &self.overlay else {
                            unreachable!()
                        };
                        let confirmed = confirm.is_confirm_focused();
                        self.overlay.close();
                        if confirmed {
                            self.delete_selected();
                        }
                    } else if let ServerOverlay::Confirm(confirm) = &mut self.overlay {
                        let _ = confirm.handle_events(modifiers, code);
                    }
                }

                // Setup dialog: `Esc`/`Tab` already routed; `Enter` resolves, other keys forwarded.
                ServerOverlay::Setup(_) => {
                    if let (KeyModifiers::NONE, KeyCode::Enter) = (modifiers, code) {
                        let resolved = if let ServerOverlay::Setup(setup) = &self.overlay {
                            setup.resolve().ok().map(|spec| (spec, setup.config_path()))
                        } else {
                            None
                        };
                        if let Some((spec, path)) = resolved {
                            self.deferred.setup = Some((spec, path));
                            self.overlay.close();
                        }
                    } else if let ServerOverlay::Setup(setup) = &mut self.overlay {
                        let _ = setup.handle_events(modifiers, code);
                    }
                }

                // Scripts editor: routes every key through its own handler; commit on done.
                ServerOverlay::Scripts(_) => {
                    let done = if let ServerOverlay::Scripts(dialog) = &mut self.overlay {
                        dialog.handle_events(modifiers, code)
                    } else {
                        false
                    };
                    if done {
                        let ServerOverlay::Scripts(dialog) = self.overlay.take() else {
                            unreachable!()
                        };
                        self.device.scripts = dialog.resolve();
                        self.start_sim();
                    }
                }

                // Action dialog: routes every key through its own `input()`.
                ServerOverlay::Action(_) => {
                    let res = if let ServerOverlay::Action(boxed) = &mut self.overlay {
                        boxed.2.input(modifiers, code)
                    } else {
                        None
                    };
                    match res {
                        Some(ActionResult::Close) => self.overlay.close(),
                        Some(ActionResult::Send(payload)) => {
                            let (conn, scope, name) =
                                if let ServerOverlay::Action(boxed) = &self.overlay {
                                    (boxed.0, boxed.1, boxed.2.name.clone())
                                } else {
                                    unreachable!()
                                };
                            // Validate before sending; keep the dialog open on an invalid payload.
                            if V::decode_call(&name, payload.clone()).is_ok() {
                                self.overlay.close();
                                self.send_to(conn, scope, &name, payload);
                            }
                        }
                        None => {}
                    }
                }

                ServerOverlay::None => {}
            }

            return EventResult::Consumed;
        }
        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Tab) => {
                self.focus_next();
                EventResult::Consumed
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => {
                self.focus_previous();
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Enter) if self.focus == ServerViewFocus::CsTable => {
                self.open_detail();
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Enter)
                if self.focus == ServerViewFocus::ScriptsButton =>
            {
                self.open_scripts();
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Enter) if self.focus == ServerViewFocus::Actions => {
                self.trigger_action();
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Char('d')) if self.focus == ServerViewFocus::CsTable => {
                if let Some(idx) = self.selected() {
                    self.overlay = ServerOverlay::Confirm(Box::new(ConfirmDeleteDialog::new(
                        &self.entries[idx].identity,
                    )));
                }
                EventResult::Consumed
            }
            _ => match self.focus {
                ServerViewFocus::CsTable => self.cs_table.state.handle_events(modifiers, code),
                ServerViewFocus::Actions => self.actions.state.handle_events(modifiers, code),
                ServerViewFocus::MsgTable => {
                    let consumed = self.msg_table.state.handle_events(modifiers, code);
                    self.sync_code();
                    consumed
                }
                ServerViewFocus::ScriptsButton => EventResult::Unhandled(modifiers, code),
                ServerViewFocus::Code => self.code.state.handle_events(modifiers, code),
            },
        }
    }

    /// Open the detail overlay for the selected entry, seeding any persisted config rows.
    pub(super) fn open_detail(&mut self) {
        let Some(idx) = self.selected() else { return };
        let entry = &self.entries[idx];
        let identity = entry.identity.clone();
        let scope = entry.scope;
        let mut overlay = DetailOverlay::new(identity.clone(), scope, V::config_has_component());
        if !scope.is_connector()
            && let Some(rows) = self.cs_configs.get(&identity)
        {
            overlay.set_config(rows.clone());
        }
        self.overlay = ServerOverlay::Detail(Box::new(overlay));
    }

    fn open_scripts(&mut self) {
        self.overlay = ServerOverlay::Scripts(Box::new(ScriptDialog::new(&self.device.scripts)));
    }

    /// Trigger the focused action against the selected entry.
    fn trigger_action(&mut self) {
        let name = self.actions.state.get_value();
        if name.is_empty() {
            return;
        }
        let Some(idx) = self.selected() else { return };
        let Some(conn) = self.entries[idx].conn else {
            return;
        };
        let scope = self.entries[idx].scope;
        match self.entries[idx].derive_payload(&name) {
            Some(payload) => self.send_to(conn, scope, &name, payload),
            None => {
                // Open a per-action dialog from the spec, or a raw JSON editor if none yet.
                let dialog = match V::action_spec(&name) {
                    Some(spec) => {
                        let entry = &self.entries[idx];
                        ActionDialog::new(
                            name.clone(),
                            &spec,
                            |f| entry.get_field_str(f),
                            gen_tx_id,
                        )
                    }
                    None => {
                        debug_assert!(
                            V::json_actions().contains(&name.as_str()),
                            "{name} has no spec and is not a registered JSON action"
                        );
                        let template = V::json_template(&name)
                            .or_else(|| {
                                V::default_action(&name).and_then(|a| V::encode_action(&a).ok())
                            })
                            .map(|v| serde_json::to_string_pretty(&v).unwrap_or_default())
                            .unwrap_or_else(|| "{}".to_string());
                        ActionDialog::json_only(name.clone(), &template)
                    }
                };
                self.overlay = ServerOverlay::Action(Box::new((conn, scope, dialog)));
            }
        }
    }
}
