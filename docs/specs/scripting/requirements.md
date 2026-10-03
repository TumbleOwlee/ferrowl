# Scripting — Requirements

Embedded Lua simulation model, per-context runtime and sandbox, `C_*` host API surface, sim thread execution model, script storage and lifecycle, error/logging semantics.

See [`../README.md`](../README.md). Companions: [`api-contract.md`](./api-contract.md) (exhaustive `C_*` API), [`edge-cases.md`](./edge-cases.md).

**Area boundaries.** Lua API surface and semantics owned here. The in-TUI code editor (vim-modal editing, syntax highlighting, `:script` dialog) is `tui/`. The device/session file *envelope* carrying scripts is `config-session/`; the script-bearing fields (`scripts`, `script_interval`, session `interval`) are specified here because they control scripting behavior. `C_Test`'s Lua-side assertion semantics owned here; the `ferrowl run` **exit-code** contract keyed off logged assertion failures is `cli-headless/`.

---

## Runtime & VM

**SC-R-001** — Scripts execute on a Lua 5.4 VM compiled into the binary; no external interpreter, no dynamic linking to a system Lua.

**SC-R-002** — The Lua API is synchronous and blocking: a `C_*` call completes before the script continues; no script can `await`, yield to, or interact with the host's async runtime.

**SC-R-003** — A script is compiled into a callable function once, when loaded into a Lua context, and invoked with no arguments and no expected return on each execution.

**SC-R-004** — A Lua context owns exactly one VM and a set of loaded scripts keyed by name. Every script in one context shares that context's single global environment.

**SC-R-005** — Loading two scripts under the same name into one context is rejected; the second load fails, nothing overwritten.

---

## Sandbox & available globals

**SC-R-006** — Each sim context loads only the pure-computation Lua standard libraries (`string`, `table`, `math`, `utf8`, `coroutine`) plus the base library.

**SC-R-040** — A sim script is untrusted input, so a sim context (SC-R-006) has no access to host filesystem, shell, environment, or dynamic code loading.

**SC-R-041** — Clock access in a sim context comes from `C_Time`, not `os` (which SC-R-006 does not load).

**SC-R-007** — `io`, `os`, `package`, `debug`, and FFI libraries are not reachable from any sim context; the base library's dynamic-code loaders (`load`, `loadfile`, `dofile`, `loadstring`, `require`) are removed from globals. Indexing any of these sees a `nil` global.

**SC-R-008** — Beyond the standard library subset, the only host-injected globals a script may rely on are the `C_*` modules registered for that context (SC-R-018) and the redirected `print` (SC-R-030).

**SC-R-009** — Dynamic values cross the Lua/host boundary as exactly one of five types: integer, float, string, boolean, nil. Any other Lua value (table, function, userdata, thread) where a host value is expected fails conversion with an error, never coerced or dropped.

**SC-R-042** — Exception to SC-R-009: an OCPP action override *table* is accepted at the boundary, and its scalar entries are flattened per the API contract.

---

## Execution model

**SC-R-010** — Because the Lua VM is not shareable across threads, each sim owner runs its context on a dedicated OS thread that builds the context inside that thread and loops until stopped. The UI event loop and async network runtime never execute Lua directly.

**SC-R-011** — A sim thread is spawned only when at least one script is enabled for its owner; with none, no sim thread exists.

**SC-R-043** — SC-R-035's on-demand single-script execution runs on its own short-lived thread, neither gated on the sim-thread condition of SC-R-011 nor counted as a sim thread.

**SC-R-012** — A sim thread is controlled by a stop flag observed between cycles and, during a cycle, at each firing of the execution hook (SC-R-034, SC-R-046). Setting the flag and joining stops the sim; the sim handle's destruction also stops and joins.

**SC-R-013** — Within each cycle the thread sleeps up to the cycle interval in small chunks, re-checking the stop flag between chunks.

**SC-R-014** — A per-module Modbus sim and the session-level sim run **every** enabled script on **every** cycle, so a script's observable cadence is approximately one execution per cycle interval.

