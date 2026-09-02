# Modbus — API Contract

Function codes per role, Modbus device/endpoint config fields. Per [`../README.md`](../README.md)'s ownership rule, Modbus config fields live here, not `config-session/` (which owns only the envelope: format, `version`, session→module list, save/load, `migrate`).

---

## Modbus function codes

### Client (initiator)

| Code | Function | Issued by | Req |
|---|---|---|---|
| 1 | Read Coils | poll loop | MB-R-041 |
| 2 | Read Discrete Inputs | poll loop | MB-R-041 |
| 3 | Read Holding Registers | poll loop | MB-R-041 |
| 4 | Read Input Registers | poll loop | MB-R-041 |
| 5 | Write Single Coil | write command | MB-R-046 |
| 6 | Write Single Register | write command | MB-R-046 |
| 15 | Write Multiple Coils | write command | MB-R-046 |
| 16 | Write Multiple Registers | write command | MB-R-046 |

Poll loop issues **only** the four read codes. Writes only on an explicit write command (TUI, `:` command, Lua script, headless run) — never on the client's own initiative. Client does **not** implement code 23 (Read/Write Multiple Registers) or any other.

### Server (responder)

| Code | Function | Behavior | Req |
|---|---|---|---|
| 1 | Read Coils | answered from the store | MB-R-058 |
| 2 | Read Discrete Inputs | answered from the store | MB-R-058 |
| 3 | Read Holding Registers | answered from the store | MB-R-058 |
| 4 | Read Input Registers | answered from the store | MB-R-058 |
| 5 | Write Single Coil | applied to the store | MB-R-058 |
| 6 | Write Single Register | applied to the store | MB-R-058 |
| 15 | Write Multiple Coils | applied to the store | MB-R-058 |
| 16 | Write Multiple Registers | applied to the store | MB-R-058 |
| 23 | Read/Write Multiple Registers | applied to the store, atomically (server-only) | MB-R-058, MB-R-063 |

Every other code — everything outside 1, 2, 3, 4, 5, 6, 15, 16, 23 — is **rejected** with `IllegalFunction` (`0x01`). Among them: Report Server Id (17), Mask Write Register (22), Read Device Identification (43 / MEI), Diagnostics (8), Get Comm Event Counter (11), Get Comm Event Log (12), Read File Record (20), Write File Record (21), Read FIFO Queue (24), any custom code.

### Exception codes emitted by the server

| Exception | When | Req |
|---|---|---|
| `IllegalFunction` (1) | function code unsupported — and only then | MB-R-059 |
| `IllegalDataAddress` (2) | any addressing or access failure on a supported code: range not fully covered, cells reject the direction, or cell type mismatch | MB-R-060 |

No other exception code is produced by the server.

---

## Transports

Exactly six: **TCP**, **RTU** (serial), **RtuOverTcp** (RTU framing over TCP), **UDP** (MBAP framing over UDP datagram), **Ascii** (ASCII framing over serial), **AsciiOverTcp** (ASCII framing over TCP).

---

## Modbus TCP connection config

Shared by client and server roles.

| Field | Type | Default | Valid range | Role | Req |
|---|---|---|---|---|---|
| `ip` | string | `127.0.0.1` | parses as an IPv4/IPv6 socket address together with `port` | client: address to connect to; server: interface to bind | MB-R-068, MB-R-069, MB-R-070 |
| `port` | u16 | `502` | 0–65535 | client: target port; server: listen port | MB-R-068, MB-R-069, MB-R-070 |
| `timeout_ms` | usize | `3000` | ≥ 0 | per-operation and connect timeout | MB-R-040, MB-R-068 |
| `delay_ms` | usize | `0` | ≥ 0 | wait before first operation after connect | MB-R-038 |
| `interval_ms` | usize | `0` | ≥ 0 (0 ⇒ ~1 ms tick) | interval between operations | MB-R-039 |
| `reconnect` | bool | `true` | — | client: auto-reconnect with backoff (MB-R-050–055); server: retry bind, serial-open, or mid-serve failure with the same backoff (MB-R-130–134) | MB-R-050, MB-R-071, MB-R-130 |
| `tls` | `ModbusTlsConfig` | both policies `None` | client+server | two-role container, `[tls.server]`/`[tls.client]`; requirements.md MB-R-104ff | MB-R-104, MB-R-105 |

