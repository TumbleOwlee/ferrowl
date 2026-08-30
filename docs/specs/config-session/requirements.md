# Config & Session — Requirements

The **configuration envelope**: TOML/JSON format, session file (module instances + session scripts), device-config file and how the two compose, save/load round-trip, `migrate` subcommand.

Per [`../README.md`](../README.md)'s ownership rule, this area owns only the *envelope*. Protocol-specific fields (Modbus register/timing/endpoint, OCPP version/role/security/config-key) are [`../modbus/`](../modbus/) and [`../ocpp/`](../ocpp/). The `:write` mechanism is [`../tui/`](../tui/); the `migrate`/`run` CLI surface is [`../cli-headless/`](../cli-headless/). This file specifies what those surfaces read and write.

---

## File format & encoding

**CS-R-001** — Every configuration file (session or device-config) is TOML or JSON. No other encoding; no YAML.

**CS-R-002** — Encoding is selected solely from the path extension: `.toml` TOML, `.json` JSON, case-insensitive. Content is never sniffed.

**CS-R-003** — A path whose extension is neither `.toml` nor `.json` (including no extension) fails with an unknown-format error on load and save, before any read or write of contents.

**CS-R-004** — The two encodings describe the same data model: a value serialized to one and re-serialized to the other (via the conversion helper) deserializes back to an equal value, no field loss.

**CS-R-005** — On TOML serialization a numeric value is emitted as a plain TOML integer or float, never an internal arbitrary-precision wrapper table. A `u64` exceeding the signed 64-bit range is emitted as a TOML float rather than wrapping.

**CS-R-006** — TOML has no null. A field with no value is omitted, not written as null. A JSON null at the top level or inside a TOML array (no key to omit) is not representable and fails serialization.

---

## Session model

**CS-R-010** — A session file consists of exactly four envelope-level fields: optional `version` string, `modules` list of module-instance entries, `scripts` list of session-level Lua scripts, `interval` sim-cycle period in seconds.

**CS-R-011** — Each `modules` entry is a self-describing object carrying a `"type"` tag naming its kind (`"modbus"` or `"ocpp"`). The loader dispatches on this tag to select the deserializer.

**CS-R-012** — An entry with no `"type"` tag is treated as `"modbus"`, so session files predating multiple module types still load.

**CS-R-013** — An entry with a `"type"` other than `"modbus"` or `"ocpp"` is rejected with a hard error aborting session resolution.

**CS-R-014** — Each module instance carries a `name`: its tab title and `C_Module` registry key. When a session yields two instances with the same name, the second and later are renamed by appending ` (2)`, ` (3)`, … in creation order, skipping any suffix already taken, so every instance is distinct.

**CS-R-015** — Each instance references its device type by a `device` field holding the device-config file path, plus the per-instance endpoint fields its protocol area defines. The entry carries no register table, timing, TLS/security, or OCPP version/role — those live in the referenced device config.

**CS-R-016** — The session `scripts` list holds session-level Lua scripts running in their own Lua state with access to every module; execution semantics in [`../scripting/`](../scripting/). A file lacking `scripts` loads with an empty list.

**CS-R-017** — The session `interval` is a sim-cycle period in seconds, default `1.0` when absent. Non-finite, zero, or negative falls back to `1.0` rather than panicking or busy-looping; a valid positive value is used as-is (no minimum floor).

**CS-R-018** — The session `version` field is informational only: stamped with the writing build's version on save, never consulted by any load-time or migration branch. Absence changes nothing.

---

## Device config composition

**CS-R-020** — The configuration model distinguishes two file kinds: a **session file** (module instances plus session scripts) and a **device-config file** (exactly one device type). One device-config file may be referenced by any number of session instances.

**CS-R-021** — The split: per-instance wire addressing (name, role, endpoint) lives in the session entry; everything describing the device type — register/variable model, timing, scripts, security — lives in the device-config file. Device-config field sets are specified in the Modbus and OCPP areas.

*Coverage note: CS-R-020 and CS-R-021 are structural — the split is a Rust-type-level fact (`Session` vs `DeviceConfig` are distinct structs with disjoint fields), not an independently testable runtime behavior. Exercised by CS-R-004's cross-encoding device round-trip, CS-R-033's session round-trip, and CS-R-015's instance-vs-device field-split assertions; no dedicated test for the umbrella.*

**CS-R-022** — A device-config file also carries an optional, informational `version` string with CS-R-018's semantics: stamped on save, never branched on.

