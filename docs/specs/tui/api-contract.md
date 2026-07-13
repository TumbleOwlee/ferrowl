# TUI — API Contract

The stable operator-facing surface owned by the TUI area: the exhaustive `:`
command list, every keybinding table by context/mode, and the code-editor
mode/command set. These names, aliases, argument shapes, and key mappings are the
contract operators rely on and shall not change without a spec change.

**Generic vs protocol-specific.** Commands are split into two classes:

- **Generic (app-level)** commands are parsed and executed by the application
  itself (§1). Their behavior is owned entirely by this area.
- **Module (protocol-specific)** commands are forwarded verbatim to the active
  view (§2). This document lists their name and general syntax; the effect on
  protocol state is specified in the owning protocol area (`modbus/`, `ocpp/`).
  Where a cell says "→ modbus" / "→ ocpp", read that area for semantics.

The application dispatches a command by matching its first token against the
generic set; a first token not in that set is forwarded to the active view. So a
generic name always wins over a same-named module command.

---

## 1. Generic `:` commands (app-level)

| Command | Aliases | Arguments | Effect (owned here) |
|---|---|---|---|
| `:quit` | `:q`, `:q!` | — | Stop and close the active tab; quit the app if it was the last tab |
| `:qall` | `:qa`, `:qa!` | — | Quit the whole app immediately, all tabs |
| `:new` | `:n` | — | Open the new-module type selector |
| `:load [path]` | `:l` | optional device-config path | Open the Modbus create dialog, pre-filling the config-path field with `path` |
| `:write [path]` | `:w`, `:s`, `:save` | optional output path (default `session.toml`) | Save all module instances plus session scripts/interval as a session file; format inferred from the path extension (`.toml`/`.json`) |
| `:swap <i> <j>` | — | two tab indices | Swap the positions of tabs `i` and `j` (no-op if equal or out of range); a non-numeric argument is rejected |
| `:session` | — | — | Open the session-level Lua scripts + sim-interval dialog |
| `:script copy <idx>` | — | source tab index | Replace the active tab's Lua script list with tab `idx`'s; errors if `idx` is missing, out of range, equals the active tab, or either tab lacks script support |
| `:log clear` | — | the literal `clear` | Clear the active tab's on-screen log ring |

Notes:

- `:log` with **any argument other than `clear`** (including a file path), and
  **bare `:log`**, are *not* generic — they are forwarded to the active view as a
  module command (§2), where `:log <file>` sets/clears the module's file sink.
- Bare `:script` (without `copy`) is *not* generic — it is forwarded to the active
  view; the Modbus view opens its script dialog on it.
- Any unrecognized first token is forwarded to the active view; if the view also
  does not recognize it, the app logs `Unknown command ':<input>'` (Warning).

## 2. Module (protocol-specific) `:` commands

Forwarded to the active view. Listed here for completeness; **semantics are owned
by the protocol area**. Each view advertises exactly its own list in the
command-help popup.

### 2.1 Modbus module

| Command | Arguments | Purpose (→ modbus) |
|---|---|---|
| `:edit` / `:e` | — | Open the module setup dialog |
| `:add` / `:a` | — | Open the add-register dialog |
| `:start` | — | Start the module (connect/bind) |
| `:stop` | — | Stop the module |
| `:restart` | — | Stop then start |
| `:reload` | — | Reload the device config from disk and restart the module |
| `:compact` | — | Toggle compact table rows |
| `:set <register> <value>` | register name, value (name may be `"quoted"` to allow spaces) | Write a value into a register → **modbus** owns type/range/codec semantics and the store-vs-wire behavior |
| `:write-device` / `:wd` `[path]` | optional path (default: configured device path) | Save the device config file |
| `:log <file>` | file base path | Set the module's log-file sink base |
| `:script` | — | Open the Lua script manager dialog |
| `:order [col] [asc\|desc]` | optional column, optional direction (default `asc`) | Sort the register table by column; bare `:order` clears the sort |

### 2.2 OCPP client module

