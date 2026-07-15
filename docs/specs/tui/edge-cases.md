# TUI — Edge Cases and Known Limitations

Boundary behavior, error semantics, and the constraints that are **intentional**
or **known**. Everything in §6 is working as implemented; it is recorded here so
it is not mistaken for an oversight and silently "fixed".

---

## 1. Command line

| Condition | Behavior |
|---|---|
| Unknown first token (`:bogus`) | Forwarded to the active view; if the view also does not recognize it, the app logs `Unknown command ':bogus'` at Warning level |
| Empty submission (`:` then `Enter`) | No-op; command mode exits |
| Extra whitespace between tokens | Collapsed on split; `:  swap   0    1` parses the same as `:swap 0 1` |
| `:swap` with a non-numeric or missing index | Rejected (parsed as unknown-`swap`); no swap performed |
| `:swap i j` with `i == j` or either index out of range | Silent no-op |
| `:script copy` with no index | Error logged: `usage: :script copy <tab-index>` |
| `:script copy <idx>` where `idx` is out of range | Error logged: `no tab [idx] (0..=max)` |
| `:script copy <active-index>` | Error logged: `cannot copy from the active tab` |
| `:script copy <idx>` where source or active tab lacks script support | Warning logged: `... has no script support` |
| `:quit` on the last remaining tab | Quits the whole application |
| `:log` bare, or `:log <path>` (path ≠ `clear`) | Not handled at app level; forwarded to the view as a module command |
| A generic name shadows a module name | The generic app-level command always wins (module commands are only reached for tokens the app does not recognize) |

## 2. Navigation and tab jumps

| Condition | Behavior |
|---|---|
| `Ctrl+t` + digit, digit indexes a tab uniquely (or is `0`) | Jumps immediately, no wait |
| `Ctrl+t` + first digit that could start a 2-digit index | Waits up to 800 ms for a second digit |
| Second digit forms an out-of-range index | Falls back to jumping to the tab named by the first digit |
| 800 ms elapses with no second digit | The pending first-digit jump commits |
| Any non-digit pressed while a first digit is pending | Commits the pending jump, then processes the key normally |
| Jump to an out-of-range or already-active index | Silent no-op |
| Tab switch with 0 or 1 tabs | Safe no-op |

## 3. Dialogs and overlays

| Condition | Behavior |
|---|---|
| `Esc` on a dialog with edits | Opens a close-confirm popup rather than discarding; `Enter`/`Space` confirms close, `Esc` returns to editing |
| Creating a tab whose name collides with an existing tab | Refused; a Warning is logged to the active tab's log and the setup dialog stays open |
| Startup new-module selector cancelled before any tab is created | The application exits — zero tabs with no dialog open is not a reachable resting state (UI-R-057) |
| A rename/session-load produces a duplicate tab name | The later duplicate is auto-suffixed and a Warning is logged into the renamed tab's own log |
| Dialog focus cycle reaches a field whose enabling condition is false | That field is skipped in the `Tab`/`Shift+Tab` cycle |
| `:` or `?` pressed while a view overlay is open | Not treated as global; the key is delivered to the overlay (so `?` types into a Lua editor, `:` into a text field) |
| A key with no binding in the current dialog/field | Left unhandled; the generic dialog defaults (`Enter`/`Esc`/`Tab`) apply only if no widget consumed it, otherwise nothing happens |
| Suggestion popup closed, `Up`/`Down`/`Enter`/`Tab`/`Esc` pressed | Passed through to the surrounding dialog rather than consumed by the popup |
| Inserting a template whose name is already used in the script list | Inserted as `<name>-2` (then `-3`, …); never refused |
| Preview pane of the template browser | A disabled code editor: vim motions and visual-yank work, edits do not |
| `?` pressed in the script dialog | Focus decides the overlay: on the script table it opens the keybind help, in the code editor's Normal mode the Lua-bindings help (in Insert/Visual it stays literal text) |
| Renaming a script to an empty or duplicate name | Refused silently; the prompt stays open (same rule as creating a script) |
| Renaming a script to its own current name | Accepted; a no-op |
| `Esc` while the rename prompt is open | Cancels the prompt; it does not reach the dialog's close-confirm |
| A rename is an edit | Like any script edit, it restarts the sim thread when the dialog closes (SC-R-024): the Lua context is keyed by script name |

