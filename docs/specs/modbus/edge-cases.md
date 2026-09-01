# Modbus — Edge Cases and Known Limitations

Boundary behavior, error semantics, intentional constraints. The known-limitations section below (`## 6. Known limitations — intentional constraints`) is working as implemented; recorded so it is not "fixed".

---

## 1. Codec boundaries

| ID | Condition | Behavior |
|---|---|---|
| **MB-E-001** | Fewer words than the format's width | decode fails with a too-few-bytes error |
| **MB-E-002** | More words than the format's width | surplus ignored; only the first `width` words consumed |
| **MB-E-003** | `Ascii` bytes not valid UTF-8 | decode fails with a packed-ASCII error |
| **MB-E-004** | `Ascii` input longer than `2 × length` bytes | truncated: `Left` keeps first bytes, `Right` keeps last. No error |
| **MB-E-005** | `Ascii` decoded from a padded block | zero-padding **not** stripped; decoded string contains the padding bytes |
| **MB-E-006** | Bit-field mask exceeding the format's width (e.g. `0x1FF` on `U8`) | decode and encode fail with a bit-field-width error |
| **MB-E-007** | Bit-field mask `0` | degenerate, not an error: shift 0, encode produces all-zero words, decode yields 0 |
| **MB-E-008** | Non-numeric text on a numeric format | encode fails with a parse error |
| **MB-E-009** | Typed value whose variant does not match the format | encode fails with a value/format mismatch error |
| **MB-E-010** | `Ascii` with `length = 0` | zero-width: encodes to no words, decodes to empty string |
| **MB-E-011** | Device-config `bitmask` string that does not parse | silently falls back to the full (no-op) mask — no error, no warning |
| **MB-E-012** | Malformed or reversed entry in a `read_ranges` string | silently skipped — no error, no warning |
| **MB-E-013** | `word_order = Reversed` on a width-1 format (`U8`/`I8`/`U16`/`I16`) | inert no-op |
| **MB-E-014** | `word_order` on `Ascii` | ignored — ASCII carries no register order, as it carries no byte order |

---

## Monitor boundaries