Absent from a serialized config: `reconnect` defaults `true`; the rest have no serde defaults and must be present.

Example — server presenting a private-CA identity under mTLS, client trusting platform roots plus one private CA:

```toml
[tls.server]
mode = "mutual"
[tls.server.identity]
source = "files"
cert_file = "/etc/ferrowl/server.crt"
key_file  = "/etc/ferrowl/server.key"
[tls.server.verification]
verify = "ca-files"
ca_files = ["/etc/ferrowl/fleet-ca.pem"]

[tls.client]
mode = "tls"
[tls.client.verification]
verify = "root-store"
extra_ca_files = ["/etc/ferrowl/private-ca.pem"]
```

`RtuOverTcp` (tag `rtu_over_tcp`) uses this exact field table. `Udp` (tag `udp`) uses it minus `tls` (MB-R-116: no handshake, no TLS/DTLS). `AsciiOverTcp` (tag `ascii_over_tcp`) uses this exact table (MB-R-125).

---

## Modbus RTU connection config

| Field | Type | Default | Valid range | Role | Req |
|---|---|---|---|---|---|
| `path` | string | — (required) | openable serial device path | serial device | MB-R-072, MB-R-074 |
| `baud_rate` | u32 | `115200` | any rate the device accepts | line speed | MB-R-072 |
| `slave` | u8 | `1` | 0–255 | slave id the client's context is initially attached to (inert — [`edge-cases.md`](./edge-cases.md) MB-E-075) | — |
| `parity` | optional string | unset (serial default) | `even`, `odd`, `none` (case-insensitive) | parity bit | MB-R-072, MB-R-073 |
| `data_bits` | optional u8 | unset (serial default) | `5`, `6`, `7`, `8` | data bits | MB-R-072, MB-R-073 |
| `stop_bits` | optional u8 | unset (serial default) | `1`, `2` | stop bits | MB-R-072, MB-R-073 |
| `timeout_ms` | usize | `3000` | ≥ 0 | per-operation timeout | MB-R-040 |
| `delay_ms` | usize | `0` | ≥ 0 | wait before first operation after connect | MB-R-038 |
| `interval_ms` | usize | `0` | ≥ 0 (0 ⇒ ~1 ms tick) | interval between operations | MB-R-039 |
| `reconnect` | bool | `true` | — | as in `## Modbus TCP connection config` | MB-R-075, MB-R-130 |

Out-of-range `parity`, `data_bits`, or `stop_bits` fails with a serial configuration error **before** the port opens.

`Ascii` (tag `ascii`) uses this exact field table (MB-R-121).

---

## Module instance spec (session / `--module`)

One Modbus instance: the per-instance on-the-wire endpoint. All *timing* lives in the device config (`## Device config (one file = one device type)`), never here.

| Field | Type | Default | Notes | Req |
|---|---|---|---|---|
| `name` | string | — (required) | tab / instance name | CS-R-014 |
| `device` | string | — (required) | device config file path | CS-R-015 |
| `role` | `client` \| `server` \| `monitor` | `server` | | MB-R-076 |
| `endpoint` | tagged union, tag `transport` | — (required) | `tcp`, `rtu`, `rtu_over_tcp`, `udp`, `ascii`, `ascii_over_tcp` | MB-R-076 |

`role = monitor` valid only with `transport = rtu` or `ascii` (MB-R-140); any other transport fails configuration resolution (MB-R-191).

### `endpoint` with `transport = "tcp"`

| Field | Type | Default | Req |
|---|---|---|---|
| `ip` | string | — (required in the session file; `127.0.0.1` from `--module`) | MB-R-068, MB-R-069 |
| `port` | u16 | — (required) | MB-R-068, MB-R-069 |

### `endpoint` with `transport = "rtu"`

| Field | Type | Default | Valid range | Req |
|---|---|---|---|---|
| `path` | string | — (required) | serial device path | MB-R-072 |
| `baud_rate` | u32 | `19200` | — | MB-R-072 |
| `parity` | optional string | unset | `even`, `odd`, `none` | MB-R-072, MB-R-073 |
| `data_bits` | optional u8 | unset | 5–8 | MB-R-072, MB-R-073 |
| `stop_bits` | optional u8 | unset | 1, 2 | MB-R-072, MB-R-073 |

