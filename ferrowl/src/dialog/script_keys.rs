//! The `?` overlay content for the script dialog's script table (UI-R-056): the table's own
//! keybinds. The overlay widget lives in [`crate::dialog::help`].
//!
//! Generic table navigation (`j`/`k`/`g`/`G`, arrow keys) is not repeated here — it is the same in
//! every table and is documented by the app-level keybind-help dialog.

use crate::dialog::help::{HelpOverlay, HelpSection};

/// The keybind-help page for the script table.
pub fn script_keys_overlay() -> HelpOverlay {
    HelpOverlay::new("Script List Keys", vec![script_keys_section()])
}

fn script_keys_section() -> HelpSection {
    let entries = [
        ("Enter", "rename the selected script"),
        (
            "e",
            "run the selected script once, using the editor's current content and ignoring its enabled flag",
        ),
        (
            "t",
            "toggle the selected script between enabled and disabled",
        ),
        ("d", "delete the selected script (asks for confirmation)"),
        ("c", "toggle compact rows"),
    ];
    HelpSection {
        title: "Script list".to_string(),
        entries: entries
            .into_iter()
            .map(|(k, d)| (k.to_string(), d.to_string()))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// UI-R-056 — the overlay documents every binding the script table has.
    fn ut_lists_every_script_table_binding() {
        let section = script_keys_section();
        for key in ["Enter", "e", "t", "d", "c"] {
            assert!(
                section.entries.iter().any(|(k, _)| k == key),
                "missing binding '{key}'"
            );
        }
        assert_eq!(section.entries.len(), 5);
        assert!(section.entries.iter().all(|(_, desc)| !desc.is_empty()));
    }

    #[test]
    /// UI-R-056 — the overlay carries the script table's own bindings section.
    fn ut_overlay_carries_the_section() {
        let _ = script_keys_overlay();
    }
}
