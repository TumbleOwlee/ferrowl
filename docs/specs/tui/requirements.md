# TUI — Requirements

Normative behavior of the terminal UI capability area: the application shell and
tab model, the focus model, keyboard navigation, the `:` command line mechanism,
the modal-dialog/overlay mechanism, the in-TUI vim-modal Lua/JSON code editor,
syntax highlighting, the reusable widget set, and live value/log rendering.

IDs are stable and append-only (`UI-R-nnn`). See [`../README.md`](../README.md).

Companion documents: [`api-contract.md`](./api-contract.md) (the exhaustive `:`
command list, every keybinding table, and the code-editor mode/command set — the
operator-facing surface), [`edge-cases.md`](./edge-cases.md) (boundary and error
behavior, and stated known limitations).

**Area boundaries.** This area owns the *mechanism*: how `:` commands are parsed
and dispatched, the generic (app-level) commands, keybindings, vim + arrow
navigation, dialogs as a mechanism, the code editor, and syntax highlighting.
It does **not** own protocol-specific command *semantics*: a command forwarded to
a module view (e.g. a Modbus register write `:set`, a Modbus `:reload`, an OCPP
`:rfid`, a `:start`/`:stop` lifecycle action) is *listed* here with its general
syntax, but what it does to protocol state is specified in `modbus/` or `ocpp/`.
Which config fields a config-editing dialog exposes and their valid ranges belong
to the protocol / `config-session/` areas; the dialog *mechanism* is owned here.
The process command line (`ferrowl run`, CLI flags) belongs to `cli-headless/`;
only the in-TUI `:` command line is owned here.

---

## App shell, tabs & focus model

**UI-R-001** — The application shall present a full-screen terminal UI in the
alternate screen buffer with raw mode enabled, and shall restore the terminal
(leave the alternate screen, disable raw mode) on normal exit, on the error exit
path, and from a panic hook, so a crash never leaves the user's terminal corrupt.

**UI-R-002** — The screen shall be laid out top-to-bottom as: a one-row tab bar,
a flexible module content area, a fixed-height log pane, and a one-row command
line. The content area absorbs the remaining height.

**UI-R-003** — The application shall own an ordered list of tabs and one active
tab index. Each tab pairs one module content view with its own log pane. Exactly
one tab is active and rendered in the content/log area at a time; the others
continue running in the background (see UI-R-030).

**UI-R-004** — Every tab shall have a unique display name. When an operation
(e.g. an in-dialog rename, a session load) would make two tabs share a name, the
later duplicate(s) shall be auto-suffixed to restore uniqueness and a warning
shall be logged into the renamed tab's own log. Tab names shall be unique at all
times so name-based session-module lookups are never ambiguous.

**UI-R-005** — Input shall be routed by a single modal layer selector with the
precedence: keybind-help dialog (topmost, modal) → app-level creation/session
dialog → the active tab's own open overlay → the command line → the active tab's
content/log panes. A layer that is open shall consume the keys its lower layers
would otherwise receive.

**UI-R-006** — Keyboard focus within the active tab shall be either the content
view or the log pane, never both. The `:` command line and any dialog shall
remove focus from the content/log panes while open and restore it on close.
Every focus transition shall route through a single choke point so a tab's stored
widget focus never goes stale after a tab switch or a modal open/close.

**UI-R-007** — Only key **press** events shall be acted upon; key release/repeat
kinds and non-key terminal events shall be ignored for command/navigation
purposes.

**UI-R-008** — Starting the application with no tabs configured shall open the
new-module type selector immediately, so the user is never left on an empty shell
with no obvious action.

## Navigation & tab switching

**UI-R-009** — `Ctrl+w` shall begin a window-switch chord; the following `j`, `k`,
`Down`, or `Up` shall toggle focus between the active tab's content view and its
log pane.

**UI-R-010** — `Ctrl+t` shall begin a tab-switch chord: a following `l` moves to
the next tab, `h` to the previous tab (both wrapping), and a digit begins a
by-index jump.

**UI-R-011** — A `Ctrl+t` digit jump shall resolve as follows: if the first digit
already uniquely identifies a tab (no two-digit index starting with that digit is
in range, or the digit is `0`), it shall jump immediately; otherwise it shall
wait up to a bounded timeout (800 ms) for a second digit. A second digit forming
an in-range two-digit index jumps there; an out-of-range combination falls back
to the tab named by the first digit alone. If the timeout elapses with no second
digit, the pending first-digit jump shall commit. Any non-digit key pressed while
a first digit is pending shall commit that jump and then be processed normally.

