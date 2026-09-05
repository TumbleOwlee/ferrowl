# TUI — Requirements

Application shell and tab model, focus model, keyboard navigation, `:` command line mechanism, modal dialog/overlay mechanism, in-TUI vim-modal Lua/JSON code editor, syntax highlighting, reusable widget set, live value/log rendering.

IDs stable, append-only (`UI-R-nnn`). See [`../README.md`](../README.md). Companions: [`api-contract.md`](./api-contract.md) (exhaustive `:` command list, every keybinding table, code-editor mode/command set), [`edge-cases.md`](./edge-cases.md).

**Area boundaries.** This area owns the *mechanism*: how `:` commands parse and dispatch, generic (app-level) commands, keybindings, vim + arrow navigation, dialogs as a mechanism, the code editor, syntax highlighting. It does **not** own protocol-specific command *semantics*: a command forwarded to a module view (Modbus `:set`, `:reload`, OCPP `:rfid`, `:start`/`:stop`) is *listed* here with general syntax; its effect on protocol state is `modbus/` or `ocpp/`. Which config fields a dialog exposes and their valid ranges belong to the protocol / `config-session/` areas; the dialog *mechanism* is owned here. The process command line (`ferrowl run`, CLI flags) is `cli-headless/`; only the in-TUI `:` line is owned here.

---

## App shell, tabs & focus model

**UI-R-001** — The application presents a full-screen terminal UI in the alternate screen buffer with raw mode enabled, and restores the terminal (leave alternate screen, disable raw mode) on normal exit, on the error exit path, and from a panic hook.

**UI-R-002** — Screen layout top-to-bottom: one-row tab bar, flexible module content area, fixed-height log pane, one-row command line. The content area absorbs remaining height.

**UI-R-003** — The application owns an ordered list of tabs and one active index. Each tab pairs one module content view with its own log pane. Exactly one tab is active and rendered; the others keep running in the background (UI-R-030).

**UI-R-004** — Every tab has a unique display name.

**UI-R-069** — When an operation (in-dialog rename, session load) would make two tabs share a display name (UI-R-004), the later duplicate(s) are auto-suffixed.

**UI-R-070** — When a tab's display name is auto-suffixed (UI-R-069), a warning is logged into the renamed tab's own log.

**UI-R-005** — Input is routed by a single modal layer selector with precedence: keybind-help dialog (topmost) → app-level creation/session dialog → active tab's open overlay → command line → active tab's content/log panes.

**UI-R-071** — An open modal layer (UI-R-005) consumes the keys its lower layers would otherwise receive.

**UI-R-006** — Keyboard focus within the active tab is the content view or the log pane, never both.

**UI-R-072** — The `:` command line and any dialog remove keyboard focus from the active tab's panes (UI-R-006) while open and restore it on close.

**UI-R-073** — Every keyboard-focus transition within the active tab (UI-R-006) routes through a single choke point.

**UI-R-007** — Only key **press** events are acted upon; release/repeat kinds and non-key terminal events are ignored for command/navigation.

**UI-R-008** — Starting with no tabs configured opens the new-module type selector immediately.

**UI-R-057** — Whenever the application holds zero tabs and no modal layer is open (no app-level creation/type-select overlay, session dialog, or keybind-help dialog), it exits through the normal terminal-restoring path (UI-R-001). Cancelling the startup selector (UI-R-008) before any tab exists therefore quits. Independent of `:quit`/`:qall` (UI-R-019).

## Navigation & tab switching

**UI-R-009** — `Ctrl+w` begins a window-switch chord; a following `j`, `k`, `Down`, or `Up` toggles focus between the active tab's content view and log pane.

**UI-R-010** — `Ctrl+t` begins a tab-switch chord: `l` next tab, `h` previous (both wrap), a digit begins a by-index jump.

**UI-R-011** — A `Ctrl+t` digit jump: if the first digit already uniquely identifies a tab (no in-range two-digit index starts with it, or it is `0`), jump immediately.

**UI-R-074** — A `Ctrl+t` first digit that does not uniquely identify a tab (UI-R-011) waits up to 800 ms for a second digit; a second digit forming an in-range two-digit index jumps there.

**UI-R-075** — A second digit after a pending `Ctrl+t` first digit (UI-R-074) that forms an out-of-range two-digit combination falls back to the first digit's tab.

