//! Execution of `:` commands against the active tab: tab lifecycle and session persistence.
//! Module-specific commands are forwarded to the active view as raw strings.

use ferrowl_util::convert::{Converter, FileType};

use crate::config::Session;
use crate::module::view::CommandResult;

use super::App;

/// Pure validation: usable index or error text. Unit-testable without an App.
fn validate_copy_index(
    idx: Option<usize>,
    tab_count: usize,
    active: usize,
) -> Result<usize, String> {
    let idx = idx.ok_or_else(|| "usage: :script copy <tab-index>".to_string())?;
    if idx >= tab_count {
        // `tab_count == 0` can't happen in practice (App always has a tab), but guard against
        // the `tab_count - 1` underflow anyway.
        return Err(match tab_count.checked_sub(1) {
            Some(max) => format!("no tab [{idx}] (0..={max})"),
            None => format!("no tab [{idx}] (no tabs open)"),
        });
    }
    if idx == active {
        return Err("cannot copy from the active tab".to_string());
    }
    Ok(idx)
}

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
                self.rebuild_registry();
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
            Cmd::ScriptCopy(idx) => {
                let msg = self.copy_scripts(idx);
                self.log_active(msg).await;
            }
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
            scripts: vec![],
            interval: 1.0,
        };
        Converter::save(&session, path, ty).map_err(|e| format!("{e:?}"))
    }

    /// `:script copy <idx>` — replace the active tab's script list with tab `<idx>`'s.
    fn copy_scripts(&mut self, idx: Option<usize>) -> String {
        let src = match validate_copy_index(idx, self.tabs.len(), self.active) {
            Ok(i) => i,
            Err(e) => return e,
        };
        // Clone source list first; avoids a split borrow across tabs.
        let Some(scripts) = self.tabs[src].view.scripts().map(<[_]>::to_vec) else {
            return format!("tab [{src}] has no script support");
        };
        let n = scripts.len();
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return "active module has no script support".to_string();
        };
        if tab.view.set_scripts(scripts) {
            format!("Replaced scripts with {n} script(s) from tab [{src}]")
        } else {
            "active module has no script support".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_validate_copy_index() {
        assert_eq!(
            validate_copy_index(None, 3, 0),
            Err("usage: :script copy <tab-index>".to_string())
        );
        assert_eq!(
            validate_copy_index(Some(5), 3, 0),
            Err("no tab [5] (0..=2)".to_string())
        );
        assert_eq!(
            validate_copy_index(Some(1), 3, 1),
            Err("cannot copy from the active tab".to_string())
        );
        assert_eq!(validate_copy_index(Some(2), 3, 0), Ok(2));
    }
}
