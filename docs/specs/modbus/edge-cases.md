# Modbus — Edge Cases and Known Limitations

Boundary behavior, error semantics, and the constraints that are **intentional**.
Everything in §5 is working as implemented; it is recorded here so it is not
mistaken for an oversight and silently "fixed".

---

## 1. Codec boundaries

| Condition | Behavior |
|---|---|
| Fewer words supplied than the format's width | decode fails with a too-few-bytes error |
| More words supplied than the format's width | the surplus is ignored; only the first `width` words are consumed |
| `Ascii` bytes are not valid UTF-8 | decode fails with a packed-ASCII error |
| `Ascii` input longer than `2 × length` bytes | truncated: `Left` keeps the first bytes, `Right` keeps the last bytes. No error |
| `Ascii` value decoded from a padded block | zero-padding is **not** stripped; the decoded string contains the padding bytes |
| Bit-field mask exceeding the format's own width (e.g. `0x1FF` on `U8`) | decode and encode both fail with a bit-field-width error |
| Bit-field mask of `0` | degenerate but not an error: shift is 0, encode produces all-zero words, decode yields 0 |
| Non-numeric text entered on a numeric format | encode fails with a parse error |
| Typed value whose variant does not match the format | encode fails with a value/format mismatch error |
| `Ascii` with `length = 0` | zero-width: encodes to no words, decodes to the empty string |
| A `bitmask` string in the device config that does not parse | silently falls back to the full (no-op) mask — no error, no warning |
| A malformed or reversed entry in a `read_ranges` string | silently skipped — no error, no warning |
| `word_order = Reversed` on a single-register format (`U8`/`I8`/`U16`/`I16`) | inert no-op: reversing a one-word sequence changes nothing |
| `word_order` on an `Ascii` format | ignored — ASCII carries no register order, as it carries no byte order |

---

## 2. Store boundaries

| Condition | Behavior |
|---|---|
| Read/write against an address range not fully covered by declared regions | fails as a whole (`AddressNotReadable` / `AddressNotWritable`); no partial result, no partial write |
| Read/write against a device key with no declared regions | fails with `UnknownKey` |
| Checked write whose value count ≠ range length | fails with a length mismatch, writing nothing |
| Checked read of a write-only cell / write of a read-only cell | fails; the unchecked paths bypass this |
| Coil request against register cells (or the reverse) | fails — the cell type must match |
| Declaring a range that intersects existing regions | all intersecting regions and the new range merge into one region spanning their union; existing values are preserved, newly covered addresses are zero-initialized |
| Declaring a range that is merely adjacent to an existing region (zero-length overlap) | not an overlap; the two may stay separate regions. Coverage is still contiguous, so a read or write spanning both succeeds |
| Declaring a range whose overlap is incompatible (mismatched cell type, or an access combination that is not a `Read`+`Write` widening) | the **whole** declaring call is rejected, including any ranges in the same call that were compatible; the key's memory is left exactly as it was |
| A range whose `start + size` overflows a `usize` | panics. Unreachable from Modbus addressing (addresses are `u16`, widths ≤ 8), but reachable from a hand-edited config only via absurd values |
| A deserialized range with `end < start` | rejected at load |

---

## 3. Client boundaries

| Condition | Behavior |
|---|---|
| Modbus exception on a read | logged, no disconnect. Retried on the following ticks; after **3 consecutive** exceptions the operation is logged invalid, skipped, and the client advances to the next operation |
| Timeout on a read or write | disconnect, end the connection run. Subject to reconnect |
| Transport error on a read or write | disconnect, end the connection run. Subject to reconnect |
| Modbus exception on a write command | logged, no disconnect, no retry — the write is simply lost |
| Store write-back of a poll result fails (range not covered) | logged; the read result is discarded. The client stays connected and advances |
| An operation whose range length exceeds 65535 | answered locally with an `IllegalDataValue` exception (never sent), which then follows the exception-retry path. Unreachable via the config-driven planner, which caps requests at 125 registers / 2000 bits |
| Empty operation list | the tick fires and does nothing; the client stays connected |
| Command sent while the client is disconnected and backing off | dropped with a log line — **not** queued for after reconnect |
| Command channel full (10 pending commands) | the sender waits; commands are never silently dropped at the channel |
| Command sent to a server-role module, or to a module that is not running | rejected with an error |
| `interval_ms = 0` | treated as a 1 ms tick, not a busy loop |
| Ticks missed while a slow request is in flight | the schedule is delayed; no burst of catch-up requests |
| `delay_ms` | applied on **every** (re)connection, not only the first |

---

## 4. Server boundaries

