# Scripting — API Contract

The stable public surface owned by the scripting area: the exhaustive `C_*` Lua
module API that simulation scripts are written against. Every module, every
method, its arguments, return value, and error behavior.

This is the contract script authors depend on. Method names, argument shapes, and
return types are part of the contract and shall not change without a spec change.

Per the ownership rule in [`../README.md`](../README.md), the script-bearing
config fields are specified here and in `requirements.md`; `config-session/` owns
only the file envelope.

---

## 1. Value types across the boundary

Every value passed between Lua and the host is one of five types:

| Host type | Lua type |
|---|---|
| integer | `number` (integer) |
| float | `number` (float) |
| string | `string` |
| boolean | `boolean` |
| nil | `nil` |

Any other Lua value (`table`, `function`, `userdata`, `thread`) supplied where a
scalar is expected raises a conversion error. The sole structured argument is the
optional OCPP action **override table** (§4), whose entries are themselves flat
scalars.

Method call syntax is always colon form: `C_Module:Method(args)` (the module is
the implicit `self`). Using dot form (`C_Module.Method`) yields the raw function
without `self` and is not part of the contract.

---

## 2. Modules available per script context

A script only sees the modules registered for its sim owner (see
`requirements.md` SC-R-018):

| Context | `C_Register` | `C_OCPP` | `C_Module` | `C_Time` | `C_Test` | `C_Log` | `print` |
|---|---|---|---|---|---|---|---|
| Modbus module | ✓ | | | ✓ | ✓ | ✓ | ✓ |
| OCPP module | | ✓ | | ✓ | ✓ | ✓ | ✓ |
| Session-level | | | ✓ | ✓ | ✓ | ✓ | ✓ |

`C_Statics` (§9) is part of the library API surface but is **not** registered by
any ferrowl sim, so no ferrowl script can currently call it.

---

## 3. `C_Register` — Modbus register access

Available in a Modbus module's sim, and through `C_Module:Get(name):Register()`
from the session-level sim.

| Method | Signature | Returns | Errors |
|---|---|---|---|
| `Get` | `Get(name)` | the register's decoded value as `number`/`string`/`boolean` | unknown register name; a fixed-address register whose backing cells are not readable; a decode failure; a virtual register that has not been set |
| `Set` | `Set(name, value)` | nothing | unknown register name; a type/range mismatch between `value` and the register format (§6); `nil` value; a fixed-address write the store rejects as not writable |
| `Has` | `Has(name)` | `boolean` — whether a register of that name is defined | never (a missing name returns `false`, not an error) |

`name` is the register's configured name, not a Modbus address. `Get`/`Set`
exchange the register's **raw, unscaled** stored value (display resolution is not
applied), so a value `Set` from Lua round-trips through `Get` unchanged.

Writes go to the module's in-memory store only; no Modbus command is sent
(`requirements.md` SC-R-028).

---

## 4. `C_OCPP` — OCPP state access and action dispatch

Available in an OCPP module's sim, and through `C_Module:Get(name):OCPP()` from
the session-level sim. The module takes one of three shapes depending on the host
module; all three share the same `Get`/`Set`/`<Action>` surface.

### 4.1 Shared surface (all shapes, and every `Accessor`)

| Method | Signature | Returns | Errors |
|---|---|---|---|
| `Get` | `Get(name)` | state field value as `number`/`string`/`boolean` | unknown field name for this scope |
| `Set` | `Set(name, value)` | nothing | field that cannot be set for this scope |
| `<Action>` | `<Action>(overrides?)` | `boolean` — `true` if the action was enqueued | a malformed override table (a non-string key or a non-scalar value) raises |

There is one `<Action>` method per action name the host module exposes for its
OCPP version; the exact set is version-specific and is defined by the OCPP area
(see `ocpp/api-contract.md`). Calling an action **enqueues** it onto the module's
action queue for the owning view (or headless runner) to send; the Lua call does
not itself perform the OCPP request and returns as soon as the action is queued.

`overrides` is an optional flat Lua table of `name = scalar` pairs merged over the
action's default payload. A missing table means no overrides. Nested tables or
non-scalar values raise.

### 4.2 Flat shape

Bare `Get`/`Set`/`<Action>` address a single state scope. No scoping methods.

### 4.3 Client shape (charging station)

Bare `Get`/`Set`/`<Action>` address the charge-point (CS) level. Additionally:

| Method | Signature | Returns |
|---|---|---|
| `Connector` | `Connector(id)` | an `Accessor` scoped to connector `id`, with its own `Get`/`Set`/`<Action>` |
| `GetConnectors` | `GetConnectors()` | array of connector ids (`number`) |

An action dispatched on a connector `Accessor` is enqueued at that connector's
scope; an action on the bare module is enqueued at CS scope.

### 4.4 Server shape (CSMS spanning many stations)

The server module spans every connected station; access is keyed by station
identity.

