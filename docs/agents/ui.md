# UI domain

Crates: `ferrowl-ui`, `ferrowl-ui-derive`, `ferrowl-syntax`. App-level: `ferrowl/src` (tabs, views, dialogs, `:` commands, keybindings, event/redraw loop, CLI).

Snapshot: v0.4.13. Update this file when behavior it documents changes (see root `CONTRIBUTING.md`).

## 1. `ferrowl-ui` widgets

All widgets follow ratatui's `StatefulWidget` pattern (`Widget<State, WidgetConfig>` pair), driven by `HandleEvents::handle_events(modifiers, code) -> EventResult` (`Consumed` | `Unhandled`) (`ferrowl-ui/src/traits.rs`). Styling is one fixed compile-time `COLOR_SCHEME` const (feature-gated `vscode_dark` / `catppuccin_mocha` / `gruvbox_dark`), not runtime-switchable (`ferrowl-ui/src/lib.rs:71`, `:103`, `:138` — one per feature). `AlternateScreen<W: Write+Init>` is the RAII terminal wrapper (`ferrowl-ui/src/screen.rs:18`): `new()` enables raw mode + `EnterAlternateScreen`/`EnableMouseCapture` then immediately disables mouse capture; `Drop` and the static `release()` (`ferrowl-ui/src/screen.rs:59`, used from panic hooks) restore the terminal.

Widget/state pairs live in `ferrowl-ui/src/widgets/<name>.rs` + `ferrowl-ui/src/state/<name>.rs` (same basename per row below).

