# CLI & Headless — Requirements

Testable requirements for the ferrowl **process command line** and the **headless
runner** (`ferrowl run`). This area owns the argument surface (top-level flags and
subcommands), the `--module`/`--ocpp` key=val mini-language, the headless run
lifecycle and its exit-code contract, and the headless stdout/stderr output
contract.

Per the ownership rules in [`../README.md`](../README.md), this area does **not**
own:

- The `:` command line **inside** the TUI — that is [`../tui/`](../tui/). This area
  owns the *process* command line only.
- The config/session **file format** and the `migrate` **transformation**
  semantics — those are [`../config-session/`](../config-session/). This area owns
  the `migrate` *CLI surface* (its flags, invocation, and exit behavior) only.
- What Lua scripts and `C_Test:Assert` **do** — that is [`../scripting/`](../scripting/).
  This area owns only how a logged script error maps to a process exit code.
- Protocol behavior — [`../modbus/`](../modbus/) and [`../ocpp/`](../ocpp/).

---

## Argument surface (top-level command)

**CL-R-001** — The program shall present a `--version` flag (printing the build
version and exiting 0) and a `--help` flag (printing usage and exiting 0). These
are the standard argument-parser behaviors and shall take precedence over starting
the TUI.

**CL-R-002** — The top-level command shall accept a repeatable `--module`
option whose value is a `key=val,...` module descriptor (the mini-language in
[`api-contract.md`](./api-contract.md)). Each occurrence shall contribute one
additional Modbus module instance, in command-line order.

**CL-R-003** — The top-level command shall accept a repeatable `--session` option
naming a session file. Each named session file's module instances shall be resolved
and contribute to the started instance set. Session instances shall be resolved
before `--module` instances.

**CL-R-004** — The top-level command shall accept a repeatable `--device` option
naming a device-config file. Each occurrence shall contribute one auto-built Modbus
**TCP client** instance named `Device <n>` (n = 0-based occurrence index) pointed at
the fixed endpoint `127.0.0.1:5020`. `--device` shall expose no endpoint or role
control; full control requires `--module`.

**CL-R-005** — The top-level command shall accept a boolean `--demo` flag. When set,
the program shall start a fixed set of built-in demo tabs and ignore `--module`,
`--session`, and `--device` for the purpose of building tabs.

**CL-R-006** — When `--demo` is set, the demo tab set shall consist of exactly eight
tabs: two Modbus tabs (one server, one client) and six OCPP tabs (a client and a
server for each of OCPP 1.6, 2.0.1, and 2.1). Each demo tab shall be started, and
the demo session shall additionally load one example session-level Lua script.

**CL-R-007** — The final instance set shall be the concatenation, in order, of
`--session` instances, then `--module` instances, then `--device` instances. Module
names shall be de-duplicated across all sources and both module types together per
the envelope rule (CS-R-014): the first occurrence keeps its name; later duplicates
receive a ` (2)`, ` (3)`, … suffix.

---

## Subcommands

**CL-R-010** — The program shall expose two subcommands, `migrate` and `run`.
Invoking a subcommand shall replace the default action (starting the TUI) with that
subcommand's action.

**CL-R-011** — The `migrate` subcommand shall require two options, `--input`/`-i`
and `--output`/`-o`, each naming a file. Its CLI contract is: read the file at
`--input`, write the converted device config to `--output`. The
legacy→current transformation itself is specified in
[`../config-session/`](../config-session/) (CS-R-040…CS-R-045); this area specifies
only the invocation and exit behavior.

**CL-R-012** — The `migrate` subcommand shall be dispatched before any async runtime
is created, shall never start the TUI or a headless run, and shall exit the process
directly with its own exit code (0 on success; non-zero on failure per CL-R-032).

**CL-R-013** — The `run` subcommand (headless mode) shall accept: repeatable
`--session`, repeatable `--module`, repeatable `--ocpp`, an optional `--duration`
(seconds), an optional `--log-file`, and a boolean `--exit-on-error`. It shall
resolve Modbus modules from `--session` + `--module` and OCPP modules from
`--session` + `--ocpp`, using the same descriptor mini-language as the top-level
command.

**CL-R-014** — `--ocpp` (ad-hoc OCPP module descriptor) shall be accepted **only** on
the `run` subcommand; the top-level command has no `--ocpp` flag and resolves OCPP
modules solely from `--session` files. Conversely `--device` shall be accepted only
on the top-level command and not on `run`.

**CL-R-015** — `--exit-on-error` shall be accepted only on the `run` subcommand; no
equivalent flag exists on the top-level (TUI) command.