**CS-R-023** — A device-config file loads even when it predates fields added later: every recognized field has a default, so an older file's missing fields take defaults rather than failing.

---

## Save / load & round-trip

**CS-R-030** — The running TUI saves the current module instances as a session file on `:write`. No path given → `session.toml`. Encoding from the target extension per CS-R-002.

**CS-R-031** — A save persists **configuration only**: module instance specs, session scripts, session interval, freshly stamped `version`. Not live runtime state — current register/coil values, in-flight Modbus transactions, the CSMS's observed station topology, runtime mutations to an OCPP config-key/variable store are not written.

**CS-R-032** — A session `:write` writes no device-config file. Device configs are saved through their own command surface ([`../tui/`](../tui/) and the protocol areas); TUI edits to a device config are not captured by a session `:write`.

**CS-R-033** — A session file saved by the TUI and loaded again reproduces the same instance list (names, types, device paths, endpoints), the same session scripts, and the same interval — the envelope round-trips exactly.

**CS-R-034** — Serialization omits fields carrying their default/empty value where the schema declares them omittable (informational `version` when unset, empty `scripts`, unset optional endpoint sub-fields). A file so written reloads to an equal value because each omitted field's load-time default matches.

---

## Migration

**CS-R-040** — The `migrate` subcommand converts a pre-rewrite (`modbus-cli-rs`, ≤ v0.3.9) configuration file into a current device-config file. CLI invocation (`--input` / `--output`) is [`../cli-headless/`](../cli-headless/); this area specifies the transformation.

**CS-R-041** — Migration applies the legacy-to-current transformation: swap the holding/input read codes, split a trailing `le` type suffix into an explicit little-endian byte order, fold each legacy per-register `on_update` Lua snippet into a named entry of the global `scripts` list, merge `[[contiguous_memory]]` ranges into `read_ranges` grouped by function code, rename `delay_after_connect_ms` to the current delay field.

**CS-R-042** — Migration drops legacy fields with no current equivalent (e.g. `history_length`, per-register `reverse`, per-range `slave_id`, UTF-8 string subtypes) and emits a warning for each, never failing silently.

**CS-R-043** — Migration stamps the output with the current build's `version`. Input and output encodings are each chosen from their own extension, so any TOML/JSON source may migrate to a TOML or JSON destination.

**CS-R-044** — A per-register conversion error (unknown read code, address exceeding 16-bit) skips only that register with a warning and lets the rest complete. An unrecognized input/output extension or a load/save failure aborts with a non-zero exit code and a diagnostic on stderr.

**CS-R-045** — `migrate` converts device-config files only. It does not convert or produce session files.

---

## Error handling

**CS-R-050** — Malformed TOML or JSON fails to load with a deserialize error. No partial or best-effort object.

**CS-R-051** — Valid TOML/JSON omitting a required field (e.g. a module instance with no `name` or no endpoint) fails to load with a deserialize error.

**CS-R-052** — A field present in a file but not in the schema is ignored silently on load, except within a TLS block, governed by CS-R-055.

**CS-R-053** — When a session references a missing or unreadable device-config file, startup does not abort. The instance is skipped with a warning naming it and the failed path — identically for Modbus and OCPP; neither silently falls back to a default device config. A **blank** device path is not a failure: a quick-start with no device file, built on the default device config rather than skipped.

**CS-R-054** — Loading a device config self-heals a legacy per-register `update` snippet on **every** load — not only via `migrate` — folding it into the global `scripts` list and clearing the per-register field, so a subsequent save writes only the global list.

**CS-R-055** — Strict field checking applies throughout a TLS subtree — the `tls` container, each `server`/`client` policy block, each policy's `identity`/`verification` payload — and to the OCPP `security` table enclosing one, so a field the enclosing variant or table does not define fails the load rather than being ignored under CS-R-052; `username` and `password` remain defined members of `security`, unaffected. A table in that subtree naming a pre-merge field — `require_client_cert`, `client_ca_files`, `client_ca_file`, `client_cert_skip_verify`, `insecure_skip_verify`, `client_cert_file`, `client_key_file`, `client_self_signed`, `ca_file`, or a bare `self_signed`/`cert_file`/`key_file` outside an `identity` block — fails the load with an error naming the retired fields found and pointing at the current block shape. No value migrated: rejection, not conversion. The sole exception to CS-R-052's silent-ignore rule, because silently ignoring a retired TLS field can weaken an endpoint's security posture rather than merely lose a setting.
