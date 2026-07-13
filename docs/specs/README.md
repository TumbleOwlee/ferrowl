# Ferrowl Specs

Authoritative specification of ferrowl's behavior, split by capability area.

These files are **normative**: the code is expected to conform to them, not the
other way around. When code and spec disagree, that is a defect in one of them —
resolve it, don't paper over it.

## Areas

| Area | Covers |
|---|---|
| [`modbus/`](./modbus/) | Modbus client & server, TCP + RTU, register model, formats, codec, reconnect |
| [`ocpp/`](./ocpp/) | OCPP Charging Station & CSMS, versions 1.6/2.0.1/2.1, actions, TLS/auth |
| [`scripting/`](./scripting/) | Lua sim model, `C_*` API contract, execution & error semantics |
| [`tui/`](./tui/) | Tabs, dialogs, `:` commands, keybindings, editor |
| [`config-session/`](./config-session/) | Device/session file envelope, TOML/JSON, save/load, `migrate` |
| [`cli-headless/`](./cli-headless/) | `ferrowl run`, CLI flags, exit codes, CI usage |

Cross-cutting: [`non-functional-requirements.md`](./non-functional-requirements.md).

## Rules for writing specs

**1. No code pointers.** Never cite `file:line`, function names, struct names, or
crate-internal identifiers. A spec states *what must be true*, not where it is
implemented — code pointers rot on every refactor and turn the authoritative doc
into a liar. Public, user-facing names (config keys, `:` commands, Lua `C_*` API,
CLI flags, OCPP action names) are part of the contract and *are* spec content.

**2. Requirement IDs are stable and append-only.** Each requirement carries an ID:

| Area | Prefix |
|---|---|
| modbus | `MB-R-nnn` |
| ocpp | `OC-R-nnn` |
| scripting | `SC-R-nnn` |
| tui | `UI-R-nnn` |
| config-session | `CS-R-nnn` |
| cli-headless | `CL-R-nnn` |
| non-functional (global) | `NF-R-nnn` |

Never renumber. Never reuse a retired ID. A deleted requirement's ID stays dead.
Reference requirements by ID in commits, PRs, and agent instructions.

**3. Owner is the protocol, not the surface.** A Modbus RTU config field is
specified in `modbus/`, not `config-session/` — it belongs with the behavior it
controls, so one change touches one file. `config-session/` owns only the
*envelope*: file format, `version` field, the session→module list, save/load and
`migrate` semantics. Likewise `tui/` owns the command mechanism and generic
commands; protocol-specific commands are specified in their protocol's area.

**4. Requirements are testable.** Write "shall" statements with observable
outcomes. "The client shall retry with exponential backoff bounded to 1s–30s" is
a requirement. "The client is robust" is not.

**5. Known gaps are specified, not hidden.** Behavior that is ugly but
intentional (no Lua execution ceiling, no OCPP auto-reconnect) belongs in the
area's `edge-cases.md` as a stated constraint — so it is not mistaken for an
oversight and silently "fixed".

## Per-area files

Not every area needs every file; add and drop based on need.

| File | Contains |
|---|---|
| `requirements.md` | Numbered, testable "shall" statements. Every area has one. |
| `api-contract.md` | The area's stable public surface: OCPP actions, Lua `C_*` methods, `:` commands, keybindings, CLI flags. |
| `data-contract.md` | Wire and file formats: register model and data formats, payload shapes, config schema. |
| `edge-cases.md` | Boundary behavior, error semantics, and stated known limitations. |

## Keeping specs true

Before changing code in an area, read that area's `requirements.md`. If the change
contradicts the spec, update the spec **in the same commit**. A behavior change
with no spec change is an incomplete change.
