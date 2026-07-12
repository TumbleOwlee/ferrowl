# Modbus domain

Crates: `ferrowl-codec`, `ferrowl-store`, `ferrowl-modbus`. App-level wiring: `ferrowl/src/module/modbus/`, `ferrowl/src/instance/`.

Snapshot: v0.4.13. Update this file when behavior it documents changes (see root `CONTRIBUTING.md`).

## 1. `ferrowl-codec` — register description & value codec

### 1.1 `Register`

`ferrowl-codec/src/lib.rs:42-57`. Built via `RegisterBuilder`; only `format` is mandatory.

| Field | Type | Default | Purpose |
|---|---|---|---|
| `slave_id` | `tokio_modbus::SlaveId` (`u8`) | `0` | Modbus slave/unit id |
| `access` | `Access` | `ReadWrite` | Allowed direction (descriptive only, see §2.2 for enforcement) |
| `kind` | `Kind` | `HoldingRegister` | Which of the 4 Modbus tables |
| `address` | `Address` | `Virtual` | Fixed Modbus address or virtual (script-only, no wire address) |
| `format` | `Format` | — required | Data type, byte order, scale, bit-field |

Methods (`ferrowl-codec/src/lib.rs:59-102`): `decode(bytes: &[u16]) -> Result<Value, CodecError>`; `encode(s: &str) -> Result<Vec<u16>, CodecError>` (parse user string → words); `encode_value(&Value) -> Result<Vec<u16>, CodecError>`; `write_mask() -> Vec<u16>` (per-word bitmask this register owns, for bit-field aliasing); `merge_write(old: &[u16], value: &[u16]) -> Vec<u16>` = `(old & !mask) | (value & mask)` (read-modify-write preserving sibling bits).

### 1.2 `Kind` — the four Modbus register tables (`ferrowl-codec/src/kind.rs:9-19`)

| Kind | Function codes | Bit width | Access |
|---|---|---|---|
| `Coil` | 1 (read), 5 (write single), 15 (write multiple) | 1 bit | R/W |
| `DiscreteInput` | 2 (read) | 1 bit | RO |
| `HoldingRegister` (default) | 3 (read), 6 (write single), 16 (write multiple) | 16 bit | R/W |
| `InputRegister` | 4 (read) | 16 bit | RO |

### 1.3 Supported / unsupported function codes at the transport layer (`ferrowl-modbus`)

Supported: `ReadCoils`(1), `ReadDiscreteInputs`(2), `ReadHoldingRegisters`(3), `ReadInputRegisters`(4), `WriteSingleCoil`(5), `WriteSingleRegister`(6), `WriteMultipleCoils`(15), `WriteMultipleRegisters`(16), `ReadWriteMultipleRegisters`(23, **server-only**, atomic under one lock — `ferrowl-modbus/src/server_core.rs:345-426`; no client-side support).

Server rejects with `ExceptionCode::IllegalFunction`: `ReportServerId`, `MaskWriteRegister`, `ReadDeviceIdentification`, any `Request::Custom(_)` (`ferrowl-modbus/src/server_core.rs:329-442`). Client `read()` only supports the 4 read codes (`ferrowl-modbus/src/client_core.rs:104-152`).

### 1.4 Data formats (`ferrowl-codec/src/format/mod.rs:22-36`)