**CL-R-016** — Top-level `--module`/`--session`/`--device`/`--demo` values supplied
alongside a `run` (or `migrate`) subcommand shall not affect that subcommand: the
headless runner reads only the `run` subcommand's own flags, and the top-level
values are ignored.

---

## Headless run lifecycle

**CL-R-020** — `ferrowl run` shall build the same module views the TUI builds and
start each one via the module's `start` command, but shall never enter the alternate
screen, read the terminal, or render a UI.

**CL-R-021** — Unlike the TUI (which skips a module whose device config fails to load
and continues), the headless runner shall treat any module's device-config load
failure or `start`-reported error as fatal to startup: it shall not start a partial
module set. (See CL-R-030.)

**CL-R-022** — After all modules start, the runner shall loop on a fixed ~100 ms tick.
On each tick it shall refresh every module, drain each module's newly appended log
lines to standard output (and, if configured, to the log file), and evaluate its
stop conditions.

**CL-R-023** — When `--session` files supply at least one **enabled** session-level
Lua script, the runner shall additionally run the session-level sim (with the
resolved cycle interval) and drain its log under the source name `session`. When no
session file supplies any script, no session sim shall be created. (Whether a script
runs is governed by its `enabled` flag, per [`../scripting/`](../scripting/).)

**CL-R-024** — With `--duration <secs>` set, the run shall exit cleanly once the
elapsed time reaches the deadline, evaluated at the end of a tick. Without
`--duration`, the run shall continue until interrupted by Ctrl-C (SIGINT).

**CL-R-025** — A Ctrl-C (SIGINT) during the run shall end the loop and be treated as
a clean shutdown (exit 0), not an error.

**CL-R-026** — On any loop exit, the runner shall stop the session sim (if any) and
then stop every module before the process returns. A stop failure shall be logged
but shall not change the exit code.

**CL-R-027** — Session-level scripts across multiple `--session` files shall be
concatenated in file order, and the session sim interval shall be the last session
file's interval — matching the TUI's multi-file session resolution so both entry
points behave identically.

---

## Exit codes

**CL-R-030** — `ferrowl run` shall return exit code **1** for any setup failure: a
module's device config failed to load, a module's `start` reported an error, a
`--session` file failed to load or parse, or the `--log-file` could not be opened. A
diagnostic beginning `Error:` shall be written to standard error, and any modules
already started shall be stopped before returning.

**CL-R-031** — `ferrowl run` shall return exit code **2** if and only if
`--exit-on-error` is set **and** a drained log line begins with the sim-error prefix
`[sim]`. On detecting such a line the runner shall stop every module and then exit
with code 2. When `--exit-on-error` is not set, a `[sim]` line shall never change the
exit code.

**CL-R-032** — `ferrowl run` shall return exit code **0** for a run that reaches its
`--duration` deadline or is interrupted by Ctrl-C without any exit-code-2 condition
having fired.

**CL-R-033** — The `migrate` subcommand shall exit **0** on a successful conversion
and **1** on failure (unrecognized input/output file extension, input parse failure,
or output write failure), writing a diagnostic beginning `error:` to standard error.
`migrate` shall never use exit code 2.

**CL-R-034** — A `C_Test:Assert` failure (or any other Lua sim error) shall **not**
by itself fail a headless run: it surfaces only as a `[sim]`-prefixed log line, so it
influences the exit code only when `--exit-on-error` is set (yielding code 2 per
CL-R-031). A CI job that must fail on assertion failure shall pass `--exit-on-error`.

**CL-R-035** — An argument-parsing error (unknown flag, missing required subcommand
option, malformed option value handled by the parser) shall abort before any run,
printing a usage diagnostic to standard error and exiting with the argument parser's
standard usage exit code (2). Requesting `--help`/`--version` shall exit 0.

---

## Output contract (headless)

**CL-R-040** — In headless mode each drained log line shall be printed to standard
output in the form `[<timestamp>] <source> | <message>`, where `<source>` is the
module's (deduped) name, or `session` for session-sim lines.

**CL-R-041** — When `--log-file <path>` is set, every line printed to standard output
shall additionally be appended to that file. The file shall be opened in
create-and-append mode, so an existing file is appended to rather than truncated.

**CL-R-042** — Setup and fatal diagnostics (the `Error:`/`error:` messages of
CL-R-030 and CL-R-033, and the TUI's module-skip warnings) shall be written to
standard error, keeping standard output as the machine-readable drained-log stream.

**CL-R-043** — Per-module log draining shall be exact-by-count (tracking total lines
written), so a message repeated verbatim within one drain window is not mis-resumed
and every occurrence is emitted. If more lines were written between ticks than the
bounded log ring can hold, the overflow shall be reported with a synthetic
`(<n> lines dropped: ring overflowed between ticks)` line rather than silently
under-counted.
