# Scripting — Requirements

Normative behavior of the scripting capability area: the embedded Lua simulation
model, the per-context runtime and sandbox, the `C_*` host API surface, the sim
thread execution model, script storage and lifecycle, and error/logging
semantics.

IDs are stable and append-only (`SC-R-nnn`). See [`../README.md`](../README.md).

Companion documents: [`api-contract.md`](./api-contract.md) (the exhaustive
`C_*` API — the contract script authors write against),
[`edge-cases.md`](./edge-cases.md) (boundary and error behavior, and stated
known limitations, including the sandbox boundary).

**Area boundaries.** The Lua API surface and its semantics are owned here. The
in-TUI code editor (vim-modal editing, syntax highlighting, the `:script`
dialog) belongs to `tui/`. The device/session file *envelope* that carries
scripts belongs to `config-session/`; the script-bearing config fields
(`scripts`, `script_interval`, session `interval`) are specified here because
they control scripting behavior. `C_Test`'s Lua-side assertion semantics are
owned here; the `ferrowl run` **exit-code** contract that keys off logged
assertion failures belongs to `cli-headless/`.

---

## Runtime & VM

**SC-R-001** — Scripts shall execute on a real Lua 5.4 virtual machine compiled
into the binary (no external Lua interpreter, no dynamic linking to a system
Lua).

**SC-R-002** — The Lua API shall be synchronous and blocking: a `C_*` method
call shall complete before the calling script continues, and no script shall be
able to `await`, yield to, or otherwise interact with the host's async runtime.

**SC-R-003** — A script shall be compiled into a callable function once, at the
time it is loaded into a Lua context, and shall be invoked with no arguments and
no expected return value on each execution.

**SC-R-004** — A Lua context shall own exactly one Lua VM and a set of loaded
scripts keyed by name. Every script in one context shall share that context's
single global environment; a global set by one script shall be visible to every
other script in the same context.

**SC-R-005** — Loading two scripts under the same name into one context shall be
rejected; the second load shall fail and no script shall silently overwrite
another.

---

## Sandbox & available globals

**SC-R-006** — Each sim context shall load only the pure-computation Lua standard
libraries — `string`, `table`, `math`, `utf8`, `coroutine` — plus the always-present
base library. A sim script is untrusted input (it arrives in a device or session
config), so it shall have no access to the host filesystem, shell, environment, or
dynamic code loading. The clock access a sim legitimately needs is provided by the
sandboxed `C_Time` module, not by `os`.

**SC-R-007** — The `io`, `os`, `package`, `debug`, and FFI libraries shall not be
reachable from any sim context, and the base library's dynamic-code loaders
(`load`, `loadfile`, `dofile`, `loadstring`, `require`) shall be removed from the
globals. A script indexing any of these shall see a `nil` global.

**SC-R-008** — Beyond the standard library subset, the only host-injected
globals a script may rely on shall be the `C_*` host modules registered for that
context (per SC-R-018) and the redirected `print` (SC-R-030). No other bespoke
host global shall be injected.

**SC-R-009** — Dynamic values shall cross the Lua/host boundary as exactly one of
five types: integer, floating-point number, string, boolean, or nil. Any other
Lua value (table, function, userdata, thread) passed where a host value is
expected shall fail conversion with an error rather than being coerced or
silently dropped. (The one exception is an OCPP action override *table*, whose
scalar entries are flattened per the API contract.)

---

## Execution model

**SC-R-010** — Because the Lua VM is not shareable across threads, each sim owner
shall run its Lua context on a dedicated OS thread that builds the context inside
that thread and loops until stopped. The UI event loop and the async network
runtime shall never execute Lua directly.

**SC-R-011** — A sim thread shall be spawned only when at least one script is
enabled for its owner; with no enabled script, no sim thread shall exist.

**SC-R-012** — A sim thread shall be controlled by a stop flag it observes only
between execution cycles. Setting the flag and joining the thread shall stop the
sim; the sim handle's destruction shall also stop and join it.

**SC-R-013** — Within each cycle a sim thread shall sleep up to the configured
cycle interval in small chunks, re-checking the stop flag between chunks, so a
stop request during the idle portion of a cycle is observed promptly.

**SC-R-014** — A per-module Modbus sim and the session-level sim shall run **every**
enabled script on **every** cycle. A per-module OCPP sim shall run each enabled
script at most once per cycle interval, skipping any script that already ran more
recently than the interval. In all cases the observable cadence of a given script
shall be approximately one execution per cycle interval.

**SC-R-015** — Script execution within a single cycle shall be sequential on the
sim thread (no script in a context runs concurrently with another script in the
same context). The relative order in which a context's scripts run within a cycle
is unspecified (see [`edge-cases.md`](./edge-cases.md) §5.2).

**SC-R-016** — The cycle interval shall be resolved from the owner's configured
interval in seconds, sanitized so that a non-finite or non-positive value falls
back to the default of 1.0 s. A per-module (Modbus or OCPP) interval shall
additionally be floored to 0.05 s; the session-level interval shall have no
floor.

