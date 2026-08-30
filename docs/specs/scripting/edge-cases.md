# Scripting — Edge Cases and Known Limitations

Boundary behavior, error semantics, intentional or known constraints. §5 is working as implemented; recorded so it is not "fixed".

---

## 1. Load-time vs run-time errors

| Condition | Behavior |
|---|---|
| Script contains a Lua **syntax** error | whole context fails to build; sim thread logs one "failed to build Lua context" error, does not loop. **No** script in that context runs — including valid ones |
| Two enabled scripts share a name | context build fails the same way (all-or-nothing); the in-TUI editor prevents duplicate names, a hand-edited file can still trigger it |
| Script raises at **run time** (`error`, failed `C_Test:Assert`/`Fail`, rejected write, malformed override table) | only that script's cycle aborted; logged at Error level with a sim/Lua prefix; every other script still runs that cycle |
| Script raises every cycle | logged every cycle (a tight loop can flood the log); never disabled automatically |
| Script references a `C_*` module not registered in its context (e.g. `C_Register` from an OCPP sim) | run-time error: indexing a nil global. Logged, cycle continues |

## 2. State access and type coercion

| Condition | Behavior |
|---|---|
| `C_Register:Get` on an unknown name | error `unknown register '<name>'` |
| `C_Register:Get` on a virtual register never written | error `virtual register '<name>' not set` |
| `C_Register:Get` on a fixed register whose cells are unreadable | error `register '<name>' not readable` |
| `C_Register:Set` with `nil` | error `cannot Set nil value` |
| `C_Register:Set` integer out of the format's range (e.g. `100000` onto `U16`) | error, no truncation |
| `C_Register:Set` fractional float onto an integer format (e.g. `3.5`) | error `not a whole number` |
| `C_Register:Set` whole-number float onto an integer format (e.g. `42.0`) | accepted, stored as the integer |
| `C_Register:Set` boolean | treated as integer `0`/`1`, then coerced to the format |
| `C_Register:Set` string | parsed through the register's string-input codec (numeric-literal rules apply) |
| `C_Register:Set` on a **virtual** register | declared format ignored: in-range integer stored as 64-bit int, out-of-range integer falls back to float, float stays float, string codec-parsed |
| `C_Register:Has` on any name | `true`/`false`, never errors; reflects *definition*, not readability |
| Lua table/function where a scalar is expected (`Set`, `C_Statics:Get` arg, action override value) | conversion error `expected number, string or boolean` |
| `C_OCPP` action override table with a nested table value | whole action call raises |
| `C_OCPP` server `ChargingStation`/`Connector` for an unknown station/connector | returns `nil` (not an error); indexing that `nil` is the script's own error |
| `C_Module:Get` for an unknown/removed module | raises `unknown module '<name>'` |
| `ModuleHandle:Register()` on a non-modbus module / `:OCPP()` on a non-ocpp module | raises `is not a modbus module` / `is not an ocpp module` |

## 3. Concurrency with the network task

| Condition | Behavior |
|---|---|
| Script reads/writes register state while the client polls or the server answers | each `Get`/`Set` individually lock-guarded and atomic; sim thread and network task share the same locked store |
| Script does read-modify-write across two calls | not transactional: a concurrent host update can land between |
| Lua write to a register on a Modbus **client** | writes the in-memory store only; **no** Modbus write command; next poll may overwrite |
| Lua write to a register on a Modbus **server** | updates the served store; a remote master reads the new value |
| Script runs while its network instance is stopped/disconnected | sim keeps running; writes land in the store (nothing on the wire) |

## 4. Sim lifecycle

| Condition | Behavior |
|---|---|
| All scripts disabled (or none) | no sim thread |
| Script edited / toggled / interval changed | sim thread stopped and a fresh one started; **all globals reset**; a mid-flight global is lost by design |
| Cycle interval non-finite/≤0 in config | falls back to 1.0 s |
| Per-module interval below 0.05 s | floored to 0.05 s; session-level interval has no floor |
| `C_Time:Get`/`GetMs` right after a restart | counts from ~0 again (origin = context build time) |

---

## 5. Known limitations and findings

### 5.1 Execution ceiling — an infinite loop is interrupted mid-cycle

Every context installs an execution hook (SC-R-034, `every_nth_instruction` every 1,000 instructions) checking the stop flag and enforcing a fixed 1,000 ms wall-clock cap. A script looping forever is interrupted the first time the hook fires after either condition is met:

- A pending stop-and-join (script edit, tab close, module reconfigure, app shutdown): the hook raises to unwind the runaway script, so the cycle ends and the join completes promptly. Before this hook the stop flag was observed only *between* cycles, so a runaway script blocked any thread joining it — typically the UI thread.
- Independent of any stop request, the 1,000 ms cap aborts a cycle (or on-demand run, SC-R-035) that runs long, bounding worst-case CPU pinning to just over the cap with no operator action.

Both constants fixed (SC-R-034), not configurable. No memory ceiling: a script allocating without bound (ever-growing table) is not stopped — the hook checks instruction count and wall-clock only.

### 5.2 Lua register writes are store-only (client)

`C_Register:Set` on a Modbus client updates the in-memory store and never emits a Modbus write command, unlike `:set`. The value is transient — the next successful poll of that address overwrites it. Intended (a sim script models the device's *own* state, not the master), but a client-side Lua write is not observable by the remote peer.

### 5.3 Script execution order within a cycle is unspecified

A context stores scripts in a hash map and runs them in hash-iteration order — not definition order, not stable. Scripts must not depend on running before or after a sibling within a cycle; shared data must tolerate arbitrary intra-cycle ordering.

### 5.4 A fresh Lua state on every restart

Every script/interval edit rebuilds the context from scratch: no persistent Lua state across restarts or config reload. The only durable state is what a script writes into host register/OCPP state via `C_*`. Lua globals live only as long as the current sim thread.

### 5.5 No script return value, no scheduling primitives

A script is a nullary function whose return is ignored. No per-script scheduling beyond the context's single cycle interval, no timers, no callbacks, no cooperative yield or sleep — a script acting less often than every cycle must gate itself with `C_Time`.

### 5.6 `C_Statics` is unreachable

`C_Statics` is in the scripting library but no ferrowl sim registers it, so no ferrowl script can call it. Specified in the API contract for completeness only.

### 5.7 Session `C_Module` staleness is surfaced, not cached

A `ModuleHandle` from `C_Module:Get` re-resolves its target on every method call. If the module is removed between obtaining and using the handle, the next call raises `unknown module` rather than returning a stale accessor. Scripts holding a handle across cycles must tolerate this.

### 5.8 A run-once (`e`) executes in an isolated Lua VM

The on-demand single-script execution (SC-R-035, `e` in the script-manager dialog) builds a **fresh** context on its own thread. It shares no Lua state with the owner's running sim: sim globals are invisible to the run, the run's globals are discarded when its thread exits, `C_Time` restarts from zero. A script depending on state built over previous sim cycles behaves differently under `e` — an isolated test run, not a step of the sim loop.

The run touches the same shared register/charging-station state as a concurrent sim, serialized only by per-operation locks. A run-once and a sim cycle interleaving writes to the same register is possible and not prevented.