| Command | Arguments | Purpose (→ ocpp) |
|---|---|---|
| `:edit` / `:e` | — | Open the module setup dialog |
| `:start` | — | Connect to the CSMS |
| `:stop` | — | Disconnect |
| `:restart` | — | Reconnect |
| `:compact` | — | Toggle compact rows |
| `:write-device` / `:wd` `[path]` | optional path | Save the device config |
| `:log [file]` | optional file path | Set the file sink; bare `:log` or empty path disables file logging |

### 2.3 OCPP server (CSMS) module

| Command | Arguments | Purpose (→ ocpp) |
|---|---|---|
| `:start` | — | Bind the CSMS listener |
| `:stop` | — | Unbind the listener (clears connected-station entries) |
| `:restart` | — | Rebind (clears entries) |
| `:edit` / `:e` | — | Open the module setup dialog |
| `:write-device` / `:wd` `[path]` | optional path | Save the device config |
| `:compact` | — | Toggle compact rows |
| `:log [file]` | optional file path | Set the file sink; bare `:log` or empty path disables file logging |
| `:rfid [add\|del <tag> \| clear]` | subcommand + tag | Manage the CSMS RFID accept-list; bare `:rfid` prints the current list |

**OCPP action send is not a `:` command.** Composing and sending an OCPP action
(request) is done through the action dialog opened by `Enter` on the message
table / action control, not through the command line. The action set and payload
semantics are owned by `ocpp/`.

## 3. Global keybindings

| Key | Context | Action |
|---|---|---|
| `:` | content focused, no view overlay open | Enter command mode |
| `?` | content focused, no view overlay open | Open the keybind-help dialog |
| `Ctrl+w` then `j`/`k`/`Down`/`Up` | content focused | Toggle focus between the tab's content view and log pane |
| `Ctrl+t` then `l` | content focused | Next tab (wraps) |
| `Ctrl+t` then `h` | content focused | Previous tab (wraps) |
| `Ctrl+t` then digit(s) | content focused | Jump to tab by index; waits up to 800 ms for a 2nd digit if one could form a valid 2-digit index (see requirements UI-R-011) |

`:` and `?` are suppressed while the active view has an overlay open, so they type
into that overlay instead of triggering the global action.

## 4. Context keybinding tables

### 4.1 Command mode

| Key | Action |
|---|---|
| `Esc` | Cancel (discard buffer) |
| `Enter` | Run the command |
| printable / `Left`/`Right` / `Home`/`End` / `Backspace`/`Delete` | Edit the command buffer |

### 4.2 Dialogs (generic)

| Key | Action |
|---|---|
| `Tab` | Next field (skips disabled fields) |
| `Shift+Tab` / `BackTab` | Previous field |
| `Enter` | Confirm the dialog |
| `Esc` | Request close (opens the close-confirm popup if edits may be lost) |

Applied only when the focused widget did not itself consume the key.

### 4.3 Close-confirm / yes-no popup

| Key | Action |
|---|---|
| `Enter` / `Space` | Confirm (close / delete) |
| `Esc` | Dismiss (back to editing) |
| `Tab` / `Shift+Tab` | Move between the confirm/cancel choices (delete confirm) |

Focus defaults to the safe (cancel) choice; confirming requires that choice to be
focused.

### 4.4 Tables and selection lists (when focused)

| Key | Action |
|---|---|
| `j` / `Down` | Row down |
| `k` / `Up` | Row up |
| `h` / `Left` | Column left (or previous item) |
| `l` / `Right` | Column right (or next item) |
| `g` | First row |
| `G` | Last row |
| `0` / `Home` | First column |
| `$` / `End` | Last column |

Selection clamps at the ends (tables do not wrap).

### 4.5 Suggestion-completion popup (open)

| Key | Action |
|---|---|
| `Up` / `Down` | Move highlighted suggestion |
| `Enter` / `Tab` | Accept highlighted suggestion (partial → keep open and re-query; else close) |
| `Esc` | Dismiss the popup |

### 4.6 Single-line text input (focused)

| Key | Action |
|---|---|
| printable (with `Shift`) | Insert character (rejected chars still consumed) |
| `Left` / `Right` | Move cursor |
| `Home` / `End` | Line start / end |
| `Backspace` / `Delete` | Delete before / at cursor |
| `Ctrl+F` | Autofill from placeholder (only when empty) |
| `Ctrl+D` | Clear the field |

