# CLI & Headless — API Contract

Exhaustive command-line surface a CI script or operator writes against: every flag and subcommand, the `--module`/`--ocpp` descriptor mini-language, the exit-code table. Stable public contract.

`migrate` transformation semantics and the config/session file formats: [`../config-session/`](../config-session/). This file: flag surface and dispatch only.

---

## 1. Top-level command

```
ferrowl [OPTIONS]
ferrowl <SUBCOMMAND> ...
```

Default action (no subcommand): start the TUI with the resolved module set.

| Flag | Value | Default | Repeatable | Purpose |
|---|---|---|---|---|
| `--module` | `KEY=VAL,...` | — | yes | one ad-hoc Modbus module (§3) |
| `--session` | `FILE` | — | yes | session file (TOML/JSON) listing module instances. Resolved before `--module` |
| `--device` | `FILE` | — | yes | device-config file → one auto-built TCP **client** named `Device <n>` at `127.0.0.1:5020`. No endpoint/role control |
| `--demo` | (flag) | off | no | eight built-in demo tabs + an example session script; config flags ignored for tab building |
| `--version` | (flag) | — | no | print version, exit 0 |
| `--help` | (flag) | — | no | print usage, exit 0 |

- Top-level has **no** `--ocpp` and **no** `--exit-on-error`. OCPP modules come only from `--session` here.
- Resolution order: `--session` instances, then `--module`, then `--device`. Names de-duplicated across all sources and both module types (later duplicates get ` (2)`, ` (3)`, …).
- `--demo` produces `Modbus Server`, `Modbus Client`, and `CSMS`/`CS` pairs for OCPP `v1.6` (port 9000), `v2.0.1` (9001), `v2.1` (9002).

---

## 2. Subcommands

### `ferrowl migrate`

```
ferrowl migrate --input FILE --output FILE
ferrowl migrate -i FILE -o FILE
```

| Flag | Short | Value | Required | Purpose |
|---|---|---|---|---|
| `--input` | `-i` | `FILE` (.toml/.json) | yes | legacy (≤ v0.3.9 `modbus-cli-rs`) config to read |
| `--output` | `-o` | `FILE` (.toml/.json) | yes | destination for the converted device config |

Dispatched before any async runtime; converts a legacy device config to the current format; warnings for dropped/approximated fields and the success line go to **stderr**. Exits directly (0 success, 1 failure). Input and output encodings each from their own extension. Transformation contract CS-R-040…CS-R-045. Device-config files only — never session files.

### `ferrowl run` (headless / CI)

```
ferrowl run [--session FILE]... [--module KEY=VAL,...]... [--ocpp KEY=VAL,...]... \
            [--duration SECS] [--log-file FILE] [--exit-on-error]
```

| Flag | Value | Default | Repeatable | Purpose |
|---|---|---|---|---|
| `--session` | `FILE` | — | yes | session file; supplies Modbus and OCPP instances and session scripts |
| `--module` | `KEY=VAL,...` | — | yes | ad-hoc Modbus module (§3) |
| `--ocpp` | `KEY=VAL,...` | — | yes | ad-hoc OCPP module (§4) |
| `--duration` | `SECS` (integer) | none | no | run this many seconds then exit 0. Omit → until Ctrl-C |
| `--log-file` | `FILE` | none | no | append every drained line to this file (create-and-append) in addition to stdout |
| `--exit-on-error` | (flag) | off | no | exit 3 (after stopping all modules) when a drained line has level Error |

- `--device` **not** available on `run`; use `--module`.
- `--exit-on-error` exists **only** on `run`.

---

## 3. `--module` descriptor mini-language (Modbus)

Comma-separated `key=value` pairs. Whitespace around keys and values trimmed; empty comma segment skipped. Segment without `=` is an error. Later duplicate keys overwrite earlier.

| Key | Required | Default | Meaning |
|---|---|---|---|
| `name` | yes | — | instance/tab name and `C_Module` registry key |
| `device` | yes* | — | device-config file path |
| `type` | — | — | **alias for `device`**: used only if `device` absent |
| `role` | — | `server` | `client` or `server`. Other → error |
| `transport` | — | `tcp` | `tcp`, `rtu`, `rtu_over_tcp`, `udp`, `ascii`, `ascii_over_tcp`. Other → error |
| `ip` | — | `127.0.0.1` | TCP/UDP only: peer/bind IP |
| `port` | yes (tcp) | — | TCP/UDP only. Required for `transport=tcp`, `rtu_over_tcp`, `udp`, `ascii_over_tcp`; numeric |
| `path` | yes (rtu) | — | RTU only: serial device path. Required for `transport=rtu` or `ascii` |
| `baud` / `baud_rate` | — | `19200` | RTU only: baud rate (aliases) |
| `parity` | — | unset | RTU only: parity string (passed through) |
| `data_bits` | — | unset | RTU only: data bits (numeric) |
| `stop_bits` | — | unset | RTU only: stop bits (numeric) |

\* `device` required, but `type` may supply it. At least one of `device`/`type` must be present.

- **Default role `server`** for `--module` (contrast `--device`, always a client).
- `port` required for TCP, **no** default; `path` required for RTU.
- RTU keys here (`baud`, `parity`, `data_bits`, `stop_bits`, …) are this mini-language's own keys, not clap short flags — [`edge-cases.md`](./edge-cases.md) RTU/clap collision.

Example:

```
--module name=evse-1,device=configs/evse.toml,transport=tcp,ip=10.0.0.5,port=502,role=server
```

---

## 4. `--ocpp` descriptor mini-language (OCPP)

Same grammar as §3. Role/version/timeout/security/scripts are **not** on the command line — they come from the device file.

| Key | Required | Default | Meaning |
|---|---|---|---|
| `name` | yes | — | instance name / registry key |
| `device` | yes | — | OCPP device-config file path |
| `protocol` | — | `ws` | `ws` or `wss`. Other → error |
| `ip` | — | `127.0.0.1` | peer/bind IP |
| `port` | yes | — | port; numeric |
| `path` | — | empty string | WebSocket path (e.g. `/ocpp/cp001`) |

Example:

```
--ocpp name=cs-1,device=configs/cs.toml,protocol=ws,ip=127.0.0.1,port=9000,path=/ocpp/cp001
```

---

## 5. Exit-code table

### `ferrowl run`

| Code | Meaning |
|---|---|
| `0` | ran to completion: `--duration` reached, or Ctrl-C (SIGINT). No error condition fired |
| `1` | setup failure: device config failed to load, `start` reported an error, `--session` failed to load/parse, or `--log-file` could not be opened. `Error: …` on stderr; started modules stopped first |
| `2` | argument-parser usage error (e.g. unknown flag) — emitted before the run |
| `3` | `--exit-on-error` set **and** a drained line had level Error. All modules stopped, then exit 3 |

### `ferrowl migrate`

| Code | Meaning |
|---|---|
| `0` | conversion succeeded; output written |
| `1` | failure: unrecognized input/output extension, input parse failure, or output write failure. `error: …` on stderr |

### Top-level / parser (all commands)

| Code | Meaning |
|---|---|
| `0` | `--help` or `--version` displayed |
| `2` | argument-parser usage error (unknown flag, missing required option) |

---

## 6. Headless output format

- **stdout** carries the drained log stream, one line per entry: `[<timestamp>] <source> | <message>`. `<source>` = module's deduped name, or `session` for session-sim lines.
- **stderr** carries setup/fatal diagnostics only (`Error:`/`error:`, the TUI's module-skip warnings), so stdout stays parseable.
- With `--log-file FILE`, every stdout line is also appended to `FILE` (create-and-append).