**UI-R-076** — A `Ctrl+t` second-digit wait (UI-R-074) that times out with no second digit commits the pending jump.

**UI-R-077** — Any non-digit while a `Ctrl+t` first digit is pending (UI-R-074) commits that jump and is then processed normally.

**UI-R-012** — A jump to an out-of-range or already-active index is a silent no-op. Tab-switch operations are safe with zero or one tabs.

**UI-R-013** — In a focused table or selection list, `j`/`Down` and `k`/`Up` move the row selection; `h`/`Left` and `l`/`Right` move the column selection (tables) or item (horizontal selection); `g` first row, `G` last, `0`/`Home` first column, `$`/`End` last. Selection clamps at the ends.

## Command line mechanism

**UI-R-014** — `:` while the content panes are focused and no view overlay is open enters command mode: focus moves to the command line, buffer cleared, printable keys type into it. With a view overlay open, `:` types into the overlay.

**UI-R-015** — In command mode, `Esc` cancels (discard buffer, restore content focus); `Enter` submits the trimmed buffer and restores content focus. Empty submission is a no-op.

**UI-R-016** — The command line is parsed by a pure, state-independent parser into a fixed set of app-level commands ([`api-contract.md`](./api-contract.md) ``## Generic `:` commands (app-level)``); leading/trailing and inter-token whitespace collapsed. Any first token not recognized at the app level is forwarded verbatim to the active view.

**UI-R-017** — App-level commands are dispatched by the application: tab lifecycle (`:quit`, `:qall`, `:new`, `:load`), session persistence (`:write`), tab reordering (`:swap`), session-script management (`:session`, `:script copy`), log-ring clear (`:log clear`). Exact syntax and aliases: [`api-contract.md`](./api-contract.md).

**UI-R-018** — A command not handled at the app level is forwarded to the active tab's view. If handled, any `(level, message)` returned is appended to the tab's log; if unhandled, the application logs `Unknown command ':<input>'` at Warning. The level of a result message is chosen by the producer, never re-derived from message text.

**UI-R-019** — `:quit` closes the active tab, stopping its module first, and quits the application only when it was the last tab. `:qall` quits immediately regardless of tab count.

**UI-R-020** — While the command line is focused, a help popup lists available commands: generic app-level commands plus whatever the active view advertises for its module type.

## Dialogs & overlays mechanism

**UI-R-021** — A dialog/overlay is a modal layer rendered over the content and log panes, consuming keyboard input while open. Overlays paint back-to-front (module overlays, command help popup, app-level dialog, keybind-help dialog on top).

**UI-R-022** — Within a dialog, `Tab` advances focus to the next field and `Shift+Tab`/`BackTab` retreats, cycling.

**UI-R-078** — A dialog's `Tab`/`Shift+Tab` focus cycle (UI-R-022) skips fields whose enabling condition is false.

**UI-R-079** — Within a dialog, `Enter` confirms; `Esc` requests close.

**UI-R-080** — The dialog key defaults (UI-R-022, UI-R-078, UI-R-079) apply only when the focused widget did not consume the key.

**UI-R-067** — A setup dialog opens with exactly one field focused, the first in its `Tab` cycle (UI-R-022), and its focus cursor names that same field. Every other field opens unfocused, nested sections included. Where the focused field is a text input, it is the dialog's only field painting a text cursor.

**UI-R-068** — A single-line input's border is styled by validation first and focus second: text failing validation paints the error style focused or not; only a field whose text validates, or has no validator, paints the focused style when focused and the normal border otherwise. A disabled single-line input never paints the focused style.

**UI-R-110** — The multi-line code editor styles its border from focus only and keeps the focused border while disabled, since a read-only viewer still holds focus for scrolling.

**UI-R-111** — As a consequence of UI-R-068, a fresh setup dialog shows a focused field with an error border (empty name input against a non-empty requirement); a dialog opened by `:edit` prefills the name and shows the focused border.

**UI-R-023** — `Esc` on a dialog that may hold unsaved edits opens a close-confirmation popup; confirming (`Enter` or `Space`) closes, dismissing (`Esc`) returns to editing. A yes/no box defaults focus to the safe (cancel) choice.

**UI-R-024** — The new-module flow is two staged overlays: module-type selector, then the chosen type's setup dialog. Confirming the selector swaps in the setup dialog; confirming a valid setup dialog creates and starts the tab. A dialog failing validation stays open.

**UI-R-025** — Creating a tab whose name collides with an existing tab is refused with a warning in the active tab's log, dialog left open.

**UI-R-026** — A field-completion popup (suggestion input), while open, consumes `Up`/`Down` to move the highlight, `Enter` to accept, `Esc` to dismiss. While closed, `Up`/`Down`/`Enter`/`Esc` pass through to the dialog.

**UI-R-081** — `Tab` is never consumed by a field-completion popup (UI-R-026), so it always moves focus to the dialog's next field.

**UI-R-082** — Accepting a field-completion popup suggestion (UI-R-026) marked *partial* keeps the popup open and re-queries (e.g. descending a directory); accepting a non-partial one closes it.

## Script-manager dialog

**UI-R-051** — In the script-manager dialog, while the script table is focused, `e` executes the selected script exactly once, using the script's current editor content (including unapplied edits) regardless of its enabled flag. No selection → no-op. Execution semantics: SC-R-035.

**UI-R-088** — An on-demand script run (UI-R-051) leaves the script-manager dialog open; the run's `print`/`C_Log` output and any error appear in the dialog's log pane.

**UI-R-052** — The script-manager dialog carries a *Templates* button in its focus cycle, after the new-script name input. `Enter` or `Space` on it opens the template-browser overlay.

**UI-R-053** — The template-browser overlay lists only templates applicable to the dialog's script context (SC-R-036), each with name and description, plus a read-only syntax-highlighted preview of the selected template's code. `Esc` or `q` closes without changing the script list. While open it takes precedence over all other dialog keys.

**UI-R-054** — Confirming a template appends it to the dialog's working script list as a new enabled script whose code copies the template body, selects it, closes the overlay, leaves the dialog open. The script takes the template's name; if taken, the first free `<name>-<n>` (n from 2); insertion is never refused for a name collision.

**UI-R-055** — In the script-manager dialog, while the script table is focused, `Enter` on a selected script opens a rename prompt pre-filled with its name. `Enter` renames; `Esc` dismisses unchanged. Renaming changes only the name.

**UI-R-089** — In the script rename prompt (UI-R-055), an empty (after trimming) or already-used name is refused and the prompt stays open.

**UI-R-090** — In the script-manager dialog, `Enter` on the script table with no selection (UI-R-055) is a no-op.

**UI-R-056** — In the script-manager dialog, while the script table is focused, `?` opens a keybind-help overlay listing the table's bindings (rename, run once, toggle enabled, delete, compact) with a one-line description each. `Esc`, `q`, or `?` closes. While open it takes precedence over all other dialog keys. The table's title advertises only this overlay.

**UI-R-058** — In the script-manager dialog, while the script table is focused, the table supports editing its working list: a non-empty name in the new-script input plus confirm adds a new enabled, empty script (empty or already-used name refused, per UI-R-089).

**UI-R-091** — In the script-manager dialog, while the script table is focused, `t` toggles the selected script enabled/disabled in the table's working list (UI-R-058); `t` is a no-op with no selection.

**UI-R-092** — In the script-manager dialog, while the script table is focused, `d` deletes the selected script from the table's working list (UI-R-058) after a yes/no confirmation (UI-R-023); `d` is a no-op with no selection.

**UI-R-093** — In the script-manager dialog, while the script table is focused, `c` toggles compact/normal rows.

**UI-R-094** — Script-table edits (UI-R-058, UI-R-091, UI-R-092) change the working list only and take effect on the owner when the dialog is applied (SC-R-024); UI-R-058, UI-R-091, UI-R-092 and UI-R-093 are the bindings advertised by UI-R-056.

## OCPP setup dialog

**UI-R-059** — In the OCPP setup dialog, a client-role instance shows two always-visible new-header inputs (name, value) backing `extra_headers`, plus the headers table once the list is non-empty; both hidden for the server role (the `#[focus(when = …)]` role-conditional pattern).

**UI-R-095** — In the OCPP setup dialog, while the headers table (UI-R-059) is focused, a name and value in the new-header inputs plus confirm adds a row, refused inline if name/value fails OC-R-118's grammar or OC-R-153's reserved-name check.

**UI-R-096** — In the OCPP setup dialog, while the headers table (UI-R-059) is focused, `Enter` on a selected row opens an edit prompt pre-filled with its name and value, UI-R-095's validation applied on confirm, `Esc` dismissing unchanged; `Enter` is a no-op with no selection.

**UI-R-097** — In the OCPP setup dialog, while the headers table (UI-R-059) is focused, `d` deletes the selected row after a yes/no confirmation (UI-R-023); `d` is a no-op with no selection.

**UI-R-098** — Rows of the OCPP setup dialog's headers table (UI-R-059) keep insertion order (OC-R-117).

**UI-R-099** — Edits to the OCPP setup dialog's headers table (UI-R-095, UI-R-096, UI-R-097) change the working list only and take effect when the dialog is applied.

## Code editor (vim-modal)

**UI-R-027** — The multi-line code editor supports two profiles: plain single-mode (printable keys insert, `Enter` splits, `Backspace`/`Delete` edit, arrows navigate with wrap) and vim-modal (`Normal`/`Insert`/`Visual`). Vim-modal is default for the Lua-script editor.

**UI-R-028** — Vim-modal: `Normal` provides motions and operators; `i`/`a`/`I`/`A`/`o`/`O` enter `Insert` at the documented position; `v`/`V` enter charwise/linewise `Visual`; `Esc` from `Insert` or `Visual` returns to `Normal`; `Esc` in `Normal` is left unhandled so it reaches the dialog. Exact motion/operator set: [`api-contract.md`](./api-contract.md) `## Code editor — modes and commands`.

**UI-R-029** — The editor keeps the vim block-cursor invariant in `Normal` (cursor rests on a character, clamping to the last column); `Insert` allows one past the last character. `h`/`l` do not wrap across lines; arrows wrap to the adjacent line.

**UI-R-030** — Yank (`y`, `yy`) and delete (`d`, `dd`, `x`) write the removed/copied text into an internal register (linewise or charwise) used by paste (`p`/`P`).

**UI-R-083** — Yank and delete (UI-R-030) also emit the removed/copied text to the system clipboard via an OSC 52 escape, best-effort, never failing the edit.

**UI-R-031** — Single-level undo (`u`): each mutating operation snapshots the buffer before applying; `u` swaps current buffer with the snapshot (pressing again redoes). Motions and mode changes do not consume the slot.

**UI-R-032** — With a language set, the editor auto-indents on newline and `o`/`O`: the new line inherits the current line's leading indentation adjusted by the language's per-line block-balance delta (four spaces per level), floored at zero. Without a language, no automatic indent.

**UI-R-033** — With a language set and the field enabled, losing focus reformats the buffer through the language formatter; if the formatter declines (e.g. invalid JSON), the buffer is unchanged. A disabled field never reformats; gaining focus never reformats.

**UI-R-034** — In `Insert`, `Tab` inserts four spaces; `Shift+Tab` removes up to four leading spaces from the current line.

**UI-R-084** — In the plain editor, two space presses at the same cursor position within a short bound (default 300 ms) expand to a four-space indent; an intervening key cancels.

**UI-R-035** — All cursor movement, insertion, deletion are character-based, not byte-based, so multi-byte UTF-8 is never split or miscounted.

**UI-R-036** — A disabled code editor ignores all mutating keys (insert, delete, paste, mode entry that would edit) while permitting navigation, and reports such keys unhandled.

## Markdown input field

**UI-R-125** — The markdown input field is a multi-line widget composing the vim-modal code-editor state (UI-R-027 through UI-R-036): buffer, `Normal`/`Insert`/`Visual` modes, motions and operators, registers, single-level undo and the disabled flag, which the markdown widget surfaces as read-only.

**UI-R-126** — Focused, editable, in `Normal` mode: every source line is drawn in its rendered form except the line holding the cursor, which is drawn as styled source, revealing that line's markup for editing.

**UI-R-127** — Focused, editable, in `Insert` or `Visual` mode: every source line is drawn as styled source, with no rendered lines.

**UI-R-128** — Unfocused, or read-only in any state, every source line is drawn in its rendered form, including the cursor line; no line reveals its source.

**UI-R-129** — The styled-source view of UI-R-126 and UI-R-127 is produced by the Markdown language of UI-R-037: source characters are drawn unchanged, styled by highlight span.

**UI-R-130** — Line wrapping is always on and cannot be disabled: content never overflows the widget width horizontally and the widget never scrolls horizontally.

**UI-R-131** — A line too long for the available width breaks at a word boundary; a single word longer than the available width breaks at a character boundary instead.

**UI-R-132** — Continuation rows of a wrapped list item or block quote are indented to the content start of that line (the column after its marker), so wrapped text aligns under the first row's text.

**UI-R-133** — A fence delimiter or fence-body line wraps at a character boundary with no hanging indent.

**UI-R-134** — `j`, `k`, their count prefixes, `dd`, `yy` and gutter line numbers address source lines, not display rows: `j` from a wrapped line moves to the next source line.

**UI-R-135** — `gj` and `gk` move the cursor one display row down or up, staying inside a wrapped source line when it spans several rows.

**UI-R-136** — `Ctrl+D` and `Ctrl+U` scroll by half the visible height measured in display rows and move the cursor by the same number of display rows.

**UI-R-137** — Scrolling is measured in display rows and the viewport always keeps the cursor's display row visible.

**UI-R-138** — Read-only, the display row range of the cursor's source line is drawn in the theme's highlighted-row style, so line navigation is visible without a text cursor.

**UI-R-139** — Read-only, `j`, `k`, their count prefixes, `yy` (with its count prefix), `gg`, `G`, `Ctrl+D` and `Ctrl+U` remain available, so a reader can navigate lines and display rows and yank them.

**UI-R-140** — An optional line-number gutter, off by default and selected on the widget builder, prints the source line number on the first display row of each source line and leaves continuation rows blank.

**UI-R-141** — Markdown rendering styles come from a markdown theme with compile-time defaults, injected on the widget builder like the syntax theme, holding per-level heading styles, per-level quote-bar styles, and link, inline-code, bullet, horizontal-rule, image and read-only highlighted-row styles.

**UI-R-142** — Rendering is line-preserving: one source line produces exactly one rendered line, wrapped over one or more display rows; lines are never joined into paragraphs and no construct is laid out across lines.

**UI-R-143** — A heading line hides its `#` markers and the space after them and draws the remaining text bold in the theme's color for that heading level.

**UI-R-144** — An unordered list item replaces its `-`, `*` or `+` marker with `•` in the theme's bullet style, keeping the leading indentation that expresses nesting depth.

**UI-R-145** — An ordered list item keeps its number and delimiter as written, unchanged.

**UI-R-146** — A task item replaces `- [ ]` with `☐` and `- [x]` with `☑`, keeping the item's text.

**UI-R-147** — A block-quote line replaces each `>` marker with a `▎` bar in the theme's quote-bar color for that nesting depth and draws the quoted text dimmed and italic.

**UI-R-148** — A horizontal rule (`---` or `***`) is drawn as a rule line spanning the full text width, after the gutter when enabled, in the theme's rule style.

**UI-R-149** — A fence delimiter line hides its backticks and info string, rendering as an empty line in the theme's code style, so the one-line-per-source-line invariant of UI-R-142 holds.

**UI-R-150** — A fence-body line is drawn verbatim, character for character, in the theme's code style.

**UI-R-151** — When the fence's info string is `lua` or `json`, its body lines are additionally highlighted through the syntax highlighter for that language (UI-R-037) with the carry-over line state threaded across the block; any other info string, or none, leaves the body in plain code style.

**UI-R-152** — Inline `**bold**`, `*italic*`/`_italic_`, `` `code` `` and `~~strike~~` hide their markers and draw the content bold, italic, in the theme's code style, and crossed out respectively.

**UI-R-153** — `[text](url)` renders as `text` in the theme's link style, underlined; the brackets, parentheses and URL are hidden.

**UI-R-154** — `![alt](url)` renders as `alt` in the theme's image style; the `!`, brackets, parentheses and URL are hidden.

**UI-R-155** — A read-only markdown input field ignores every mutating key and every mode-entry key, reporting them unhandled as the disabled code editor does (UI-R-036), so the widget never leaves `Normal` mode.

## Syntax highlighting

**UI-R-037** — Syntax highlighting is pure text-to-span computation: for a language and one line of source (plus a carry-over line state for multi-line constructs) it returns `(start_char, end_char, kind)` spans, sorted by start, non-overlapping, character indices. Three languages: Lua, JSON and Markdown.

**UI-R-038** — The carry-over state lets multi-line constructs (Lua long strings and long comments) highlight correctly across lines when highlighted in order.

**UI-R-039** — Highlight kinds are a fixed enumeration (keyword, identifier, number, string, comment, punctuation, JSON key, literal, object identifier, function identifier); the consumer maps kind to colors. Highlighting never mutates the source.

**UI-R-120** — The Markdown language exposes, beside the span path of UI-R-037, a per-line block-model entry point: given one source line and a carried state it returns that line's block kind — paragraph, heading with level 1–6, unordered list item with nesting depth and marker character, ordered list item with nesting depth, task item with checked state, block quote with nesting depth, horizontal rule, fence delimiter with info string, or fence body — together with the inline spans of UI-R-122.

**UI-R-121** — The block-model carried state of UI-R-120 records whether the parser is inside a fenced code block and that fence's info string; while inside a fence every line is classified as fence body regardless of its own content, until a matching closing fence delimiter line.

**UI-R-122** — Inline parsing reports, over source character columns of one line, spans for `**bold**`, `*italic*`, `_italic_`, `` `code` ``, `~~strike~~`, `[text](url)` and `![alt](url)`, each distinguishing its marker columns (delimiters, brackets, URL) from its content columns, so a consumer can hide markers without recomputing positions.

**UI-R-123** — A character preceded by a backslash is literal: the backslash is a marker column and the escaped character never opens or closes an inline construct.

**UI-R-124** — Constructs the block model does not recognize — tables, raw HTML, footnotes, reference links, setext headings, autolinks — are reported as paragraph text with no inline spans over them.

## Tables, live updates & logging

**UI-R-040** — On every UI tick the application polls **all** tabs' views to refresh state, not only the active, so background modules keep sending/receiving and their values and logs stay current. Refreshes are polled concurrently, so tick latency is bounded by the slowest tab, not their sum.

**UI-R-041** — The UI redraws on input and on a periodic timeout (≈100 ms) with no input.

**UI-R-042** — A view may request replacement by a different view (e.g. an OCPP role switch); the application applies it on the next tick, carries over the tab's log channel and focus, rebuilds the session-module registry.

**UI-R-043** — Each tab owns a bounded ring log of timestamped, severity-tagged lines (Info/Warning/Error). The log pane shows the most recent lines and auto-follows the tail unless the user has focused that tab's log pane, in which case scroll position holds.

**UI-R-044** — A log line longer than the per-line cap is truncated to it. A monotonic total-written counter lets a consumer holding only a bounded snapshot compute how many lines are new since its last read, across ring eviction.

**UI-R-045** — `:log clear` clears the active tab's on-screen ring.

**UI-R-085** — File-sink logging (`:log <file>`) is module-forwarded, semantics in the module's area; with a file sink configured, buffered lines flush to disk once per UI tick (and on sink teardown), not per line.

**UI-R-046** — A table cell wider than the visible width is reachable by horizontal scroll tied to the selected column; the tab bar keeps the active tab visible by scrolling horizontally on overflow. Live-updated cells can be highlighted briefly after they change.

## Widget & focus-derive contract

**UI-R-047** — Reusable widgets follow one event contract: offered a `(modifiers, code)` key event, each returns *consumed* or *unhandled* (carrying the original key back). Unhandled propagates to the enclosing layer; consumed stops propagation.

**UI-R-048** — A single-line text input supports `Home`/`End`, `Left`/`Right`, `Backspace`/`Delete`, printable insertion (including `Shift` for capitals and symbols), `Ctrl+F` autofill from placeholder (only when empty), `Ctrl+D` clear.

**UI-R-086** — A focused single-line text input (UI-R-048) consumes printable keys even when a per-field filter rejects the character, so disallowed characters never leak to app-level shortcuts.

**UI-R-087** — Editing in a single-line text input (UI-R-048) is character-based.

**UI-R-049** — A view is composable as a focusable node: set/query focus and next/previous focus stepping, so the owning tab treats "switch content↔log" as one focus step and toggles the whole tab's focus recursively into whichever pane is active. A focusable container's cycle skips fields whose enabling condition is false.

**UI-R-050** — The color scheme is a single compile-time constant selected by build feature; no runtime switch.

## Modbus monitor view

**UI-R-060** — A Modbus monitor module's content view has a left panel listing every unit id observed (updated live) and, on the right, sections scoped to the selected unit id: a message table (MB-R-146 records), a memory layout (MB-R-144's observed-value table, grouped by table kind), and a resolved-registers table (MB-R-145 interpretations applied to that unit id's memory).

**UI-R-100** — The Modbus monitor content view's resolved-registers table (UI-R-060) is hidden entirely when no interpretation exists for the selected unit id.

**UI-R-101** — A Modbus monitor module's own log occupies the bottom log pane of its tab per UI-R-003 (UI-R-060).

**UI-R-061** — On a monitor view, `:add`/`:a` opens a dialog to add a register interpretation (MB-R-145) scoped to the selected unit id, mirroring the client/server `:add` dialog (`api-contract.md`). The resolved-registers table reflects it immediately. With no unit id discovered yet, `:add` is rejected with a Warning-level message instead of opening.

**UI-R-062** — The message table (UI-R-060) renders one row per MB-R-146 record for the selected unit id, most recent first, columns Time, Status, Slave, Operation, Address, Quantity, Values/Payload.

**UI-R-102** — The message table's Values/Payload column (UI-R-062) renders `[XXXX XXXX ...]` (4-digit lowercase hex per 16-bit word, space-separated, matching the modbus module's raw-value formatting) for register-shaped operations, `[0 1 0 ...]` (one digit per bit) for coil/discrete-input-shaped, empty for any operation MB-R-146 carries no address/quantity/value for.