`Format` — 13 variants: `Ascii(Alignment, Width)`, `U8/I8/U16/I16` (1 register), `U32/I32` (2), `U64/I64` (4), `U128/I128` (8), `F32` (2), `F64` (4); integer/int8/16/32/64/128 and float carry `(Endian, Resolution, BitField)` (float has no bit-field, it's a no-op there). `Format::width()`/`length()` at `ferrowl-codec/src/format/mod.rs:67-117`.

`U8`/`I8` still occupy a **whole 16-bit register** (byte packed into high or low byte depending on `Endian`) — not a half-register (`ferrowl-codec/src/codec.rs:326-341`).

### 1.5 Format parameters

- **`Endian`**: `Big` | `Little` — byte order across the register words of a multi-word value. Note: each individual `u16` word is always big-endian internally; `Endian` only controls the order the *words themselves* are chained.
- **`Resolution(f64)`**: display scale, `displayed = raw * resolution`, default `1.0`. Display-only — `encode`/`encode_value`/`decode` operate on the raw unscaled value (`ferrowl-codec/src/codec.rs:257`).
- **`Width(usize)`**: ASCII width in registers (2 chars/register).
- **`Alignment`**: `Left` | `Right` — ASCII padding/truncation side; oversized input truncated (Left keeps first bytes, Right keeps last `length` bytes, `ferrowl-codec/src/codec.rs:307-326`).
- **`BitField { mask: u128 }`**: default = `u128::MAX` (no-op). `shift()` = trailing-zero count of mask. Read: `(raw & mask) >> shift`. Write: `(value << shift) & mask`. `fits(bits)` errors `CodecError::BitFieldWidth` if the mask exceeds the format's own integer width.

### 1.6 Access modes (`ferrowl-codec/src/access.rs:8-16`)

`Access`: `ReadOnly`, `WriteOnly`, `ReadWrite` (default) — descriptive metadata on `Register`; real enforcement is address-level, in `ferrowl-store::Memory` (§2).

### 1.7 `Address` (`ferrowl-codec/src/address.rs:9-14`)

`Fixed(u16)` or `Virtual` (no Modbus address; Lua-script-only value).

### 1.8 Word ↔ value conversion mechanics

- `decode`: words → byte stream (2 bytes/word, MSB first) → `.rev()` if `Endian::Little` → big-endian fold into unsigned int → bit-field mask+shift → cast to typed value.
- `encode_value`: value → bit-field placed → `to_be_bytes()`/`to_le_bytes()` per `Endian` → packed back into `u16` words (odd trailing byte becomes high byte of the final word).
- `F32`/`F64`: `to_bits()`/`from_bits()` IEEE-754 raw pattern, same endian handling, no bit-field.
- `Ascii`: 2 bytes/register, no endian/bit-field option, padded/truncated per `Alignment` to `2*Width(n)` bytes.
- String parsing accepts plain decimal, `0x`-hex (unsigned bit pattern), `-0x`-hex (two's-complement negative) for ints; decimal or `0x`-hex-as-bits for floats.
- `Value::as_hex_str()` formats raw unscaled value as zero-padded hex (two's complement / IEEE-754 bits / 2 hex digits per ASCII byte).

### 1.9 `CodecError`

`TooFewBytes(Format)`, `PackedAscii` (invalid UTF-8), `ParseInt`, `ParseFloat`, `ValueFormatMismatch(Format)`, `BitFieldWidth(Format)`.

## 2. `ferrowl-store` — in-memory register space

### 2.1 `Memory<K>` (`ferrowl-store/src/memory.rs:33-38`)

```rust
pub struct Memory<K: Hash + Eq + Clone + Default> {
    slices: HashMap<K, BTreeMap<Range, Slice>>,
}
```

Keyed by `K` — in `ferrowl-modbus`, `K = Key<SlaveKey>` where `SlaveKey{slave_id, kind}`: one memory region per (slave id, register table) pair. Per key, non-overlapping address ranges live in a `BTreeMap<Range, Slice>` ordered by start. `Range{start,end}` half-open `[start,end)`; `Range::new` panics on `usize` overflow; deserialization rejects `end < start`.

### 2.2 Cell model

- `CellType`: `Coil` (1 bit) | `Register` (16 bit).
- `Cell`: `Read(CellType,u16)` | `Write(CellType,u16)` | `ReadWrite(CellType,u16)` — access + type + stored value.
- `CellKind`: same without a value, used when declaring regions.
- Checked accessors: `accepts_write`/`accepts_read`, `try_set_value`/`try_value` (no-op / `None` on disallowed direction); unchecked: `value`/`set_value`.

### 2.3 `Slice`

Contiguous run of `Cell`s over one `Range`. `from_range` (zero-init), `from_value_range` (seeded), `extend` (grows only if adjacent, else no-op), `writable`/`write`/`write_unchecked`, `readable`/`read`/`read_unchecked`.

### 2.4 `add_ranges` — declaring regions (`ferrowl-store/src/memory.rs:52-104`)

`Memory::add_ranges(id, kind, ranges) -> bool`. **All-or-nothing**: works on a private copy; if any range in the batch hits an incompatible overlap, the entire call is aborted and `self` is unchanged. Overlapping ranges of matching `CellType` merge (`Read`+overlapping `Write` → widens to `ReadWrite`; identical kind = no-op; any other combination = incompatible, fails the call). Non-overlapping/adjacent ranges are appended or `extend`ed.

### 2.5 Read/write API

| Method | Checked? | Behavior |
|---|---|---|
| `write(id, ty, range, values)` | yes | Errors: `LengthMismatch{expected,got}`, `UnknownKey`, `AddressNotWritable` |
| `write_unchecked(id, range, values)` | no | Bypasses access rights — "for administrative UI writes; do not use on the hot path" |
| `writable(id, ty, range)` | — | `UnknownKey` / `AddressNotWritable` |
| `read(id, ty, range)` | yes | Errors: `UnknownKey`, `AddressNotReadable` |
| `read_unchecked(id, range)` | no | Returns stored value of `Write`-only cells too; `None` only if range not fully covered |
| `readable(id, ty, range)` | — | `UnknownKey` / `AddressNotReadable` |

Multi-slice reads/writes walk adjacent slices in order (`walk_slices`/`walk_slices_mut`) and verify full coverage.

### 2.6 Concurrency

`Memory` itself has **no internal locking**. `ferrowl-modbus` wraps it as `Arc<parking_lot::RwLock<Memory<Key<T>>>>` (sync lock, not `tokio::sync::RwLock`) — deliberate: server request handling takes the lock synchronously and drops it *before* any log `.await`, so it works correctly even on a single-threaded tokio runtime with no worker to block on. Separately, `Operation` lists and `Config` structs use `tokio::sync::RwLock` (crossed by `.await`).

## 3. `ferrowl-modbus` — client/server over TCP and RTU

### 3.1 Transports

```rust
pub enum Transport { Tcp(tcp::Config), Rtu(rtu::Config) }
```

Only TCP and RTU (serial). No Modbus ASCII, no gateway/TCP-over-serial mode.

**TCP `Config`**: `ip` (default `127.0.0.1`), `port` (default `502`), `timeout_ms` (3000), `delay_ms` (0), `interval_ms` (0), `reconnect` (default `true`, client-only).

**RTU `Config`**: `path` (required), `baud_rate` (default `115200`), `slave` (default `1`), `parity: Option<"even"|"odd"|"none">`, `data_bits: Option<5-8>`, `stop_bits: Option<1-2>`, plus the same timing fields. Known pre-existing bug: `Config` can't flatten into a `clap::Parser` — short-flag collision `-s` (slave/stop_bits), `-d` (data_bits/delay_ms) (`ferrowl-modbus/src/rtu/mod.rs:62-68`).

Serial validation: data bits 5/6/7/8, stop bits 1/2, parity `even|odd|none` case-insensitive; else `SerialError::Configuration`.

### 3.2 Client architecture (`client_core.rs`)

- Poll loop: after `sleep(delay_ms)`, ticks on `interval(max(interval_ms,1))` (0 ⇒ ~1ms tick, fastest). Each tick round-robins the shared `operations: Arc<tokio::sync::RwLock<Vec<Operation>>>`, reads via `tokio_modbus`, then `write_unchecked`s into `Memory` keyed by `(slave_id, fn_code)`.
- Concurrently `select!`s on an `mpsc::Receiver<Command>` for writes (`WriteSingleCoil`, `WriteMultipleCoils`, `WriteSingleRegister`, `WriteMultipleRegister`) or `Terminate`.
- **Retry**: an exception response increments a retry counter; after `MAX_RETRIES = 3` consecutive exceptions the operation is logged invalid and skipped (index advances) — exceptions don't disconnect. A **timeout or transport `Error`** on read/write *does* disconnect and ends the current connection run.
- **Reconnect/backoff**: if `config.reconnect`, exponential backoff on connect/run failure — `INITIAL_BACKOFF = 1s`, `MAX_BACKOFF = 30s`, doubling, reset to `INITIAL_BACKOFF` after any run with at least one successful read. `Command::Terminate` (or channel close) aborts a backoff wait immediately. Any other command received while backing off is dropped (not queued).
- Every individual operation is wrapped in `tokio::time::timeout(timeout_ms, ...)`.

### 3.3 Server architecture (`server_core.rs`)

- `Server<T,L>` implements `tokio_modbus::server::Service` (`call()` returns a boxed future, no thread-blocking bridge).
- Dispatches every inbound `Request` against shared `Memory` (function-code support: §1.3). Reads/writes logged: "request received" always; success/failure only when `verbose`.
- `verbose`: **TCP** logs per-request outcomes (`verbose=true`); **RTU** stays quiet on outcomes (`verbose=false`, only "received" logged).
- `ReadWriteMultipleRegisters` is atomic under one lock acquisition (readable→writable→read→write all in one guard scope).

### 3.4 Task lifecycle

- `tcp::ClientBuilder::spawn` / `rtu::ClientBuilder::spawn` / `*::ServerBuilder::spawn` each `tokio::spawn` a background task, return `JoinHandle<Result<(), Error>>`.
- TCP server: `TcpListener` + `tokio_modbus::server::tcp::Server::serve` accept loop, one `Server` instance per connection.
- RTU server: opens the `SerialStream` once, `RtuServer::serve_forever` — single persistent connection, no accept loop (serial is point-to-point).
- App-level wrapper `ferrowl::instance::Instance<T>` — see `infra.md` §3 for full lifecycle detail (start/stop/restart, 100ms grace, abort).

### 3.5 Memory keying

```rust
trait KeyParams { fn from_slave_fn(slave_id: SlaveId, fn_code: FunctionCode) -> Self; }
struct SlaveKey { slave_id: SlaveId, kind: ferrowl_codec::Kind }  // default KeyParams
```

One memory region per `(slave_id, table)` pair; unrecognized function codes fall back to `Kind::HoldingRegister`.

### 3.6 `Operation`, `Command`, aliases

`Operation{slave_id, fn_code, range}` — one recurring poll job. `Command`: `Terminate`, `WriteSingleCoil(SlaveId,Address,Coil)`, `WriteMultipleCoils(SlaveId,Address,Vec<Coil>)`, `WriteSingleRegister(SlaveId,Address,Word)`, `WriteMultipleRegister(SlaveId,Address,Vec<Word>)`. Aliases: `Address=u16`, `Word=u16`, `Coil=bool`.

### 3.7 Errors

```
Error { Modbus(ModbusError), Serial(SerialError), Tcp(TcpError), Server(io::Error) }
ModbusError { Exception(ExceptionCode), Error(tokio_modbus::Error), Timeout(Elapsed) }
SerialError { Error(tokio_serial::Error), Configuration(String) }
TcpError { Address(AddrParseError), Configuration(String), Error(io::Error), Timeout(Elapsed) }
```

Every read/write result is classified into one of these three shapes, driving retry-vs-disconnect (§3.2).

### 3.8 Logging

`trait LogFn` — client/server take separate `log`/`status` sinks. App-level `network_log_level` classifies lines heuristically: "disconnecting"/"reconnect disabled"/"timed out" → Error; "disconnected"/"reconnecting"/"invalid"/"dropped"/"failed" → Warning; else Info.

## 4. App-level Modbus module config (`ferrowl/src/module/modbus/config/`)

### `DeviceConfig` (one file = one device type)

- `version`, `timeout_ms/delay_ms/interval_ms: Option<usize>` (fall back to app-level defaults `DEFAULT_TIMEOUT_MS=3000`, `DEFAULT_DELAY_MS=1000`, `DEFAULT_INTERVAL_MS=1000`, `DEFAULT_RECONNECT=true` — **differ from crate-level defaults** `delay_ms=0, interval_ms=0`), `reconnect`, `log_file` (runtime-only).
- `read_ranges: ReadRanges` — explicit per-function-code batched read windows (`holding/input/coils/discrete: Option<String>`, e.g. `"0-100,140-160"` or bare `"5"`); empty ⇒ contiguous registers auto-merged instead.
- `definitions: BTreeMap<String, RegisterDef>` — the register table.
- `scripts: Vec<ScriptDef>` — see `lua.md`. Legacy per-register `update` snippets migrate on load.
- `script_interval: f64` seconds, default `1.0`, floored at `MIN_SCRIPT_INTERVAL_SECS = 0.05` (NaN/inf/≤0 also fall back to `1.0`).

### `RegisterDef`

`slave_id`, `kind` (default `InputRegister`), `address: Option<u16>` + `is_virtual: bool` (virtual wins even with a concrete address set), `access` (default `ReadWrite`), `value_type` (13 variants mirroring `Format`), `endian` (default `Big`), `resolution` (default `1.0`), `bitmask: Option<String>` (hex or decimal, unparseable/absent → full mask), `length` (ASCII width, default `1`), `alignment` (default `Left`), `values: Vec<NamedValue>` (enum-style named states), `update: Option<String>` (legacy), `description`, `default: Option<Scalar>` (written to memory on load).

### `Session` (`--session <file>`)

`version`, `modules: Vec<serde_json::Value>` (tagged by `"type"`, missing tag assumed `"modbus"`), session-level `scripts`, `interval: f64` (default `1.0`, sanitized, no floor). See `infra.md` §5 for full session-loading mechanics shared with OCPP.

### `ModuleSpec` / `Endpoint`

`ModuleSpec{name, device: String, role: Role (Client|Server, default Server), endpoint}`. `Endpoint`, tagged `transport`: `Tcp{ip,port}` or `Rtu{path, baud_rate (default 19200 — differs from crate-level RTU default 115200), parity, data_bits, stop_bits}`.

`ModbusModule::new` builds shared `Memory` from `definitions`: fixed-address registers get `MemKind::ReadWrite` (Coil/HoldingRegister) or `MemKind::Read` (DiscreteInput/InputRegister) ranges; virtual registers instead get an in-process `VirtualStore: Arc<RwLock<HashMap<String,Value>>>` entry (never touch `Memory`); defaults are encoded and `write_unchecked` at construction; explicit `read_ranges` gaps get extra `Read` cells so a single batched client request can span them.

`configs/evse.toml` in this repo is OCPP-only — no Modbus example config ships at the repo root; Modbus examples live only in crate unit tests.

## 5. Exact numeric limits

| Constant | Value | Location |
|---|---|---|
| `MAX_RETRIES` (client exception retries before skip) | 3 | `ferrowl-modbus/src/client_core.rs:34` |
| `INITIAL_BACKOFF` | 1 s | `ferrowl-modbus/src/client_core.rs:37` |
| `MAX_BACKOFF` | 30 s | `ferrowl-modbus/src/client_core.rs:39` |
| Default TCP port | 502 | `ferrowl-modbus/src/tcp/mod.rs:22` |
| Default TCP ip | `127.0.0.1` | `ferrowl-modbus/src/tcp/mod.rs:17` |
| Default RTU baud (crate-level) | 115200 | `ferrowl-modbus/src/rtu/mod.rs:20-21` |
| Default RTU baud (app-level `Endpoint::Rtu`) | 19200 | `ferrowl/src/module/modbus/config/session.rs:94,142-144` |
| Default RTU slave id | 1 | `ferrowl-modbus/src/rtu/mod.rs:24-25` |
| Crate-level default `timeout_ms` | 3000 | `ferrowl-modbus/src/tcp/mod.rs:26`, `ferrowl-modbus/src/rtu/mod.rs:41` |
| Crate-level default `delay_ms` | 0 | `ferrowl-modbus/src/tcp/mod.rs:30`, `ferrowl-modbus/src/rtu/mod.rs:45` |
| Crate-level default `interval_ms` | 0 (clamped to 1ms tick) | `ferrowl-modbus/src/tcp/mod.rs:34`, `ferrowl-modbus/src/rtu/mod.rs:49`; clamp at `ferrowl-modbus/src/client_core.rs:336` |
| App-level `DEFAULT_TIMEOUT_MS` | 3000 | `ferrowl/src/module/modbus/config/device.rs:17` |
| App-level `DEFAULT_DELAY_MS` | 1000 | `ferrowl/src/module/modbus/config/device.rs:18` |
| App-level `DEFAULT_INTERVAL_MS` | 1000 | `ferrowl/src/module/modbus/config/device.rs:19` |
| `MIN_SCRIPT_INTERVAL_SECS` | 0.05 s | `ferrowl/src/module/modbus/config/device.rs:68` |
| Default `script_interval`/session `interval` | 1.0 s | `ferrowl/src/module/modbus/config/device.rs:61-63`, `ferrowl/src/module/modbus/config/session.rs:30-32` |
| `Instance::stop` grace wait before abort | 100 ms | `ferrowl/src/instance/mod.rs:152` |
| Command channel buffer per instance | 10 | `instance/mod.rs:91,114` |
| Format widths (registers) | U8/I8/U16/I16=1, U32/I32/F32=2, U64/I64/F64=4, U128/I128=8 | `ferrowl-codec/src/format/mod.rs:68-76` |
| Serial data bits allowed | 5,6,7,8 | `ferrowl-modbus/src/common.rs:17-27` |
| Serial stop bits allowed | 1,2 | `ferrowl-modbus/src/common.rs:29-38` |
| Slave id range | `u8` (0-255) | `ferrowl-modbus/src/key.rs:37` |
| Register address range | `u16` (0-65535) | `ferrowl-codec/src/address.rs:11` |

No max-registers-per-request or max-concurrent-connections constant exists — bounded only by the wire's `u16` format (a poll range exceeding 65535 registers errors `IllegalDataValue` via a `u16::try_from` cast, `ferrowl-modbus/src/client_core.rs:96`); TCP concurrent connections are unbounded (one task spawned per accepted connection, no cap).
