# TUI — Requirements

Application shell and tab model, focus model, keyboard navigation, `:` command line mechanism, modal dialog/overlay mechanism, in-TUI vim-modal Lua/JSON code editor, syntax highlighting, reusable widget set, live value/log rendering.

IDs stable, append-only (`UI-R-nnn`). See [`../README.md`](../README.md). Companions: [`api-contract.md`](./api-contract.md) (exhaustive `:` command list, every keybinding table, code-editor mode/command set), [`edge-cases.md`](./edge-cases.md).

**Area boundaries.** This area owns the *mechanism*: how `:` commands parse and dispatch, generic (app-level) commands, keybindings, vim + arrow navigation, dialogs as a mechanism, the code editor, syntax highlighting. It does **not** own protocol-specific command *semantics*: a command forwarded to a module view (Modbus `:set`, `:reload`, OCPP `:rfid`, `:start`/`:stop`) is *listed* here with general syntax; its effect on protocol state is `modbus/` or `ocpp/`. Which config fields a dialog exposes and their valid ranges belong to the protocol / `config-session/` areas; the dialog *mechanism* is owned here. The process command line (`ferrowl run`, CLI flags) is `cli-headless/`; only the in-TUI `:` line is owned here.

---

## App shell, tabs & focus model

**UI-R-001** — The application presents a full-screen terminal UI in the alternate screen buffer with raw mode enabled, and restores the terminal (leave alternate screen, disable raw mode) on normal exit, on the error exit path, and from a panic hook, so a crash never leaves the terminal corrupt.

**UI-R-002** — Screen layout top-to-bottom: one-row tab bar, flexible module content area, fixed-height log pane, one-row command line. The content area absorbs remaining height.

**UI-R-003** — The application owns an ordered list of tabs and one active index. Each tab pairs one module content view with its own log pane. Exactly one tab is active and rendered; the others keep running in the background (UI-R-030).

**UI-R-004** — Every tab has a unique display name. When an operation (in-dialog rename, session load) would make two tabs share a name, the later duplicate(s) are auto-suffixed and a warning logged into the renamed tab's own log. Names are unique at all times so name-based session-module lookups are never ambiguous.

**UI-R-005** — Input is routed by a single modal layer selector with precedence: keybind-help dialog (topmost, modal) → app-level creation/session dialog → active tab's open overlay → command line → active tab's content/log panes. An open layer consumes the keys its lower layers would otherwise receive.

**UI-R-006** — Keyboard focus within the active tab is the content view or the log pane, never both. The `:` command line and any dialog remove focus from the panes while open and restore it on close. Every focus transition routes through a single choke point so a tab's stored widget focus never goes stale after a tab switch or modal open/close.

**UI-R-007** — Only key **press** events are acted upon; release/repeat kinds and non-key terminal events are ignored for command/navigation.

**UI-R-008** — Starting with no tabs configured opens the new-module type selector immediately.

**UI-R-057** — Whenever the application holds zero tabs and no modal layer is open — no app-level creation/type-select overlay, no session dialog, no keybind-help dialog — it exits through the normal terminal-restoring path (UI-R-001). In particular, cancelling the startup selector (UI-R-008) before any tab exists quits rather than leaving an empty shell. A safety net independent of `:quit`/`:qall` (UI-R-019).

## Navigation & tab switching

**UI-R-009** — `Ctrl+w` begins a window-switch chord; a following `j`, `k`, `Down`, or `Up` toggles focus between the active tab's content view and log pane.

**UI-R-010** — `Ctrl+t` begins a tab-switch chord: `l` next tab, `h` previous (both wrap), a digit begins a by-index jump.

**UI-R-011** — A `Ctrl+t` digit jump: if the first digit already uniquely identifies a tab (no in-range two-digit index starts with it, or it is `0`), jump immediately; otherwise wait up to 800 ms for a second digit. A second digit forming an in-range two-digit index jumps there; an out-of-range combination falls back to the first digit's tab. Timeout with no second digit → the pending jump commits. Any non-digit while a first digit is pending commits that jump and is then processed normally.

**UI-R-012** — A jump to an out-of-range or already-active index is a silent no-op. Tab-switch operations are safe with zero or one tabs.

