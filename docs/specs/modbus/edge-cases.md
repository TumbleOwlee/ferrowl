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

## Monitor boundaries

| Condition | Behavior |
|---|---|
| A monitor observes a frame that fails CRC (RTU) or LRC (Ascii) validation, or is otherwise malformed | logged to the monitor's message log at Warning level and discarded, rather than silently dropped — expected during normal operation on a live multi-drop bus (noise, a device's own retry, or the monitor attaching mid-frame), and the visibility is worth the log volume |

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
| Read operation addressed to slave id 0 on RTU | fails locally, never sent; follows the exception-retry path (MB-R-101). On TCP, unit 0 is an ordinary unit and is sent normally |
| Write command addressed to slave id 0 on RTU | fire-and-forget: written and not awaited, always logged as executed even if no device applied it (MB-R-102) |

---

## 4. Server boundaries

| Condition | Behavior |
|---|---|
| Read of an address range not declared in the store | exception `IllegalDataAddress` |
| Write to an address range that is not writable | exception `IllegalDataAddress` |
| Coil request against register cells, or the reverse | exception `IllegalDataAddress` |
| Any function code outside the nine of MB-R-058 (report-server-id, mask-write-register, read-device-identification, diagnostics, comm-event, file record, FIFO queue, custom) | exception `IllegalFunction`, request logged |
| Read/write-multiple-registers whose read range is unreadable or write range is unwritable | exception `IllegalDataAddress`, and **no** write is applied |
| Read/write-multiple-registers under concurrent load | the read-check, write-check, read and write happen under a single exclusive hold; no request can interleave |
| Request for a slave id with no declared regions, on `Tcp`/`RtuOverTcp`/`AsciiOverTcp`/`Udp` | the store lookup fails → exception `IllegalDataAddress`. The server does not filter by slave id up front |
| Request for a slave id with no declared regions, on `Rtu`/`Ascii` (physical serial) | store lookup fails, answered with silence — a real multi-drop bus may carry another device that owns that id and will answer instead; the server must not contend on the wire (MB-R-128) |
| Request for a slave id with ≥1 declared region but address outside all of them, on `Rtu`/`Ascii` | unchanged: exception `IllegalDataAddress` — this id is this server's own, a bad range on it is a genuine error |
| RTU request addressed to slave id 0 | applied to the store, answered with silence — a store failure that would otherwise be an `IllegalDataAddress` exception is invisible to the sender (MB-R-103) |
| Malformed frame / framing error on the wire | rejected by the protocol layer before it reaches the request handler; the TCP server logs a processing failure and drops the connection, and the accept loop keeps running |
| TCP client disconnects mid-request | the connection's serve task ends; the accept loop and the store are unaffected |
| RTU serial port disappears mid-serve | the serve loop ends and the server task ends with an error. There is **no** RTU server reconnect (see §6.4) |

---

## 5. TLS boundaries

| Condition | Behavior |
|---|---|
| `self_signed` set together with explicit `cert_file`/`key_file` | `self_signed` wins unconditionally; the files are structurally unreachable, not merely ignored (MB-R-106) |
| `cert_file`/`key_file` set alone while `self_signed` is not set | configuration resolution fails, not server bind (MB-R-107) |
| The Modbus TCP setup dialog's Self-Signed toggled On after `cert_file`/`key_file` text was entered | resolved config excludes both files entirely; the widgets' stored text is preserved for when the toggle goes back Off (MB-R-135) |
| The Modbus TCP setup dialog's Skip-Verify toggled On after `ca_file` text was entered | resolved config excludes `ca_file` entirely (MB-R-135) |
| `tls` set in an RTU device config | ignored — the RTU `Config` has no `tls` field (MB-R-112), so the key is unreachable rather than rejected |
| A `cert_file`/`key_file`/`ca_file`/`client_cert_file`/`client_key_file`/`client_ca_files` path that is malformed PEM or unreadable | server or client start fails with a TLS configuration error, the same tier as MB-R-107/MB-R-108 |
| A `ServerTlsPolicy::MutualTls` client certificate signed by any one of several configured `ca_files` | accepted — verification is "any one matches", not "all must match"; `ca_files` is a trust-anchor set, not an ordered chain (MB-R-108) |
| `ClientCertVerification::Verify` constructed with an empty `ca_files` list | fails at construction/`resolve()` time, never at handshake time — the same tier as `ServerCertSource`'s own construction-time checks (MB-R-105/MB-R-108) |
| `ServerTlsPolicy::MutualTls` with `ClientCertVerification::SkipVerify` and a connection presenting no client certificate at all | handshake still fails — `SkipVerify` skips the CA/identity check on a *presented* cert, it does not make presenting one optional (MB-R-108) |
| The Modbus TCP setup dialog's server-role Skip Verify toggled On while mTLS is selected | the `client_ca_files` list widget is hidden and excluded from the resolved config regardless of entries already present; list contents preserved, restored when the toggle goes back Off (MB-R-136) |
| The Modbus TCP setup dialog's client-role Self Signed toggled On while mTLS is selected | the Client Cert/Key inputs are hidden and excluded from the resolved config regardless of text already present; text preserved, restored when the toggle goes back Off (MB-R-139) |
| A self-signed certificate/key pair, once generated for a module instance | cached and reused across every subsequent bind/connect/reconnect and `:restart`/`:reload`, including a config edit that leaves the resolved source self-signed; regenerated only on a transition *into* self-signed from something else (explicit files removed, the toggle flipped Off→On); a fresh module instance (not a `:restart`/`:reload` of the same one) discards the cache instead of reusing it, since the pair is never written to disk (MB-R-106/MB-R-138) |
| A legacy config file with the old singular `client_ca_file` set and `require_client_cert: true`, no `client_ca_files` present | still deserializes — `client_ca_file` is read as a one-element `ca_files` list when `client_ca_files` is absent or empty; a config carrying both prefers `client_ca_files` (MB-R-105/MB-R-108) |