**UI-R-103** — Horizontal overflow of the message table (UI-R-062) scrolls rather than truncating or wrapping.

**UI-R-063** — The memory layout panel (UI-R-060) renders each of MB-R-144's populated table kinds for the selected unit id as an independent hex-editor-style block. Holding/Input Registers address by register, 8 per line; Coils/Discrete Inputs pack 8 bits per byte (MSB-first), address by byte, 16 per line.

**UI-R-104** — Each memory layout line (UI-R-063) shows, in order: table kind (the modbus module's `Kind` naming), starting address, hex byte/word values, character representation.

**UI-R-105** — A memory layout line (UI-R-063) with no observed value in its range is omitted; within a rendered line, an unobserved byte/word renders as a dim placeholder (`··`), not a decoded zero.

**UI-R-106** — Each memory layout byte/word (UI-R-063) is colorized by value class (zero in the placeholder color, printable ASCII in normal text color, other non-zero in a distinguishing highlight).

**UI-R-107** — Independently of its value-class color (UI-R-106), a memory layout byte/word is painted with the change-highlight color while an MB-R-147 recency marker is active for its address.

**UI-R-064** — The resolved-registers table (UI-R-060, MB-R-145) renders columns Name, Description, Address, Kind, Format, Length, Resolution, Value, Raw Value: the modbus module's register table's set (`ferrowl::module::modbus::table::TableHeader`) less Slave ID and Access.

**UI-R-108** — Enter on a selected resolved-registers table row (UI-R-064) opens an edit dialog prefilled from that interpretation, offering Confirm (MB-R-148 edit) and Delete (MB-R-148 remove), mirroring the modbus edit-register dialog.

**UI-R-065** — Tab/Shift+Tab on a monitor content view cycles focus across panels: Units, Messages, Memory layout, Resolved registers, back to Units, skipping Resolved registers whenever hidden (UI-R-100). Each panel keeps its own selection/scroll position; Up/Down/Left/Right and Enter act on the focused panel.

**UI-R-066** — The shared `Table` widget's optional selection-marker gutter reserves zero width whenever no row is selected, otherwise exactly the highlight symbol's rendered width, for every table regardless of whether the marker is shown.

**UI-R-109** — A `Table` widget's selected row's background is painted with its highlight style in every case except an unfocused table with the selection marker (UI-R-066) shown, where the marker glyph alone is the cue.

**UI-R-112** — `Esc` on a Modbus monitor view overlay — the monitor setup-edit dialog, the add-interpretation dialog (UI-R-061), the edit-interpretation dialog (UI-R-108) — opens that overlay's own close-confirmation popup per UI-R-023 and never closes the overlay directly.

**UI-R-113** — The add-interpretation (UI-R-061) and edit-interpretation (UI-R-108) dialogs each carry the close-confirmation popup state UI-R-023 defines, so UI-R-112 has a popup to open in every monitor overlay.