**UI-R-013** — In a focused table or selection list, `j`/`Down` and `k`/`Up` move the row selection; `h`/`Left` and `l`/`Right` move the column selection (tables) or item (horizontal selection); `g` first row, `G` last, `0`/`Home` first column, `$`/`End` last. Selection clamps at the ends (no wrap for tables).

## Command line mechanism

**UI-R-014** — `:` while the content panes are focused and no view overlay is open enters command mode: focus moves to the command line, buffer cleared, printable keys type into it. `:` does not enter command mode while a view overlay is open (it types into the overlay).

**UI-R-015** — In command mode, `Esc` cancels (discard buffer, restore content focus); `Enter` submits the trimmed buffer and restores content focus. Empty submission is a no-op.

**UI-R-016** — The command line is parsed by a pure, state-independent parser into a fixed set of app-level commands ([`api-contract.md`](./api-contract.md) ``## 1. Generic `:` commands (app-level)``); leading/trailing and inter-token whitespace collapsed on split. Any first token not recognized at the app level is classified unknown and forwarded verbatim to the active view.

**UI-R-017** — App-level commands are dispatched by the application: tab lifecycle (`:quit`, `:qall`, `:new`, `:load`), session persistence (`:write`), tab reordering (`:swap`), session-script management (`:session`, `:script copy`), log-ring clear (`:log clear`). Exact syntax and aliases: [`api-contract.md`](./api-contract.md).

**UI-R-018** — A command not handled at the app level is forwarded to the active tab's view. If handled, any `(level, message)` returned is appended to the tab's log; if unhandled, the application logs `Unknown command ':<input>'` at Warning. The level of a result message is chosen explicitly by the producer, never re-derived from message text.

**UI-R-019** — `:quit` closes the active tab, stopping its module first, and quits the application only when it was the last tab. `:qall` quits immediately regardless of tab count.

**UI-R-020** — While the command line is focused, a help popup lists available commands: generic app-level commands plus whatever list the active view advertises for its module type.

## Dialogs & overlays mechanism

**UI-R-021** — A dialog/overlay is a modal layer rendered over the content and log panes, consuming keyboard input while open. Overlays paint back-to-front (module overlays, command help popup, app-level dialog, keybind-help dialog on top) so a higher layer is never overdrawn by a lower.

**UI-R-022** — Within a dialog, `Tab` advances focus to the next field and `Shift+Tab`/`BackTab` retreats, cycling; fields whose enabling condition is false are skipped. `Enter` confirms; `Esc` requests close. These defaults apply only when the focused field/widget did not consume the key.

**UI-R-067** — A setup dialog opens with exactly one field focused — the first in its `Tab` cycle (UI-R-022) — and its focus cursor names that same field, so the first `Tab` moves from where the focus styling appears. Every other field opens unfocused, nested sections included. Where the focused field is a text input, it is the dialog's only field painting a text cursor.

**UI-R-068** — A single-line input's border is styled by validation first and focus second: text failing validation paints the error style focused or not; only a field whose text validates, or has no validator, paints the focused style when focused and the normal border otherwise. A disabled single-line input never paints the focused style. Governs the single-line widget alone; the multi-line code editor styles its border from focus only and keeps the focused border while disabled, since a read-only viewer still holds focus for scrolling. A focused field with an error border is therefore the normal look of a fresh setup dialog (empty name input against a non-empty requirement); a dialog opened by `:edit` prefills the name and shows the focused border.

**UI-R-023** — `Esc` on a dialog that may hold unsaved edits opens a close-confirmation popup rather than discarding; confirming (`Enter` or `Space`) closes, dismissing (`Esc`) returns to editing. A yes/no box defaults focus to the safe (cancel) choice and requires an explicit move to confirm.

**UI-R-024** — The new-module flow is two staged overlays: module-type selector, then the chosen type's setup dialog. Confirming the selector swaps in the setup dialog; confirming a valid setup dialog creates and starts the tab. A dialog failing validation stays open.

**UI-R-025** — Creating a tab whose name collides with an existing tab is refused with a warning in the active tab's log, dialog left open; never silently overwrite or duplicate a name.

**UI-R-026** — A field-completion popup (suggestion input), while open, consumes `Up`/`Down` to move the highlight, `Enter` to accept, `Esc` to dismiss. `Tab` is never consumed by the popup, even open, so it always moves focus to the dialog's next field. Accepting a suggestion marked *partial* keeps the popup open and re-queries (e.g. descending a directory); accepting a non-partial one closes it. While closed, `Up`/`Down`/`Enter`/`Esc` pass through to the dialog.

**UI-R-051** — In the script-manager dialog, while the script table is focused, `e` executes the selected script exactly once, using the script's current editor content (including unapplied edits) regardless of its enabled flag. No selection → no-op. Dialog stays open; the run's `print`/`C_Log` output and any error appear in the dialog's log pane. Execution semantics: SC-R-035 in [`../scripting/requirements.md`](../scripting/requirements.md).

**UI-R-052** — The script-manager dialog carries a *Templates* button in its focus cycle, after the new-script name input. `Enter` or `Space` on it opens the template-browser overlay.

**UI-R-053** — The template-browser overlay lists only templates applicable to the dialog's script context (SC-R-036 in [`../scripting/requirements.md`](../scripting/requirements.md)), each with name and description, plus a read-only syntax-highlighted preview of the selected template's code. `Esc` or `q` closes without changing the script list. While open it takes precedence over all other dialog keys.

**UI-R-054** — Confirming a template appends it to the dialog's working script list as a new enabled script whose code copies the template body, selects it, closes the overlay, leaves the dialog open. The script takes the template's name; if taken, the first free `<name>-<n>` (n from 2) — insertion never refused for a name collision.

**UI-R-055** — In the script-manager dialog, while the script table is focused, `Enter` on a selected script opens a rename prompt pre-filled with its name. `Enter` renames; `Esc` dismisses unchanged. An empty (after trimming) or already-used name is refused and the prompt stays open. No selection → no-op. Renaming changes only the name — code and enabled flag preserved.

**UI-R-056** — In the script-manager dialog, while the script table is focused, `?` opens a keybind-help overlay listing the table's bindings (rename, run once, toggle enabled, delete, compact) with a one-line description each. `Esc`, `q`, or `?` closes. While open it takes precedence over all other dialog keys. The table's title advertises only this overlay, not the individual bindings.

**UI-R-058** — In the script-manager dialog, while the script table is focused, the table supports editing its working list: a non-empty name in the new-script input plus confirm adds a new enabled, empty script (empty or already-used name refused, per UI-R-055); `t` toggles the selected script enabled/disabled; `d` deletes it after a yes/no confirmation (UI-R-023); `c` toggles compact/normal rows. `t` and `d` are no-ops with no selection. Edits change the working list only and take effect on the owner when the dialog is applied (SC-R-024); these are the bindings advertised by the help overlay (UI-R-056).

**UI-R-059** — In the OCPP setup dialog, a client-role instance shows two always-visible new-header inputs (name, value) backing `extra_headers`, plus the headers table once the list is non-empty — both hidden for the server role, per the `#[focus(when = …)]` role-conditional pattern used for other client-only fields. While the table is focused: a name and value in the inputs plus confirm adds a row, refused inline (prompt stays open) if name/value fails OC-R-118's grammar or OC-R-117's reserved-name check; `Enter` on a selected row opens an edit prompt pre-filled with its name and value, same validation on confirm, `Esc` dismissing unchanged; `d` deletes the selected row after a yes/no confirmation (UI-R-023); `Enter` and `d` are no-ops with no selection. Rows keep insertion order (OC-R-117). Edits change the working list only and take effect when the dialog is applied.

## Code editor (vim-modal)

**UI-R-027** — The multi-line code editor supports two profiles: plain single-mode (printable keys insert, `Enter` splits, `Backspace`/`Delete` edit, arrows navigate with wrap) and vim-modal (`Normal`/`Insert`/`Visual`). Vim-modal is default for the Lua-script editor.

**UI-R-028** — Vim-modal: `Normal` provides motions and operators; `i`/`a`/`I`/`A`/`o`/`O` enter `Insert` at the documented position; `v`/`V` enter charwise/linewise `Visual`; `Esc` from `Insert` or `Visual` returns to `Normal`; `Esc` in `Normal` is left unhandled so it reaches the dialog (opening close-confirm). Exact motion/operator set: [`api-contract.md`](./api-contract.md) `## 5. Code editor — modes and commands`.

**UI-R-029** — The editor keeps the vim block-cursor invariant in `Normal` (cursor rests on a character, clamping to the last column, not past it); `Insert` allows one past the last character. `h`/`l` do not wrap across lines; arrows wrap to the adjacent line.

**UI-R-030** — Yank (`y`, `yy`) and delete (`d`, `dd`, `x`) write the removed/copied text into an internal register (linewise or charwise) used by paste (`p`/`P`), and also emit it to the system clipboard via an OSC 52 escape, best-effort, never failing the edit.

**UI-R-031** — Single-level undo (`u`): each mutating operation snapshots the buffer before applying; `u` swaps current buffer with the snapshot (pressing again redoes). Motions and mode changes do not consume the slot.

**UI-R-032** — With a language set, the editor auto-indents on newline and `o`/`O`: the new line inherits the current line's leading indentation adjusted by the language's per-line block-balance delta (four spaces per level), floored at zero. Without a language, no automatic indent.

**UI-R-033** — With a language set and the field enabled, losing focus reformats the buffer through the language formatter; if the formatter declines (e.g. invalid JSON), the buffer is unchanged. A disabled field never reformats; gaining focus never reformats.

**UI-R-034** — In `Insert`, `Tab` inserts four spaces; `Shift+Tab` removes up to four leading spaces from the current line. In the plain editor, two space presses at the same cursor position within a short bound (default 300 ms) expand to a four-space indent; an intervening key cancels.

**UI-R-035** — All cursor movement, insertion, deletion are character-based, not byte-based, so multi-byte UTF-8 is edited without splitting or miscounting.

**UI-R-036** — A disabled code editor ignores all mutating keys (insert, delete, paste, mode entry that would edit) while permitting navigation, and reports such keys unhandled so higher layers can use them.

## Syntax highlighting

**UI-R-037** — Syntax highlighting is pure text-to-span computation, no rendering: for a language and one line of source (plus a carry-over line state for multi-line constructs) it returns `(start_char, end_char, kind)` spans, sorted by start, non-overlapping, character indices. Two languages: Lua and JSON.

**UI-R-038** — The carry-over state lets multi-line constructs (Lua long strings and long comments) highlight correctly across lines when highlighted in order.

**UI-R-039** — Highlight kinds are a fixed enumeration (keyword, identifier, number, string, comment, punctuation, JSON key, literal, object identifier, function identifier); the consumer maps kind to colors. Highlighting never mutates the source.

## Tables, live updates & logging

**UI-R-040** — On every UI tick the application polls **all** tabs' views to refresh state — not only the active — so background modules keep sending/receiving and their values and logs stay current. Refreshes are polled concurrently so tick latency is bounded by the slowest tab, not their sum.

**UI-R-041** — The UI redraws on input and on a periodic timeout (≈100 ms) with no input, so live values and inbound traffic update without a keypress.

**UI-R-042** — A view may request replacement by a different view (e.g. an OCPP role switch turning client into server); the application applies it on the next tick, carries over the tab's log channel and focus, rebuilds the session-module registry.

**UI-R-043** — Each tab owns a bounded ring log of timestamped, severity-tagged lines (Info/Warning/Error). The log pane shows the most recent lines and auto-follows the tail unless the user has focused that tab's log pane, in which case scroll position holds.

**UI-R-044** — A log line longer than the per-line cap is truncated to it. A monotonic total-written counter lets a consumer holding only a bounded snapshot compute how many lines are new since its last read, across ring eviction.

**UI-R-045** — `:log clear` clears the active tab's on-screen ring. File-sink logging (`:log <file>`) is module-forwarded, semantics in the module's area; with a file sink configured, buffered lines flush to disk once per UI tick (and on sink teardown), not per line.

**UI-R-046** — A table cell wider than the visible width is reachable by horizontal scroll tied to the selected column; the tab bar keeps the active tab visible by scrolling horizontally on overflow. Live-updated cells can be highlighted briefly after they change.

## Widget & focus-derive contract

**UI-R-047** — Reusable widgets follow one event contract: offered a `(modifiers, code)` key event, each returns *consumed* or *unhandled* (carrying the original key back). Unhandled propagates to the enclosing layer; consumed stops propagation.

**UI-R-048** — A single-line text input supports `Home`/`End`, `Left`/`Right`, `Backspace`/`Delete`, printable insertion (including `Shift` for capitals and symbols), `Ctrl+F` autofill from placeholder (only when empty), `Ctrl+D` clear. A focused input consumes printable keys even when a per-field filter rejects the character, so disallowed characters never leak to app-level shortcuts. Editing is character-based (multi-byte safe).

**UI-R-049** — A view is composable as a focusable node: set/query focus and next/previous focus stepping, so the owning tab treats "switch content↔log" as one focus step and toggles the whole tab's focus recursively into whichever pane is active. A focusable container's cycle skips fields whose enabling condition is false.

**UI-R-050** — The color scheme is a single compile-time constant selected by build feature; no runtime switch.

## Modbus monitor view

**UI-R-060** — A Modbus monitor module's content view has a left panel listing every unit id observed (updated live) and, on the right, sections scoped to the selected unit id: a message table (MB-R-146 records for that unit id), a memory layout (MB-R-144's observed-value table for that unit id, grouped by table kind), and a resolved-registers table (MB-R-145 interpretations applied to that unit id's memory) — the resolved-registers table hidden entirely when no interpretation exists for the selected unit id, reappearing once one does. As every module tab, the module's own scrolling log occupies the bottom log pane per UI-R-003, independent of these sections.

**UI-R-061** — On a monitor view, `:add`/`:a` opens a dialog to add a register interpretation (MB-R-145) scoped to the selected unit id, mirroring the client/server `:add` dialog (`api-contract.md`). The interpretation is added to that unit id's set; the resolved-registers table reflects it immediately. With no unit id discovered yet, `:add` is rejected with a Warning-level message instead of opening — no unit id to scope to.

**UI-R-062** — The message table (UI-R-060) renders one row per MB-R-146 record for the selected unit id, most recent first, columns Time, Status, Slave, Operation, Address, Quantity, Values/Payload. Values/Payload renders `[XXXX XXXX ...]` (4-digit lowercase hex per 16-bit word, space-separated, matching the modbus module's raw-value formatting) for register-shaped operations, `[0 1 0 ...]` (one digit per bit) for coil/discrete-input-shaped, empty for any operation MB-R-146 carries no address/quantity/value for. Horizontal overflow scrolls rather than truncating or wrapping.

**UI-R-063** — The memory layout panel (UI-R-060) renders each of MB-R-144's populated table kinds for the selected unit id as an independent hex-editor-style block. Holding/Input Registers address by register, 8 per line; Coils/Discrete Inputs pack 8 bits per byte (MSB-first), address by byte, 16 per line. Each line shows, in order: table kind (Coil, Discrete Input, Holding Register, Input Register — the modbus module's `Kind` naming), starting address, hex byte/word values, character representation. A line with no observed value in its range is omitted; within a rendered line, an unobserved byte/word renders as a dim placeholder (`··`), not a decoded zero. Each byte/word is colorized by value class — zero in the placeholder color, printable ASCII in normal text color, other non-zero in a distinguishing highlight — and, independently, painted with the change-highlight color while an MB-R-147 recency marker is active for its address.

**UI-R-064** — The resolved-registers table (UI-R-060, MB-R-145) renders columns Name, Description, Address, Kind, Format, Length, Resolution, Value, Raw Value — the modbus module's register table's set (`ferrowl::module::modbus::table::TableHeader`) less Slave ID and Access, neither applying to a monitor interpretation. Enter on a selected row opens an edit dialog prefilled from that interpretation, offering Confirm (MB-R-148 edit) and Delete (MB-R-148 remove), mirroring the modbus edit-register dialog.

**UI-R-065** — Tab/Shift+Tab on a monitor content view cycles focus across panels: Units, Messages, Memory layout, Resolved registers, back to Units — skipping Resolved registers whenever hidden (UI-R-060). Each panel keeps its own selection/scroll position; Up/Down/Left/Right and Enter act on the focused panel.

**UI-R-066** — The shared `Table` widget's optional selection-marker gutter reserves zero width whenever no row is selected, otherwise exactly the highlight symbol's rendered width, for every table regardless of whether the marker is shown. A selected row's background is painted with its highlight style in every case except an unfocused table with the marker shown, where the marker glyph alone is the cue and a row highlight is omitted as redundant.
