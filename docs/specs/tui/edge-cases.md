# TUI — Edge Cases and Known Limitations

Boundary behavior, error semantics, intentional or known constraints. §6 is working as implemented; recorded so it is not "fixed".

---

## 1. Command line

| Condition | Behavior |
|---|---|
| Unknown first token (`:bogus`) | forwarded to the active view; if the view also does not recognize it, app logs `Unknown command ':bogus'` at Warning |
| Empty submission (`:` then `Enter`) | no-op; command mode exits |
| Extra whitespace between tokens | collapsed on split; `:  swap   0    1` = `:swap 0 1` |
| `:swap` with a non-numeric or missing index | rejected (parsed as unknown-`swap`); no swap |
| `:swap i j` with `i == j` or either out of range | silent no-op |
| `:script copy` with no index | error logged: `usage: :script copy <tab-index>` |
| `:script copy <idx>` out of range | error logged: `no tab [idx] (0..=max)` |
| `:script copy <active-index>` | error logged: `cannot copy from the active tab` |
| `:script copy <idx>` where source or active tab lacks script support | warning logged: `... has no script support` |
| `:quit` on the last tab | quits the application |
| `:log` bare, or `:log <path>` (path ≠ `clear`) | not app-level; forwarded to the view as a module command |
| Generic name shadows a module name | generic always wins (module commands reached only for unrecognized tokens) |

## 2. Navigation and tab jumps

| Condition | Behavior |
|---|---|
| `Ctrl+t` + digit uniquely indexing a tab (or `0`) | jumps immediately |
| `Ctrl+t` + first digit that could start a 2-digit index | waits up to 800 ms for a second digit |
| Second digit forms an out-of-range index | falls back to the first digit's tab |
| 800 ms elapses with no second digit | pending first-digit jump commits |
| Non-digit pressed while a first digit is pending | commits the pending jump, then processes the key |
| Jump to out-of-range or already-active index | silent no-op |
| Tab switch with 0 or 1 tabs | safe no-op |

## 3. Dialogs and overlays

| Condition | Behavior |
|---|---|
| `Esc` on a dialog with edits | close-confirm popup; `Enter`/`Space` confirms close, `Esc` returns to editing |
| Creating a tab whose name collides | refused; Warning to the active tab's log; setup dialog stays open |
| Startup new-module selector cancelled before any tab exists | application exits — zero tabs with no dialog is not a resting state (UI-R-057) |
| Rename/session-load produces a duplicate tab name | later duplicate auto-suffixed; Warning into the renamed tab's log |
| Focus cycle reaches a field whose enabling condition is false | skipped in `Tab`/`Shift+Tab` |
| `:` or `?` pressed while a view overlay is open | not global; delivered to the overlay (`?` types into a Lua editor, `:` into a text field) |
| Key with no binding in the current dialog/field | left unhandled; generic defaults (`Enter`/`Esc`/`Tab`) apply only if no widget consumed it, otherwise nothing |
| Suggestion popup closed, `Up`/`Down`/`Enter`/`Tab`/`Esc` pressed | passed through to the dialog |
| Inserting a template whose name is already used | inserted as `<name>-2` (then `-3`, …); never refused |
| Template browser preview pane | a disabled code editor: vim motions and visual-yank work, edits do not |
| `?` in the script dialog | focus decides: on the script table → keybind help; in the code editor's Normal mode → Lua-bindings help (Insert/Visual: literal text) |
| Renaming a script to an empty or duplicate name | refused silently; prompt stays open (same rule as creating) |
| Renaming a script to its current name | accepted; no-op |
| `Esc` while the rename prompt is open | cancels the prompt; does not reach the dialog's close-confirm |
| A rename is an edit | like any script edit, restarts the sim thread when the dialog closes (SC-R-024): the Lua context is keyed by script name |

## 4. Code editor

