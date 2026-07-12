# Modbus — API Contract

The stable public surface owned by the Modbus area: the Modbus function codes
supported per role, and the Modbus device/endpoint configuration fields.

Per the ownership rule in [`../README.md`](../README.md), the Modbus config fields
are specified here, not in `config-session/`. `config-session/` owns only the file
envelope (format, `version`, the session→module list, save/load, `migrate`).

---

## 1. Modbus function codes

### 1.1 Client (initiator)

| Code | Function | Issued by |
|---|---|---|
| 1 | Read Coils | poll loop |
| 2 | Read Discrete Inputs | poll loop |
| 3 | Read Holding Registers | poll loop |
| 4 | Read Input Registers | poll loop |
| 5 | Write Single Coil | write command |
| 6 | Write Single Register | write command |
| 15 | Write Multiple Coils | write command |
| 16 | Write Multiple Registers | write command |

The client's poll loop issues **only** the four read codes. Writes are only ever
issued in response to an explicit write command (from the TUI, a `:` command, a
Lua script, or a headless run) — the client never writes on its own initiative.

The client does **not** implement function code 23 (Read/Write Multiple
Registers), nor any other code.

### 1.2 Server (responder)

| Code | Function | Behavior |
|---|---|---|
| 1 | Read Coils | answered from the store |
| 2 | Read Discrete Inputs | answered from the store |
| 3 | Read Holding Registers | answered from the store |
| 4 | Read Input Registers | answered from the store |
| 5 | Write Single Coil | applied to the store |
| 6 | Write Single Register | applied to the store |
| 15 | Write Multiple Coils | applied to the store |
| 16 | Write Multiple Registers | applied to the store |
| 23 | Read/Write Multiple Registers | applied to the store, atomically (server-only) |

Explicitly **rejected** with exception `IllegalFunction` (`0x01`):

- Report Server Id (17)
- Mask Write Register (22)
- Read Device Identification (43 / MEI)
- any custom/user-defined function code

### 1.3 Exception codes emitted by the server

| Exception | When |
|---|---|
| `IllegalFunction` (1) | the function code itself is unsupported — and only then |
| `IllegalDataAddress` (2) | any addressing or access failure on a supported code: the range is not fully covered by declared regions, the cells reject the requested direction, or the cell type does not match |

No other exception code is ever produced by the server.

---

## 2. Transports

Exactly two transports are supported: **TCP** and **RTU** (serial). There is no
Modbus ASCII, no Modbus-over-UDP, and no RTU-over-TCP gateway mode.

---

## 3. Modbus TCP connection config

Fields of the TCP transport config, shared by the client and server roles.

| Field | Type | Default | Valid range | Role |
|---|---|---|---|---|
| `ip` | string | `127.0.0.1` | any address that parses as an IPv4/IPv6 socket address together with `port` | client: address to connect to; server: interface to bind |
| `port` | u16 | `502` | 0–65535 | client: target port; server: listen port |
| `timeout_ms` | usize | `3000` | ≥ 0 | per-operation and connect timeout |
| `delay_ms` | usize | `0` | ≥ 0 | wait before the first operation after connect |
| `interval_ms` | usize | `0` | ≥ 0 (0 ⇒ ~1 ms tick) | interval between successive operations |
| `reconnect` | bool | `true` | — | client-only: auto-reconnect with backoff. Ignored by the server. |

When these fields are absent from a serialized config, `reconnect` defaults to
`true`; the remaining fields have no serde defaults and must be present.

---

## 4. Modbus RTU connection config

| Field | Type | Default | Valid range | Role |
|---|---|---|---|---|
| `path` | string | — (required) | an openable serial device path | serial device |
| `baud_rate` | u32 | `115200` | any rate the serial device accepts | line speed |
| `slave` | u8 | `1` | 0–255 | slave id the client's context is initially attached to (see [`edge-cases.md`](./edge-cases.md)) |
| `parity` | optional string | unset (serial default) | `even`, `odd`, `none` (case-insensitive) | parity bit |
| `data_bits` | optional u8 | unset (serial default) | `5`, `6`, `7`, `8` | data bits |
| `stop_bits` | optional u8 | unset (serial default) | `1`, `2` | stop bits |
| `timeout_ms` | usize | `3000` | ≥ 0 | per-operation timeout |
| `delay_ms` | usize | `0` | ≥ 0 | wait before the first operation after connect |
| `interval_ms` | usize | `0` | ≥ 0 (0 ⇒ ~1 ms tick) | interval between successive operations |
| `reconnect` | bool | `true` | — | client-only: auto-reconnect with backoff. Ignored by the server. |

An out-of-range `parity`, `data_bits`, or `stop_bits` fails with a serial
configuration error **before** the port is opened.

---

