# Config & Session — Data Contract

The **envelope schema**: the top-level shape of a session file and of a
device-config file, the fields that are envelope-level (not protocol-specific), the
`version` field, how a session instance references a device config, and the
encoding rules that relate TOML and JSON.

This document deliberately does **not** list protocol-specific field blocks. For
the Modbus module-spec endpoint and device-config fields, see
[`../modbus/api-contract.md`](../modbus/api-contract.md) §5–6. For the OCPP
module-spec endpoint and device-config (version, role, timeout, security,
config-keys) fields, see [`../ocpp/api-contract.md`](../ocpp/api-contract.md) §7–9.

---

## 1. Two file kinds

| File | Contains | Cardinality |
|---|---|---|
| **Session file** | A list of module instances + session-level scripts + a sim interval | One per launch config; loaded via `--session` and written by `:write` |
| **Device-config file** | The configuration of one device type (registers/variables, timing, scripts, security) | One file = one device type; referenced by any number of instances |

A session instance does **not** embed a device config; it references one **by path**.
The same device-config file can back several instances (e.g. two TCP servers of the
same device type on different ports).

---

## 2. Encoding rules

- A file is TOML or JSON, chosen by extension: `.toml` → TOML, `.json` → JSON,
  matched case-insensitively. No other extension is accepted, and content is never
  sniffed.
- Both encodings carry the same data model; either encoding round-trips to the
  other with no field loss.
- **Field omission:** a field that is unset/empty and marked omittable is left out
  of the serialized output entirely, and takes its default on load. This keeps the
  informational `version` (when unset), an empty `scripts` list, and unset optional
  endpoint sub-fields out of written files.
- **Numbers in TOML:** emitted as plain TOML integers/floats. A `u64` above the
  signed-64-bit range is written as a float rather than wrapping.
- **Null:** TOML has no null. An absent value is an omitted key, never an explicit
  null; a null with no key to omit (top level or inside an array) is a
  serialization error.
- **Unknown fields:** ignored on load. No schema uses strict/`deny_unknown_fields`
  handling, so a field the loader does not recognize is silently dropped rather than
  rejected.

---

## 3. Session file — top-level shape

```
Session {
    version:  optional string   // informational; stamped on save, never branched on
    modules:  list of objects   // one per module instance; each carries a "type" tag
    scripts:  list of ScriptDef // session-level Lua scripts (empty when omitted)
    interval: float seconds     // sim-cycle period; default 1.0
}
```

| Field | Type | Default | Notes |
|---|---|---|---|
| `version` | optional string | unset | Writing build's version, stamped on save. Omitted from output when unset. Purely informational (§6). |
| `modules` | list of objects | empty | Each object is a module-instance spec plus a `"type"` tag (§4). Stored opaquely so both module types share one list. |
| `scripts` | list of `ScriptDef` | empty | Session-level Lua scripts; run in one Lua state with `C_Module` access to every instance. Semantics in [`../scripting/`](../scripting/). Omitted when empty. |
| `interval` | float | `1.0` | Session sim-cycle seconds. Non-finite/zero/negative → `1.0`; otherwise used verbatim (no floor). |

### 3.1 `ScriptDef` (shared envelope type)

Session scripts and device-config scripts use the same entry shape.

| Field | Type | Default | Notes |
|---|---|---|---|
| `name` | string | — (required) | Script name. |
| `code` | string | empty | Lua source. |
| `enabled` | bool | `true` | Whether it runs in the sim loop. A flag-less entry is active. |

---

## 4. Module-instance entry — the `"type"` tag and the reference

Every entry in `modules` is an object with:

- a **`"type"`** discriminator — `"modbus"` or `"ocpp"` — used by the loader to
  pick the deserializer. An entry **without** `"type"` is treated as `"modbus"`
  (back-compat with pre-multi-type files). Any other value is a hard error.
- a **`name`** — the tab title and `C_Module` registry key. Duplicate names across
  the whole session (both types together) are de-duplicated by appending
  ` (2)`, ` (3)`, … in creation order.
- a **`device`** — the path to the device-config file this instance is an instance
  of.
- the **per-instance endpoint** fields, which are protocol-specific:

| `"type"` | Endpoint / instance fields specified in |
|---|---|
| `"modbus"` | [`../modbus/api-contract.md`](../modbus/api-contract.md) §5 (`role`, `endpoint` = `tcp`/`rtu`) |
| `"ocpp"` | [`../ocpp/api-contract.md`](../ocpp/api-contract.md) §7 (`protocol`, `ip`, `port`, `path`) |

The envelope guarantees only that each entry carries `type`, `name`, and `device`
plus whatever its protocol area defines. Timing, registers/variables, scripts,
TLS/security, and OCPP version/role are **not** in the instance entry — they are in
the referenced device config.

---

## 5. Device-config file — envelope-level fields

The full device-config field sets are protocol-owned (see the modbus/ and ocpp/
data/api contracts). Only these fields are envelope-level and common in intent:

| Field | Type | Default | Notes |
|---|---|---|---|
| `version` | optional string | unset | Stamped on save; informational only (§6). Omitted from output when unset. |
| `scripts` | list of `ScriptDef` | empty | Device-type Lua sim scripts (§3.1). Omitted when empty. |

Everything else in a device config — Modbus `definitions`/`read_ranges`/timing,
OCPP role/version/timeout/security/config-keys — is specified in its protocol area
and is not re-listed here.

A device config additionally loads with **every** unknown field ignored and every
recognized-but-absent field defaulted, so a file written by an older build still
loads (CS-R-023).

---

## 6. The `version` field — informational only

Both the session file and the device-config file carry an optional `version`
string. Its full contract:

- On **save**, it is overwritten with the writing build's version; the value that
  was in the loaded file does not survive a save.
- On **load**, it is **never read by any branch**. No migration, compatibility
  shim, or format-selection logic keys off it. (A source comment describes it as
  enabling "future compatibility shims" — that capability does not exist today; the
  field is inert.)
- Its absence changes nothing about how a file loads.

It is retained purely as a human-readable provenance stamp. See
[`edge-cases.md`](./edge-cases.md) for this as a stated known limitation.

---

## 7. What round-trips through `:write`

A `:write` serializes the session envelope — the instance list, the session
scripts, the interval, and a fresh `version` stamp. It captures **configuration**,
not **live state**:

| Round-trips (persisted) | Does NOT round-trip (dropped) |
|---|---|
| Instance name, type, device path, endpoint | Live register/coil values, in-flight Modbus transactions |
| Session scripts + enabled flags | CSMS observed station/connector topology |
| Session interval | OCPP runtime config-key/variable mutations |
| Stamped `version` | The device-config files themselves (saved separately) |

A saved session reloaded reproduces the same instance list, scripts, and interval.
It does not reproduce any runtime data a running module had accumulated, and it does
not re-serialize the device configs the instances reference.