**UI-R-012** — A jump to an out-of-range or already-active tab index shall be a
silent no-op. Tab-switch operations shall be safe when there are zero or one tabs.

**UI-R-013** — Within a focused table or selection list, `j`/`Down` and `k`/`Up`
shall move the row selection, and `h`/`Left` and `l`/`Right` shall move the
column selection (tables) or (for a horizontal selection) the item; `g` shall jump
to the first row, `G` to the last, `0`/`Home` to the first column, and `$`/`End`
to the last column. Row and column selection shall clamp at the ends (no wrap for
tables).

## Command line mechanism

**UI-R-014** — Pressing `:` while the content panes are focused and no view
overlay is open shall enter command mode: focus moves to the command line, its
buffer is cleared, and subsequent printable keys type into it. `:` shall not
enter command mode while a view overlay is open (it types into the overlay
instead).

**UI-R-015** — In command mode, `Esc` shall cancel (discard the buffer, restore
content focus) and `Enter` shall submit the trimmed buffer for execution and
restore content focus. An empty submission shall be a no-op.

**UI-R-016** — The command line shall be parsed by a pure, state-independent
parser into a fixed set of recognized app-level commands (see
[`api-contract.md`](./api-contract.md) §1); leading/trailing and inter-token
whitespace shall be collapsed on split. Any first token not recognized at the app
level shall be classified as unknown and forwarded verbatim to the active view.

**UI-R-017** — App-level commands shall be dispatched by the application itself:
tab lifecycle (`:quit`, `:qall`, `:new`, `:load`), session persistence
(`:write`), tab reordering (`:swap`), session-script management (`:session`,
`:script copy`), and the log-ring clear form (`:log clear`). Their exact syntax
and aliases are the contract in [`api-contract.md`](./api-contract.md).

**UI-R-018** — A command not handled at the app level shall be forwarded to the
active tab's view. If the view handles it, any `(level, message)` it returns shall
be appended to the active tab's log; if the view reports it unhandled, the
application shall log a `Unknown command ':<input>'` warning. The severity level
of a command's result message shall be chosen explicitly by the producer, never
re-derived by inspecting the message text.

**UI-R-019** — `:quit` shall close the active tab, stopping its module first, and
shall quit the whole application only when it is the last remaining tab. `:qall`
shall quit immediately regardless of tab count.

**UI-R-020** — While the command line is focused, a help popup listing the
available commands shall be shown: the generic app-level commands plus whatever
command list the active view advertises for its module type.

## Dialogs & overlays mechanism

**UI-R-021** — A dialog/overlay shall be a modal layer that renders on top of the
content and log panes and consumes keyboard input while open. Overlays shall be
painted in a defined back-to-front order (module overlays, then the command help
popup, then the app-level dialog, then the keybind-help dialog on top) so a
higher layer is never overdrawn by a lower one.

**UI-R-022** — Within a dialog, `Tab` shall advance focus to the next field and
`Shift+Tab`/`BackTab` shall retreat to the previous field, cycling; fields whose
enabling condition is currently false shall be skipped in the cycle. `Enter` shall
confirm the dialog and `Esc` shall request close. These defaults shall apply only
when the focused field/widget did not itself consume the key.

**UI-R-023** — `Esc` on a dialog that may hold unsaved edits shall open a
close-confirmation popup rather than discarding immediately; confirming the popup
(`Enter` or `Space`) closes the dialog, dismissing it (`Esc`) returns to editing.
A yes/no confirmation box shall default focus to the safe (cancel) choice and
require an explicit move to the confirm choice.

**UI-R-024** — The new-module flow shall be two staged overlays: a module-type
selector, then the chosen type's setup dialog. Confirming the type selector swaps
in the setup dialog; confirming a valid setup dialog creates and starts the tab.
A setup dialog that fails validation shall stay open.

**UI-R-025** — Creating a tab whose name collides with an existing tab shall be
refused with a warning in the active tab's log, leaving the setup dialog open;
tab creation shall not silently overwrite or duplicate a name.

**UI-R-026** — A field-completion popup (suggestion input) shall, while open,
consume `Up`/`Down` to move the highlighted suggestion, `Enter`/`Tab` to accept
it, and `Esc` to dismiss it. Accepting a suggestion marked *partial* shall keep
the popup open and re-query (e.g. descending a directory); accepting a
non-partial suggestion shall close it. While closed, those keys shall pass through
to the surrounding dialog.