**SC-R-017** — Time observed through `C_Time` shall be measured from the moment
the sim thread's context is built. Rebuilding the context (SC-R-024) shall reset
this origin to zero.

---

## Host module availability per context

**SC-R-018** — The set of `C_*` modules registered into a context shall depend on
the sim owner:

| Sim owner | Registered modules |
|---|---|
| Modbus module | `C_Register`, `C_Time`, `C_Test`, `C_Log`, `print` |
| OCPP module (client or server) | `C_OCPP`, `C_Time`, `C_Test`, `C_Log`, `print` |
| Session-level | `C_Module`, `C_Time`, `C_Test`, `C_Log`, `print` |

**SC-R-019** — `C_Register` shall be reachable only from a Modbus module's own
sim; `C_OCPP` only from an OCPP module's own sim; `C_Module` only from the
session-level sim. A script that names a module not registered in its context
shall fail at run time with a Lua "attempt to index a nil value" style error,
not silently no-op.

**SC-R-020** — The session-level sim shall reach every other module's state
indirectly through `C_Module`, which resolves modules by name and hands out the
same `C_Register`-shaped or `C_OCPP`-shaped accessor those modules expose to
their own sims.

**SC-R-021** — An OCPP **server** module shall run its scripts the same way a
client module does (both roles are simulated); scripting shall not be limited to
the client role.

---

## Script lifecycle

**SC-R-022** — A script shall be defined by a name, a code body (default empty),
and an enabled flag (default enabled). Only enabled scripts with a non-empty code
body shall be handed to a sim thread. (The persisted shape of this definition is
part of `config-session/`'s envelope; its meaning is specified here.)

**SC-R-023** — Scripts shall be stored inline in the device/session configuration
files, not as external `.lua` files.

**SC-R-024** — Editing a script, toggling its enabled flag, or changing the cycle
interval shall stop any running sim thread and start a fresh one built from the
current enabled-script set (or leave it stopped if none remain enabled). The new
thread's Lua context shall be fresh: all globals reset. A running sim shall not
pick up an edited script without such a restart.

**SC-R-025** — Legacy per-register `update` script snippets found in an older
Modbus device config shall be migrated on load into named, enabled entries in the
module's script list, preserving their code, and shall thereafter run through the
same sim model as any other script.

**SC-R-026** — A Modbus module's sim shall run independently of the module's
network instance connection state: enabled scripts shall execute whether or not
the client is connected or the server is bound.

---

## State access semantics

**SC-R-027** — A value read from a register or an OCPP state field shall be
returned to Lua as its natural type (number for numeric fields, string for
strings, boolean for booleans). A value written from Lua shall be applied to the
host state per the API contract, with type/range mismatches failing rather than
silently coercing (see [`api-contract.md`](./api-contract.md) and
[`edge-cases.md`](./edge-cases.md) §2).

**SC-R-028** — A register or OCPP state write from Lua shall be applied to the
module's in-memory/observed state only. A Modbus Lua write shall never emit a
Modbus write command on the wire (unlike an interactive `:set`); a written value
on a client is therefore transient and may be overwritten by the next poll (see
[`edge-cases.md`](./edge-cases.md) §5.2).

**SC-R-029** — Host state reached from Lua shall be guarded by the same locks the
network task uses, so each individual `Get`/`Set`/action call is atomic against
concurrent host access. No cross-call transaction is provided: a script's
read-then-write may interleave with a concurrent host update between the two
calls.

---

## Logging & error handling

**SC-R-030** — The Lua global `print` shall be redirected to the sim owner's log
sink (never real stdout, which would corrupt the TUI). `print` shall follow Lua
semantics: each argument converted with tostring (honoring `__tostring`) and the
results joined by tab characters, emitted as one log line at Info level.

**SC-R-031** — `C_Log:Info/Warn/Error` and `print` output from a module's sim
shall be routed to that module's dedicated **script** log (distinct from the
module's connection/traffic log), and, for a Modbus module, additionally to the
module's file log sink when one is configured.

**SC-R-032** — A runtime error raised by one script (an uncaught `error`, a
failed `C_Test:Assert`/`Fail`, a rejected state write, a malformed OCPP override
table) shall not crash the sim thread and shall not prevent other scripts in the
same context from running that cycle. Every such error shall be collected and
written to the sim owner's log at Error level, prefixed to mark it as a sim/Lua
diagnostic.

**SC-R-033** — If building the Lua context itself fails — a Lua **syntax** error
in any script, or a duplicate script name (SC-R-005) — the entire sim thread
shall log a single "failed to build Lua context" error and shall not loop; **no**
script in that context shall run. A load-time failure is therefore all-or-nothing
per context, whereas a run-time failure (SC-R-032) is isolated per script.

**SC-R-034** — There shall be no execution-time limit, instruction-count limit,
or memory ceiling on a script. This is a stated constraint, not an oversight; see
[`edge-cases.md`](./edge-cases.md) §5.1.
