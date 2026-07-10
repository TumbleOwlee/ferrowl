//! Static content for the `?` keybind help dialog.

/// One titled group of keybinds shown in the help dialog.
pub(super) struct HelpSection {
    pub title: &'static str,
    pub keys: &'static [(&'static str, &'static str)],
}

/// App-wide keybinds, grouped by context. Module-specific keys are appended
/// from the active view's [`crate::module::view::ModuleView::keybinds`].
pub(super) static GLOBAL_SECTIONS: &[HelpSection] = &[
    HelpSection {
        title: "Global",
        keys: &[
            (":", "enter command mode"),
            ("C-w j/k", "switch table/log pane"),
            ("C-t l", "next tab"),
            ("C-t h", "previous tab"),
            ("C-t <digit[digit]>", "jump to tab by number"),
            ("?", "this help"),
        ],
    },
    HelpSection {
        title: "Command mode",
        keys: &[("Esc", "cancel"), ("Enter", "run command")],
    },
    HelpSection {
        title: "Dialogs",
        keys: &[
            ("Esc", "close (opens confirm)"),
            ("Enter", "confirm"),
            ("Tab / Shift+Tab", "next / previous field"),
        ],
    },
    HelpSection {
        title: "Code editor (vim)",
        keys: &[
            ("Esc", "Normal mode (again: close dialog via confirm)"),
            ("i a I A o O", "enter Insert mode"),
            ("h j k l, w b e, 0 $, gg G", "move"),
            ("v / V", "Visual / Visual-Line mode"),
            ("y / d", "yank / delete (line: yy dd, char: x)"),
            ("p / P", "paste after / before"),
            ("u", "undo last change"),
            ("Tab / Shift+Tab", "indent / dedent (Insert mode)"),
            ("?", "show Lua bindings (script dialogs, Normal mode)"),
        ],
    },
    HelpSection {
        title: "Tables",
        keys: &[
            ("j/k or Up/Down", "row up / down"),
            ("h/l or Left/Right", "column left / right"),
            ("g / G", "first / last row"),
            ("0/$ or Home/End", "first / last column"),
        ],
    },
];
