//! Shared "Add CA file" sub-dialog for the Modbus/OCPP shared CA list widget (MB-R-136/MB-R-156,
//! OC-R-113/OC-R-125). A minimal one-field sibling of `crate::module::modbus::dialog::
//! AddNamedValueDialog` (label+value, register-specific): this one just takes a non-empty path
//! and hands it back to the caller, which pushes it onto `TlsSection`'s `ca_files: Vec<String>`
//! — the "add" half of the add/remove list interaction both roles' CA row uses (delete is a
//! plain "remove the selected entry", handled by the caller directly against the `Vec` without
//! needing a dialog).

use super::NonEmpty;
use crate::dialog::path_suggest::FsPathProvider;
use derive_builder::Builder;
use ferrowl_ui::{
    Border, COLOR_SCHEME,
    state::{InputFieldStateBuilder, SuggestInputState, SuggestInputStateBuilder},
    style::{InputFieldStyle, TextStyle},
    widgets::{
        InputFieldBuilder, SuggestInput, SuggestInputBuilder, Text, TextBuilder, Validate,
        ValidateResult, Widget,
    },
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, HorizontalAlignment, Layout, Margin, Rect},
    widgets::{Block, StatefulWidget, Widget as UiWidget},
};
use std::fmt::Debug;
use std::path::{Path, PathBuf};

/// Extensions accepted for a client-CA file, shared between the path field's completion
/// suggestions and `AddCaFileDialog::validate`'s own check.
const CA_EXTENSIONS: &[&str] = &["pem", "crt", "key"];

#[derive(Builder, Clone, Debug)]
pub struct AddCaFileDialog {
    pub path: Widget<SuggestInputState<FsPathProvider>, SuggestInput<NonEmpty, FsPathProvider>>,
    pub error: Widget<String, Text>,
    pub keybinds: Widget<String, Text>,
    /// The directory a typed path is checked and stored against (NF-R-073): the owning module's
    /// device-config file directory, or `None` to fall back to the working directory (a module
    /// with no device file yet).
    #[builder(default)]
    pub base: Option<PathBuf>,
}

impl AddCaFileDialog {
    pub fn new(base: Option<PathBuf>) -> Self {
        let input_style = InputFieldStyle::default();
        let error_style = TextStyle {
            general: ratatui::prelude::Style::default()
                .fg(COLOR_SCHEME.error)
                .bg(COLOR_SCHEME.bg),
        };
        let text_style = TextStyle::default();

        AddCaFileDialogBuilder::default()
            .path(Widget {
                state: SuggestInputStateBuilder::default()
                    .field(
                        InputFieldStateBuilder::default()
                            .focused(true)
                            .disabled(false)
                            .placeholder(Some("ca.pem".to_string()))
                            .build()
                            .expect("all required builder fields are set"),
                    )
                    .provider(FsPathProvider::with_extensions(CA_EXTENSIONS))
                    .build()
                    .expect("all required builder fields are set"),
                widget: SuggestInputBuilder::default()
                    .input_field(
                        InputFieldBuilder::default()
                            .border(Border::Full(Margin::new(1, 0)))
                            .title(Some("CA File Path".into()))
                            .margin(Margin {
                                vertical: 0,
                                horizontal: 1,
                            })
                            .style(input_style)
                            .build()
                            .expect("all required builder fields are set"),
                    )
                    .build()
                    .expect("all required builder fields are set"),
            })
            .error(Widget {
                state: "".to_string(),
                widget: TextBuilder::default()
                    .title(Some("Error".into()))
                    .border(Border::Full(Margin::new(1, 0)))
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .style(error_style)
                    .build()
                    .expect("all required builder fields are set"),
            })
            .keybinds(Widget {
                state: "<Esc>: cancel | <Enter>: confirm".to_string(),
                widget: TextBuilder::default()
                    .margin(Margin {
                        vertical: 0,
                        horizontal: 1,
                    })
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .style(text_style)
                    .build()
                    .expect("all required builder fields are set"),
            })
            .base(base)
            .build()
            .expect("all required builder fields are set")
    }