### 4.7 Keybind-help dialog (`?`)

| Key | Action |
|---|---|
| `Esc` / `q` / `?` | Close |
| `j` / `Down` | Scroll down |
| `k` / `Up` | Scroll up |
| `g` | Top |
| `G` | Bottom |

### 4.8 Lua-bindings help overlay (`?` in a script editor, Normal mode)

Same navigation as §4.7 (`Esc`/`q`/`?` close, `j`/`k`/Up/Down scroll, `g`/`G`
top/bottom). It is only reachable from the code editor while in Normal mode; in
Insert/Visual mode `?` is literal text.

### 4.9 Script-manager dialog

| Key | Context | Action |
|---|---|---|
| `Tab` / `Shift+Tab` | dialog | Cycle focus (script table → name input → code editor → interval → log); the code editor is skipped while no script is selected |
| `Esc` | dialog | Open close-confirm |
| `t` | script table focused | Toggle the selected script's enabled flag |
| `d` | script table focused | Delete the selected script (opens confirm) |
| `c` | script table focused | Toggle compact rows |
| `Enter` | name input focused | Create a new script with the typed name |
| `?` | code editor, Normal mode | Open the Lua-bindings help overlay |

## 5. Code editor — modes and commands

The vim-modal editor (default for the Lua-script editor). Modes: `NORMAL`,
`INSERT`, `VISUAL` (charwise), `V-LINE` (linewise).

### 5.1 Mode transitions

| Key | From | Action |
|---|---|---|
| `i` | Normal | Insert at cursor |
| `a` | Normal | Insert after cursor |
| `I` | Normal | Insert at first non-blank |
| `A` | Normal | Insert at end of line |
| `o` | Normal | Open (auto-indented) line below, Insert |
| `O` | Normal | Open line above (copying indent), Insert |
| `v` | Normal | Charwise Visual |
| `V` | Normal | Linewise Visual |
| `Esc` | Insert / Visual | Back to Normal (Insert also steps cursor back one, vim-style) |
| `Esc` | Normal | Left unhandled → reaches dialog (opens close-confirm) |
| `v` | Visual | Back to Normal |
| `V` | Visual (charwise) | Switch to linewise Visual |

### 5.2 Motions (Normal and Visual)

| Key | Motion |
|---|---|
| `h` / `l` | Left / right one char (no line wrap) |
| `j` / `k` | Down / up one line |
| `Left`/`Right`/`Up`/`Down` | Arrow-key move (wraps to adjacent line) |
| `0` | First column |
| `$` | Last column |
| `w` | Start of next word (crosses lines; punctuation runs are their own word) |
| `b` | Start of previous word |
| `e` | End of current/next word |
| `gg` | First line, first column |
| `G` | Last line |

### 5.3 Edits (Normal)

| Key | Action |
|---|---|
| `x` | Delete char under cursor (to register, charwise) |
| `dd` | Delete current line (to register, linewise) |
| `yy` | Yank current line (to register, linewise) |
| `p` | Paste register after cursor / below line |
| `P` | Paste register before cursor / above line |
| `u` | Undo last change (press again to redo) |

### 5.4 Edits (Visual)

| Key | Action |
|---|---|
| `y` | Yank selection, return to Normal at selection start |
| `d` / `x` | Delete selection, return to Normal |

### 5.5 Insert-mode keys

| Key | Action |
|---|---|
| printable / `Enter` / `Backspace` / `Delete` / arrows | Standard editing (auto-indent on `Enter` when a language is set) |
| `Tab` | Insert four spaces |
| `Shift+Tab` / `BackTab` | Remove up to four leading spaces |

### 5.6 Plain (non-vim) editor

Printable keys insert, `Enter` splits with auto-indent (when a language is set),
`Backspace`/`Delete` edit, arrows navigate with line wrap, `Home`/`End` and
character-based editing as in §4.6. Two space presses at the same position within
~300 ms expand to a four-space indent (an intervening key cancels it).

Yank/delete additionally copy to the system clipboard via OSC 52. A `language`
setting drives syntax highlighting and format-on-blur (JSON may decline to
reformat invalid input; Lua always reformats).
