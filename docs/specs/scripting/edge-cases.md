# Scripting — Edge Cases and Known Limitations

Boundary behavior, error semantics, and the constraints that are **intentional**
or **known**. Everything in §5 is working as implemented; it is recorded here so
it is not mistaken for an oversight and silently "fixed".

---

## 1. Load-time vs run-time errors

| Condition | Behavior |
|---|---|
| A script contains a Lua **syntax** error | the whole context fails to build; the sim thread logs one "failed to build Lua context" error and does not loop. **No** script in that context runs — including the syntactically valid ones |
| Two enabled scripts share the same name | the context build fails the same way (all-or-nothing); the in-TUI editor prevents creating duplicate names, but a hand-edited config file can still trigger this |
| A script raises at **run time** (`error`, failed `C_Test:Assert`/`Fail`, rejected write, malformed override table) | only that script's cycle is aborted; the error is logged at Error level with a sim/Lua prefix, and every other script in the context still runs that cycle |
| A script raises every cycle | it is logged every cycle (a tight loop can flood the log); it is never disabled automatically |
| A script references a `C_*` module not registered in its context (e.g. `C_Register` from an OCPP sim) | run-time error: indexing a nil global. Logged, cycle continues |

## 2. State access and type coercion

| Condition | Behavior |
|---|---|
| `C_Register:Get` on an unknown name | error `unknown register '<name>'` |
| `C_Register:Get` on a virtual register never written | error `virtual register '<name>' not set` |
| `C_Register:Get` on a fixed register whose cells are unreadable | error `register '<name>' not readable` |
| `C_Register:Set` with a `nil` value | error `cannot Set nil value` |
| `C_Register:Set` integer out of the format's range (e.g. `100000` onto `U16`) | error, no truncation |
| `C_Register:Set` fractional float onto an integer format (e.g. `3.5`) | error `not a whole number` |
| `C_Register:Set` whole-number float onto an integer format (e.g. `42.0`) | accepted, stored as the integer |
| `C_Register:Set` boolean | treated as integer `0`/`1`, then coerced to the format |
| `C_Register:Set` string | parsed through the register's string-input codec (numeric-literal rules apply) |
| `C_Register:Set` on a **virtual** register | the declared format is ignored: an in-range integer is stored as 64-bit int, an out-of-range integer falls back to float, a float stays float, a string is codec-parsed |
| `C_Register:Has` on any name | returns `true`/`false`, never errors; reflects *definition*, not readability |
| Passing a Lua table/function where a scalar is expected (`Set`, `C_Statics:Get` arg, action override value) | conversion error `expected number, string or boolean` |
| `C_OCPP` action override table with a nested table value | the whole action call raises |
| `C_OCPP` server `ChargingStation`/`Connector` for an unknown station/connector | returns `nil` (not an error); the script indexing that `nil` is its own error |
| `C_Module:Get` for an unknown/removed module | raises `unknown module '<name>'` |
| `ModuleHandle:Register()` on a non-modbus module / `:OCPP()` on a non-ocpp module | raises `is not a modbus module` / `is not an ocpp module` |

## 3. Concurrency with the network task

| Condition | Behavior |
|---|---|
| A script reads/writes register state while the Modbus client is polling or the server is answering | each `Get`/`Set` is individually lock-guarded and atomic; the sim thread and the network task share the same locked store |
| A script does read-modify-write across two calls | not transactional: a concurrent host update can land between the read and the write |
| A Lua write to a register on a Modbus **client** | writes the module's in-memory store only; it does **not** send a Modbus write command, and the next poll may overwrite it |
| A Lua write to a register on a Modbus **server** | updates the served store; a remote master reads the new value |
| A script runs while its module's network instance is stopped/disconnected | the sim keeps running; state writes still land in the store (there is just nothing on the wire) |

## 4. Sim lifecycle

| Condition | Behavior |
|---|---|
| All scripts disabled (or none defined) | no sim thread exists |
| A script edited / toggled / interval changed | the running sim thread is stopped and a fresh one started; **all Lua globals reset**; a mid-flight in-memory global is lost by design |
| Cycle interval set to a non-finite/≤0 value in config | falls back to the 1.0 s default |
| Per-module interval set below 0.05 s | floored to 0.05 s; the session-level interval has no floor |
| `C_Time:Get`/`GetMs` right after a restart | counts from ~0 again (origin is context build time) |

---

## 5. Known limitations and findings

### 5.1 No execution ceiling — an infinite loop hangs, and joining it blocks

There is no execution-time limit, no instruction-count hook, and no memory
ceiling configured on the VM. A script that loops forever never returns control
to the sim loop, so:

- That module's dedicated sim thread spins forever, pinning one CPU core. Other
  modules' sim threads and the UI keep running **on their own**.
- **But** the stop flag is only observed *between* cycles. Because the runaway
  script never lets its cycle finish, the stop-and-join that a script edit, a tab
  close, a module reconfigure, or app shutdown performs will **block on the join
  forever** — freezing whichever thread requested the stop (which, for an edit or
  a tab close, is the UI thread). So the blast radius of a runaway script is
  larger than just its own thread: it is any subsequent operation that must join
  that thread.

Working as implemented given the absence of any budget; recorded so it is not
mistaken for a hang bug. A real fix requires an instruction/time budget on the VM
so a stop request can interrupt a running cycle.

### 5.2 Lua register writes are store-only (client)

A `C_Register:Set` on a Modbus client updates the module's in-memory store
directly and never emits a Modbus write command, unlike an interactive `:set`.
On a client the written value is therefore transient — the next successful poll
of that address overwrites it. This is intended (a sim script models the device's
*own* state, it does not drive the master), but it means a client-side Lua write
is not observable by the remote peer.

### 5.3 Script execution order within a cycle is unspecified

A context stores its scripts in a hash map and runs them in hash-iteration order,
which is not the config's definition order and is not stable. Scripts must not
depend on running before or after a sibling script within the same cycle. Data
they share must be tolerant of arbitrary intra-cycle ordering.

### 5.4 A fresh Lua state on every restart

Every script/interval edit rebuilds the context from scratch, so there is no
persistent Lua state across restarts and no persistent state across a config
reload. The only durable state a script can accumulate is what it writes into
host register/OCPP state via the `C_*` API. In-Lua globals live only as long as
the current sim thread.

### 5.5 No script return value, no scheduling primitives

A script is invoked as a nullary function whose return value is ignored. There is
no per-script scheduling beyond the single cycle interval shared by the whole
context, no timers, no callbacks, and no way for a script to yield or sleep
cooperatively — a script that wants to act less often than every cycle must gate
itself using `C_Time`.

### 5.6 `C_Statics` is unreachable

The `C_Statics` module is part of the scripting library but no ferrowl sim
registers it, so no ferrowl script can call it. It is specified in the API
contract for completeness only.

### 5.7 Session `C_Module` staleness is surfaced, not cached

A `ModuleHandle` obtained from `C_Module:Get` re-resolves its target on every
method call. If the module is removed from the session between obtaining the
handle and using it, the next call raises `unknown module` rather than returning a
stale accessor. Scripts holding a handle across cycles must tolerate this.