| Condition | Behavior |
|---|---|
| Read of an address range not declared in the store | exception `IllegalDataAddress` |
| Write to an address range that is not writable | exception `IllegalDataAddress` |
| Coil request against register cells, or the reverse | exception `IllegalDataAddress` |
| Unsupported function code (report-server-id, mask-write-register, read-device-identification, custom) | exception `IllegalFunction`, request logged |
| Read/write-multiple-registers whose read range is unreadable or write range is unwritable | exception `IllegalDataAddress`, and **no** write is applied |
| Read/write-multiple-registers under concurrent load | the read-check, write-check, read and write happen under a single exclusive hold; no request can interleave |
| Request for a slave id with no declared regions | the store lookup fails → exception `IllegalDataAddress`. The server does not filter by slave id up front |
| Malformed frame / framing error on the wire | rejected by the protocol layer before it reaches the request handler; the TCP server logs a processing failure and drops the connection, and the accept loop keeps running |
| TCP client disconnects mid-request | the connection's serve task ends; the accept loop and the store are unaffected |
| RTU serial port disappears mid-serve | the serve loop ends and the server task ends with an error. There is **no** RTU server reconnect (see §5.4) |

---

## 5. Known limitations — intentional constraints

### 5.1 No max-registers-per-request bound at the protocol layer

Neither the client core nor the server core enforces the Modbus per-request limits
(125 registers / 2000 bits). The **only** enforcement is in the application-level
read-operation planner, which splits generated poll batches at those limits.

Consequences:

- A poll operation constructed directly (bypassing the planner) may exceed 125
  registers and will be sent as-is; the only guard is the `u16` count field, which
  fabricates an `IllegalDataValue` above 65535 registers.
- The **server** answers any request count the peer sends, limited only by the
  wire's `u16` count field and by whether the addresses are declared. It does not
  reject an over-long request with `IllegalDataValue`.
- A write command is never split: a register wider than the limit would be sent as
  a single write. Unreachable in practice — the widest format is 8 registers.

### 5.2 The RTU `Config` cannot be flattened into a `clap` command

The RTU connection config doubles as a `clap` argument group, but its short flags
collide: `-s` is claimed by both `slave` and `stop_bits`, and `-d` by both
`data_bits` and `delay_ms` (both derived from the field initial). Flattening it
into a `clap::Parser` command panics at parse time via clap's debug assertions.

The config is therefore only ever reached through its serde path (session and
device config files, and the `--module` key/value form), which is unaffected. No
Modbus RTU flag is exposed as a top-level CLI flag.

### 5.3 The RTU `slave` config field is inert in application use

The RTU config carries a `slave` field (default 1), used to attach the serial
context to a slave on connect. An application-built RTU client always passes `0`
for it, because the client re-targets the slave before every single request using
the slave id carried by the operation or command. The field therefore has no
observable effect on a running module.

An RTU **server** ignores the field entirely: it answers for whichever slave ids
have declared memory regions, not for a single configured one.

### 5.4 No server-side reconnect

`reconnect` is client-only. A server whose listener or serial port fails ends its
task; it does not retry the bind or the port open. Restarting it is the operator's
(or the module lifecycle's) job.

### 5.5 Unbounded TCP server connections

A TCP server spawns one task per accepted connection with no cap on the number of
concurrent connections and no idle timeout.

### 5.6 Only TCP and RTU

There is no Modbus ASCII, no Modbus-over-UDP, and no RTU-over-TCP gateway mode.

### 5.7 Display resolution is one-way

`resolution` scales a value for *display only*. Encoding does not divide by it, so
value input is in raw, unscaled units. Entering `10` on a register with
`resolution = 0.5` stores the raw word `10` and then displays `5`. This is
consistent — display always scales, input never does — but it means the string you
type is not the string you read back.

### 5.8 Declaration failures are silent

Declaring a memory region reports success or failure, but the module-construction
and runtime register-edit paths ignore that result. A declaration that is rejected
for an incompatible overlap therefore fails quietly: the register exists in the
table but has no backing memory, so reads and writes against it fail at runtime.

The reachable case: a register added at runtime at an address that a `read_ranges`
gap already declared as a read-only cell. The overlap is (existing `Read` cell,
requested `ReadWrite` region), which is not one of the widening combinations, so
the declaration is rejected and dropped.

### 5.9 Client writes are fire-and-forget

A client-side write is dispatched to the client task's command channel and the
caller is told "sent" as soon as it is queued. The Modbus response (including an
exception response) is logged by the client task but is not reported back to the
caller, and the store is not updated from it. The polled value is what eventually
reflects the truth — except for write-only registers, whose written value is
mirrored into the store locally because it is not otherwise observable.

### 5.10 The register's `access` does not gate store access

A register's `access` (`ReadOnly` / `WriteOnly` / `ReadWrite`) does **not**
determine the direction of its backing memory cells; the register's *kind* does
(coils and holding registers get read/write cells, discrete inputs and input
registers get read-only cells). `access` only governs whether the register is
polled (write-only registers are excluded from read operations) and whether a
client-side write is mirrored into the store.

A `ReadOnly` holding register is therefore still writable by a remote master
against a server module.
