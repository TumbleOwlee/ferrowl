# Config & Session — Edge Cases & Known Limitations

Boundary and error behavior of the envelope, plus known limitations. Protocol-specific cases (bad register range, wss server without certs) belong to `modbus/` or `ocpp/`.

---

## 1. Malformed and ill-typed files

- **Malformed TOML / JSON** — bytes not parsing as the extension's format fail with a deserialize error. No partial object: complete value or error (CS-R-050).
- **Wrong / missing extension** — neither `.toml` nor `.json` (including none) rejected with an unknown-format error before the file is read or written. Never inferred from content (CS-R-002, CS-R-003).
- **Missing required field** — syntactically valid file omitting a required field (module instance with no `name`, `device`, or endpoint) fails with a deserialize error (CS-R-051).
- **Unknown field** — a field the schema does not define is silently ignored. No config type uses strict unknown-field rejection, so a typo'd or obsolete key is dropped, not flagged. Deliberate leniency: a **misspelled key silently takes its default** — a known sharp edge for hand-edited files.

## 2. Module `"type"` dispatch

- **Absent `"type"`** — treated as `"modbus"`, so pre-multi-type session files load (CS-R-012).
- **Unsupported `"type"`** — anything other than `"modbus"`/`"ocpp"` is a hard error aborting session resolution (CS-R-013). Differs from the missing-device-file case (§3), which is non-fatal.
- **Type/spec mismatch** — an entry tagged `"modbus"` whose fields are an OCPP endpoint (or vice versa) fails deserialization as its declared type and aborts resolution.

## 3. A session referencing a device config that does not exist

Both module types handle a missing or unreadable device path the same way: the instance is **skipped** with a warning on stderr (`Skipping '<name>': failed to load '<path>': …`), startup continues without that tab. A broken `device` path never silently degrades to defaults — the operator is told which instance was dropped and why.

A **blank** `device` path is not an error: a quick-start with no device file, built on the default device config. Only a *non-blank* path that fails is skipped.

Neither case aborts startup.

## 4. Duplicate instance names

Two instances resolving to the same `name` do not collide: the first keeps it; each later duplicate gets ` (2)`, ` (3)`, … in creation order, skipping taken suffixes. De-duplication spans **both** module types (a modbus `evse` and an ocpp `evse` become `evse` and `evse (2)`). The renamed tab logs a warning.

## 5. Save targets

- **No path** — `:write` defaults to `session.toml` (TOML, current working directory).
- **Unwritable / non-existent target directory** — save fails with a create/write error in the active tab's log; in-memory session unchanged. No partial-file guarantee beyond the OS — a failed write may leave a truncated file, but running state is not corrupted.
- **Format from extension** — `:write out.json` writes JSON, `:write out.toml` TOML; unrecognized extension fails with an unknown-format message, writes nothing.

## 6. Round-trip omissions (working as designed)

- `:write` persists configuration, not runtime state: live register/coil values, in-flight transactions, CSMS observed topology, OCPP runtime config-key mutations are **not** written (CS-R-031). A reloaded session starts every instance from its device-config baseline.
- `:write` does **not** save referenced device-config files (CS-R-032). TUI edits to a device config must be saved through the device-config save command ([`../tui/`](../tui/) / protocol areas); otherwise lost even though the session saved cleanly.

## 7. Migration edge cases

- **Migrating an already-current config** — `migrate` always parses input against the **legacy** schema. A current file fed to it is read as legacy: fields sharing names/shapes carry through, current-only fields are unknown and ignored, result stamped with the current version. No already-current detection, no no-op short-circuit. Not guaranteed identity-preserving — point it only at genuinely pre-v0.4.0 files.
- **Unrecognized / unparseable legacy config** — extension not `.toml`/`.json`, or contents not deserializing against the legacy schema, aborts with a non-zero exit code and a diagnostic; nothing written (CS-R-044).
- **Per-register failures non-fatal** — unknown read code or address above 16-bit skips only that register with a warning; migration completes with the rest (CS-R-044).
- **Lossy-but-intended drops** — `history_length`, per-register `reverse`, per-range `slave_id`, UTF-8 string subtypes have no equivalent; dropped with a warning each. Expected.

## 8. Legacy `update` self-heal on ordinary load

Loading a Modbus device config folds any legacy per-register `update` snippet into the global `scripts` list and clears the field — on **every** load, not only `migrate`. A device config still carrying `update` fields will, once loaded and saved, be rewritten with the snippets in `scripts` and the `update` fields gone. Intentional; `update` is never written back.

---

## Known limitations (stated, not bugs)

- **The `version` field is inert.** Session and device-config files carry a `version` stamped on save and **never read by any load-time or migration branch**. A source comment says it "enables future compatibility shims"; none exists. Loading a file from any past or future build behaves identically regardless of the stamp. (CS-R-018, CS-R-022.)
- **Strict field validation is scoped to the TLS subtree and the OCPP `security` table (CS-R-055).** Everywhere else no config type rejects unknown fields, so a misspelled key outside them is silently ignored and its intended field takes its default, no feedback.
- **Missing device file drops the tab; both types.** The §3 skip is easy to miss because startup continues.
- **`migrate` has no already-current guard.** §7: input unconditionally interpreted as legacy; meaningful only on pre-v0.4.0 `modbus-cli-rs` files.
- **Retired TLS field** — a TLS block naming a pre-merge field fails the load with an error naming the retired fields and the current block shape, rather than being ignored under CS-R-052. Ignoring is not a neutral loss: dropping a server's `require_client_cert`/`client_ca_files` would downgrade a mutual-TLS listener to one accepting any client, silently, at startup. The other retired names fail closed on their own — a client that stops presenting an identity, or stops trusting a private CA, fails its handshake loudly — but are rejected alongside so one rule covers the block.
- **Retired-field scan is keyed off field names, not schema position.** The generic re-parse phrasing the retired-field error (CS-R-055) enters scope on any key literally named `tls` or `security`, at any depth, not only at known TLS-subtree positions — a register/script/module named `tls` or `security` would be scanned as a TLS container. No current schema uses either name otherwise, so latent, not live. The scan descends into array elements.