| ID | Condition | Behavior |
|---|---|---|
| **MB-E-015** | Frame fails CRC (RTU) or LRC (Ascii), or otherwise malformed | logged at Warning level and discarded, not silently dropped — expected on a live multi-drop bus (noise, a device's retry, monitor attaching mid-frame); visibility worth the log volume |
| **MB-E-016** | Completed pairing's operation (MB-R-146) is `ReadWriteMultipleRegisters` — two addresses, two quantities | Address renders `read_address/write_address`; Quantity `read_quantity/write_quantity`; Values/Payload shows the read response's registers only — the write's values are visible in Memory layout once applied, and both payloads would double the column width for a rare operation |
| **MB-E-017** | No traffic at all for a table kind on the selected unit id | that kind's hex-editor block (UI-R-063) omitted from Memory layout, per MB-R-144's "non-empty kinds only" — not shown as an empty block |
| **MB-E-018** | Retransmitted `WriteSingleRegister`/`WriteSingleCoil` request arrives while awaiting that request's response | decodes as the response to itself (byte-identical on the wire) — false `Ok` record and phantom observed-table write; `WriteMultiple*` and reads unaffected (request/response encodings differ) |
| **MB-E-019** | Two configured instances share the same Rtu/Ascii path | MB-R-150 catches it before the OS-level open, reports a distinct conflict status, recovers once one instance stops or moves |
| **MB-E-020** | Serial path held by something the session doesn't know (external process, bridge-mode downstream/upstream) | MB-R-150 sees only same-session instances; external holder surfaces as an ordinary OS-level open failure/retry |

---

## 2. Store boundaries

| ID | Condition | Behavior |
|---|---|---|
| **MB-E-021** | Read/write against a range not fully covered by declared regions | fails as a whole (`AddressNotReadable` / `AddressNotWritable`); no partial result, no partial write |
| **MB-E-022** | Read/write against a key with no declared regions | fails with `UnknownKey` |
| **MB-E-023** | Checked write whose value count ≠ range length | fails with a length mismatch, writes nothing |
| **MB-E-024** | Checked read of a write-only cell / write of a read-only cell | fails; unchecked paths bypass this |
| **MB-E-025** | Coil request against register cells (or reverse) | fails — cell type must match |
| **MB-E-026** | Declaring a range intersecting existing regions | all intersecting regions and the new range merge into one spanning their union; values preserved, new addresses zero-initialized |
| **MB-E-027** | Declaring a range merely adjacent to an existing region (zero-length overlap) | not an overlap; may stay separate regions. Coverage still contiguous, so a spanning read/write succeeds |
| **MB-E-028** | Declaring a range whose overlap is incompatible (mismatched cell type, or an access combination that is not a `Read`+`Write` widening) | the **whole** call rejected, including compatible ranges in the same call; key's memory unchanged |
| **MB-E-029** | Range whose `start + size` overflows `usize` | panics. Unreachable from Modbus addressing (`u16` addresses, widths ≤ 8); reachable only from a hand-edited config with absurd values |
| **MB-E-030** | Deserialized range with `end < start` | rejected at load |

---

## 3. Client boundaries

| ID | Condition | Behavior |
|---|---|---|
| **MB-E-031** | Modbus exception on a read | logged, no disconnect. Retried on following ticks; after **3 consecutive** the operation is logged invalid, skipped, client advances |
| **MB-E-032** | Timeout on a read or write | disconnect, end the run. Subject to reconnect |
| **MB-E-033** | Transport error on a read or write | disconnect, end the run. Subject to reconnect |
| **MB-E-034** | Modbus exception on a write command | logged, no disconnect, no retry — write lost |
| **MB-E-035** | Store write-back of a poll result fails (range not covered) | logged; result discarded. Client stays connected, advances |
| **MB-E-036** | Operation whose start address exceeds 65535 | answered locally with `IllegalDataValue` (never sent), same shape as an over-long range (MB-R-149). Unreachable via the config-driven planner (`addr: u16`); hand-edited config only |
| **MB-E-037** | Operation whose range length exceeds 65535 | answered locally with `IllegalDataValue` (never sent), then exception-retry path (MB-R-149). Unreachable via the planner, which caps at 125 registers / 2000 bits |
| **MB-E-038** | Empty operation list | tick fires, does nothing; client stays connected |
| **MB-E-039** | Command sent while disconnected and backing off | dropped with a log line — **not** queued |
| **MB-E-040** | Command channel full (10 pending) | sender waits; never silently dropped at the channel |
| **MB-E-041** | Command to a server-role module, or a module not running | rejected with an error |
| **MB-E-042** | `interval_ms = 0` | 1 ms tick, not a busy loop |
| **MB-E-043** | Ticks missed while a slow request is in flight | schedule delayed; no burst of catch-up requests |
| **MB-E-044** | `delay_ms` | applied on **every** (re)connection, not only the first |
| **MB-E-045** | Read operation to slave id 0 on RTU | fails locally, never sent; exception-retry path (MB-R-101). On TCP, unit 0 is ordinary and sent |
| **MB-E-046** | Write command to slave id 0 on RTU | fire-and-forget: written, not awaited, always logged as executed even if no device applied it (MB-R-102) |

---

## 4. Server boundaries

| ID | Condition | Behavior |
|---|---|---|
| **MB-E-047** | Read of a range not declared in the store | `IllegalDataAddress` |
| **MB-E-048** | Write to a range not writable | `IllegalDataAddress` |
| **MB-E-049** | Coil request against register cells, or reverse | `IllegalDataAddress` |
| **MB-E-050** | Function code outside MB-R-058's nine (report-server-id, mask-write-register, read-device-identification, diagnostics, comm-event, file record, FIFO queue, custom) | `IllegalFunction`, request logged |
| **MB-E-051** | Read/write-multiple-registers whose read range is unreadable or write range unwritable | `IllegalDataAddress`, **no** write applied |
| **MB-E-052** | Read/write-multiple-registers under concurrent load | read-check, write-check, read, write under a single exclusive hold; no interleaving |
| **MB-E-053** | Request for a slave id with no declared regions, on `Tcp`/`RtuOverTcp`/`AsciiOverTcp`/`Udp` | store lookup fails → `IllegalDataAddress`. No up-front slave id filter |
| **MB-E-054** | Request for a slave id with no declared regions, on `Rtu`/`Ascii` (physical serial) | store lookup fails, answered with silence — another device on a multi-drop bus may own that id and answer; server must not contend (MB-R-128) |
| **MB-E-055** | Request for a slave id with ≥1 declared region but address outside all, on `Rtu`/`Ascii` | `IllegalDataAddress` — this id is this server's own; a bad range on it is a genuine error |
| **MB-E-056** | RTU request to slave id 0 | applied to the store, answered with silence — a store failure that would be `IllegalDataAddress` is invisible to the sender (MB-R-103) |
| **MB-E-057** | Malformed frame / framing error | rejected by the protocol layer before the handler; TCP server logs a processing failure and drops the connection, accept loop continues |
| **MB-E-058** | TCP client disconnects mid-request | that connection's serve task ends; accept loop and store unaffected |
| **MB-E-059** | RTU serial port disappears mid-serve | serve loop ends, server task ends with an error; retry per MB-E-076 |

---

## 5. TLS boundaries

| ID | Condition | Behavior |
|---|---|---|
| **MB-E-060** | Dialog Self-Signed toggled On after `cert_file`/`key_file` text entered | resolved config excludes both files; stored text preserved for when the toggle goes Off (MB-R-135) |
| **MB-E-061** | Dialog client-role Skip-Verify toggled On after Root Store/CA list state entered | resolved verification excludes both, becoming `CertVerification::Skip` (MB-R-135) |
| **MB-E-062** | `tls` set in an RTU device config | ignored — RTU `Config` has no `tls` field (MB-R-112); key unreachable rather than rejected |
| **MB-E-063** | `cert_file`/`key_file`/`ca_files`/`extra_ca_files`/client-identity path malformed PEM or unreadable | server or client start fails with a TLS configuration error, same tier as MB-R-107/MB-R-108 |
| **MB-E-064** | `ServerTlsPolicy::Mutual` client certificate signed by any one of several `ca_files` | accepted — "any one matches", not "all"; `ca_files` is a trust-anchor set, not an ordered chain (MB-R-108) |
| **MB-E-065** | `CertVerification::CaFiles` with empty `ca_files` | rejected at construction, never at handshake (MB-R-108/MB-R-109) |
| **MB-E-066** | `ServerTlsPolicy::Mutual` with `CertVerification::Skip`, connection presents no client certificate | handshake still fails — `Skip` skips the CA/identity check on a *presented* cert, does not make presenting optional (MB-R-108) |
| **MB-E-067** | Dialog server-role Skip Verify toggled On while mTLS selected | shared CA list hidden, resolved verification `CertVerification::Skip` regardless of entries; list preserved, restored when Off (MB-R-136) |
| **MB-E-068** | Dialog client-role Self Signed toggled On while mTLS selected | Client Cert/Key inputs hidden, resolved identity `CertSource::SelfSigned` regardless of text; text preserved, restored when Off (MB-R-139) |
| **MB-E-069** | Self-signed pair, once generated for a module instance | cached and reused across every bind/connect/reconnect and `:restart`/`:reload`, including a config edit leaving the source self-signed; regenerated only on a transition *into* self-signed (explicit files removed, toggle Off→On); a fresh instance (not `:restart`/`:reload`) discards the cache — pair never on disk (MB-R-106/MB-R-138) |
| **MB-E-070** | Client-role `CertVerification::CaFiles` with empty `ca_files` | refused at construction and dialog submit (MB-R-109/MB-R-156): the trust store would reject every server certificate, so "trust nothing" is a misconfiguration. `RootStore` needs no such rule — platform roots always trusted, `extra_ca_files` may be empty |
| **MB-E-071** | `CertVerification::RootStore` on a server's client-certificate verification | rejected at construction; client-only, no dialog offers it (MB-R-108). The platform root store is a serverAuth trust list, and many public end-entity certificates also carry clientAuth EKU, so honoring it would accept any client holding a publicly trusted certificate for any domain it controls — an mTLS bypass. Private-CA anchors are the only sanctioned client-certificate source |
| **MB-E-072** | `CertSource::Ephemeral` as a client's mTLS identity | rejected at construction: `Ephemeral` = "nothing configured, fall back and log", a server-side listener behavior. A client with no identity is `ClientTlsPolicy::Tls`, not `Mutual` |

---

## 6. Known limitations — intentional constraints

### 6.1 No max-registers-per-request bound at the protocol layer

**MB-E-073** — Neither client core nor server core enforces the Modbus per-request limits (125 registers / 2000 bits). The **only** enforcement is the application-level read-operation planner, which splits generated poll batches at those limits.

- A poll operation constructed directly (bypassing the planner) may exceed 125 registers and is sent as-is; the only guard is the `u16` count field, which fabricates `IllegalDataValue` above 65535.
- The **server** answers any count the peer sends, limited only by the `u16` count field and declared addresses. It does not reject an over-long request with `IllegalDataValue`.
- A write command is never split: a register wider than the limit would go as one write. Unreachable — widest format is 8 registers.

### 6.2 The RTU `Config` cannot be flattened into a `clap` command

**MB-E-074** — The RTU connection config doubles as a `clap` argument group whose short flags collide: `-s` claimed by `slave` and `stop_bits`, `-d` by `data_bits` and `delay_ms` (both derived from the field initial). Flattening it into a `clap::Parser` command panics at parse time via clap's debug assertions.

The config is reached only through serde (session and device config files, `--module` key/value form), which is unaffected. No Modbus RTU flag is a top-level CLI flag.

### 6.3 The RTU `slave` config field is inert

**MB-E-075** — The RTU config carries `slave` (default 1), read by no code path: the client carries a slave id on every request from the operation or command and never attaches the link to one slave. Kept only so existing config files keep parsing; no observable effect.

An RTU **server** ignores it entirely: it answers whichever slave ids have declared regions.

### 6.4 Server-side reconnect retries only after the current serve loop ends

**MB-E-076** — Every server transport (TCP, RTU, `RtuOverTcp`, `Udp`, `Ascii`, `AsciiOverTcp`) honors `reconnect` — bind failure, serial-open failure, or mid-serve failure retries with the shared backoff driver (MB-R-051, MB-R-071, MB-R-075, MB-R-120, MB-R-124, MB-R-130–134).

Nuance: a mid-serve failure does not retry immediately. The server waits for the *current* serve loop to end on its own before the backoff wait — an in-flight connection is never torn down early to reach a retry sooner.

### 6.5 Unbounded TCP server connections

**MB-E-077** — A TCP server spawns one task per accepted connection, no cap, no idle timeout.

### 6.6 Only six transports

**MB-E-078** — `RtuOverTcp` reuses the TCP config verbatim; only wire framing differs. `Udp` reuses the TCP config minus `tls` (upstream UDP transport has no handshake, no DTLS). Unlike `RtuOverTcp`, `Udp` does not inherit the RTU-family broadcast slave id 0 handling (MB-R-101–MB-R-103) — on `Udp`, slave id 0 is ordinary.

`Ascii` reuses the RTU config verbatim; only framing differs — LRC checksum and `:`/CR LF delimiters instead of CRC and silence-delimited binary. `AsciiOverTcp` reuses the TCP config verbatim, same framing swap as `RtuOverTcp`. Both `Ascii` and `AsciiOverTcp` inherit the RTU-family broadcast handling (MB-R-101–MB-R-103), unlike `Udp`.

### 6.7 Display resolution is one-way

**MB-E-079** — `resolution` scales for *display only*. Encoding does not divide by it, so input is raw, unscaled. Entering `10` with `resolution = 0.5` stores raw word `10` and displays `5`. Consistent — display always scales, input never — but the typed string is not the string read back.

### 6.8 Declaration failures are warned, not silent

**MB-E-080** — Declaring a region reports success/failure via `Memory::add_ranges`'s `bool`; a rejected declaration leaves the register or gap cell without backing memory, so runtime reads/writes against it still fail. Every module-construction, module-reconfiguration, and runtime register-edit call site logs a Warning (MB-R-129) naming the register (or, for an explicit-read-range gap cell, slave id and register kind) and the rejected range at rejection time, so the eventual runtime failure is traceable.

Reachable case: a register added at runtime at an address a `read_ranges` gap already declared read-only. Overlap (existing `Read` cell, requested `ReadWrite` region) is not a widening combination, so the declaration is rejected and dropped — with a Warning naming register and range.

### 6.9 Client writes are fire-and-forget

**MB-E-081** — A client-side write is dispatched to the client task's command channel; the caller is told "sent" once queued. The Modbus response (including an exception) is logged by the client task but not reported back, and the store is not updated from it. The polled value eventually reflects the truth — except write-only registers, whose written value is mirrored into the store locally because it is not otherwise observable.

### 6.10 The register's `access` does not gate store access

**MB-E-082** — A register's `access` (`ReadOnly` / `WriteOnly` / `ReadWrite`) does **not** determine its cells' direction; its *kind* does (coils and holding registers get read/write cells, discrete inputs and input registers read-only). `access` governs whether the register is polled (write-only excluded from reads), whether a client-side write is mirrored into the store, and — MB-R-091/MB-R-151 — whether the client attempts a write at all: a `ReadOnly` register on a **client** rejects the UI's `:set`/dialog write locally (no command, no store touch); on a **server** MB-R-090 bypasses cell access checks entirely.

A `ReadOnly` holding register is therefore still writable by a remote master against a server module, even though the local UI cannot write it from a client module.

### 6.11 Bit-field mask absent from the width error

**MB-E-083** — `BitFieldWidth`'s message names the format by display text (MB-R-155), which carries byte order but not the mask, so the offending mask does not appear. The user supplied that mask; the alternative is dumping the whole format struct into a user-facing message.
