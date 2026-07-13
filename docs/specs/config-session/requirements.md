# Config & Session — Requirements

Testable requirements for the **configuration envelope**: the TOML/JSON file
format, the session file (module instances + session scripts), the device-config
file and how the two compose, save/load round-trip, and the `migrate` subcommand.

Per the ownership rule in [`../README.md`](../README.md), this area owns only the
*envelope*. Protocol-specific config fields (Modbus register/timing/endpoint
fields, OCPP version/role/security/config-key fields) are specified in
[`../modbus/`](../modbus/) and [`../ocpp/`](../ocpp/), not here. The `:write`
command mechanism itself belongs to [`../tui/`](../tui/); the `migrate`/`run` CLI
flag surface belongs to [`../cli-headless/`](../cli-headless/). This file specifies
what those surfaces read and write.

---

## File format & encoding

**CS-R-001** — Every configuration file (session file or device-config file) shall
be encoded as either TOML or JSON. No other encoding shall be accepted; there is
no YAML support.

**CS-R-002** — The encoding shall be selected solely from the file-path extension:
`.toml` is TOML and `.json` is JSON, matched case-insensitively. The file's content
shall never be sniffed to guess a format.

**CS-R-003** — A path whose extension is neither `.toml` nor `.json` (including a
path with no extension) shall fail with an unknown-format error on both load and
save, before any read or write of the file's contents is attempted.

**CS-R-004** — The two encodings shall describe the same data model: a value
serialized to one encoding and re-serialized to the other (via the conversion
helper) shall deserialize back to an equal value, with no field loss.

**CS-R-005** — On TOML serialization, a numeric value shall be emitted as a plain
TOML integer or float, never as an internal arbitrary-precision wrapper table. A
`u64` that exceeds the signed 64-bit range shall be emitted as a TOML float rather
than silently wrapping.

**CS-R-006** — TOML has no null type. A field with no value shall be omitted from
the serialized output, not written as an explicit null. A JSON null that would
appear at the top level or inside a TOML array (where there is no key to omit) is
not representable and shall fail serialization.

---

## Session model

**CS-R-010** — A session file shall consist of exactly four envelope-level fields:
an optional `version` string, a `modules` list of module-instance entries, a
`scripts` list of session-level Lua scripts, and an `interval` sim-cycle period in
seconds.

**CS-R-011** — Each entry in `modules` shall be a self-describing object carrying a
`"type"` tag that names its module kind (`"modbus"` or `"ocpp"`). The loader shall
dispatch on this tag to select the deserializer for that entry.

**CS-R-012** — A module entry with no `"type"` tag shall be treated as `"modbus"`,
so that session files written before multiple module types existed still load.

**CS-R-013** — A module entry with a `"type"` tag other than `"modbus"` or
`"ocpp"` shall be rejected with a hard error that aborts session resolution.

**CS-R-014** — Each module instance shall carry a `name` that serves as its tab
title and its `C_Module` registry key. When a session yields two instances with the
same name, the second and later occurrences shall be renamed by appending
` (2)`, ` (3)`, … in creation order, skipping any suffix already taken, so every
instance receives a distinct name.

**CS-R-015** — Each module instance shall reference its device type by a `device`
field holding the path to a device-config file, plus the per-instance endpoint
fields defined by its protocol area. The instance entry shall carry no register
table, no timing, no TLS/security, and no OCPP version/role — those live in the
referenced device config.

**CS-R-016** — The session `scripts` list shall hold session-level Lua scripts that
run in their own Lua state with access to every module in the session; their
execution semantics are specified in [`../scripting/`](../scripting/). A session
file lacking a `scripts` field shall load with an empty list.

**CS-R-017** — The session `interval` shall be a sim-cycle period in seconds,
defaulting to `1.0` when the field is absent. A non-finite, zero, or negative
`interval` shall fall back to `1.0` rather than panicking or busy-looping; a valid
positive value shall be used as-is (no minimum floor is applied to the session
interval).

**CS-R-018** — The session `version` field shall be informational only: it is
stamped with the writing build's version on save and is never consulted by any
load-time or migration branch. Absence of the field shall not change loading
behavior.

---

## Device config composition

**CS-R-020** — The configuration model shall distinguish two file kinds: a
**session file** (a list of module instances plus session scripts) and a
**device-config file** (the configuration of exactly one device type). One
device-config file describes one device type and may be referenced by any number of
session instances.