**SC-R-044** — A per-module OCPP sim runs each enabled script at most once per cycle interval, skipping any that ran more recently than the interval, so a script's observable cadence is approximately one execution per cycle interval, as under SC-R-014.

**SC-R-015** — Execution within a cycle is sequential on the sim thread. Relative order within a cycle is unspecified (SC-E-034).

**SC-R-016** — The cycle interval resolves from the owner's configured interval in seconds; non-finite or non-positive falls back to 1.0 s.

**SC-R-045** — A per-module (Modbus or OCPP) cycle interval (SC-R-016) is additionally floored to 0.05 s; the session-level interval has no floor.

**SC-R-017** — Time observed through `C_Time` is measured from the moment the sim thread's context is built. Rebuilding the context (SC-R-024) resets the origin to zero.

**SC-R-035** — An owner supports executing a single script **once, on demand** (script-manager dialog, UI-R-051); the run's other properties are SC-R-052–SC-R-060.

**SC-R-052** — An on-demand run (SC-R-035) builds its own Lua context on its own short-lived thread.

**SC-R-053** — An on-demand run's context (SC-R-052) registers the same `C_*` modules its owner's sim thread would (SC-R-018).

**SC-R-054** — An on-demand run (SC-R-035) loads only the script being run, no other script of the owner's list.

**SC-R-055** — An on-demand run (SC-R-035) calls the loaded script exactly once.

**SC-R-056** — An error raised by an on-demand run (SC-R-035) is logged to the owner's script log.

**SC-R-057** — An on-demand run's thread (SC-R-052) exits once the single call returns.

**SC-R-058** — An on-demand run (SC-R-035) requires no running sim thread; it runs whether or not the owner's sim thread is started.

**SC-R-059** — An on-demand run's context (SC-R-052) shares no Lua state with a sim thread's context (SC-E-039).

**SC-R-060** — An on-demand run (SC-R-035) ignores the script's enabled flag; a disabled script runs.

**SC-R-049** — Errors of an on-demand run (SC-R-035) are logged under a `[run]` prefix, distinct from the `[sim]` prefix of sim diagnostics (SC-R-032), since `ferrowl run --exit-on-error` keys its exit code off `[sim]` (CL-R-031).

---

## Host module availability per context

**SC-R-018** — `C_*` modules registered into a context depend on the sim owner:

| Sim owner | Registered modules |
|---|---|
| Modbus module | `C_Register`, `C_Time`, `C_Test`, `C_Log`, `print` |
| OCPP module (client or server) | `C_OCPP`, `C_Time`, `C_Test`, `C_Log`, `print` |
| Session-level | `C_Module`, `C_Time`, `C_Test`, `C_Log`, `print` |

**SC-R-019** — `C_Register` is reachable only from a Modbus module's own sim; `C_OCPP` only from an OCPP module's own sim; `C_Module` only from the session-level sim. A script naming a module not registered in its context fails at run time with a Lua "attempt to index a nil value" style error.

**SC-R-020** — The session-level sim reaches every other module's state through `C_Module`, which resolves modules by name and hands out the same `C_Register`-shaped or `C_OCPP`-shaped accessor those modules expose to their own sims.

**SC-R-077** — `C_Module:Get(name)` in the session-level sim resolves `name` against the session's module set as it stands at the moment of the call, so a module added after the session sim started is returned on the next call.

**SC-R-078** — A `ModuleHandle` (SC-E-038) and the `Register()` accessor obtained from it address the module instance current at the moment of each call, so after a Modbus reconfigure (MB-R-089) the next call reaches the new instance's state and never raises `unknown register`, `unknown module` or an unknown-field error for a name the new instance defines; an `OCPP()` accessor follows its module only across an in-place reconfiguration (OC-R-085), which keeps the module instance, and any change of the instance behind the name is SC-R-080.

**SC-R-080** — Once the OCPP module instance an `OCPP()` accessor (or an `Accessor` derived from it) was obtained from is no longer the instance behind its module name (replaced by a role or version change, OC-R-085, SC-R-076; or its module closed, with or without a module reopened under the same name), every call on that accessor raises `module '<name>' was replaced; call OCPP() again`, while the `ModuleHandle` it came from stays live and its next `OCPP()` call returns an accessor for the current instance (or raises `unknown module` per SC-E-038 when none exists).

