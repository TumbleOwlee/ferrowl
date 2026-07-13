# CLI & Headless — API Contract

The exhaustive command-line surface a CI script or operator writes against: every
flag and subcommand, the `--module`/`--ocpp` descriptor mini-language, and the
exit-code table. This is a stable public contract; changes here are contract
changes.

The `migrate` transformation semantics and the config/session file formats these
flags read and write are specified in
[`../config-session/`](../config-session/); this file specifies only the flag
surface and dispatch behavior.

---

## 1. Top-level command

```
ferrowl [OPTIONS]
ferrowl <SUBCOMMAND> ...
```

Default action (no subcommand): start the TUI with the resolved module set.

| Flag | Value | Default | Repeatable | Purpose |
|---|---|---|---|---|
| `--module` | `KEY=VAL,...` | — | yes | One ad-hoc Modbus module (see §3). |
| `--session` | `FILE` | — | yes | A session file (TOML/JSON) listing module instances. Resolved before `--module`. |
| `--device` | `FILE` | — | yes | A device-config file → one auto-built TCP **client** named `Device <n>` at `127.0.0.1:5020`. No endpoint/role control. |
| `--demo` | (flag) | off | no | Start eight built-in demo tabs + an example session script; config flags are ignored for tab building. |
| `--version` | (flag) | — | no | Print version, exit 0. |
| `--help` | (flag) | — | no | Print usage, exit 0. |

Notes:

- The top-level command has **no** `--ocpp` and **no** `--exit-on-error` flag. OCPP
  modules are resolved only from `--session` files here.
- Resolution order of the started set: `--session` instances, then `--module`,
  then `--device`. Names are de-duplicated across all sources and both module types
  together (later duplicates get ` (2)`, ` (3)`, …).
- `--demo` produces: `Modbus Server`, `Modbus Client`, and `CSMS`/`CS` pairs for
  OCPP `v1.6` (port 9000), `v2.0.1` (9001), and `v2.1` (9002).

---

## 2. Subcommands

### `ferrowl migrate`

```
ferrowl migrate --input FILE --output FILE
ferrowl migrate -i FILE -o FILE
```

| Flag | Short | Value | Required | Purpose |
|---|---|---|---|---|
| `--input` | `-i` | `FILE` (.toml/.json) | yes | Legacy (≤ v0.3.9 `modbus-cli-rs`) config to read. |
| `--output` | `-o` | `FILE` (.toml/.json) | yes | Destination for the converted device config. |

Behavior: dispatched before any async runtime; converts a legacy device config to
the current device-config format; warnings for dropped/approximated fields and the
success line are printed to **standard error**. Exits directly (0 success, 1
failure). Input and output encodings are each chosen from their own extension. The
transformation contract is CS-R-040…CS-R-045. `migrate` converts device-config files
only — never session files.

### `ferrowl run` (headless / CI)

```
ferrowl run [--session FILE]... [--module KEY=VAL,...]... [--ocpp KEY=VAL,...]... \
            [--duration SECS] [--log-file FILE] [--exit-on-error]
```

| Flag | Value | Default | Repeatable | Purpose |
|---|---|---|---|---|
| `--session` | `FILE` | — | yes | Session file; supplies both Modbus and OCPP instances and session scripts. |
| `--module` | `KEY=VAL,...` | — | yes | Ad-hoc Modbus module (see §3). |
| `--ocpp` | `KEY=VAL,...` | — | yes | Ad-hoc OCPP module (see §4). |
| `--duration` | `SECS` (integer) | none | no | Run this many seconds then exit 0. Omit → run until Ctrl-C. |
| `--log-file` | `FILE` | none | no | Append every drained log line to this file (create-and-append) in addition to stdout. |
| `--exit-on-error` | (flag) | off | no | Exit 2 (after stopping all modules) when a drained log line begins with `[sim]`. |

Notes:

- `--device` is **not** available on `run`; use `--module`.
- `--exit-on-error` exists **only** on `run`.

---

## 3. `--module` descriptor mini-language (Modbus)

A `--module` value is a comma-separated list of `key=value` pairs. Whitespace
around keys and values is trimmed; an empty comma segment is skipped. A segment
without `=` is an error. Later duplicate keys overwrite earlier ones.

