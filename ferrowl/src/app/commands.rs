//! Execution of `:` commands against the active tab: tab lifecycle and session persistence.
//! Module-specific commands are forwarded to the active view as raw strings.

use ferrowl_util::convert::{Converter, FileType};

use crate::config::Session;
use crate::module::view::CommandResult;

use super::App;

impl App {
    /// Execute a parsed `:` command. Returns `true` when the app should quit.
    pub(super) async fn run_command(&mut self, input: &str) -> bool {
        use crate::command::Cmd;
        match crate::command::parse(input) {
            Cmd::Empty => {}
            Cmd::Quit => {
                if self.tabs.len() <= 1 {
                    return true;
                }
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.view.handle_command("stop").await;
                }
                self.tabs.remove(self.active);
                self.active = self.active.min(self.tabs.len() - 1);
            }
            Cmd::QuitAll => return true,
            Cmd::New => self.enter_new(),
            Cmd::Load(path) => self.enter_load(path.as_deref()),
            Cmd::Write(path) => {
                let path = path.unwrap_or_else(|| "session.toml".to_string());
                match self.save_session(&path) {
                    Ok(()) => self.log_active(format!("Saved session to {path}")).await,
                    Err(e) => self.log_active(format!("Save failed: {e}")).await,
                }
            }
            Cmd::Log(file) => match file.as_deref() {
                Some("clear") => {
                    if let Some(tab) = self.tabs.get(self.active) {
                        tab.log.write().await.clear();
                    }
                }
                // Any other `:log ...` arg (e.g. a file path) is module-specific.
                _ => self.forward_to_view(input).await,
            },
            // Everything not recognised at the app level is forwarded to the active view.
            Cmd::Unknown(_) => {
                let result = if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.view.handle_command(input).await
                } else {
                    CommandResult::Unhandled
                };
                match result {
                    CommandResult::Handled(msg) => {
                        if let Some(m) = msg {
                            self.log_active(m).await;
                        }            
                        if let Some(tab) = self.tabs.get_mut(self.active) {
                            tab.log = tab.view.log();
                        }
                    }
                    CommandResult::Unhandled => {
                        self.log_active(format!("Unknown command ':{input}'")).await;
                    }
                }
            }
        }
        false
    }

    /// Forward a raw command string to the active view and log any returned message.
    async fn forward_to_view(&mut self, cmd: &str) {
        let result = if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.view.handle_command(cmd).await
        } else {
            CommandResult::Unhandled
        };
        if let CommandResult::Handled(Some(msg)) = result {
            self.log_active(msg).await;
        }
    }

    /// Save the current module instances as a session file.
    fn save_session(&self, path: &str) -> Result<(), String> {
        let ty = FileType::from_path(path)
            .ok_or_else(|| format!("unknown format for '{path}' (use .toml or .json)"))?;
        let modules: Vec<serde_json::Value> = self
            .tabs
            .iter()
            .filter_map(|t| t.view.session_spec())
            .collect();
        let session = Session {
            version: Some(crate::config::VERSION.to_string()),
            modules,
        };
        Converter::save(&session, path, ty).map_err(|e| format!("{e:?}"))
    }
}