**SC-R-079** — Adding, editing, rebuilding or replacing a session module neither stops nor restarts the session-level sim thread and leaves its Lua globals intact.

**SC-R-021** — An OCPP **server** module runs its scripts as a client module does; scripting is not limited to the client role.

---

## Script lifecycle

**SC-R-022** — A script is defined by a name, a code body (default empty), and an enabled flag (default enabled).

**SC-R-061** — Every enabled script (SC-R-022) is handed to a sim thread; the code body is not a selection criterion.

**SC-R-062** — A script's persisted shape (SC-R-022) is `config-session/`'s envelope.

**SC-R-023** — Scripts are stored inline in device/session config files, not external `.lua` files.

**SC-R-024** — Editing a script, toggling its enabled flag, changing the cycle interval, or a Modbus module's `:reload` (which reloads the module's scripts) stops any running sim thread and starts a fresh one from the current enabled-script set (or leaves it stopped if none remain). The new context is fresh: all globals reset.

**SC-R-070** — Adding, editing or deleting a register at runtime (MB-R-088) neither stops nor restarts the module's sim thread and leaves its Lua globals intact; only the edits listed in SC-R-024 restart a sim.

**SC-R-081** — Reconfiguring a Modbus module's endpoint or role (MB-R-089) neither stops nor restarts the module's sim thread and leaves its Lua globals intact (SC-R-070).

**SC-R-075** — An in-place OCPP module reconfiguration (OC-R-085) neither stops nor restarts the module's sim thread and leaves its Lua globals intact.

**SC-R-076** — A role or version change of an OCPP module (OC-R-085) stops the module's running sim thread and starts a fresh one against the replacement view, with all globals reset as under SC-R-024, because the `C_OCPP` shape and action set are fixed per role and version (SC-E-044).

**SC-R-025** — Legacy per-register `update` snippets in an older Modbus device config are migrated on load into named, enabled entries in the module's script list, preserving code.

**SC-R-026** — A Modbus module's sim runs independently of the network instance's connection state.

---

## Script templates

**SC-R-036** — The binary carries a fixed library of Lua script templates, compiled in at build time.

**SC-R-063** — Each built-in script template (SC-R-036) has a name, a one-line description, a Lua code body, and the set of script contexts (Modbus, OCPP client, OCPP server, session) it applies to.

**SC-R-064** — A built-in template (SC-R-036) becomes a script only by being copied into a script list; nothing loads template code from disk at run time (SC-R-023).

**SC-R-037** — Every template's code body is loadable by the Lua runtime; a template failing to compile is a build/test failure.

**SC-R-038** — `C_Test` exposes `Assert(cond, msg)`, which raises runtime error `assertion failed: <msg>` when `cond` is Lua-falsy (`nil` or `false`), otherwise returns with no effect; every other value, including `0` and `""`, passes.

**SC-R-050** — `C_Test` exposes `Fail(msg)`, which always raises `assertion failed: <msg>`.

**SC-R-051** — The `assertion failed:` prefix (SC-R-038, SC-R-050) is the text a headless runner keys its exit code off ([`../cli-headless/requirements.md`](../cli-headless/requirements.md)).

---

## State access semantics

**SC-R-027** — A value read from a register or OCPP state field returns to Lua as its natural type (number, string, boolean). A value written from Lua applies to host state per the API contract, type/range mismatches failing rather than coercing ([`edge-cases.md`](./edge-cases.md) `## State access semantics`).

**SC-R-028** — A register or OCPP state write from Lua applies to the module's in-memory/observed state only. A Modbus Lua write never emits a Modbus write command (unlike interactive `:set`); a written value on a client is transient and may be overwritten by the next poll (SC-E-033).

**SC-R-029** — Host state reached from Lua is guarded by the same locks the network task uses, so each `Get`/`Set`/action call is atomic against concurrent host access. No cross-call transaction: a read-then-write may interleave with a concurrent host update.