| Method | Signature | Returns |
|---|---|---|
| `GetChargingStations` | `GetChargingStations()` | sorted array of station identity strings |
| `GetConnectors` | `GetConnectors(cs)` | sorted array of connector ids for station `cs` (empty if unknown) |
| `ChargingStation` | `ChargingStation(cs)` | a CS-level `Accessor` for station `cs`, or `nil` if unknown |
| `Connector` | `Connector(cs, id)` | a connector `Accessor` for `(cs, id)`, or `nil` if unknown |

Unknown stations/connectors resolve to `nil` (not an error); indexing that `nil`
in Lua is the script's own error.

### 4.5 `Accessor`

The value returned by `Connector(...)` / `ChargingStation(...)`. Not a global. It
exposes exactly the shared surface of §4.1 scoped to one connector or one
station. Reads/writes/actions on it route to that scope.

---

## 5. `C_Module` — session-level module directory

Available only in the session-level sim. Resolves the other modules in the
session by name, live: a handle re-resolves on every call, so a module removed
after resolution starts erroring rather than returning stale state.

| Method | Signature | Returns | Errors |
|---|---|---|---|
| `List` | `List()` | sorted array of the names of every module currently in the session | never |
| `Get` | `Get(name)` | a `ModuleHandle` for `name` | raises `unknown module '<name>'` if no such module currently exists |

### 5.1 `ModuleHandle` (return of `C_Module:Get`)

| Method | Signature | Returns | Errors |
|---|---|---|---|
| `Type` | `Type()` | module kind string (`"modbus"` / `"ocpp"`) | raises `unknown module '<name>'` if the module was removed after `Get` |
| `Role` | `Role()` | role string (`"client"` / `"server"`) | same staleness error |
| `Register` | `Register()` | a `C_Register`-shaped accessor for the module | raises `module '<name>' is not a modbus module` for a non-modbus module; staleness error |
| `OCPP` | `OCPP()` | a `C_OCPP`-shaped accessor (client or server shape) for the module | raises `module '<name>' is not an ocpp module` for a non-ocpp module; staleness error |

The accessor returned by `Register()` / `OCPP()` behaves exactly as §3 / §4.

---

## 6. Type coercion on `C_Register:Set`

`Set` applies the Lua value to the register according to the register's format:

- A Lua **string** is parsed through the register's string-input codec (the same
  path as interactive text entry), honoring the format's numeric-literal rules.
- A Lua **integer** is placed into the format's integer variant, **range-checked**
  against the format width; an out-of-range value raises rather than truncating.
- A Lua **float** onto an integer format is accepted only if it is a finite whole
  number in range; a fractional or non-finite value raises. Onto a float format
  it is stored directly.
- A Lua **boolean** is treated as the integer `0`/`1` and then coerced as above.
- A Lua **nil** always raises `cannot Set nil value`.

**Virtual registers ignore the declared format**: a scalar is stored as a 64-bit
integer or float (an integer outside 64-bit range falls back to float), mirroring
the interactive virtual-store rule; a string is parsed through the codec. See
[`edge-cases.md`](./edge-cases.md) §2.

---

## 7. `C_Time` — elapsed time

| Method | Signature | Returns |
|---|---|---|
| `Get` | `Get()` | whole seconds elapsed since the sim context was built (`number`) |
| `GetMs` | `GetMs()` | whole milliseconds elapsed since the sim context was built (`number`) |

The origin is the sim thread's context construction; a sim restart (script/interval
edit) resets it to zero. `C_Time` provides no sleep, no wall-clock, and no date.

---

## 8. `C_Test` — assertions

| Method | Signature | Behavior |
|---|---|---|
| `Assert` | `Assert(cond, msg)` | raises `assertion failed: <msg>` when `cond` is Lua-falsy (`nil` or `false`); otherwise returns nothing. Every non-`nil`, non-`false` value (including `0` and `""`) is truthy and passes. |
| `Fail` | `Fail(msg)` | always raises `assertion failed: <msg>` |

Both surface as ordinary script runtime errors (SC-R-032): the error is logged and
the cycle continues. The headless `ferrowl run --exit-on-error` exit-code contract
that keys off these logged failures is owned by `cli-headless/`.

---

## 9. `C_Log` and `C_Statics`

### 9.1 `C_Log` — host log

| Method | Signature | Behavior |
|---|---|---|
| `Info` | `Info(line)` | appends `line` to the sim owner's script log at Info level |
| `Warn` | `Warn(line)` | appends at Warning level |
| `Error` | `Error(line)` | appends at Error level |

Each takes a single string. Output routing is per SC-R-031.

### 9.2 `C_Statics` — read-only constants (library surface, not wired)

| Method | Signature | Returns | Errors |
|---|---|---|---|
| `Get` | `Get(name)` | the stored constant as `number`/`string`/`boolean` | raises `unknown static '<name>'` for a missing key |

`C_Statics` exists in the scripting library but is not registered into any ferrowl
sim context, so it is unreachable from ferrowl scripts today. It is documented for
completeness and forward compatibility.

---

## 10. `print`

`print(...)` is redirected to the sim owner's log (Info level), not stdout. It
converts each argument with Lua tostring semantics (honoring `__tostring`) and
joins them with tab characters into one log line.