| Condition | Behavior |
|---|---|
| `Esc` in Normal mode | left unhandled → reaches the dialog, opens close-confirm (two `Esc` from Insert exit: first to Normal, second toward closing) |
| Unrecognized printable key in Normal mode (e.g. `z`, `q`) | consumed and ignored (no typing, no fall-through) |
| `?` in Insert or Visual mode | literal text; the Lua-bindings overlay opens only from Normal |
| Disabled editor | mutating keys ignored and reported unhandled; navigation works; never reformats on blur |
| Format-on-blur with invalid JSON | formatter declines; buffer left as typed |
| Format-on-blur with Lua | always reformats (never declines) |
| `h`/`l` at a line edge | do not wrap; arrows do wrap to the adjacent line |
| Multi-byte UTF-8 | edited character by character; cursor columns count characters, never bytes |
| `u` pressed twice | first undoes, second redoes (single-level) |
| `gg`/`dd`/`yy` first press | held pending; any non-matching key cancels the chord before doing its own action |
| Yank/delete with no clipboard-capable terminal | OSC 52 best-effort; failure ignored; internal register still holds the text |

## 5. Rendering and terminal size

| Condition | Behavior |
|---|---|
| Terminal resize | next tick re-lays out; content, log, command rows reflow |
| Very small terminal | no app-level minimum-size guard; content area squeezed, content clips. Popups skip drawing when their area is zero-sized |
| Log line longer than the per-line cap | truncated before storage |
| Table cell wider than the column | reachable via horizontal scroll tied to the selected column |
| Tabs overflow the bar width | tab bar scrolls horizontally to keep the active tab visible |
| No input for one redraw interval (~100 ms) | UI redraws anyway |

## 6. Known limitations and stated constraints

### 6.1 Single compile-time color scheme

Build-time feature-selected constant; no runtime theme switch. Changing themes requires rebuilding. Intentional.

### 6.2 Single-level undo only

Exactly one undo snapshot: `u` toggles between the current buffer and the last pre-edit state. No multi-step history, no separate redo stack.

### 6.3 Editor consumes unmapped Normal-mode keys

In Normal mode any printable key that is not a recognized motion/operator is consumed and discarded. Keeps stray keystrokes from leaking into the enclosing dialog, at the cost of silently swallowing them.

### 6.4 No minimum terminal size

No refusal or "terminal too small" message; lays out as best it can and lets content clip. Rendering stays panic-safe (zero-sized popups skipped); usability on a tiny terminal not guaranteed.

### 6.5 Protocol-command results depend on the view

A forwarded `:` command produces whatever `(level, message)` the view chooses; the TUI area does not standardize per-module result text or severities beyond requiring the level be chosen explicitly (never derived from text). The forwarded commands a view accepts are that module's contract, listed in its command-help popup.

### 6.6 OSC 52 clipboard is best-effort

Yank/delete emit an OSC 52 escape. Terminals without OSC 52 (or with it disabled) do not receive the copy; failure silent; in-app register still works for `p`/`P`. No fallback clipboard.

### 6.7 Command help lists a fixed generic set

The command-help popup lists a fixed generic set plus the active view's list. Generic aliases beyond those shown (`:q!`, `:save`, `:write`) are still accepted by the parser though the popup shows one spelling.

### 6.8 Module commands match on the exact first token

A forwarded command is recognized by its exact first whitespace-delimited token: `:setfoo` is unknown, not a malformed `:set`. Argument validation applies only after the token matches (`:set` alone still reports the usage warning).

### 6.9 Terminal-restore paths are not unit-tested

UI-R-001 requires terminal restore on normal exit, error exit, and from a panic hook. None of the three — `AlternateScreen`'s `Drop` impl (`ferrowl-ui/src/screen.rs`), `AlternateScreen::release()` from the error-exit branch (`ferrowl/src/main.rs`, after `app.run()` returns `Err`), or the same `release()` in the panic hook (`main.rs`, before `runtime.block_on`) — is exercised by an automated test. All three mutate the real terminal's raw-mode/alternate-screen state; doing so inside the test harness's process would corrupt its terminal (the panic-hook path also requires actually panicking), so this is left to manual verification (`cargo run -- --demo`, then exit normally, force an error exit, trigger a panic, checking the prompt is intact each time).
