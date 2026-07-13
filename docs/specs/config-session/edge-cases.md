# Config & Session — Edge Cases & Known Limitations

Boundary and error behavior of the configuration envelope, plus the stated
known-limitations this area owns. Anything protocol-specific (a bad register range,
a wss server without certs) belongs to the modbus/ or ocpp/ area.

---

## 1. Malformed and ill-typed files

- **Malformed TOML / JSON** — a file whose bytes do not parse as its extension's
  format fails to load with a deserialize error. No partial object is built; the
  load either yields a complete value or an error (CS-R-050).
- **Wrong / missing extension** — a path ending in neither `.toml` nor `.json`
  (including no extension at all) is rejected with an unknown-format error before
  the file is read or written. The format is never inferred from content (CS-R-002,
  CS-R-003).
- **Missing required field** — a syntactically valid file that omits a required
  field (a module instance with no `name`, no `device`, or no endpoint) fails to
  load with a deserialize error (CS-R-051).
- **Unknown field** — a field the schema does not define is silently ignored. No
  config type uses strict unknown-field rejection, so a typo'd or obsolete key is
  dropped, not flagged. This is a deliberate leniency, and it means a
  **misspelled key silently takes its default** rather than erroring — a known
  sharp edge for hand-edited files.

## 2. Module `"type"` dispatch

- **Absent `"type"`** — a module entry with no `"type"` tag is treated as
  `"modbus"`, so pre-multi-type session files still load (CS-R-012).
- **Unsupported `"type"`** — a module entry tagged with anything other than
  `"modbus"` or `"ocpp"` is a hard error that aborts session resolution
  (CS-R-013). This differs from the missing-device-file case below, which is
  non-fatal.
- **Type/spec mismatch** — an entry tagged `"modbus"` whose fields are actually an
  OCPP endpoint (or vice versa) fails deserialization as its declared type and
  aborts resolution.

## 3. A session referencing a device config that does not exist

Both module types handle a missing or unreadable device-config path the same way:
the instance is **skipped** with a warning on stderr (`Skipping '<name>': failed to
load '<path>': …`), and startup continues without that tab. A broken `device` path
never silently degrades a tab to defaults — the operator is always told which
instance was dropped and why.

A **blank** `device` path is different, and is not an error: it is a deliberate
quick-start with no device file, so the instance is built on the default device
config rather than skipped. Only a *non-blank* path that fails to load is skipped.

Neither case aborts startup — the remaining instances still come up.

## 4. Duplicate instance names

Two instances resolving to the same `name` do not collide: the first keeps the
name; each later duplicate gets ` (2)`, ` (3)`, … appended in creation order,
skipping any suffix already taken. De-duplication spans **both** module types
together (a modbus `evse` and an ocpp `evse` in one session become `evse` and
`evse (2)`). The renamed tab logs a warning.

## 5. Save targets

- **No path given** — `:write` defaults its target to `session.toml` (TOML in the
  current working directory).
- **Unwritable / non-existent target directory** — the save fails with a
  create/write error surfaced to the active tab's log; the in-memory session is
  unchanged. There is no partial-file guarantee beyond what the OS provides — a
  failed write may leave a truncated file, but the running session state is not
  corrupted by a failed save.
- **Format from extension** — `:write out.json` writes JSON, `:write out.toml`
  writes TOML; an unrecognized extension fails with an unknown-format message and
  writes nothing.

## 6. Round-trip omissions (working as designed)

- A `:write` persists configuration, not runtime state: live register/coil values,
  in-flight transactions, CSMS observed topology, and OCPP runtime config-key
  mutations are **not** written (CS-R-031). Reloading a saved session starts every
  instance from its device-config baseline again.
- A `:write` does **not** save the device-config files an instance references
  (CS-R-032). Edits made to a device config in the TUI must be saved through the
  device-config save command (see [`../tui/`](../tui/) / the protocol areas);
  otherwise they are lost even though the session file saved cleanly.

## 7. Migration edge cases

- **Migrating an already-current config** — the `migrate` subcommand always parses
  its input against the **legacy** schema, not the current one. A current
  device-config file fed to `migrate` is interpreted as if it were legacy: fields
  that happen to share names/shapes carry through, current-only fields are unknown
  and ignored, and the result is stamped with the current version. There is no
  detection that the input is already current and no no-op short-circuit. Migrating
  an already-current file is therefore not guaranteed to be identity-preserving —
  point a migration only at genuinely pre-v0.4.0 files.
- **Unrecognized / unparseable legacy config** — an input whose extension is not
  `.toml`/`.json`, or whose contents do not deserialize against the legacy schema,
  aborts with a non-zero exit code and a diagnostic; nothing is written (CS-R-044).
- **Per-register failures are non-fatal** — an unknown read code or an address
  above the 16-bit range skips only that register with a warning; the migration
  completes and writes the remaining registers (CS-R-044).
- **Lossy-but-intended drops** — `history_length`, per-register `reverse`,
  per-range `slave_id`, and UTF-8 string subtypes have no current equivalent and are
  dropped with a warning each. This is expected, not a bug.

## 8. Legacy `update` self-heal on ordinary load

Loading a Modbus device config folds any legacy per-register `update` Lua snippet
into the global `scripts` list and clears the per-register field — on **every**
load, not only via `migrate`. Consequently a device config that still carries
`update` fields will, once loaded and then saved, be rewritten with those snippets
moved into `scripts` and the `update` fields gone. This is intentional self-healing;
the per-register `update` field is never written back.

---

## Known limitations (stated, not bugs)

- **The `version` field is inert.** Both the session file and the device-config
  file carry a `version` string that is stamped on save and **never read by any
  load-time or migration branch**. A source comment says it "enables future
  compatibility shims"; no such shim exists — nothing keys off the version. Loading
  a file written by any past or future build behaves identically regardless of the
  stamped value. (CS-R-018, CS-R-022.)
- **No strict field validation.** Because no config type rejects unknown fields, a
  misspelled key is silently ignored and its intended field silently takes its
  default. Hand-edited files get no feedback on typos.
- **Missing Modbus device file drops the tab silently; missing OCPP device file
  degrades to defaults.** The asymmetry in §3 is by construction, not a mistake, but
  either outcome is easy to miss because startup continues.
- **`migrate` has no already-current guard.** §7: it unconditionally interprets its
  input as legacy, so it is only meaningful on pre-v0.4.0 `modbus-cli-rs` files.
