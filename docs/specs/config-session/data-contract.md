# Config & Session — Data Contract

The **envelope schema**: top-level shape of a session file and a device-config file, envelope-level fields, `version`, how a session instance references a device config, TOML/JSON encoding rules.

Protocol-specific field blocks are not listed here: Modbus module-spec endpoint and device-config fields → [`../modbus/api-contract.md`](../modbus/api-contract.md) §5–6; OCPP module-spec endpoint and device-config (version, role, timeout, security, config-keys) → [`../ocpp/api-contract.md`](../ocpp/api-contract.md) §7–9.

---

## 1. Two file kinds

| File | Contains | Cardinality |
|---|---|---|
| **Session file** | module instances + session-level scripts + sim interval | one per launch config; loaded via `--session`, written by `:write` |
| **Device-config file** | one device type (registers/variables, timing, scripts, security) | one file = one device type; referenced by any number of instances |

A session instance does **not** embed a device config; it references one **by path**. One device-config file can back several instances (e.g. two TCP servers of the same type on different ports).

---

## 2. Encoding rules

- TOML or JSON by extension: `.toml` → TOML, `.json` → JSON, case-insensitive. No other extension; content never sniffed.
- Both encodings carry the same data model; either round-trips to the other with no field loss.
- **Field omission:** an unset/empty omittable field is left out of output and takes its default on load. Keeps the informational `version` (when unset), an empty `scripts` list, and unset optional endpoint sub-fields out of written files.
- **Numbers in TOML:** plain integers/floats. A `u64` above the signed-64-bit range is written as a float rather than wrapping.
- **Null:** TOML has no null. An absent value is an omitted key, never an explicit null; a null with no key to omit (top level or inside an array) is a serialization error.
- **Unknown fields:** ignored on load. No schema uses `deny_unknown_fields`, so an unrecognized field is dropped — except the TLS subtree (§8) and the OCPP `security` table enclosing one, which reject a field they do not define (CS-R-055).

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
| `version` | optional string | unset | writing build's version, stamped on save. Omitted when unset. Informational (§6) |
| `modules` | list of objects | empty | each a module-instance spec plus a `"type"` tag (§4). Stored opaquely so both types share one list |
| `scripts` | list of `ScriptDef` | empty | session-level Lua scripts; one Lua state with `C_Module` access to every instance. Semantics [`../scripting/`](../scripting/). Omitted when empty |
| `interval` | float | `1.0` | session sim-cycle seconds. Non-finite/zero/negative → `1.0`; otherwise verbatim (no floor) |

### 3.1 `ScriptDef` (shared envelope type)

Session scripts and device-config scripts share this shape.

| Field | Type | Default | Notes |
|---|---|---|---|
| `name` | string | — (required) | script name |
| `code` | string | empty | Lua source |
| `enabled` | bool | `true` | runs in the sim loop. A flag-less entry is active |

---

## 4. Module-instance entry — the `"type"` tag and the reference

Every `modules` entry is an object with:

- **`"type"`** — `"modbus"` or `"ocpp"`, selects the deserializer. Absent → `"modbus"` (back-compat). Any other value → hard error.
- **`name`** — tab title and `C_Module` registry key. Duplicates across the whole session (both types) de-duplicated by appending ` (2)`, ` (3)`, … in creation order.
- **`device`** — path to the device-config file.
- **per-instance endpoint** fields, protocol-specific:

| `"type"` | Endpoint / instance fields specified in |
|---|---|
| `"modbus"` | [`../modbus/api-contract.md`](../modbus/api-contract.md) §5 (`role`, `endpoint` = `tcp`/`rtu`) |
| `"ocpp"` | [`../ocpp/api-contract.md`](../ocpp/api-contract.md) §7 (`protocol`, `ip`, `port`, `path`) |

The envelope guarantees only `type`, `name`, `device` plus whatever the protocol area defines. Timing, registers/variables, scripts, TLS/security, OCPP version/role are **not** in the entry — they are in the referenced device config.

---

## 5. Device-config file — envelope-level fields

Full field sets are protocol-owned. Envelope-level:

| Field | Type | Default | Notes |
|---|---|---|---|
| `version` | optional string | unset | stamped on save; informational (§6). Omitted when unset |
| `scripts` | list of `ScriptDef` | empty | device-type Lua sim scripts (§3.1). Omitted when empty |

Everything else — Modbus `definitions`/`read_ranges`/timing, OCPP role/version/timeout/security/config-keys — is specified in its protocol area.

A device config loads with **every** unknown field ignored and every recognized-but-absent field defaulted, so a file from an older build still loads (CS-R-023).

---

## 6. The `version` field — informational only

Both file kinds carry an optional `version` string:

- On **save**, overwritten with the writing build's version; the loaded value does not survive.
- On **load**, **never read by any branch**. No migration, compatibility shim, or format selection keys off it. (A source comment describes it as enabling "future compatibility shims" — none exists; inert.)
- Absence changes nothing.

Retained as a human-readable provenance stamp. [`edge-cases.md`](./edge-cases.md) lists it as a known limitation.

---

## 7. What round-trips through `:write`

`:write` serializes the envelope — instance list, session scripts, interval, fresh `version` stamp. **Configuration**, not **live state**:

| Round-trips (persisted) | Does NOT round-trip (dropped) |
|---|---|
| instance name, type, device path, endpoint | live register/coil values, in-flight Modbus transactions |
| session scripts + enabled flags | CSMS observed station/connector topology |
| session interval | OCPP runtime config-key/variable mutations |
| stamped `version` | the device-config files themselves (saved separately) |

A reloaded session reproduces the same instance list, scripts, interval; not any runtime data, and not the referenced device configs.

---

## 8. TLS configuration shape

A device config's TLS material is a tagged-enum tree (MB-R-105) in a two-role container (MB-R-104/OC-R-126): Modbus `[tls.server]`/`[tls.client]`, OCPP one level deeper `[security.tls.server]`/`[security.tls.client]`. Each role block carries `mode` (policy tag), an `identity` sub-table with `source` (certificate-source tag) when the mode calls for one, and a `verification` sub-table with `verify` (peer-verification tag) when the mode calls for one. Both container fields default independently to `mode = "none"`, so an absent `tls`/`security.tls` block, an empty one, and one whose two policies are both `none` are the same state.
