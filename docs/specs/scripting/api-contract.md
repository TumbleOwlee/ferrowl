# Scripting — API Contract

Exhaustive `C_*` Lua module API scripts are written against: every module, method, arguments, return, error behavior. Method names, argument shapes, return types are contract and shall not change without a spec change. Per [`../README.md`](../README.md)'s ownership rule, script-bearing config fields are specified here and in `requirements.md`; `config-session/` owns only the envelope.

---

## 1. Value types across the boundary

Every value between Lua and host is one of five types:

| Host type | Lua type | Req |
|---|---|---|
| integer | `number` (integer) | SC-R-009 |
| float | `number` (float) | SC-R-009 |
| string | `string` | SC-R-009 |
| boolean | `boolean` | SC-R-009 |
| nil | `nil` | SC-R-009 |

Any other Lua value (`table`, `function`, `userdata`, `thread`) where a scalar is expected raises a conversion error. Sole structured argument: the optional OCPP action **override table** (``## 4. `C_OCPP` — OCPP state access and action dispatch``), whose entries are flat scalars.

Call syntax is always colon form: `C_Module:Method(args)` (module is implicit `self`). Dot form (`C_Module.Method`) yields the raw function without `self` and is not contract.

---

## 2. Modules available per script context

A script sees only the modules registered for its sim owner (`requirements.md` SC-R-018):

| Context | `C_Register` | `C_OCPP` | `C_Module` | `C_Time` | `C_Test` | `C_Log` | `print` | Req |
|---|---|---|---|---|---|---|---|---|
| Modbus module | ✓ | | | ✓ | ✓ | ✓ | ✓ | SC-R-018 |
| OCPP module | | ✓ | | ✓ | ✓ | ✓ | ✓ | SC-R-018 |
| Session-level | | | ✓ | ✓ | ✓ | ✓ | ✓ | SC-R-018 |

`C_Statics` (``## 9. `C_Log` and `C_Statics` ``) is library surface but **not** registered by any ferrowl sim; no ferrowl script can call it.

---

## 3. `C_Register` — Modbus register access

In a Modbus module's sim, and via `C_Module:Get(name):Register()` from the session-level sim.

| Method | Signature | Returns | Errors | Req |
|---|---|---|---|---|
| `Get` | `Get(name)` | decoded value as `number`/`string`/`boolean` | unknown name; fixed-address register whose cells are not readable; decode failure; virtual register not yet set | SC-R-019, SC-R-027 |
| `Set` | `Set(name, value)` | nothing | unknown name; type/range mismatch between `value` and the format (``## 6. Type coercion on `C_Register:Set` ``); `nil` value; fixed-address write the store rejects as not writable | SC-R-019, SC-R-027, SC-R-028 |
| `Has` | `Has(name)` | `boolean` — whether a register of that name is defined | never (missing name → `false`) | SC-R-019 |

`name` is the configured register name, not an address. `Get`/`Set` exchange the **raw, unscaled** stored value (display resolution not applied), so a `Set` value round-trips through `Get` unchanged (SC-R-027).

Writes go to the in-memory store only; no Modbus command (`requirements.md` SC-R-028).

---

## 4. `C_OCPP` — OCPP state access and action dispatch

In an OCPP module's sim, and via `C_Module:Get(name):OCPP()` from the session-level sim. Three shapes depending on host module; all share the `Get`/`Set`/`<Action>` surface.

### 4.1 Shared surface (all shapes, and every `Accessor`)

| Method | Signature | Returns | Errors | Req |
|---|---|---|---|---|
| `Get` | `Get(name)` | state field as `number`/`string`/`boolean` | unknown field for this scope | SC-R-019, SC-R-027 |
| `Set` | `Set(name, value)` | nothing | field not settable for this scope | SC-R-019, SC-R-027, SC-R-028 |
| `<Action>` | `<Action>(overrides?)` | `boolean` — `true` if enqueued | malformed override table (non-string key or non-scalar value) raises | SC-R-042 |

One `<Action>` method per action name the host module exposes for its version; the set is version-specific, defined by the OCPP area (`ocpp/api-contract.md`). Calling an action **enqueues** it on the module's action queue for the owning view (or headless runner) to send; the Lua call does not perform the request and returns once queued (SC-R-009).

`overrides` is an optional flat table of `name = scalar` pairs merged over the action's default payload. Missing table = no overrides. Nested tables or non-scalar values raise (SC-R-042).

### 4.2 Flat shape

Bare `Get`/`Set`/`<Action>` address a single state scope. No scoping methods.

### 4.3 Client shape (charging station)

Bare `Get`/`Set`/`<Action>` address the charge-point (CS) level. Additionally:

| Method | Signature | Returns | Req |
|---|---|---|---|
| `Connector` | `Connector(id)` | `Accessor` scoped to connector `id`, with its own `Get`/`Set`/`<Action>` | — |
| `GetConnectors` | `GetConnectors()` | array of connector ids (`number`) | — |

An action on a connector `Accessor` is enqueued at that connector's scope; on the bare module at CS scope.

### 4.4 Server shape (CSMS spanning many stations)

Access keyed by station identity.

| Method | Signature | Returns | Req |
|---|---|---|---|
| `GetChargingStations` | `GetChargingStations()` | sorted array of station identity strings | — |
| `GetConnectors` | `GetConnectors(cs)` | sorted array of connector ids for station `cs` (empty if unknown) | — |
| `ChargingStation` | `ChargingStation(cs)` | CS-level `Accessor` for `cs`, or `nil` if unknown | — |
| `Connector` | `Connector(cs, id)` | connector `Accessor` for `(cs, id)`, or `nil` if unknown | — |

Unknown stations/connectors resolve to `nil` (not an error); indexing that `nil` is the script's own error (SC-R-019).

### 4.5 `Accessor`

Returned by `Connector(...)` / `ChargingStation(...)`. Not a global. Exposes exactly ``### 4.1 Shared surface (all shapes, and every `Accessor`)``'s surface scoped to one connector or station.

---

## 5. `C_Module` — session-level module directory

Session-level sim only. Resolves session modules by name, live: a handle re-resolves on every call, so a removed module starts erroring rather than returning stale state (SC-R-020).

| Method | Signature | Returns | Errors | Req |
|---|---|---|---|---|
| `List` | `List()` | sorted array of every current module name | never | SC-R-020 |
| `Get` | `Get(name)` | `ModuleHandle` for `name` | raises `unknown module '<name>'` if none exists | SC-R-020 |

### 5.1 `ModuleHandle` (return of `C_Module:Get`)

| Method | Signature | Returns | Errors | Req |
|---|---|---|---|---|
| `Type` | `Type()` | module kind (`"modbus"` / `"ocpp"`) | raises `unknown module '<name>'` if removed after `Get` | SC-R-020 |
| `Role` | `Role()` | role (`"client"` / `"server"`) | same staleness error | SC-R-020 |
| `Register` | `Register()` | `C_Register`-shaped accessor | raises `module '<name>' is not a modbus module` for non-modbus; staleness error | SC-R-019, SC-R-020 |
| `OCPP` | `OCPP()` | `C_OCPP`-shaped accessor (client or server shape) | raises `module '<name>' is not an ocpp module` for non-ocpp; staleness error | SC-R-019, SC-R-020 |

The accessor from `Register()` / `OCPP()` behaves exactly as ``## 3. `C_Register` — Modbus register access`` / ``## 4. `C_OCPP` — OCPP state access and action dispatch``.

---

## 6. Type coercion on `C_Register:Set`

`Set` applies the Lua value per the register's format:

- **string** — parsed through the register's string-input codec (same path as interactive entry), honoring numeric-literal rules (SC-R-027).
- **integer** — placed into the format's integer variant, **range-checked** against the width; out of range raises, no truncation (SC-R-027).
- **float** onto an integer format — accepted only if finite and whole and in range; fractional or non-finite raises. Onto a float format stored directly (SC-R-027).
- **boolean** — integer `0`/`1`, then coerced as above (SC-R-027).
- **nil** — always raises `cannot Set nil value` (SC-R-009).

**Virtual registers ignore the declared format**: a scalar is stored as 64-bit integer or float (integer outside 64-bit range falls back to float), mirroring the interactive virtual-store rule; a string is codec-parsed. [`edge-cases.md`](./edge-cases.md) SC-E-015.

---

## 7. `C_Time` — elapsed time

| Method | Signature | Returns | Req |
|---|---|---|---|
| `Get` | `Get()` | whole seconds since the sim context was built (`number`) | SC-R-017 |
| `GetMs` | `GetMs()` | whole milliseconds since the sim context was built (`number`) | SC-R-017 |

Origin = the sim thread's context construction; a sim restart (script/interval edit) resets to zero. No sleep, no wall-clock, no date (SC-R-017).

---

## 8. `C_Test` — assertions

| Method | Signature | Behavior | Req |
|---|---|---|---|
| `Assert` | `Assert(cond, msg)` | raises `assertion failed: <msg>` when `cond` is Lua-falsy (`nil` or `false`); otherwise returns nothing. Every non-`nil`, non-`false` value (including `0` and `""`) passes | SC-R-038 |
| `Fail` | `Fail(msg)` | always raises `assertion failed: <msg>` | SC-R-050 |

Both surface as ordinary runtime errors (SC-R-032): logged, cycle continues. The headless `ferrowl run --exit-on-error` exit-code contract keyed off these is `cli-headless/`.

---

## 9. `C_Log` and `C_Statics`

### 9.1 `C_Log` — host log

| Method | Signature | Behavior | Req |
|---|---|---|---|
| `Info` | `Info(line)` | appends `line` to the sim owner's script log at Info | SC-R-031 |
| `Warn` | `Warn(line)` | appends at Warning | SC-R-031 |
| `Error` | `Error(line)` | appends at Error | SC-R-031 |

Each takes one string. Routing per SC-R-031.

### 9.2 `C_Statics` — read-only constants (library surface, not wired)

| Method | Signature | Returns | Errors | Req |
|---|---|---|---|---|
| `Get` | `Get(name)` | stored constant as `number`/`string`/`boolean` | raises `unknown static '<name>'` for a missing key | — |

Exists in the scripting library, not registered into any ferrowl sim context; unreachable from ferrowl scripts today. Documented for completeness and forward compatibility.

---

## 10. `print`

`print(...)` is redirected to the sim owner's log (Info), not stdout. Converts each argument with Lua tostring semantics (honoring `__tostring`), joins with tabs into one log line (SC-R-030).