**SC-R-067** — `C_Register` `Get`, `Set` and `Has` resolve `name` against the module's register definitions as they stand at the moment of the call, never against a set captured when the sim's Lua context was built.

**SC-R-068** — A register added to a Modbus module at runtime (MB-R-088) is reachable by its name from a sim thread running since before the add, on that sim's next `C_Register` call, with no sim restart (SC-R-067).

**SC-R-069** — A register whose name, address, kind or format is edited at runtime (MB-R-088) is reachable by its post-edit name on a running sim's next `C_Register` call, and `Get`/`Set` read and write at its post-edit address in its post-edit format (SC-R-067).

**SC-R-071** — A `C_Register`-shaped accessor obtained in the session-level sim through `C_Module:Get(name):Register()` resolves register names per SC-R-067, so SC-R-068 and SC-R-069 hold for it unchanged.

**SC-R-072** — `C_OCPP` `Get`, `Set`, `<Action>` and the scoping methods (`Connector`, `GetConnectors`, `ChargingStation`, `GetChargingStations`) resolve against the OCPP module's state as it stands at the moment of the call, never against state captured when the sim's Lua context was built.

**SC-R-073** — After an in-place OCPP module reconfiguration (OC-R-085), every field name a running sim's `C_OCPP` `Get`/`Set` accepted before the edit is accepted again on its next call at any scope still present after the edit (SC-R-072).

**SC-R-074** — A connector added to an OCPP client module at runtime through the client view's connector add is listed by a running sim's next `GetConnectors` call and reachable through `Connector` (SC-R-072).

---

## Logging & error handling

**SC-R-030** — The Lua global `print` is redirected to the sim owner's log sink, never real stdout. `print` follows Lua semantics: each argument converted with tostring (honoring `__tostring`), joined by tabs, emitted as one Info line.

**SC-R-031** — `C_Log:Info/Warn/Error` and `print` output from a module's sim route to that module's dedicated **script** log (distinct from its connection/traffic log), and, for a Modbus module, also to the module's file log sink when configured.

**SC-R-032** — A runtime error raised by one script (uncaught `error`, failed `C_Test:Assert`/`Fail`, rejected state write, malformed OCPP override table) does not crash the sim thread and does not prevent other scripts in the context from running that cycle. Every such error is written to the owner's log at Error level with a sim/Lua prefix.

**SC-R-033** — If building the context itself fails (a Lua **syntax** error in any script, or a duplicate name, SC-R-005), the sim thread logs a single "failed to build Lua context" error and does not loop; **no** script in that context runs. Load-time failure is all-or-nothing per context; run-time failure (SC-R-032) is isolated per script.

**SC-R-034** — Every Lua context, sim thread (SC-R-010) and on-demand run (SC-R-035) alike, installs an execution hook via mlua's `every_nth_instruction`, firing every 1,000 instructions. The instruction count is fixed, not exposed as config key or CLI flag.

**SC-R-046** — On each firing of the execution hook (SC-R-034), a sim-thread context checks the stop flag and, if set, raises a Lua error to unwind the executing script.

**SC-R-047** — On each firing of the execution hook (SC-R-034), every context unconditionally checks elapsed wall-clock since the current cycle (or the on-demand run's single execution) began, raising a Lua error past a fixed 1,000 ms cap. The cap is fixed, not exposed as config key or CLI flag.

**SC-R-048** — The execution hook (SC-R-034) enforces no memory ceiling (SC-E-032).

**SC-R-039** — An error raised by the hook (stop-flag or wall-clock) flows through the same per-script path as any runtime error: SC-R-032's isolation and `[sim]` logging for a sim thread, SC-R-049's `[run]` logging for an on-demand run.

**SC-R-065** — A hook-raised error in one script (SC-R-039) does not stop the cycle: the sim thread's other scripts still run that cycle.

**SC-R-066** — A hook-raised stop (SC-R-039) lets a pending stop-and-join complete within SC-R-047's fixed 1,000 ms cap, measured from the hook firing.