**UI-R-051** — In the script-manager dialog, while the script table is focused, `e`
shall execute the selected script exactly once, using the script's current editor
content (including edits not yet applied to the owner) and regardless of the script's
enabled flag. With no script selected it shall be a no-op. The dialog shall stay open,
and the run's `print`/`C_Log` output and any error shall appear in the dialog's log
pane. The execution semantics are specified by SC-R-035 in
[`../scripting/requirements.md`](../scripting/requirements.md).

**UI-R-052** — The script-manager dialog shall carry a *Templates* button in its
focus cycle, positioned after the new-script name input. `Enter` or `Space` on the
focused button shall open the template-browser overlay.

**UI-R-053** — The template-browser overlay shall list only the templates
applicable to the dialog's script context (SC-R-036 in
[`../scripting/requirements.md`](../scripting/requirements.md)), each with its name
and description, alongside a read-only preview of the selected template's Lua code
with syntax highlighting. `Esc` or `q` shall close the overlay without changing the
script list. While the overlay is open it shall take precedence over all other
dialog keys.

**UI-R-054** — Confirming a template in the overlay shall append it to the dialog's
working script list as a new enabled script whose code is a copy of the template
body, select that script, close the overlay, and leave the dialog open. The new
script shall take the template's name; if that name is already taken in the list, it
shall take the first free `<name>-<n>` (n from 2 upwards) — insertion shall never be
refused for a name collision.

**UI-R-055** — In the script-manager dialog, while the script table is focused,
`Enter` on a selected script shall open a rename prompt pre-filled with that
script's current name. Confirming with `Enter` shall rename the script; `Esc` shall
dismiss the prompt leaving the name unchanged. A name that is empty (after trimming)
or already used by another script in the list shall be refused and the prompt shall
stay open. With no script selected, `Enter` shall be a no-op. Renaming shall change
only the script's name — its code and enabled flag shall be preserved.

## Code editor (vim-modal)

**UI-R-027** — The multi-line code editor shall support two operating profiles:
a plain single-mode editor (printable keys insert, `Enter` splits the line,
`Backspace`/`Delete` edit, arrow keys navigate with wrap) and a vim-modal editor
(`Normal`/`Insert`/`Visual` modes). The vim-modal profile is the default for the
Lua-script editor.

**UI-R-028** — In the vim-modal editor: `Normal` mode shall provide motions and
operators; `i`/`a`/`I`/`A`/`o`/`O` shall enter `Insert` mode at the documented
cursor position; `v`/`V` shall enter charwise/linewise `Visual` mode; `Esc` from
`Insert` or `Visual` shall return to `Normal`; and `Esc` in `Normal` mode shall
be left unhandled so it reaches the dialog (opening its close-confirm). The exact
motion/operator set is the contract in [`api-contract.md`](./api-contract.md) §5.

**UI-R-029** — The editor shall keep the vim block-cursor invariant in `Normal`
mode (the cursor rests on a character, clamping to the last column, not one past
it), while `Insert` mode allows the cursor one past the last character. `h`/`l`
shall not wrap across lines; arrow keys shall wrap to the adjacent line.

**UI-R-030** — Yank (`y`, `yy`) and delete (`d`, `dd`, `x`) shall write the
removed/copied text into an internal register (linewise or charwise) used by
paste (`p`/`P`), and shall additionally emit the text to the system clipboard via
an OSC 52 terminal escape as a best-effort side effect that never fails the edit.

**UI-R-031** — The editor shall provide single-level undo (`u`): each mutating
operation snapshots the buffer before applying, and `u` swaps the current buffer
with the snapshot (so pressing `u` again redoes). Motions and mode changes shall
not consume the undo slot.

**UI-R-032** — When a language is set, the editor shall auto-indent on newline and
on `o`/`O`: a new line inherits the current line's leading indentation adjusted by
the language's per-line block-balance delta (four spaces per level), floored at
zero. Without a language, new lines take no automatic indent.

**UI-R-033** — When a language is set and the field is enabled, losing focus shall
reformat the buffer through the language formatter; if the formatter declines
(e.g. syntactically invalid JSON), the buffer shall be left unchanged. A disabled
field shall never reformat, and gaining focus shall never reformat.

