# TUI — API Contract

Operator-facing surface: exhaustive `:` command list, every keybinding table by context/mode, code-editor mode/command set. Names, aliases, argument shapes, key mappings are contract and shall not change without a spec change.

**Generic vs protocol-specific.** **Generic (app-level)** commands are parsed and executed by the application (§1); behavior owned here. **Module (protocol-specific)** commands are forwarded verbatim to the active view (§2); this document lists name and syntax, the effect on protocol state is the owning area (`modbus/`, `ocpp/`) — "→ modbus" / "→ ocpp" points there.

Dispatch: first token matched against the generic set; not in the set → forwarded to the active view. A generic name always wins over a same-named module command.

---

## 1. Generic `:` commands (app-level)

| Command | Aliases | Arguments | Effect (owned here) |
|---|---|---|---|
| `:quit` | `:q`, `:q!` | — | Stop and close the active tab; quit the app if it was the last |
| `:qall` | `:qa`, `:qa!` | — | Quit the whole app immediately |
| `:new` | `:n` | — | Open the new-module type selector |
| `:load [path]` | `:l` | optional device-config path | Open the Modbus create dialog, config-path field pre-filled with `path` |
| `:write [path]` | `:w`, `:s`, `:save` | optional output path (default `session.toml`) | Save all module instances plus session scripts/interval as a session file; format from extension (`.toml`/`.json`) |
| `:swap <i> <j>` | — | two tab indices | Swap tabs `i` and `j` (no-op if equal or out of range); non-numeric rejected |
| `:session` | — | — | Open the session-level Lua scripts + sim-interval dialog |
| `:script copy <idx>` | — | source tab index | Replace the active tab's script list with tab `idx`'s; errors if `idx` missing, out of range, equals the active tab, or either tab lacks script support |
| `:log clear` | — | the literal `clear` | Clear the active tab's on-screen log ring |

- `:log` with **any argument other than `clear`** (including a path), and **bare `:log`**, are *not* generic — forwarded to the active view (§2), where `:log <file>` sets/clears the module's file sink.
- Bare `:script` (without `copy`) is *not* generic — forwarded; the Modbus view opens its script dialog.
- Any unrecognized first token is forwarded; if the view also does not recognize it, app logs `Unknown command ':<input>'` (Warning).

## 2. Module (protocol-specific) `:` commands

Forwarded to the active view; **semantics owned by the protocol area**. Each view advertises exactly its own list in the command-help popup.

### 2.1 Modbus module

| Command | Arguments | Purpose (→ modbus) |
|---|---|---|
| `:edit` / `:e` | — | Open the module setup dialog |
| `:add` / `:a` | — | Open the add-register dialog |
| `:start` | — | Start the module (connect/bind) |
| `:stop` | — | Stop the module |
| `:restart` | — | Stop then start |
| `:reload` | — | Reload the device config from disk and restart |
| `:compact` | — | Toggle compact table rows |
| `:set <register> <value>` | register name, value (name may be `"quoted"` to allow spaces) | Write a value into a register → **modbus** owns type/range/codec semantics and store-vs-wire behavior |
| `:write-device` / `:wd` `[path]` | optional path (default: configured device path) | Save the device config file |
| `:log <file>` | file base path | Set the module's log-file sink base |
| `:script` | — | Open the Lua script manager dialog |
| `:order [col] [asc\|desc]` | optional column, optional direction (default `asc`) | Sort the register table by column; bare `:order` clears |

Modbus monitor module (`role = monitor`, MB-R-140–145):

| Command | Arguments | Purpose (→ modbus) |
|---|---|---|
| `:edit` / `:e` | — | Open the module setup dialog |
| `:add` / `:a` | — | Open the add-register-interpretation dialog (UI-R-061) |
| `:start` | — | Start the module (open the serial port receive-only) |
| `:stop` | — | Stop the module |
| `:restart` | — | Stop then start |
| `:reload` | — | Reload the device config from disk and restart |
| `:compact` | — | Toggle compact table rows |
| `:write-device` / `:wd` `[path]` | optional path (default: configured device path) | Save the device config file |
| `:log <file>` | file base path | Set the module's log-file sink base |
| `:order [col] [asc\|desc]` | optional column, optional direction (default `asc`) | Sort the resolved-registers table; bare `:order` clears |

`:set` (nothing to write) and `:script` (no Lua surface) are omitted for this role — unrecognized commands on a monitor view, not errors.

### 2.2 OCPP client module

| Command | Arguments | Purpose (→ ocpp) |
|---|---|---|
| `:edit` / `:e` | — | Open the module setup dialog |
| `:start` | — | Connect to the CSMS |
| `:stop` | — | Disconnect |
| `:restart` | — | Reconnect |
| `:compact` | — | Toggle compact rows |
| `:write-device` / `:wd` `[path]` | optional path | Save the device config |
| `:log [file]` | optional file path | Set the file sink; bare `:log` or empty path disables |

### 2.3 OCPP server (CSMS) module

| Command | Arguments | Purpose (→ ocpp) |
|---|---|---|
| `:start` | — | Bind the CSMS listener |
| `:stop` | — | Unbind (clears connected-station entries) |
| `:restart` | — | Rebind (clears entries) |
| `:edit` / `:e` | — | Open the module setup dialog |
| `:write-device` / `:wd` `[path]` | optional path | Save the device config |
| `:compact` | — | Toggle compact rows |
| `:log [file]` | optional file path | Set the file sink; bare `:log` or empty path disables |
| `:rfid [add\|del <tag> \| clear]` | subcommand + tag | Manage the CSMS RFID accept-list; bare `:rfid` prints it |