---

## 6. Known limitations — intentional constraints

### 6.1 No max-registers-per-request bound at the protocol layer

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

### 6.2 The RTU `Config` cannot be flattened into a `clap` command

The RTU connection config doubles as a `clap` argument group, but its short flags
collide: `-s` is claimed by both `slave` and `stop_bits`, and `-d` by both
`data_bits` and `delay_ms` (both derived from the field initial). Flattening it
into a `clap::Parser` command panics at parse time via clap's debug assertions.

The config is therefore only ever reached through its serde path (session and
device config files, and the `--module` key/value form), which is unaffected. No
Modbus RTU flag is exposed as a top-level CLI flag.

### 6.3 The RTU `slave` config field is inert

The RTU config carries a `slave` field (default 1). It is read by no code path:
the client carries a slave id on every individual request, taken from the
operation or command, and never attaches the link to one slave. The field is kept
only so existing session and device config files keep parsing, and has no
observable effect on a running module.

An RTU **server** ignores the field entirely: it answers for whichever slave ids
have declared memory regions, not for a single configured one.

### 6.4 Server-side reconnect retries only after the current serve loop ends

Resolved: every server transport (TCP, RTU, `RtuOverTcp`, `Udp`, `Ascii`,
`AsciiOverTcp`) now honors `reconnect` — a listener bind failure, a serial-port
open failure, or a mid-serve failure retries using the same shared backoff driver
as the client (MB-R-051, MB-R-071, MB-R-075, MB-R-120, MB-R-124, MB-R-130–134).

Retained nuance: a mid-serve failure does not retry immediately. The server waits
for the *current* serve loop to fully end on its own before starting the backoff
wait — an in-flight connection is never torn down early just to reach a retry
sooner.

### 6.5 Unbounded TCP server connections

A TCP server spawns one task per accepted connection with no cap on the number of
concurrent connections and no idle timeout.

### 6.6 Only six transports

`RtuOverTcp` reuses the TCP config verbatim (no new/removed fields); its only difference from plain TCP is wire framing. `Udp` reuses the TCP config too, except it drops `tls`: the upstream UDP transport performs no handshake and offers no DTLS option, so there is nothing for a `tls` field to configure. Unlike `RtuOverTcp`, `Udp` also does not inherit RTU/RtuOverTcp's broadcast slave id 0 handling (MB-R-101–MB-R-103) — on `Udp`, slave id 0 is an ordinary slave id.

`Ascii` reuses the RTU config verbatim; its only difference from plain `Rtu` is wire framing — LRC checksum and `:`/CR LF delimiters instead of CRC and silence-delimited binary. `AsciiOverTcp` reuses the TCP config verbatim, the same framing swap applied to `RtuOverTcp`. Both `Ascii` and `AsciiOverTcp` inherit the RTU-family broadcast slave id 0 handling (MB-R-101–MB-R-103), unlike `Udp`.

### 6.7 Display resolution is one-way

`resolution` scales a value for *display only*. Encoding does not divide by it, so
value input is in raw, unscaled units. Entering `10` on a register with
`resolution = 0.5` stores the raw word `10` and then displays `5`. This is
consistent — display always scales, input never does — but it means the string you
type is not the string you read back.

### 6.8 Declaration failures are warned, not silent

Declaring a memory region still reports success or failure via `Memory::add_ranges`'s
`bool` return, and a rejected declaration still leaves the register or gap cell
without backing memory — reads and writes against it still fail at runtime. But
every module-construction, module-reconfiguration, and runtime register-edit call
site now logs a Warning (MB-R-129) identifying the register name (or, for an
explicit-read-range gap cell with no single register name, the slave id and
register kind) and the rejected address/range at the moment of rejection, so the
eventual runtime read/write failure is traceable back to its cause instead of
being a mystery.

The reachable case: a register added at runtime at an address that a `read_ranges`
gap already declared as a read-only cell. The overlap is (existing `Read` cell,
requested `ReadWrite` region), which is not one of the widening combinations, so
the declaration is rejected and dropped — and now logs a Warning naming the
register and the rejected range.

### 6.9 Client writes are fire-and-forget

A client-side write is dispatched to the client task's command channel and the
caller is told "sent" as soon as it is queued. The Modbus response (including an
exception response) is logged by the client task but is not reported back to the
caller, and the store is not updated from it. The polled value is what eventually
reflects the truth — except for write-only registers, whose written value is
mirrored into the store locally because it is not otherwise observable.

### 6.10 The register's `access` does not gate store access

A register's `access` (`ReadOnly` / `WriteOnly` / `ReadWrite`) does **not**
determine the direction of its backing memory cells; the register's *kind* does
(coils and holding registers get read/write cells, discrete inputs and input
registers get read-only cells). `access` only governs whether the register is
polled (write-only registers are excluded from read operations) and whether a
client-side write is mirrored into the store.

A `ReadOnly` holding register is therefore still writable by a remote master
against a server module.