**UI-R-034** — In `Insert` mode, `Tab` shall insert four spaces and `Shift+Tab`
shall remove up to four leading spaces from the current line. In the plain editor,
two space presses at the same cursor position within a short bound (default
300 ms) shall expand to a four-space indent; an intervening key cancels the
pending expansion.

**UI-R-035** — All editor cursor movement, insertion, and deletion shall be
character-based, not byte-based, so multi-byte UTF-8 text is edited without
splitting or miscounting characters.

**UI-R-036** — A disabled code editor shall ignore all mutating keys (insert,
delete, paste, mode-entry that would edit) while still permitting navigation, and
shall report such keys as unhandled so they are free for higher layers.

## Syntax highlighting

**UI-R-037** — Syntax highlighting shall be provided as pure text-to-span
computation with no rendering: for a given language and one line of source (plus a
carry-over line state for multi-line constructs) it shall return a list of
`(start_char, end_char, kind)` spans, sorted by start and non-overlapping, using
character indices. Two languages shall be supported: Lua and JSON.

**UI-R-038** — The carry-over line state shall let multi-line constructs (Lua long
strings and long comments) be highlighted correctly across line boundaries when
lines are highlighted in order.

**UI-R-039** — The set of highlight kinds shall be a fixed enumeration
(keyword, identifier, number, string, comment, punctuation, JSON key, literal,
object identifier, function identifier); the consumer owns the mapping from kind
to concrete colors. Highlighting shall never mutate the source text.

## Tables, live updates & logging

**UI-R-040** — On every UI tick the application shall poll **all** tabs' views to
refresh their state — not only the active tab — so background modules keep
sending/receiving and their live values and logs stay current. Refreshes across
tabs shall be polled concurrently so tick latency is bounded by the slowest tab,
not their sum.

**UI-R-041** — The UI shall redraw both on input and on a periodic timeout
(≈100 ms) when no input arrives, so live register/state values and inbound traffic
update on screen without a keypress.

**UI-R-042** — A view may request to be replaced by a different view (e.g. an OCPP
role switch turning a client view into a server view); the application shall apply
the pending replacement on the next tick, carry over the tab's log channel and
focus, and rebuild the session-module registry.

**UI-R-043** — Each tab shall own a bounded ring log of timestamped, severity-
tagged lines (Info/Warning/Error). The on-screen log pane shall show the most
recent lines and auto-follow the tail unless the user has focused that tab's log
pane, in which case the scroll position shall be held for reading.

**UI-R-044** — A log line longer than the per-line cap shall be truncated to the
cap. A monotonic total-written counter shall be maintained so a consumer holding
only a bounded snapshot can compute how many lines are new since its last read
even across ring eviction.

**UI-R-045** — `:log clear` shall clear the active tab's on-screen log ring.
File-sink logging (`:log <file>`) is a module-forwarded command whose semantics
belong to the module's area; when a file sink is configured, buffered lines shall
be flushed to disk once per UI tick (and on sink teardown), not once per line.

**UI-R-046** — A table cell whose content exceeds the visible width shall be made
reachable by horizontal scroll tied to the selected column; the tab bar shall keep
the active tab visible by scrolling horizontally when the tabs overflow the width.
Live-updated cells shall be visually highlightable for a brief window after they
change.

## Widget & focus-derive contract

**UI-R-047** — Reusable widgets shall follow one event contract: each offered a
`(modifiers, code)` key event returns either *consumed* or *unhandled* (carrying
the original key back for the caller to handle). An unhandled result shall let the
key propagate to the enclosing layer; a consumed result shall stop propagation.

**UI-R-048** — A single-line text input shall support `Home`/`End`, `Left`/
`Right`, `Backspace`/`Delete`, printable insertion (including `Shift` for capitals
and symbols), `Ctrl+F` autofill from placeholder (only when empty), and `Ctrl+D`
clear. A focused input shall consume printable keys even when a per-field filter
rejects the character, so disallowed characters never leak to app-level shortcuts.
All editing shall be character-based (multi-byte safe).

**UI-R-049** — A view shall be composable as a focusable node: it exposes
set/query-focus and next/previous-focus stepping, so the tab that owns it can
treat "switch content↔log" as one focus step and toggle the whole tab's focus
recursively into whichever pane is active. A focusable container's focus cycle
shall skip fields whose enabling condition is currently false.

**UI-R-050** — The color scheme shall be a single compile-time constant selected
by build feature; the running application shall not switch color schemes at
runtime.