The RTU baud default here (`19200`) differs from the transport-level default (`115200`); the module spec's value reaches the wire. The module spec carries **no** `slave` field — a client addresses each request with the slave id of the register polled or written.

### endpoint with `transport = "rtu_over_tcp"`

Same fields as `transport = "tcp"` (`## Modbus TCP connection config`).

### endpoint with `transport = "udp"`

Same fields as `transport = "tcp"` (`## Modbus TCP connection config`).

### endpoint with `transport = "ascii"`

Same fields as `transport = "rtu"` (above).

### endpoint with `transport = "ascii_over_tcp"`

Same fields as `transport = "tcp"` (`## Modbus TCP connection config`).

### `--module` key/value form

`--module name=…,device=…,transport=…,…` accepts the same keys, with:

- `type` alias for `device` (CL-R-002)
- `baud` alias for `baud_rate` (CL-R-002)
- `transport` default `tcp` (CL-R-002)
- `role` default `server` (CL-R-002)
- `ip` default `127.0.0.1` (CL-R-002)
- `port` **required** for `transport=tcp`, `rtu_over_tcp`, `udp`, `ascii_over_tcp`; `path` **required** for `transport=rtu` or `ascii` (CL-R-002)

---

## Device config (one file = one device type)

| Field | Type | Default | Notes | Req |
|---|---|---|---|---|
| `version` | optional string | unset | stamped on save | CS-R-022 |
| `timeout_ms` | optional usize | `3000` | per-operation timeout | MB-R-040, MB-R-087 |
| `delay_ms` | optional usize | `1000` | delay before first operation after connect | MB-R-038, MB-R-087 |
| `interval_ms` | optional usize | `1000` | poll interval | MB-R-039, MB-R-087 |
| `reconnect` | optional bool | `true` | client: auto-reconnect (MB-R-050–055); server: bind/serial-open/mid-serve retry (MB-R-130–134) | MB-R-050, MB-R-130, MB-R-087 |
| `read_ranges` | `ReadRanges` | empty | explicit batched read windows (``### `read_ranges` ``) | MB-R-082, MB-R-083 |
| `definitions` | map name → `RegisterDef` | — (required) | register table (``### `RegisterDef` ``) | MB-R-077 |
| `scripts` | list | empty | Lua sim scripts — `scripting/` | SC-R-022 |
| `script_interval` | f64 seconds | `1.0` | Lua sim cycle; floored at `0.05`; NaN/∞/≤0 → `1.0` | SC-R-016, SC-R-045 |

Device-config timing defaults (`delay_ms` = 1000, `interval_ms` = 1000) are what an application-built module uses; they deliberately differ from the transport-level `0`.

`role = monitor`: device config carries only `version`, `reconnect` (also gating MB-R-192's serial-open retry), and `definitions` (list of `MonitorRegisterDef`, ``### `MonitorRegisterDef` `` — each entry carries its own `name`, since two interpretations on different `slave_id`s may share a name; MB-R-148 scopes edit/remove to one slave id's set, and a name-keyed map would collapse same-named entries across slave ids). `timeout_ms`, `delay_ms`, `interval_ms`, `read_ranges`, `scripts`, `script_interval` dropped: a monitor never initiates a transaction, has no poll loop, and (display-only) no Lua sim surface.

### `read_ranges`

| Field | Type | Applies to | Req |
|---|---|---|---|
| `holding` | optional string | holding registers | MB-R-082, MB-R-083 |
| `input` | optional string | input registers | MB-R-082, MB-R-083 |
| `coils` | optional string | coils | MB-R-082, MB-R-083 |
| `discrete` | optional string | discrete inputs | MB-R-082, MB-R-083 |

Each value: comma-separated **inclusive** address ranges, e.g. `"0-100,140-160"`. Bare number (`"5"`) = single address 5. Malformed or reversed entries skipped silently.

### `RegisterDef`

| Field | Type | Default | Valid values | Req |
|---|---|---|---|---|
| `slave_id` | u8 | `1` | 0–255 | MB-R-002 |
| `kind` | enum | `InputRegister` | `Coil`, `DiscreteInput`, `HoldingRegister`, `InputRegister` | MB-R-004, MB-R-097 |
| `address` | optional u16 | unset ⇒ virtual | 0–65535 | MB-R-003 |
| `virtual` | bool | `false` | `true` forces virtual even with `address` set | MB-R-003, MB-R-080 |
| `access` | enum | `ReadWrite` | `ReadOnly`, `WriteOnly`, `ReadWrite` | MB-R-005 |
| `type` | enum | — (required) | `U8`, `U16`, `U32`, `U64`, `U128`, `I8`, `I16`, `I32`, `I64`, `I128`, `F32`, `F64`, `Ascii` | MB-R-006, MB-R-010, MB-R-011 |
| `endian` | enum | `Big` | `Big`, `Little` | MB-R-013 |
| `word_order` | enum | `Normal` | `Normal`, `Reversed` (register order, numeric only) | MB-R-099, MB-R-100 |
| `resolution` | f64 | `1.0` | display scale (`displayed = raw × resolution`) | MB-R-021 |
| `bitmask` | optional string | unset ⇒ full mask | `0x`-prefixed hex or decimal; integer types only | MB-R-014, MB-R-015, MB-R-016 |
| `length` | usize | `1` | ASCII width in registers (ignored for numeric) | MB-R-011 |
| `alignment` | enum | `Left` | `Left`, `Right` (ASCII only) | MB-R-019 |
| `values` | list of `{name, value}` | empty | named/enum-style values for selection registers | — |
| `description` | string | empty | | — |
| `default` | optional scalar | unset | int, float, or string; written to memory on load | MB-R-079 |
| `update` | optional string | unset | **legacy**: per-register Lua snippet; migrated into `scripts` on load, never written back | SC-R-025, CS-R-054 |

`value`/`default` scalars are untagged: `10` integer, `1.5` float, `"idle"` text.

### `MonitorRegisterDef`

MB-R-145 — display-only interpretation against a monitor's observed-value table (`## Device config (one file = one device type)`): identical to `RegisterDef` (``### `RegisterDef` ``) minus `access` (table is observed, not owned — no direction to declare) and `update` (no store cell to script against).

| Field | Type | Default | Valid values | Req |
|---|---|---|---|---|
| `name` | string | — (required) | display name; unique within its own `slave_id`, not across (MB-R-148) | MB-R-148 |
| `slave_id` | u8 | `0` | 0–255 | MB-R-002 |
| `kind` | enum | `InputRegister` | `Coil`, `DiscreteInput`, `HoldingRegister`, `InputRegister` | MB-R-004, MB-R-097 |
| `address` | optional u16 | unset ⇒ virtual | 0–65535 | MB-R-003 |
| `virtual` | bool | `false` | `true` forces virtual even with `address` set | MB-R-003 |
| `type` | enum | — (required) | `U8`, `U16`, `U32`, `U64`, `U128`, `I8`, `I16`, `I32`, `I64`, `I128`, `F32`, `F64`, `Ascii` | MB-R-006, MB-R-010, MB-R-011 |
| `endian` | enum | `Big` | `Big`, `Little` | MB-R-013 |
| `word_order` | enum | `Normal` | `Normal`, `Reversed` (register order, numeric only) | MB-R-099, MB-R-100 |
| `resolution` | f64 | `1.0` | display scale (`displayed = raw × resolution`) | MB-R-021 |
| `bitmask` | optional string | unset ⇒ full mask | `0x`-prefixed hex or decimal; integer types only | MB-R-014, MB-R-015, MB-R-016 |
| `length` | usize | `1` | ASCII width in registers (ignored for numeric) | MB-R-011 |
| `alignment` | enum | `Left` | `Left`, `Right` (ASCII only) | MB-R-019 |
| `values` | list of `{name, value}` | empty | named/enum-style values for selection registers | — |
| `description` | string | empty | | — |
| `default` | optional scalar | unset | int, float, or string; no memory store to write into, so accepted but no effect — kept so a `RegisterDef`-shaped fragment still deserializes | — |

A pasted-in fragment carrying `access` and/or `update` deserializes cleanly, both ignored as unknown fields (same tolerance `## Device config (one file = one device type)`/``### `read_ranges` `` document for other role-conditional shapes).
