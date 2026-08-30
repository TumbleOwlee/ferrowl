# Scripting — Requirements

Embedded Lua simulation model, per-context runtime and sandbox, `C_*` host API surface, sim thread execution model, script storage and lifecycle, error/logging semantics.

IDs stable, append-only (`SC-R-nnn`). See [`../README.md`](../README.md). Companions: [`api-contract.md`](./api-contract.md) (exhaustive `C_*` API), [`edge-cases.md`](./edge-cases.md).

**Area boundaries.** Lua API surface and semantics owned here. The in-TUI code editor (vim-modal editing, syntax highlighting, `:script` dialog) is `tui/`. The device/session file *envelope* carrying scripts is `config-session/`; the script-bearing fields (`scripts`, `script_interval`, session `interval`) are specified here because they control scripting behavior. `C_Test`'s Lua-side assertion semantics owned here; the `ferrowl run` **exit-code** contract keyed off logged assertion failures is `cli-headless/`.

---

## Runtime & VM

**SC-R-001** — Scripts execute on a real Lua 5.4 VM compiled into the binary (no external interpreter, no dynamic linking to a system Lua).

**SC-R-002** — The Lua API is synchronous and blocking: a `C_*` call completes before the script continues; no script can `await`, yield to, or interact with the host's async runtime.

**SC-R-003** — A script is compiled into a callable function once, when loaded into a Lua context, and invoked with no arguments and no expected return on each execution.

**SC-R-004** — A Lua context owns exactly one VM and a set of loaded scripts keyed by name. Every script in one context shares that context's single global environment; a global set by one script is visible to every other in the same context.

**SC-R-005** — Loading two scripts under the same name into one context is rejected; the second load fails, nothing silently overwritten.

---

## Sandbox & available globals

**SC-R-006** — Each sim context loads only the pure-computation Lua standard libraries — `string`, `table`, `math`, `utf8`, `coroutine` — plus the base library. A sim script is untrusted input (device or session config), so it has no access to host filesystem, shell, environment, or dynamic code loading. Clock access comes from the sandboxed `C_Time`, not `os`.

**SC-R-007** — `io`, `os`, `package`, `debug`, and FFI libraries are not reachable from any sim context; the base library's dynamic-code loaders (`load`, `loadfile`, `dofile`, `loadstring`, `require`) are removed from globals. Indexing any of these sees a `nil` global.

**SC-R-008** — Beyond the standard library subset, the only host-injected globals a script may rely on are the `C_*` modules registered for that context (SC-R-018) and the redirected `print` (SC-R-030). No other bespoke host global.

**SC-R-009** — Dynamic values cross the Lua/host boundary as exactly one of five types: integer, float, string, boolean, nil. Any other Lua value (table, function, userdata, thread) where a host value is expected fails conversion with an error, never coerced or dropped. (Exception: an OCPP action override *table*, whose scalar entries are flattened per the API contract.)

---

## Execution model

**SC-R-010** — Because the Lua VM is not shareable across threads, each sim owner runs its context on a dedicated OS thread that builds the context inside that thread and loops until stopped. The UI event loop and async network runtime never execute Lua directly.

**SC-R-011** — A sim thread is spawned only when at least one script is enabled for its owner; with none, no sim thread exists. Constrains *sim* threads only: SC-R-035's on-demand single-script execution runs on its own short-lived thread, neither gated on nor counted as a sim thread.

**SC-R-012** — A sim thread is controlled by a stop flag observed between cycles and, during a cycle, at each firing of the execution hook (SC-R-034). Setting the flag and joining stops the sim; the sim handle's destruction also stops and joins.

**SC-R-013** — Within each cycle the thread sleeps up to the cycle interval in small chunks, re-checking the stop flag between chunks, so a stop during the idle portion is observed promptly.

**SC-R-014** — A per-module Modbus sim and the session-level sim run **every** enabled script on **every** cycle. A per-module OCPP sim runs each enabled script at most once per cycle interval, skipping any that ran more recently than the interval. In all cases the observable cadence of a script is approximately one execution per cycle interval.

**SC-R-015** — Execution within a cycle is sequential on the sim thread (no script in a context runs concurrently with another in the same context). Relative order within a cycle is unspecified ([`edge-cases.md`](./edge-cases.md) §5.3).

**SC-R-016** — The cycle interval resolves from the owner's configured interval in seconds, sanitized so non-finite or non-positive falls back to 1.0 s. A per-module (Modbus or OCPP) interval is additionally floored to 0.05 s; the session-level interval has no floor.

**SC-R-017** — Time observed through `C_Time` is measured from the moment the sim thread's context is built. Rebuilding the context (SC-R-024) resets the origin to zero.

**SC-R-035** — An owner supports executing a single script **once, on demand** (script-manager dialog, UI-R-051). Such a run builds its own context on its own short-lived thread, registers the same `C_*` modules its owner's sim would (SC-R-018), loads only that script, calls it exactly once, logs any error to the owner's script log, exits. It requires no running sim thread, shares no Lua state with one ([`edge-cases.md`](./edge-cases.md) §5.8), and ignores the enabled flag. Its errors are logged under a `[run]` prefix, distinct from the `[sim]` prefix marking sim diagnostics (SC-R-032): `ferrowl run --exit-on-error` keys exit code 2 off `[sim]` (CL-R-031), so an interactive test run must not be mistakable for a sim failure.

---

## Host module availability per context

**SC-R-018** — `C_*` modules registered into a context depend on the sim owner:

| Sim owner | Registered modules |
|---|---|
| Modbus module | `C_Register`, `C_Time`, `C_Test`, `C_Log`, `print` |
| OCPP module (client or server) | `C_OCPP`, `C_Time`, `C_Test`, `C_Log`, `print` |
| Session-level | `C_Module`, `C_Time`, `C_Test`, `C_Log`, `print` |