**CS-R-021** — The split between the two files shall be: per-instance wire
addressing (name, role, endpoint) lives in the session entry; everything that
describes the device type — register/variable model, timing, scripts, security —
lives in the device-config file. The device-config field sets are specified in the
Modbus and OCPP areas, not here.

**CS-R-022** — A device-config file shall also carry an optional, informational
`version` string with the same semantics as CS-R-018: stamped on save, never
branched on.

**CS-R-023** — A device-config file shall load even when it predates fields added
in later releases: every field the loader recognizes shall have a default so that
an older file's missing fields take their defaults rather than failing the load.

---

## Save / load & round-trip

**CS-R-030** — The running TUI shall save the current module instances as a session
file on the `:write` command. When no path is given, the target shall default to
`session.toml`. The file's encoding shall be chosen from the target extension per
CS-R-002.

**CS-R-031** — A save shall persist **configuration only**: the module instance
specs, the session scripts, the session interval, and a freshly stamped `version`.
It shall not persist live runtime state — current register/coil values, in-flight
Modbus transactions, the CSMS's observed station topology, or runtime mutations to
an OCPP config-key/variable store are not written to the session file.

**CS-R-032** — A `:write` of the session file shall not write any device-config
file. Device-config files are saved through their own separate command surface
(specified in [`../tui/`](../tui/) and the protocol areas); edits made to a device
config in the TUI are not captured by a session `:write`.

**CS-R-033** — A session file saved by the TUI and then loaded again shall
reproduce the same list of module instances (same names, types, device paths, and
endpoints), the same session scripts, and the same interval — i.e. the envelope
round-trips exactly.

**CS-R-034** — Serialization shall omit fields that carry their default/empty
value where the schema declares them omittable (the informational `version` when
unset, an empty `scripts` list, unset optional endpoint sub-fields). A file so
written shall reload to an equal value because each omitted field's load-time
default matches what was omitted.

---

## Migration

**CS-R-040** — The `migrate` subcommand shall convert a pre-rewrite
(`modbus-cli-rs`, ≤ v0.3.9) configuration file into a current device-config file.
Its CLI invocation (`--input` / `--output`) is specified in
[`../cli-headless/`](../cli-headless/); this area specifies the transformation.

**CS-R-041** — Migration shall apply the legacy-to-current transformation contract:
swap the holding/input read codes, split a trailing `le` type suffix into an
explicit little-endian byte order, fold each legacy per-register `on_update` Lua
snippet into a named entry of the global `scripts` list, merge `[[contiguous_memory]]`
ranges into `read_ranges` grouped by function code, and rename
`delay_after_connect_ms` to the current delay field.

**CS-R-042** — Migration shall drop legacy fields that have no current equivalent
(e.g. `history_length`, per-register `reverse`, per-range `slave_id`, UTF-8 string
subtypes) and shall emit a warning for each dropped field rather than failing
silently.

**CS-R-043** — Migration shall stamp the output device config with the current
build's `version`. Input and output encodings shall each be chosen independently
from their own file extension, so any TOML/JSON source may be migrated to a TOML or
JSON destination.

**CS-R-044** — A per-register conversion error (e.g. an unknown read code, an
address exceeding the 16-bit range) shall skip only that register with a warning and
allow the rest of the migration to complete. An unrecognized input/output extension
or a load/save failure shall abort the migration with a non-zero exit code and a
diagnostic on standard error.

**CS-R-045** — The `migrate` subcommand shall convert device-config files only. It
shall not convert or produce session files.

---

## Error handling

**CS-R-050** — A file whose contents are malformed TOML or JSON shall fail to load
with a deserialize error. No partial or best-effort object shall be constructed
from a malformed file.

**CS-R-051** — A file that parses as valid TOML/JSON but omits a field the schema
marks required (e.g. a module instance with no `name` or no endpoint) shall fail to
load with a deserialize error.

**CS-R-052** — A field present in a file but not present in the schema shall be
ignored silently on load. Unknown fields shall not cause a load failure.

**CS-R-053** — When a session references a device-config file that is missing or
unreadable, startup shall not abort. The instance shall be skipped with a warning
naming it and the failed path — identically for Modbus and OCPP; neither type shall
silently fall back to a default device config. A **blank** device path is not a
failure: it is a quick-start with no device file, and the instance shall be built
on the default device config rather than skipped.

**CS-R-054** — Loading a device config shall self-heal a legacy per-register
`update` snippet on **every** load — not only through the `migrate` subcommand — by
folding it into the global `scripts` list and clearing the per-register field, so a
subsequent save writes only the global list.