| Key | Required | Default | Meaning |
|---|---|---|---|
| `name` | yes | — | Instance/tab name and `C_Module` registry key. |
| `device` | yes* | — | Path to the device-config file. |
| `type` | — | — | **Alias for `device`**: used only if `device` is absent. |
| `role` | — | `server` | `client` or `server`. Any other value is an error. |
| `transport` | — | `tcp` | `tcp` or `rtu`. Any other value is an error. |
| `ip` | — | `127.0.0.1` | TCP only: peer/bind IP. |
| `port` | yes (tcp) | — | TCP only: port. Required for `transport=tcp`; must parse as a number. |
| `path` | yes (rtu) | — | RTU only: serial device path. Required for `transport=rtu`. |
| `baud` / `baud_rate` | — | `19200` | RTU only: baud rate (`baud` and `baud_rate` are aliases). |
| `parity` | — | unset | RTU only: parity string (passed through). |
| `data_bits` | — | unset | RTU only: data bits (numeric). |
| `stop_bits` | — | unset | RTU only: stop bits (numeric). |

\* `device` is required, but `type` may supply it. At least one of `device`/`type`
must be present.

Contract points worth pinning:

- **Default role is `server`** for `--module` (contrast `--device`, which is always
  a client).
- `port` is required for TCP and has **no** default; `path` is required for RTU.
- The RTU keys here (`baud`, `parity`, `data_bits`, `stop_bits`, …) are this
  mini-language's own keys, not clap short flags — see
  [`edge-cases.md`](./edge-cases.md) §RTU/clap collision.

Example:

```
--module name=evse-1,device=configs/evse.toml,transport=tcp,ip=10.0.0.5,port=502,role=server
```

---

## 4. `--ocpp` descriptor mini-language (OCPP)

Same `key=value` grammar as §3. Role/version/timeout/security/scripts are **not**
on the command line — they come from the referenced device file.

| Key | Required | Default | Meaning |
|---|---|---|---|
| `name` | yes | — | Instance name / registry key. |
| `device` | yes | — | Path to the OCPP device-config file. |
| `protocol` | — | `ws` | `ws` or `wss`. Any other value is an error. |
| `ip` | — | `127.0.0.1` | Peer/bind IP. |
| `port` | yes | — | Port; must parse as a number. |
| `path` | — | empty string | WebSocket path (e.g. `/ocpp/cp001`). |

Example:

```
--ocpp name=cs-1,device=configs/cs.toml,protocol=ws,ip=127.0.0.1,port=9000,path=/ocpp/cp001
```

---

## 5. Exit-code table

### `ferrowl run`

| Code | Meaning |
|---|---|
| `0` | Ran to completion: `--duration` deadline reached, or Ctrl-C (SIGINT) received. No error condition fired. |
| `1` | Setup failure: a module's device config failed to load, a module's `start` reported an error, a `--session` file failed to load/parse, or `--log-file` could not be opened. Diagnostic (`Error: …`) on stderr; started modules are stopped first. |
| `2` | `--exit-on-error` was set **and** a drained log line began with `[sim]` (the Lua sim-error marker). All modules are stopped, then exit 2. |
| `2` | (Argument-parser usage error, e.g. unknown flag — emitted by the parser before the run starts; same integer, different origin. See edge-cases.) |

### `ferrowl migrate`

| Code | Meaning |
|---|---|
| `0` | Conversion succeeded; output written. |
| `1` | Failure: unrecognized input/output extension, input parse failure, or output write failure. Diagnostic (`error: …`) on stderr. |

### Top-level / parser (all commands)

| Code | Meaning |
|---|---|
| `0` | `--help` or `--version` displayed. |
| `2` | Argument-parser usage error (unknown flag, missing required option). |

---

## 6. Headless output format

- **Standard output** carries the drained log stream, one line per log entry:
  `[<timestamp>] <source> | <message>`. `<source>` is the module's deduped name, or
  `session` for session-sim lines.
- **Standard error** carries setup/fatal diagnostics only (`Error:`/`error:` and the
  TUI's module-skip warnings), so stdout stays parseable by CI.
- With `--log-file FILE`, every stdout line is also appended to `FILE`
  (create-and-append; existing content preserved).