| Widget | State type | Purpose |
|---|---|---|
| `Button` | `ButtonState` | Bordered push-button; double border, focus-highlighted, disabled variant, multiline wrap |
| `Text` | `String` | Read-only text display, optional border/title, multiline wrap |
| `InputField<V: Validate>` | `InputFieldState` | Single-line typed text input; cursor, placeholder, autofill (Ctrl+F), Ctrl+D clear, per-type char filtering |
| `CodeInputField` | `CodeInputFieldState` | Multi-line code editor with gutter, syntax highlighting (via `ferrowl-syntax`), h/v scroll, optional vim modal editing |
| `Selection<V: ToLabel>` | `SelectionState<V>` | Vertical pick-one list, horizontal-scrolls the selected row if it overflows |
| `SuggestInput<V, P: SuggestionProvider>` | `SuggestInputState<P>` | `InputField` + popup completion list; `render_overlay` must be called after all sibling widgets (painter's-algorithm z-order) |
| `Table<V: TableEntry<N>, H: Header<N>, N>` | `TableState<V,N>` | Scrollable N-column table with word-wrap-per-cell, vertical scrollbar, horizontal scroll via manual buffer blit when content > area width |
| `ScrollingTabs<T: ToLabel>` | `ScrollingTabsState<T>` | Horizontally scrolling tab bar keeping selected tab centered |

Shared helpers: `render_border` draws an optional titled bordered `Block`, returns inner rect; `Border::{None, Full(Margin)}` (`ferrowl-ui/src/border.rs`); `button()` (`ferrowl-ui/src/widgets/build.rs:16`) — shared constructor for an unfocused, center-aligned button used by modbus dialogs + OCPP overlay code.

Key behaviors:
- `InputFieldState` (`ferrowl-ui/src/state/input_field.rs`): Home/End, Ctrl+F autofill-from-placeholder, Ctrl+D clear, char-level `allowed: Option<fn(char)->bool>` filter, Left/Right cursor, Backspace/Delete — all multibyte-safe via `char_indices`.
- `CodeInputFieldState` vim mode (`ferrowl-ui/src/state/code_input_field.rs`, motion/edit dispatch in `ferrowl-ui/src/state/vim.rs`): Normal/Insert/Visual(charwise|linewise) modes; motions `h j k l 0 $ w b e G gg`; edits `x p P u i a I A o O v V y d dd yy`; yank/delete write to an internal register and emit an OSC 52 clipboard escape; format-on-blur via `ferrowl_syntax::format` when a `language` is set and the field isn't disabled; double-space chord expands to 4-space indent within a `space_indent` threshold (default 300ms, `ferrowl-ui/src/state/code_input_field.rs:44`).

## 2. `ferrowl-syntax`

Pure text-to-spans syntax highlighting, no rendering, no dependencies. Lexers walk one line of source and emit `(start_char, end_char, SyntaxKind)` spans plus a carry-over `LineState` for constructs spanning multiple lines (Lua long strings/comments). Consumers own the mapping from `SyntaxKind` to actual colors.

- `Language`: `Lua` | `Json` (`ferrowl-syntax/src/lang/mod.rs:11`). Lexers: `ferrowl-syntax/src/lang/lua.rs`, `ferrowl-syntax/src/lang/json.rs`.
- `SyntaxKind` (`ferrowl-syntax/src/lib.rs:18`): `Keyword`, `Ident`, `Number`, `String`, `Comment`, `Punct`, `Key` (JSON object key), `Literal` (`true`/`false`/`nil`/`null`), `Object` (identifier accessed via `.`/`:`, e.g. `C_Register`), `Function` (identifier in call/method position, e.g. `Set`).
- `highlight_line(lang, line, state) -> (Vec<(usize,usize,SyntaxKind)>, LineState)` (`ferrowl-syntax/src/lib.rs:53`) — spans sorted by start, non-overlapping, char-index based.
- `indent_delta` (`ferrowl-syntax/src/indent.rs:14`) — per-line indent adjustment helper for the code editor.
- `format(lang, source) -> Option<String>` (`ferrowl-syntax/src/format/mod.rs:12`) — reformats source; `None` means "leave the content unchanged" (e.g. invalid JSON) — the caller (editor widget losing focus) keeps the existing buffer as-is. JSON formatting can fail (`None`); Lua formatting always returns `Some`.

## 3. `ferrowl-ui-derive` macros

Entrypoints all in `ferrowl-ui-derive/src/lib.rs` (`Focus` `:32`, `focusable` `:45`, `TableEntry` `:68`, `Overlay` `:91`); implementations in the sibling `focus.rs` / `table_entry.rs` / `overlay.rs`.

| Macro | Attribute syntax | Generates | Problem solved |
|---|---|---|---|
| `#[proc_macro_attribute] focusable` | above `#[derive(Focus)]` | Appends `focus: <Struct>Focus` and `view_focused: bool` fields (with `#[builder(default)]` on `view_focused` if the struct also derives `Builder`) | Boilerplate injection so `Focus` has state to operate on |
| `#[derive(Focus)]` | `#[focus]` / `#[focus(when = expr)]` on named fields | `<Struct>Focus` enum (one variant per tagged field); `focus_previous()`/`focus_next()` cycling (skips fields whose `when` is false); whole-struct `SetFocus`/`IsFocus` impls (composability — a focusable view is itself a focusable node in a parent); `HandleEvents` impl dispatching to the currently-focused field | Keyboard focus cycling + event dispatch |
| `#[derive(TableEntry)]` | struct-level `#[table_entry(header = Name, styles = path)]`, `#[row(height = N)]`; field-level `#[column(name="…", min=N, max=M)]` | `impl TableEntry<N>` (`values()` stringifies tagged fields via `ToString`, `height()` from `#[row(height)]` default 1, optional `cell_styles()` forwarding to a `styles` fn); companion unit struct `<Struct>Header` implementing `Header<N>` | Row + column-header boilerplate for `ferrowl_ui::widgets::Table` |
| `#[derive(Overlay)]` | enum variant attrs `#[overlay(none)]` (exactly one, unit variant), `#[overlay(esc_close)]`, `#[overlay(focus_cycle)]` (single-field variants only) | Inherent `is_active()`, `close()`, `take()`, `route_keys(modifiers, code) -> OverlayRoute` (`Esc`→`Closed` for `esc_close` variants, `Tab`/`BackTab`→`Cycled` via `OverlayKeys::focus_cycle` for `focus_cycle` variants, else `Unhandled`) | Structural boilerplate + common-key routing for mutually-exclusive modal-overlay enums |

`OverlayKeys` trait (`ferrowl-ui/src/traits.rs:90`) adapts a payload's own `focus_next`/`focus_previous` to a `forward: bool` step; `impl_overlay_keys!(Type, ...)` (`ferrowl-ui/src/traits.rs:107`) blanket-implements it by forwarding to `#[derive(Focus)]`-generated methods — used throughout app code (e.g. `ferrowl_ui::impl_overlay_keys!(SetupDialog);`).

## 4. App structure

**Module registration/tabs**: `App` owns `Vec<Tab>` (`ferrowl/src/app/mod.rs`); `Tab` is `#[focusable] #[derive(Focus)]` pairing `view: Box<dyn ModuleView>` and `log_view: LogView` (content↔log focus cycling via `Ctrl+w j/k`). `MODULE_TYPES` (`ferrowl/src/module/mod.rs:16`) registers `"Modbus"` → `ModbusSetupView`, `"OCPP"` → `OcppSetupView` for the `:new` type-selector (consumed at `ferrowl/src/module/type_select.rs:46` and `ferrowl/src/app/overlay.rs:63`).

**Views**:
- Modbus: `ModbusModuleView` — register table (`TableView`/`Definition`), sortable via `:order [col] [asc|desc]`, compact-mode toggle.
- OCPP client: `ClientView<V>` — NV-state table, connector table, config table, message table (`StateTable`, `ConnTable`, `ConfigTable`, `MsgTable`).
- OCPP server: `ServerView<V>` — CS table (`CsTable`) + message table (`MsgTable`); per-entry `DetailOverlay` drills into a config table (with/without Component column per protocol version), a metering table, and an RFID table.

**Dialogs**:

| Dialog | Purpose |
|---|---|
| `SetupDialog` (modbus) | Create/edit a modbus module (name, device path, role, endpoint, timing, read-ranges) |
| `OcppSetupDialog` | Create/edit an OCPP module (spec, security, config-path suggest) |
| `TypeSelectDialog` | `:new` module-type picker (Modbus/OCPP) |
| `EditInputDialog` / `EditSelectionDialog<V>` | Register add/edit form (typed input vs. choice list per field type) |
| `AddNamedValueDialog` | Add a named enum value to a register |
| `ConfirmDeleteDialog` | Delete confirmation (registers, OCPP CS/connector, RFID) |
| `CloseConfirmDialog` | Generic "close without saving?" popup opened by Esc on any dialog |
| `ScriptDialog` | Lua script manager (see `lua.md` §4) |
| `LuaHelpOverlay` | `?`-triggered Lua-bindings reference inside a script editor |
| `FsPathProvider` | `SuggestionProvider` for filesystem-path autocompletion (`SuggestInput`) |
| `ActionDialog` | Compose/send an OCPP action (request payload editor) |
| `DetailOverlay` | Per-CS/connector detail: config table(s), metering table, RFID table + add input |
| module re-setup overlays | `ModbusViewOverlay::Setup`, `ClientOverlay::Setup`, `ServerOverlay::Setup` wrap `SetupDialog`/`OcppSetupDialog` as an in-view overlay |

Modal-overlay enums (all `#[derive(Overlay)]`): `ModbusViewOverlay` (None/Register/Setup[focus_cycle]/Scripts), `ClientOverlay` (None/Edit[esc_close]/Config[focus_cycle]/Action/Setup[focus_cycle]/Scripts), `ServerOverlay` (None/Detail/Confirm[esc_close,focus_cycle]/Setup[focus_cycle]/Scripts/Action). App-level `Overlay` (non-derived): `TypeSelect(TypeSelectDialog)` → `Creation(Box<dyn SetupView>)` two-stage `:new` flow.

**Event/redraw loop** (`App::run`, `ferrowl/src/app/mod.rs:366`): a background `std::thread` blocking-reads `crossterm::event::read()`, forwards via `tokio::sync::mpsc::channel::<Event>(64)`. The async loop does `tokio::time::timeout(REDRAW_INTERVAL=100ms, rx.recv())` (`ferrowl/src/app/mod.rs:36`, used at `:388`) — key events dispatch via `App::handle_key` (`ferrowl/src/app/mod.rs:516`), timeout/no-event redraws anyway (drives live value polling). Every tick calls `refresh_snapshot()` (`ferrowl/src/app/mod.rs:402`), which polls **all** tabs' `ModuleView::refresh()` concurrently (`futures_util::future::join_all` — background tabs keep ticking even when not active), flushes log-file sinks once/tick, handles `take_replacement()` (e.g. OCPP role switch swapping the view), re-dedupes tab names on collision. `App::run` is `async` inside a multithreaded tokio `Runtime`; background modbus/OCPP tasks run concurrently with the UI. Terminal restore on panic: `std::panic::set_hook` wraps the previous hook, calls `AlternateScreen::<Stdout>::release()` before delegating; `App::run`'s error path and normal `Drop` also release it. Key routing top-level (`ferrowl/src/app/keys.rs`): `Focus::{Command, Dialog, Content}` dispatches to `handle_command_key`/`handle_session_dialog_key`/`handle_dialog_key`/`handle_nav_key`. A `Ctrl+t`-digit tab-jump chord has an 800ms timeout (`DIGIT_CHORD_TIMEOUT`, `ferrowl/src/app/mod.rs:40`, applied at `ferrowl/src/app/keys.rs:119`).

## 5. `:` commands

App-level parser (`parse`, `ferrowl/src/command.rs:23` — pure/unit-tested), dispatched in `run_command` (`ferrowl/src/app/commands.rs:34`):

| Command | Aliases | Args | Purpose |
|---|---|---|---|
| `:quit` | `q`, `q!` | — | Close active tab (stops its module first); quits app if last tab |
| `:qall` | `qa`, `qa!` | — | Quit immediately, all tabs |
| `:new` | `n` | — | Open module-type selector |
| `:load [path]` | `l` | optional device-config path | Open modbus creation dialog pre-filled with `path` |
| `:write [path]` | `s`, `save`, `w` | optional path (default `session.toml`) | Save current module instances + session scripts/interval as a session file |
| `:log [file\|clear]` | — | optional arg | `clear` clears active tab's log ring; any other arg forwarded to the active view |
| `:swap <i> <j>` | — | two tab indices | Swap two tabs' positions |
| `:session` | — | — | Open the session-level scripts/interval dialog |
| `:script copy <idx>` | — | source tab index | Replace active tab's Lua scripts with tab `<idx>`'s |
| anything else | — | — | Forwarded verbatim to the active `ModuleView::handle_command` |

**Modbus module commands** (`MODBUS_COMMANDS`, 12 entries, `ferrowl/src/module/modbus/view/mod.rs:757`): `:e`/`:edit` (open setup dialog), `:a`/`:add` (add register), `:start`, `:stop`, `:restart`, `:reload` (reload device config from disk, restart module), `:compact` (toggle compact table), `:set <register> <value>` (write register value), `:wd`/`:wd <path>` (`write-device`, save device config), `:log <file>` (set log-file base), `:script` (open script manager), `:order [col] [asc|desc]` / bare `:order` (sort table / clear sort).

**OCPP server module commands** (`OCPP_SERVER_COMMANDS`, 9 entries, `ferrowl/src/module/ocpp/server/view/mod.rs:520`): `:start`/`:stop` (bind/unbind CSMS listener), `:restart` (rebind, clears entries), `:e`/`:edit`, `:wd`/`:write-device [path]`, `:compact`, `:log [file]`, `:rfid [add|del <tag> | clear]` (CSMS RFID accept-list). Plus keybinds `d` = delete selected station, `Enter` = open detail overlay.

**OCPP client module commands** (`OCPP_CLIENT_COMMANDS`, 7 entries, `ferrowl/src/module/ocpp/client/view/mod.rs:661`): `:e`/`:edit`, `:start` (connect to CSMS), `:stop` (disconnect), `:restart` (reconnect), `:compact`, `:wd`/`:write-device [path]`, `:log [file]`.

The command-line help popup (shown while typing) hardcodes the app-level list plus whatever `ModuleView::commands()` returns for the active tab.

## 6. Keybindings

Global:

| Key | Action |
|---|---|
| `:` | Enter command mode |
| `Ctrl+w` then `j`/`k` (or Down/Up) | Switch tab's content↔log pane focus |
| `Ctrl+t` then `l` | Next tab |
| `Ctrl+t` then `h` | Previous tab |
| `Ctrl+t` then digit(s) | Jump to tab by index (waits up to 800ms for a 2nd digit if one could form a valid 2-digit index) |
| `?` | Open keybind help |
| Command mode: `Esc` | Cancel |
| Command mode: `Enter` | Run command |
| Dialogs: `Esc` | Close (opens confirm popup) |
| Dialogs: `Enter` | Confirm |
| Dialogs: `Tab`/`Shift+Tab` | Next/previous focus field |
| Code editor (vim): `Esc` | Normal mode / close dialog |
| Code editor: `i a I A o O` | Enter Insert mode (various cursor placements) |
| Code editor: `h j k l`, `w b e`, `0 $`, `gg G` | Motion |
| Code editor: `v`/`V` | Visual / Visual-Line mode |
| Code editor: `y`/`d` (line: `yy`/`dd`, char: `x`) | Yank / delete |
| Code editor: `p`/`P` | Paste after/before |
| Code editor: `u` | Undo |
| Code editor: `Tab`/`Shift+Tab` (Insert mode) | Indent/dedent |
| Code editor: `?` (Normal mode) | Show Lua bindings help overlay |
| Tables: `j`/`k` or Up/Down | Row up/down |
| Tables: `h`/`l` or Left/Right | Column left/right |
| Tables: `g`/`G` | First/last row |
| Tables: `0`/`$` or Home/End | First/last column |

Help dialog own navigation: `Esc`/`q`/`?` close, `j`/Down and `k`/Up scroll, `g` = top, `G` = bottom.

Module-specific (shown appended in the `?` dialog):
- Modbus (5 entries): `Enter` (edit selected register), `Enter` in dialog (confirm edit), `Space` in dialog (press button/toggle), `z` in dialog (toggle compact table), `Esc` in dialog (close).
- OCPP server (3): `Tab`/`Shift+Tab` (pane), `Enter` (open detail/scripts/trigger action), `d` (delete selected CS).
- OCPP client (4): `Tab`/`Shift+Tab` (pane), `Enter` (activate focused pane: edit/add/trigger), `Space` (activate focused table/button), `d` (delete selected connector/config key).

## 7. CLI flags (`ferrowl/src/cli.rs`)

Top-level `CliArgs` (clap `Parser`, `ferrowl/src/cli.rs:14`):

| Flag | Type | Purpose |
|---|---|---|
| `--module KEY=VAL,...` | repeatable string | Ad-hoc modbus module spec (`name`, `device`/`type`, `role` client\|server, `transport` tcp\|rtu, `ip`, `port`, `path`, `baud`/`baud_rate`, `parity`, `data_bits`, `stop_bits`) |
| `--session FILE` | repeatable string | Session file (TOML/JSON) listing multiple module instances |
| `--device FILE` | repeatable string | Device config file → auto-built TCP client spec pointed at `127.0.0.1:5020` |
| `--demo` | bool | Launch with 8 built-in demo tabs (2 modbus + 3 OCPP versions × client/server) and an example session script, no config files needed |
| subcommand | `migrate` \| `run` | see `infra.md` §4 (migrate) |

`run` subcommand (`RunArgs`, `ferrowl/src/cli.rs:62` — headless/CI mode):

| Flag | Purpose |
|---|---|
| `--session FILE` (repeatable) | Same as top-level |
| `--module KEY=VAL,...` (repeatable) | Same as top-level modbus module spec |
| `--ocpp KEY=VAL,...` (repeatable) | Ad-hoc OCPP module spec (`name`, `device`, `protocol` ws\|wss, `ip`, `port`, `path`) |
| `--duration SECS` | Run for N seconds then exit 0; omit to run until Ctrl-C |
| `--log-file FILE` | Also append every drained log line to this file |
| `--exit-on-error` | Exit code 2 after stopping all modules if a drained log line starts with `[sim]` (Lua script error marker) |

`--exit-on-error` is specific to `run`; no equivalent flag exists on the top-level (TUI) command.