    fn validate(&self) -> Result<(), String> {
        if let ValidateResult::Error(e) = NonEmpty::validate(self.path.state.input()) {
            return Err(format!("Path: {e}"));
        }
        let path = self.path.state.input().trim();
        let base = self.base.as_deref().unwrap_or_else(|| Path::new(""));
        let resolved = PathBuf::from(ferrowl_util::path::resolve_against(base, path));
        if !resolved.exists() {
            return Err(format!("Path: file not found: {path}"));
        }
        if resolved.is_dir() {
            return Err(format!("Path: is a directory, not a file: {path}"));
        }
        let has_valid_extension = resolved
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| {
                CA_EXTENSIONS
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(e))
            });
        if !has_valid_extension {
            return Err(format!(
                "Path: unsupported extension (expected one of {}): {path}",
                CA_EXTENSIONS.join(", ")
            ));
        }
        Ok(())
    }

    /// Validate and return the resolved path (NF-R-075), or the validation error.
    pub fn apply(&self) -> Result<String, String> {
        self.validate()?;
        let path = self.path.state.input().trim();
        let base = self.base.as_deref().unwrap_or_else(|| Path::new(""));
        Ok(ferrowl_util::path::resolve_against(base, path))
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        match self.validate() {
            Ok(_) => self.error.state.clear(),
            Err(e) => self.error.state = e,
        }

        let horizontal_layout: [Rect; 3] = Layout::horizontal([
            Constraint::Min(1),
            Constraint::Length(60),
            Constraint::Min(1),
        ])
        .areas(area);

        let error_height = if self.error.state.is_empty() { 0 } else { 3 };
        // 2 border + 2 margin-vertical + 3 path + error + 1 keybinds
        let total_height = 2 + 2 + 3 + error_height + 1;
        let vertical_layout: [Rect; 3] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(total_height),
            Constraint::Min(1),
        ])
        .areas(horizontal_layout[1]);

        let block = Block::bordered()
            .style(
                ratatui::prelude::Style::default()
                    .fg(COLOR_SCHEME.hi)
                    .bg(COLOR_SCHEME.bg),
            )
            .title_alignment(HorizontalAlignment::Center)
            .title("Add CA File");

        let inner = block.inner(vertical_layout[1]).inner(Margin::new(2, 1));
        ratatui::prelude::Widget::render(&ratatui::widgets::Clear, vertical_layout[1], buf);
        block.render(vertical_layout[1], buf);

        let inner_layout: [Rect; 3] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(error_height),
            Constraint::Length(1),
        ])
        .areas(inner);

        StatefulWidget::render(
            &self.path.widget,
            inner_layout[0],
            buf,
            &mut self.path.state,
        );
        if !self.error.state.is_empty() {
            StatefulWidget::render(
                &self.error.widget,
                inner_layout[1],
                buf,
                &mut self.error.state,
            );
        }
        StatefulWidget::render(
            &self.keybinds.widget,
            inner_layout[2],
            buf,
            &mut self.keybinds.state,
        );

        // Drawn last, over every sibling widget above, matching every other suggest-input field
        // in this codebase (e.g. the setup dialogs' own `cert_file`/`ca_file` popups).
        self.path
            .widget
            .render_overlay(area, buf, &mut self.path.state);
    }
}

