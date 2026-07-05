//! Key handling: overlay routing and the content-pane key dispatch, plus the pane actions they
//! trigger (add/remove connector, open the edit/config/action overlays, apply an edit).

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::{EventResult, traits::HandleEvents, widgets::GetValue};

use crate::dialog::scripts::ScriptDialog;
use crate::module::ocpp::action_dialog::{ActionDialog, ActionResult, gen_tx_id, value_to_string};
use crate::module::ocpp::client::config::{ConfigEditDialog, ConfigKey};
use crate::module::ocpp::client::lua_sim::ClientFields;
use crate::module::ocpp::lock::{HasState, with_state};
use crate::module::ocpp::scope::Scope;

use super::{
    ClientOverlay, ClientState, ClientVersion, ClientView, ClientViewFocus, EditField, EditKind,
    EditOverlay, ResolvedEdit, conn_rows,
};

impl<V: ClientVersion> ClientView<V> {
    pub(super) fn handle_events_impl(
        &mut self,
        modifiers: KeyModifiers,
        code: KeyCode,
    ) -> EventResult {
        if self.overlay.is_active() {
            // Setup dialog: offer the key to the dialog before common routing, so a future
            // dialog-owned popup can consume Esc/Enter/Tab/BackTab while it is open.
            if let ClientOverlay::Setup(setup) = &mut self.overlay
                && let EventResult::Consumed = setup.handle_events(modifiers, code)
            {
                return EventResult::Consumed;
            }

            // Common keys first: `Esc` closes `esc_close` variants, `Tab`/`BackTab` cycle focus on
            // `focus_cycle` variants. Anything else falls through to per-variant `Enter`/inner keys.
            match self.overlay.route_keys(modifiers, code) {
                ferrowl_ui::traits::OverlayRoute::Closed
                | ferrowl_ui::traits::OverlayRoute::Cycled => {
                    return EventResult::Consumed;
                }
                ferrowl_ui::traits::OverlayRoute::Unhandled => {}
            }

            match &mut self.overlay {
                // Scripts editor: routes every key through its own handler; commit on done.
                ClientOverlay::Scripts(_) => {
                    let done = if let ClientOverlay::Scripts(dialog) = &mut self.overlay {
                        dialog.handle_events(modifiers, code)
                    } else {
                        false
                    };
                    if done {
                        let ClientOverlay::Scripts(dialog) = self.overlay.take() else {
                            unreachable!()
                        };
                        self.device.scripts = dialog.resolve();
                        self.start_sim();
                    }
                }

                // Setup dialog: `Esc`/`Tab` already routed; `Enter` resolves, other keys are
                // forwarded.
                ClientOverlay::Setup(_) => {
                    if let (KeyModifiers::NONE, KeyCode::Enter) = (modifiers, code) {
                        let resolved = if let ClientOverlay::Setup(setup) = &self.overlay {
                            setup.resolve().ok().map(|spec| (spec, setup.config_path()))
                        } else {
                            None
                        };
                        if let Some((spec, path)) = resolved {
                            self.deferred.setup = Some((spec, path));
                            self.overlay.close();
                        }
                    } else if let ClientOverlay::Setup(setup) = &mut self.overlay {
                        let _ = setup.handle_events(modifiers, code);
                    }
                }

                // Action dialog: routes every key through its own `input()`.
                ClientOverlay::Action(_) => {
                    let res = if let ClientOverlay::Action(dlg) = &mut self.overlay {
                        dlg.input(modifiers, code)
                    } else {
                        None
                    };
                    match res {
                        Some(ActionResult::Close) => self.overlay.close(),
                        Some(ActionResult::Send(payload)) => {
                            let name = match &self.overlay {
                                ClientOverlay::Action(dlg) => dlg.name.clone(),
                                _ => unreachable!(),
                            };
                            if V::decode_call(&name, payload.clone()).is_ok() {
                                let scope = self.selected_scope();
                                self.deferred.send = Some((name, payload, scope));
                                self.overlay.close();
                            }
                        }
                        None => {}
                    }
                }

                // State-row edit: `Esc` already routed; `Enter` applies, other keys hit the inner
                // widget.
                ClientOverlay::Edit(_) => {
                    if let (KeyModifiers::NONE, KeyCode::Enter) = (modifiers, code) {
                        self.apply_edit();
                    } else if let ClientOverlay::Edit(edit) = &mut self.overlay {
                        match &mut edit.kind {
                            EditKind::Choice(sel) => {
                                let _ = sel.state.handle_events(modifiers, code);
                            }
                            EditKind::Number(input) => {
                                let _ = input.state.handle_events(modifiers, code);
                            }
                            EditKind::Text(input) => {
                                let _ = input.state.handle_events(modifiers, code);
                            }
                        }
                    }
                }

                // Config-key editor: `Esc`/`Tab` already routed; `Enter` applies, other keys
                // forwarded.
                ClientOverlay::Config(_) => {
                    if let (KeyModifiers::NONE, KeyCode::Enter) = (modifiers, code) {
                        self.apply_config_edit();
                    } else if let ClientOverlay::Config(dialog) = &mut self.overlay {
                        dialog.handle_events(modifiers, code);
                    }
                }

                ClientOverlay::None => {}
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
            (KeyModifiers::NONE, KeyCode::Enter) => {
                match self.focus {
                    ClientViewFocus::ConnTable => self.sync_actions(),
                    ClientViewFocus::ConnInput => self.add_connector(),
                    ClientViewFocus::StateTable => self.open_edit(),
                    ClientViewFocus::ScriptsButton => self.open_scripts(),
                    ClientViewFocus::ConfigTable => self.open_config_edit(),
                    ClientViewFocus::KeyInput | ClientViewFocus::ValueInput => {
                        self.add_config_key()
                    }
                    ClientViewFocus::Actions => self.trigger_action(),
                    ClientViewFocus::MsgTable | ClientViewFocus::Code => {}
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Char('d'))
                if matches!(self.focus, ClientViewFocus::ConfigTable) =>
            {
                let selected = self.config_table.state.table_state().selected();
                let removed = self.with_state_mut(|s| match selected {
                    Some(i) if i < s.config().len() => {
                        s.config_mut().remove(i);
                        true
                    }
                    _ => false,
                });
                if removed {
                    self.config_table.state.move_up();
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Char('d'))
                if matches!(self.focus, ClientViewFocus::ConnTable) =>
            {
                self.remove_connector();
                EventResult::Consumed
            }
            // Space activates list/table panes, but must type into the text inputs.
            (KeyModifiers::NONE, KeyCode::Char(' '))
                if !matches!(
                    self.focus,
                    ClientViewFocus::KeyInput
                        | ClientViewFocus::ValueInput
                        | ClientViewFocus::ConnInput
                ) =>
            {
                match self.focus {
                    ClientViewFocus::StateTable => self.open_edit(),
                    ClientViewFocus::ScriptsButton => self.open_scripts(),
                    ClientViewFocus::ConfigTable => self.open_config_edit(),
                    ClientViewFocus::Actions => self.trigger_action(),
                    _ => {}
                }
                EventResult::Consumed
            }
            _ => match self.focus {
                ClientViewFocus::ConnTable => {
                    let r = self.conn_table.state.handle_events(modifiers, code);
                    self.sync_actions();
                    r
                }
                ClientViewFocus::ConnInput => self.conn_input.state.handle_events(modifiers, code),
                ClientViewFocus::StateTable => {
                    self.state_table.state.handle_events(modifiers, code)
                }
                ClientViewFocus::ScriptsButton => EventResult::Consumed,
                ClientViewFocus::ConfigTable => {
                    self.config_table.state.handle_events(modifiers, code)
                }
                ClientViewFocus::KeyInput => self.key_input.state.handle_events(modifiers, code),
                ClientViewFocus::ValueInput => {
                    self.value_input.state.handle_events(modifiers, code)
                }
                ClientViewFocus::Actions => self.actions.state.handle_events(modifiers, code),
                ClientViewFocus::MsgTable => {
                    let r = self.msg_table.state.handle_events(modifiers, code);
                    self.sync_code();
                    r
                }
                ClientViewFocus::Code => self.code.state.handle_events(modifiers, code),
            },
        }
    }

    pub(super) fn open_scripts(&mut self) {
        self.overlay = ClientOverlay::Scripts(Box::new(ScriptDialog::new(&self.device.scripts)));
    }

    /// Rebuild the action list for the selected level (CS vs connector), preserving the selection
    /// while the level is unchanged.
    pub(super) fn sync_actions(&mut self) {
        let want = !self.cs_selected();
        if self.actions_for_connector == Some(want) {
            return;
        }
        let names = if want {
            <V::Cs as ClientFields>::conn_actions()
        } else {
            <V::Cs as ClientFields>::cs_actions()
        };
        let values: Vec<String> = names.into_iter().map(|s| s.to_string()).collect();
        self.actions.state.set_values(values);
        self.actions_for_connector = Some(want);
    }

    /// Add a connector from the input field, then clear it and select the new row.
    fn add_connector(&mut self) {
        let raw = self.conn_input.state.input().trim().to_string();
        let id = self.with_state_mut(|s| V::add_connector(s, &raw));
        self.conn_input.state.set_input(String::new());
        self.conn_input.state.set_cursor(0);
        if let Some(id) = id {
            // Rebuild the (now-sorted) table and select the new connector's row (CS row = 0).
            let cp = self.spec.name.clone();
            let (rows, row) = self.with_state(|s| {
                let row = s.connector_position(id).map(|p| p + 1).unwrap_or(0);
                (conn_rows::<V>(&cp, s), row)
            });
            self.conn_table.state.set_values(rows);
            self.conn_table.state.select_index(row);
            self.sync_actions();
        }
    }

    /// Remove the selected connector (never the CS row, never the last connector).
    fn remove_connector(&mut self) {
        let Some(i) = self.conn_table.state.table_state().selected() else {
            return;
        };
        if i == 0 {
            return;
        }
        let removed = self.with_state_mut(|s| {
            if s.connector_count() <= 1 || i > s.connector_count() {
                return false;
            }
            s.remove_connector_at(i - 1);
            true
        });
        if !removed {
            return;
        }
        self.conn_table.state.move_up();
        self.sync_actions();
    }

    /// Enqueue the focused action for sending, or open a dialog when it needs more than state.
    fn trigger_action(&mut self) {
        let name = self.actions.state.get_value();
        if name.is_empty() {
            return;
        }
        let scope = self.selected_scope();
        match name.as_str() {
            "StartTransaction" if V::has_tx_shortcuts() => {
                let payload = self.start_event(scope);
                self.deferred.send = Some(("TransactionEvent".to_string(), payload, scope));
            }
            "StopTransaction" if V::has_tx_shortcuts() => {
                if let Some(payload) = self.stop_event(scope) {
                    self.deferred.send = Some(("TransactionEvent".to_string(), payload, scope));
                }
            }
            n if V::state_driven().contains(&n) => {
                let payload = self.state_payload(n, scope);
                self.deferred.send = Some((name, payload, scope));
            }
            _ => {
                self.overlay = ClientOverlay::Action(match V::action_spec(&name) {
                    Some(spec) => {
                        let state = self.state.clone();
                        let lookup = move |f: &str| {
                            with_state(&state, |s| {
                                // Resolve from the targeted connector first, then CS-level.
                                V::connector_index(s, scope)
                                    .and_then(|i| s.conn_get_field(i, f))
                                    .or_else(|| s.cs_get_field_named(f))
                                    .map(value_to_string)
                            })
                        };
                        Box::new(ActionDialog::new(name, &spec, lookup, gen_tx_id))
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
                        Box::new(ActionDialog::json_only(name, &template))
                    }
                });
            }
        }
    }

    /// Append a config key from the key/value inputs (readonly=false), then clear them.
    fn add_config_key(&mut self) {
        let key = self.key_input.state.input().trim().to_string();
        if key.is_empty() {
            return;
        }
        let value = self.value_input.state.input().trim().to_string();
        self.with_state_mut(|s| match s.config_mut().iter_mut().find(|c| c.key == key) {
            Some(c) => c.value = value,
            None => s.config_mut().push(ConfigKey {
                key,
                value,
                readonly: false,
            }),
        });
        self.key_input.state.set_input(String::new());
        self.key_input.state.set_cursor(0);
        self.value_input.state.set_input(String::new());
        self.value_input.state.set_cursor(0);
    }

    fn open_config_edit(&mut self) {
        let Some(row) = self.config_table.state.table_state().selected() else {
            return;
        };
        let dialog = self.with_state(|s| {
            s.config()
                .get(row)
                .map(|current| Box::new(ConfigEditDialog::new(row, current)))
        });
        if let Some(dialog) = dialog {
            self.overlay = ClientOverlay::Config(dialog);
        }
    }

    fn apply_config_edit(&mut self) {
        let ClientOverlay::Config(dialog) = self.overlay.take() else {
            return;
        };
        let Some(edited) = dialog.resolve() else {
            return;
        };
        self.with_state_mut(|s| {
            if let Some(slot) = s.config_mut().get_mut(dialog.index()) {
                *slot = edited;
            }
        });
    }

    fn open_edit(&mut self) {
        let Some(row) = self.state_table.state.table_state().selected() else {
            return;
        };
        let cs = self.cs_selected();
        let field = if cs {
            EditField::from_cs_row(row)
        } else {
            V::conn_edit_field(row)
        };
        let Some(field) = field else { return };
        let scope = if cs { Scope::CS } else { self.selected_scope() };
        let Some(kind) = self.with_state(|s| V::edit_kind(s, scope, cs, field)) else {
            return;
        };
        self.overlay = ClientOverlay::Edit(Box::new(EditOverlay { field, scope, kind }));
    }

    fn apply_edit(&mut self) {
        let ClientOverlay::Edit(edit) = self.overlay.take() else {
            return;
        };
        let resolved = match &edit.kind {
            EditKind::Choice(sel) => ResolvedEdit::Choice(sel.state.get_value()),
            EditKind::Number(input) => {
                let Ok(value) = input.state.input().trim().parse::<f64>() else {
                    return;
                };
                ResolvedEdit::Number(value)
            }
            EditKind::Text(input) => ResolvedEdit::Text(input.state.input().trim().to_string()),
        };
        self.with_state_mut(|s| V::apply_edit(s, &edit, resolved));
    }

    pub(super) fn sync_code(&mut self) {
        let selected = self.msg_table.state.table_state().selected();
        let content = selected
            .and_then(|i| self.visible_messages.get(i))
            .map(|m| serde_json::to_string_pretty(&m.payload).unwrap_or_default())
            .unwrap_or_default();
        if content != self.code_content {
            self.code.state.set_content(&content);
            self.code_content = content;
        }
    }
}