## 5. Module instance spec (session / `--module`)

One Modbus module instance. This is the per-instance, on-the-wire endpoint; all
*timing* lives in the device config (§6), never here.

| Field | Type | Default | Notes |
|---|---|---|---|
| `name` | string | — (required) | tab / instance name |
| `device` | string | — (required) | path to the device config file |
| `role` | `client` \| `server` | `server` | |
| `endpoint` | tagged union, tag `transport` | — (required) | `tcp` or `rtu` |

### `endpoint` with `transport = "tcp"`

| Field | Type | Default |
|---|---|---|
| `ip` | string | — (required in the session file; `127.0.0.1` when built from `--module`) |
| `port` | u16 | — (required) |

### `endpoint` with `transport = "rtu"`

| Field | Type | Default | Valid range |
|---|---|---|---|
| `path` | string | — (required) | serial device path |
| `baud_rate` | u32 | `19200` | — |
| `parity` | optional string | unset | `even`, `odd`, `none` |
| `data_bits` | optional u8 | unset | 5–8 |
| `stop_bits` | optional u8 | unset | 1, 2 |

Note the RTU baud default here (`19200`) differs from the transport-level default
(`115200`); the module spec's value is the one that reaches the wire. The module
spec carries **no** `slave` field — a client addresses each request with the slave
id of the register being polled or written.

### `--module` key/value form

`--module name=…,device=…,transport=…,…` accepts the same keys, with:

- `type` as an alias for `device`
- `baud` as an alias for `baud_rate`
- `transport` defaulting to `tcp`
- `role` defaulting to `server`
- `ip` defaulting to `127.0.0.1`
- `port` **required** for `transport=tcp`; `path` **required** for `transport=rtu`

---

## 6. Device config (one file = one device type)

| Field | Type | Default | Notes |
|---|---|---|---|
| `version` | optional string | unset | stamped on save |
| `timeout_ms` | optional usize | `3000` | per-operation timeout |
| `delay_ms` | optional usize | `1000` | delay before first operation after connect |
| `interval_ms` | optional usize | `1000` | poll interval |
| `reconnect` | optional bool | `true` | client-only |
| `read_ranges` | `ReadRanges` | empty | explicit batched read windows (§6.1) |
| `definitions` | map of name → `RegisterDef` | — (required) | the register table (§6.2) |
| `scripts` | list | empty | Lua sim scripts — see `scripting/` |
| `script_interval` | f64 seconds | `1.0` | Lua sim cycle; floored at `0.05`; NaN/∞/≤0 fall back to `1.0` |

The device-config timing defaults (`delay_ms` = 1000, `interval_ms` = 1000) are
what an application-built module actually uses; they deliberately differ from the
transport-level defaults of `0`.

### 6.1 `read_ranges`

| Field | Type | Applies to |
|---|---|---|
| `holding` | optional string | holding registers |
| `input` | optional string | input registers |
| `coils` | optional string | coils |
| `discrete` | optional string | discrete inputs |

Each value is a comma-separated list of **inclusive** address ranges, e.g.
`"0-100,140-160"`. A bare number (`"5"`) is the single address 5. Malformed or
reversed entries are skipped silently.

### 6.2 `RegisterDef`

| Field | Type | Default | Valid values |
|---|---|---|---|
| `slave_id` | u8 | `0` | 0–255 |
| `kind` | enum | `InputRegister` | `Coil`, `DiscreteInput`, `HoldingRegister`, `InputRegister` |
| `address` | optional u16 | unset ⇒ virtual | 0–65535 |
| `virtual` | bool | `false` | `true` forces virtual even with an `address` set |
| `access` | enum | `ReadWrite` | `ReadOnly`, `WriteOnly`, `ReadWrite` |
| `type` | enum | — (required) | `U8`, `U16`, `U32`, `U64`, `U128`, `I8`, `I16`, `I32`, `I64`, `I128`, `F32`, `F64`, `Ascii` |
| `endian` | enum | `Big` | `Big`, `Little` |
| `resolution` | f64 | `1.0` | display scale (`displayed = raw × resolution`) |
| `bitmask` | optional string | unset ⇒ full mask | `0x`-prefixed hex or decimal; integer types only |
| `length` | usize | `1` | ASCII width in registers (ignored for numeric types) |
| `alignment` | enum | `Left` | `Left`, `Right` (ASCII only) |
| `values` | list of `{name, value}` | empty | named/enum-style values for selection registers |
| `description` | string | empty | |
| `default` | optional scalar | unset | int, float, or string; written to memory on load |
| `update` | optional string | unset | **legacy**: per-register Lua snippet; migrated into `scripts` on load and never written back |

`value`/`default` scalars are untagged: `10` is an integer, `1.5` a float, `"idle"`
text.