**SC-R-019** — `C_Register` reachable only from a Modbus module's own sim; `C_OCPP` only from an OCPP module's own sim; `C_Module` only from the session-level sim. A script naming a module not registered in its context fails at run time with a Lua "attempt to index a nil value" style error, never a silent no-op.

**SC-R-020** — The session-level sim reaches every other module's state indirectly through `C_Module`, which resolves modules by name and hands out the same `C_Register`-shaped or `C_OCPP`-shaped accessor those modules expose to their own sims.

**SC-R-021** — An OCPP **server** module runs its scripts as a client module does (both roles simulated); scripting is not limited to the client role.

---

## Script lifecycle

**SC-R-022** — A script is defined by a name, a code body (default empty), an enabled flag (default enabled). Only enabled scripts with non-empty code are handed to a sim thread. (Persisted shape is `config-session/`'s envelope; meaning specified here.)

**SC-R-023** — Scripts are stored inline in device/session config files, not external `.lua` files.

**SC-R-024** — Editing a script, toggling its enabled flag, or changing the cycle interval stops any running sim thread and starts a fresh one from the current enabled-script set (or leaves it stopped if none remain). The new context is fresh: all globals reset. A running sim does not pick up an edited script without such a restart.

**SC-R-025** — Legacy per-register `update` snippets in an older Modbus device config are migrated on load into named, enabled entries in the module's script list, preserving code, and thereafter run through the same sim model.

**SC-R-026** — A Modbus module's sim runs independently of the network instance's connection state: enabled scripts execute whether or not the client is connected or the server is bound.

---

## Script templates

**SC-R-036** — The binary carries a fixed library of Lua script templates, compiled in at build time. Each has a name, one-line description, Lua code body, and the set of script contexts (Modbus, OCPP client, OCPP server, session) it applies to. A template is not a script: it becomes one only by being copied into a script list; nothing loads template code from disk at run time (SC-R-023 stands).

**SC-R-037** — Every template's code body is loadable by the Lua runtime — a template failing to compile is a build/test failure, not a run-time one.

**SC-R-038** — `C_Test` exposes `Assert(cond, msg)` and `Fail(msg)`. `Assert` raises runtime error `assertion failed: <msg>` when `cond` is Lua-falsy (only `nil` or `false`), otherwise returns with no effect — every other value, including `0` and `""`, is truthy and passes. `Fail(msg)` always raises `assertion failed: <msg>`. The `assertion failed:` prefix is the text a headless runner keys its exit code off ([`../cli-headless/requirements.md`](../cli-headless/requirements.md)).

---

## State access semantics

**SC-R-027** — A value read from a register or OCPP state field returns to Lua as its natural type (number for numeric, string, boolean). A value written from Lua applies to host state per the API contract, type/range mismatches failing rather than coercing ([`api-contract.md`](./api-contract.md), [`edge-cases.md`](./edge-cases.md) §2).

**SC-R-028** — A register or OCPP state write from Lua applies to the module's in-memory/observed state only. A Modbus Lua write never emits a Modbus write command (unlike interactive `:set`); a written value on a client is therefore transient and may be overwritten by the next poll ([`edge-cases.md`](./edge-cases.md) §5.2).

**SC-R-029** — Host state reached from Lua is guarded by the same locks the network task uses, so each `Get`/`Set`/action call is atomic against concurrent host access. No cross-call transaction: a read-then-write may interleave with a concurrent host update between the calls.

---

## Logging & error handling

**SC-R-030** — The Lua global `print` is redirected to the sim owner's log sink (never real stdout, which would corrupt the TUI). `print` follows Lua semantics: each argument converted with tostring (honoring `__tostring`), joined by tabs, emitted as one Info line.

**SC-R-031** — `C_Log:Info/Warn/Error` and `print` output from a module's sim route to that module's dedicated **script** log (distinct from its connection/traffic log), and, for a Modbus module, also to the module's file log sink when configured.

**SC-R-032** — A runtime error raised by one script (uncaught `error`, failed `C_Test:Assert`/`Fail`, rejected state write, malformed OCPP override table) does not crash the sim thread and does not prevent other scripts in the context from running that cycle. Every such error is collected and written to the owner's log at Error level, prefixed as a sim/Lua diagnostic.

**SC-R-033** — If building the context itself fails — a Lua **syntax** error in any script, or a duplicate name (SC-R-005) — the sim thread logs a single "failed to build Lua context" error and does not loop; **no** script in that context runs. Load-time failure is all-or-nothing per context; run-time failure (SC-R-032) is isolated per script.

**SC-R-034** — Every Lua context — sim thread (SC-R-010) and on-demand run (SC-R-035) alike — installs an execution hook via mlua's `every_nth_instruction`, firing every 1,000 instructions. On each firing: (a) sim-thread context: check the stop flag and, if set, raise a Lua error to unwind the executing script; (b) unconditionally check elapsed wall-clock since the current cycle (or, for an on-demand run, the single execution) began, raising a Lua error past a fixed 1,000 ms cap. Both constants fixed; neither exposed as config key or CLI flag. No memory ceiling. [`edge-cases.md`](./edge-cases.md) §5.1.

**SC-R-039** — An error raised by the hook (stop-flag or wall-clock) flows through the same per-script path as any runtime error: SC-R-032's isolation and `[sim]` logging for a sim thread, SC-R-035's `[run]` logging for an on-demand run. It does not crash the sim thread, and, for a sim thread, other scripts still run that cycle. A hook-raised stop lets a pending stop-and-join complete promptly instead of blocking on the join.