**OCPP action send is not a `:` command.** Composing and sending an action goes through the action dialog opened by `Enter` on the message table / action control. Action set and payload semantics: `ocpp/`.

## 3. Global keybindings

| Key | Context | Action |
|---|---|---|
| `:` | content focused, no view overlay | Enter command mode |
| `?` | content focused, no view overlay | Open the keybind-help dialog |
| `Ctrl+w` then `j`/`k`/`Down`/`Up` | content focused | Toggle focus between content view and log pane |
| `Ctrl+t` then `l` | content focused | Next tab (wraps) |
| `Ctrl+t` then `h` | content focused | Previous tab (wraps) |
| `Ctrl+t` then digit(s) | content focused | Jump to tab by index; waits up to 800 ms for a 2nd digit if one could form a valid 2-digit index (UI-R-011) |

`:` and `?` are suppressed while the active view has an overlay open; they type into it instead.

## 4. Context keybinding tables

### 4.1 Command mode

| Key | Action |
|---|---|
| `Esc` | Cancel (discard buffer) |
| `Enter` | Run the command |
| printable / `Left`/`Right` / `Home`/`End` / `Backspace`/`Delete` | Edit the buffer |

### 4.2 Dialogs (generic)

| Key | Action |
|---|---|
| `Tab` | Next field (skips disabled) |
| `Shift+Tab` / `BackTab` | Previous field |
| `Enter` | Confirm |
| `Esc` | Request close (close-confirm popup if edits may be lost) |

Applied only when the focused widget did not consume the key.

### 4.3 Close-confirm / yes-no popup

| Key | Action |
|---|---|
| `Enter` / `Space` | Confirm (close / delete) |
| `Esc` | Dismiss (back to editing) |
| `Tab` / `Shift+Tab` | Move between confirm/cancel (delete confirm) |

Focus defaults to the safe (cancel) choice; confirming requires that choice focused.

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

Selection clamps at the ends (no wrap).

### 4.5 Suggestion-completion popup (open)

| Key | Action |
|---|---|
| `Up` / `Down` | Move highlight |
| `Enter` / `Tab` | Accept highlight (partial → keep open and re-query; else close) |
| `Esc` | Dismiss |

### 4.6 Single-line text input (focused)

| Key | Action |
|---|---|
| printable (with `Shift`) | Insert character (rejected chars still consumed) |
| `Left` / `Right` | Move cursor |
| `Home` / `End` | Line start / end |
| `Backspace` / `Delete` | Delete before / at cursor |
| `Ctrl+F` | Autofill from placeholder (only when empty) |
| `Ctrl+D` | Clear |

### 4.7 Keybind-help dialog (`?`)

| Key | Action |
|---|---|
| `Esc` / `q` / `?` | Close |
| `j` / `Down` | Scroll down |
| `k` / `Up` | Scroll up |
| `g` | Top |
| `G` | Bottom |

### 4.8 Lua-bindings help overlay (`?` in a script editor, Normal mode)

Same navigation as §4.7. Reachable only from the code editor in Normal mode; in Insert/Visual `?` is literal text.

### 4.9 Script-manager dialog

| Key | Context | Action |
|---|---|---|
| `Tab` / `Shift+Tab` | dialog | Cycle focus (script table → name input → Templates button → code editor → interval → log); code editor skipped while no script selected |
| `Esc` | dialog | Open close-confirm |
| `t` | script table focused | Toggle the selected script's enabled flag |
| `d` | script table focused | Delete the selected script (opens confirm) |
| `c` | script table focused | Toggle compact rows |
| `e` | script table focused | Execute the selected script once (current editor content, enabled or not) |
| `Enter` | script table focused | Open the rename prompt |
| `Enter` | name input focused | Create a new script with the typed name |
| `Enter` / `Space` | Templates button focused | Open the template-browser overlay |
| `?` | script table focused | Open the script-table keybind-help overlay (`Esc`/`q`/`?` closes) |
| `?` | code editor, Normal mode | Open the Lua-bindings help overlay |

### 4.10 Template-browser overlay (Templates button in the script-manager dialog)

| Key | Context | Action |
|---|---|---|
| `j` / `k` / `Up` / `Down` | template list | Move selection; preview follows |
| `Tab` / `Shift+Tab` | overlay | Cycle focus between list and (read-only) preview |
| `Enter` | overlay | Insert the selected template as a new enabled script, close |
| `Esc` / `q` | overlay | Close, changing nothing |

### 4.11 Script rename prompt (`Enter` on the script table)

| Key | Context | Action |
|---|---|---|
| `Enter` | prompt | Commit; empty or duplicate name refused, prompt stays open |
| `Esc` | prompt | Cancel, name unchanged |

## 5. Code editor — modes and commands

Vim-modal editor (default for the Lua-script editor). Modes: `NORMAL`, `INSERT`, `VISUAL` (charwise), `V-LINE` (linewise).

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
| `Left`/`Right`/`Up`/`Down` | Arrow move (wraps to adjacent line) |
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
| `u` | Undo last change (again to redo) |

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

Printable keys insert; `Enter` splits with auto-indent (when a language is set); `Backspace`/`Delete` edit; arrows navigate with line wrap; `Home`/`End` and character-based editing as §4.6. Two space presses at the same position within ~300 ms expand to a four-space indent (an intervening key cancels).

Yank/delete also copy to the system clipboard via OSC 52. A `language` setting drives syntax highlighting and format-on-blur (JSON may decline invalid input; Lua always reformats).