## 4. Code editor

| Condition | Behavior |
|---|---|
| `Esc` in Normal mode | Left unhandled → reaches the dialog and opens its close-confirm (so two `Esc` from Insert exits: first to Normal, second toward closing) |
| An unrecognized printable key in Normal mode (e.g. `z`, `q`) | Consumed and ignored (does not type, does not fall through) |
| `?` in Insert or Visual mode | Literal text; the Lua-bindings overlay only opens from Normal mode |
| Disabled editor | All mutating keys ignored and reported unhandled; navigation still works; never reformats on blur |
| Format-on-blur with invalid JSON | Formatter declines; the buffer is left exactly as typed |
| Format-on-blur with Lua | Always reformats (Lua formatting never declines) |
| `h`/`l` at a line edge | Do not wrap (stay on the line); arrow keys at an edge do wrap to the adjacent line |
| Multi-byte UTF-8 text | Edited character-by-character; cursor columns count characters, never bytes |
| `u` pressed twice | First undoes, second redoes (single-level history) |
| `gg`/`dd`/`yy` first press | Held as pending; any non-matching key cancels the pending chord before doing its own action |
| Yank/delete with no clipboard-capable terminal | OSC 52 write is best-effort; failure is ignored and the internal register still holds the text |

## 5. Rendering and terminal size

| Condition | Behavior |
|---|---|
| Terminal resize | The next tick re-lays out to the new size; content, log, and command rows reflow |
| Very small terminal | There is no explicit minimum-size guard at the app level; the flexible content area is squeezed and content clips. Individual popups skip drawing when their area is zero-sized |
| A log line longer than the per-line cap | Truncated to the cap before storage |
| A table cell wider than the column | Reachable via horizontal scroll tied to the selected column |
| Tabs overflow the bar width | The tab bar scrolls horizontally to keep the active tab visible |
| No input for one redraw interval (~100 ms) | The UI redraws anyway so live values/traffic update without a keypress |

## 6. Known limitations and stated constraints

### 6.1 Single compile-time color scheme

The color scheme is a build-time feature-selected constant; there is no runtime
theme switch. Changing themes requires rebuilding with a different feature. This
is intentional, not a missing setting.

### 6.2 Single-level undo only

The code editor keeps exactly one undo snapshot: `u` toggles between the current
buffer and the last pre-edit state. There is no multi-step undo history and no
separate redo stack. Deep edit histories are out of scope by design.

### 6.3 Editor consumes unmapped Normal-mode keys

In vim Normal mode, any printable key that is not a recognized motion/operator is
consumed and discarded rather than passed through. This keeps stray keystrokes
from leaking into the enclosing dialog while the editor is focused, at the cost of
those keys being silently swallowed.

### 6.4 No minimum terminal size

The application does not refuse to run or show a "terminal too small" message on a
tiny terminal; it lays out as best it can and lets content clip. Rendering stays
panic-safe (zero-sized popups are skipped), but usability on a very small terminal
is not guaranteed.

### 6.5 Protocol-command results depend on the view

A `:` command forwarded to a view produces whatever `(level, message)` that view
chooses; the TUI area does not standardize per-module result text or severities
beyond requiring the level be chosen explicitly (never re-derived from the
message text). The set of forwarded commands a view accepts is that module's
contract, listed in its command-help popup.

### 6.6 OSC 52 clipboard is best-effort

Yank/delete emit an OSC 52 escape to the terminal to populate the system
clipboard. Terminals that do not support OSC 52 (or have it disabled) simply do
not receive the copy; the failure is silent and the in-app register still works
for `p`/`P`. There is no fallback clipboard mechanism.

### 6.7 Command help lists a fixed generic set

The command-help popup shown while typing `:` lists a fixed set of generic
commands plus the active view's advertised list. Generic aliases beyond those
shown (e.g. `:q!`, `:save`, `:write`) are still accepted by the parser even
though the popup shows only one representative spelling.