impl Default for AddCaFileDialog {
    fn default() -> Self {
        Self::new(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};
    use ferrowl_test_support::{TempDirGuard, reserve_temp_dir};
    use ferrowl_ui::traits::{HandleEvents, SetFocus};

    fn type_into(state: &mut SuggestInputState<FsPathProvider>, s: &str) {
        state.set_focused(true);
        for c in s.chars() {
            state.handle_events(KeyModifiers::NONE, KeyCode::Char(c));
        }
    }

    fn tmp_file(dir: &TempDirGuard, name: &str) -> String {
        let path = dir.join(name);
        std::fs::write(&path, b"").unwrap();
        path.to_str().unwrap().to_string()
    }

    #[test]
    /// NF-R-054 — a valid `~/...` path validates the same way it will later load.
    fn ut_validate_tilde_path_that_exists_succeeds() {
        let home = std::env::home_dir().expect("HOME must resolve in test environment");
        let name = format!("ferrowl_ca_file_list_test_tilde_{}.pem", std::process::id());
        std::fs::write(home.join(&name), b"").unwrap();

        let mut d = AddCaFileDialog::new(None);
        type_into(&mut d.path.state, &format!("~/{name}"));
        let result = d.validate();

        let _ = std::fs::remove_file(home.join(&name));
        result.expect("a valid ~/-prefixed .pem path must validate");
    }

    /// MB-R-136, OC-R-149 — an empty path fails validation; a non-empty, existing one is trimmed
    /// and returned.
    #[test]
    fn ut_apply_requires_non_empty_path() {
        assert!(AddCaFileDialog::new(None).apply().is_err());
        let dir = reserve_temp_dir("ferrowl_ca_file_list");
        let ca = tmp_file(&dir, "nonempty.pem");
        let mut d = AddCaFileDialog::new(None);
        type_into(&mut d.path.state, &format!("  {ca}  "));
        // The input field itself doesn't trim as typed; `apply` trims on read.
        assert_eq!(d.apply().unwrap(), ca.trim());
    }

    /// MB-R-136, OC-R-149 — a path that doesn't exist on disk fails validation, so it can't be
    /// confirmed into the client-CA list.
    #[test]
    fn ut_apply_rejects_nonexistent_path() {
        let mut d = AddCaFileDialog::new(None);
        type_into(&mut d.path.state, "/nonexistent/ca-does-not-exist.pem");
        let err = d.apply().expect_err("nonexistent path must not apply");
        assert!(err.contains("not found"), "unexpected error: {err}");
    }

    /// MB-R-136, OC-R-149 — a directory can't be confirmed as a client-CA file, even though it
    /// exists on disk.
    #[test]
    fn ut_apply_rejects_directory() {
        let mut d = AddCaFileDialog::new(None);
        let dir = reserve_temp_dir("ferrowl_ca_file_list_dir");
        type_into(&mut d.path.state, &dir.path().to_string_lossy());
        let err = d.apply().expect_err("a directory must not apply");
        assert!(err.contains("directory"), "unexpected error: {err}");
    }

    /// MB-R-136, OC-R-149 — a file with an extension outside pem/crt/key is rejected, even though
    /// it exists on disk and isn't a directory.
    #[test]
    fn ut_apply_rejects_wrong_extension() {
        let mut d = AddCaFileDialog::new(None);
        let dir = reserve_temp_dir("ferrowl_ca_file_list");
        let bad = tmp_file(&dir, "ca.txt");
        type_into(&mut d.path.state, &bad);
        let err = d.apply().expect_err("a .txt file must not apply");
        assert!(err.contains("extension"), "unexpected error: {err}");
    }

    /// MB-R-136, OC-R-149 — the extension check is case-insensitive, matching `FsPathProvider`'s
    /// own completion-suggestion matching.
    #[test]
    fn ut_apply_accepts_uppercase_extension() {
        let mut d = AddCaFileDialog::new(None);
        let dir = reserve_temp_dir("ferrowl_ca_file_list");
        let ca = tmp_file(&dir, "uppercase.PEM");
        type_into(&mut d.path.state, &ca);
        assert_eq!(d.apply().unwrap(), ca);
    }

    /// MB-R-187, NF-R-073 — a relative path that exists under the dialog's `base` (the owning
    /// module's device-config directory) validates and is accepted.
    #[test]
    fn ut_ca_add_accepts_relative_path_under_device_dir() {
        let dir = reserve_temp_dir("ferrowl_ca_file_list_base");
        tmp_file(&dir, "ca.pem");
        let mut d = AddCaFileDialog::new(Some(dir.path().to_path_buf()));
        type_into(&mut d.path.state, "ca.pem");
        assert!(d.apply().is_ok());
    }

    /// MB-R-187, NF-R-073 — a bare filename that exists in the working directory (not under
    /// `base`) is rejected: the dialog checks against the device dir, not the CWD. `Cargo.toml`
    /// is real in the test binary's CWD, so a buggy implementation that fell back to checking
    /// the CWD instead of `base` would find it and fail on the extension check instead — a
    /// different error than the "not found" this test asserts.
    #[test]
    fn ut_ca_add_rejects_relative_path_absent_from_device_dir() {
        let dir = reserve_temp_dir("ferrowl_ca_file_list_base_absent");
        let mut d = AddCaFileDialog::new(Some(dir.path().to_path_buf()));
        type_into(&mut d.path.state, "Cargo.toml");
        let err = d.apply().expect_err("path absent from base must not apply");
        assert!(err.contains("not found"), "unexpected error: {err}");
    }

    /// NF-R-073 — `base: None` falls back to checking the working directory, the process's own
    /// CWD. A read-only probe: `Cargo.toml` is real in the test binary's CWD, so finding it (and
    /// failing only on its extension, not "not found") proves the check ran against the CWD.
    /// `type_into`/`apply` read `std::env::current_dir()` via `expand`, a second live read
    /// another test in this binary can mutate between this test's own read and that one;
    /// sandwiching the call between two reads and only asserting once they agree narrows the
    /// window a mutation has to land in (see `app::commands::
    /// ut_write_default_target_base_is_process_cwd`'s doc comment for the accepted residual gap).
    #[test]
    fn ut_ca_add_with_no_base_checks_cwd() {
        for _ in 0..50 {
            let before = std::env::current_dir().unwrap();
            let mut d = AddCaFileDialog::new(None);
            type_into(&mut d.path.state, "Cargo.toml");
            let result = d.apply();
            let after = std::env::current_dir().unwrap();
            if before != after {
                continue;
            }
            let err = result.expect_err("Cargo.toml has no CA extension");
            assert!(err.contains("extension"), "unexpected error: {err}");
            return;
        }
        panic!("process cwd never stabilized long enough to observe the CWD fallback");
    }

    /// NF-R-075 — the confirmed entry is the resolved path (base-joined), not the bare filename
    /// the user typed.
    #[test]
    fn ut_ca_add_stores_resolved_path() {
        let dir = reserve_temp_dir("ferrowl_ca_file_list_resolved");
        tmp_file(&dir, "ca.pem");
        let mut d = AddCaFileDialog::new(Some(dir.path().to_path_buf()));
        type_into(&mut d.path.state, "ca.pem");
        let resolved = d.apply().unwrap();
        assert_eq!(resolved, dir.join("ca.pem").to_str().unwrap());
    }

    /// MB-R-136, OC-R-149 — the add-CA sub-dialog renders its title and an inline error while
    /// empty.
    #[test]
    fn ut_render_shows_title_and_inline_error() {
        let mut d = AddCaFileDialog::new(None);
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        d.render(area, &mut buf);
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("Add CA File"));
        assert!(text.contains("Non-empty input required") || text.contains("Path:"));
    }
}
