# Modbus — Requirements

Normative behavior of the Modbus capability area: the register model and codec,
the in-memory register store, the client and server roles, the TCP and RTU
transports, reconnect, and the Modbus device configuration.

IDs are stable and append-only (`MB-R-nnn`). See [`../README.md`](../README.md).

Companion documents: [`api-contract.md`](./api-contract.md) (function codes and
config fields), [`data-contract.md`](./data-contract.md) (register tables, data
formats, addressing), [`edge-cases.md`](./edge-cases.md) (boundary and error
behavior, stated limitations).

---

## Register model

**MB-R-001** — A register shall be described by exactly five properties: a slave
id, an access direction, a register table (kind), an address, and a data format.

**MB-R-002** — Slave id shall be an 8-bit value (0–255). It shall default to 0
when unspecified.

**MB-R-003** — A register's address shall be either a fixed 16-bit Modbus address
(0–65535) or *virtual* (no wire address). It shall default to virtual when
unspecified.

**MB-R-004** — A register's kind shall be one of exactly four Modbus register
tables: coil, discrete input, holding register, input register.

**MB-R-005** — A register's access shall be one of `ReadOnly`, `WriteOnly`,
`ReadWrite`, defaulting to `ReadWrite`.

**MB-R-097** — A register definition's kind shall default to holding register
when unspecified.

**MB-R-006** — A register's format shall determine its width in 16-bit registers,
and that width shall be the number of consecutive addresses the register occupies
starting at its address.

**MB-R-007** — Decoding shall convert raw 16-bit register words into a typed value
according to the register's format; encoding shall convert a user-entered string,
or a typed value, into raw register words according to the same format.

**MB-R-008** — Encoding a typed value whose type does not match the register's
format shall fail with a value/format mismatch error rather than silently
coercing.

**MB-R-009** — A register shall expose a per-word write mask selecting exactly the
bits it owns, and a merge operation `(old & !mask) | (new & mask)` per word, so
that writing one bit-field register preserves bits owned by sibling registers
aliasing the same address. Words absent from `old` shall be treated as zero.

---

## Data formats and codec

**MB-R-010** — The codec shall support exactly thirteen data formats: `Ascii`,
`U8`, `U16`, `U32`, `U64`, `U128`, `I8`, `I16`, `I32`, `I64`, `I128`, `F32`,
`F64`.

**MB-R-011** — Format widths in 16-bit registers shall be: `U8`/`I8`/`U16`/`I16` =
1; `U32`/`I32`/`F32` = 2; `U64`/`I64`/`F64` = 4; `U128`/`I128` = 8; `Ascii` = its
configured width. Byte length shall be twice the register width.

**MB-R-012** — `U8` and `I8` shall occupy a whole 16-bit register; the byte shall
sit in the low byte for big-endian and the high byte for little-endian.

**MB-R-013** — Every integer and float format shall carry a byte order of either
`Big` or `Little`. Big-endian shall interpret the register words' byte stream in
wire order; little-endian shall interpret it fully reversed.

**MB-R-014** — Every integer format shall carry a bit-field mask; the field shift
shall be *derived* from the mask as its trailing-zero count and shall never be
configured independently.

**MB-R-015** — Decoding an integer format shall yield `(raw & mask) >> shift`;
encoding shall place the value as `(value << shift) & mask` with all bits outside
the mask left zero.

**MB-R-016** — A bit-field mask that sets any bit at or above the format's own
integer width shall be rejected with a bit-field-width error on both decode and
encode. The full-width default mask shall always be accepted for every integer
format.

**MB-R-017** — Float formats shall carry no bit-field; their bit-field shall
behave as the no-op full mask.

**MB-R-018** — Float formats shall be encoded and decoded as their raw IEEE 754
bit pattern, subject to the same byte-order rule as integers.

**MB-R-019** — `Ascii` shall pack two characters per register, shall carry no byte
order and no bit-field, and shall carry an alignment of `Left` or `Right`.

**MB-R-020** — Encoding `Ascii` shall pad the input with zero bytes to exactly
`2 × width` bytes, on the right for `Left` alignment and on the left for `Right`
alignment. Input longer than the block shall be truncated, keeping the *first*
bytes for `Left` alignment and the *last* bytes for `Right` alignment.

**MB-R-021** — Every numeric format shall carry a display resolution (a scale
factor, default `1.0`). Displaying a decoded value shall yield `raw × resolution`.
Encoding and decoding shall not apply the resolution — the words on the wire are
always the raw, unscaled value.

**MB-R-022** — Numeric string input shall accept a plain decimal literal or a
`0x`-prefixed hexadecimal literal. Signed integer formats shall additionally
accept a `-0x`-prefixed literal, taken as the negation of the hex bit pattern. A
`0x` literal on a float format shall be taken as the IEEE 754 bit pattern.

**MB-R-023** — Decoding shall fail with a too-few-bytes error when fewer words are
supplied than the format's width. Only the first `width` words shall be consumed
when more are supplied.

**MB-R-024** — Decoding an `Ascii` format whose bytes are not valid UTF-8 shall
fail with a packed-ASCII error.

**MB-R-025** — A decoded value shall additionally be renderable as its raw,
unscaled, zero-padded hexadecimal bit pattern (two's complement for signed
integers, IEEE 754 bits for floats, two hex digits per byte for ASCII).

---

## Register store

**MB-R-026** — The register store shall partition memory by device key; the
default key shall be the pair (slave id, register table), so each slave's four
tables are four independent address spaces.

**MB-R-027** — The register table for a request's key shall be derived from its
function code: coil-family codes map to the coil table, discrete-input reads to
the discrete-input table, holding-register-family codes to the holding-register
table, input-register reads to the input-register table. Any other function code
shall map to the holding-register table.

**MB-R-028** — Address ranges shall be half-open `[start, end)`. A range whose end
precedes its start shall be rejected on deserialization.

**MB-R-029** — A memory region shall be declared before it can be read or written;
reads and writes shall only succeed on addresses fully covered by declared
regions.

**MB-R-030** — Each declared cell shall carry both a cell type (single-bit for
coils and discrete inputs, 16-bit for registers) and an access direction (read,
write, or read/write).

**MB-R-031** — Declaring a range that overlaps an existing region of the same cell
type shall merge into it (see MB-R-095); a read region overlapping a write cell
(or vice versa) shall widen that cell to read/write.

**MB-R-032** — Declaring a range that overlaps an existing region with an
incompatible cell type or access combination shall fail, and shall leave the
store's memory for that key entirely unchanged — including when the declaring
call carries multiple ranges, of which earlier ones were compatible.

**MB-R-033** — A checked read shall fail when the key is unregistered, when the
range is not fully covered, or when any addressed cell is not readable as the
requested cell type. A checked write shall fail under the equivalent conditions,
and additionally when the supplied value count does not equal the range length.

**MB-R-034** — The store shall additionally offer unchecked read and write paths
that ignore per-cell access direction (an unchecked read shall return the stored
value of write-only cells; an unchecked write shall overwrite read-only cells).
Both shall still require the range to be fully covered by declared regions.

**MB-R-095** — Declaring a range that intersects one or more existing regions of a
key shall merge **all** of the intersecting regions and the new range into a single
region spanning the union of their address ranges. Every value already stored in
each merged region shall be preserved at its own address; addresses newly brought
into coverage shall be zero-initialized with the declared access direction.

**MB-R-096** — The declared regions of a key shall never overlap: after any
declaration, every address shall be covered by at most one region.

---

## Client

**MB-R-035** — A client shall poll a list of operations, each being a (slave id,
read function code, address range) triple, and shall write every successful read
result into the shared register store under the key derived per MB-R-027.

**MB-R-036** — The client's operation list shall be shared and mutable at runtime;
a change to it shall take effect on subsequent poll cycles without reconnecting or
respawning the client.

**MB-R-037** — The client shall poll operations round-robin, advancing to the next
operation after each successful read.

**MB-R-038** — Before its first poll on each connection, the client shall wait
`delay_ms`.

**MB-R-039** — The client shall poll on a fixed tick of `interval_ms`
milliseconds. An `interval_ms` of 0 shall be treated as a 1 ms tick. A missed tick
shall delay the schedule rather than fire a burst of catch-up ticks.

**MB-R-040** — Every individual Modbus request the client issues (read or write)
shall be bounded by a `timeout_ms` timeout.

**MB-R-041** — The client shall issue only read function codes from its poll loop:
read coils, read discrete inputs, read holding registers, read input registers.

**MB-R-042** — Coil and discrete-input reads shall be stored as one word per bit,
`1` for set and `0` for clear.

**MB-R-043** — A read returning a Modbus exception shall not disconnect the
client. The client shall retry the same operation on subsequent ticks; after 3
consecutive exceptions for that operation it shall log the operation as invalid,
skip it, and advance to the next operation, resetting the retry counter.

**MB-R-044** — The retry counter shall reset to zero on any successful read.

**MB-R-045** — A read that times out, or that fails with a transport error, shall
disconnect the client and end the current connection run.

**MB-R-046** — The client shall accept write commands over a command channel
concurrently with polling: write single coil, write multiple coils, write single
register, write multiple registers, and terminate.

**MB-R-047** — A write command returning a Modbus exception shall be logged and
shall not disconnect the client. A write command that times out, or fails with a
transport error, shall disconnect the client and end the current connection run.

**MB-R-048** — Each read and each write command shall address the slave id carried
by the operation or command, independent of any slave id configured on the
transport.

**MB-R-049** — Receiving the terminate command, or the command channel closing,
shall disconnect the client, end its task with success, and emit a client-
disconnected status.

---

## Reconnect

**MB-R-050** — With `reconnect` enabled (the default), neither a refused/failed
connection attempt nor a transport error during a run shall end the client task;
the client shall wait a backoff and retry the connection.

**MB-R-051** — The reconnect backoff shall start at 1 s, double after each failed
attempt, and be capped at 30 s.

**MB-R-052** — The backoff shall reset to 1 s after any connection run during
which at least one read succeeded.

**MB-R-053** — The terminate command, or the command channel closing, shall abort
a backoff wait immediately and end the client task with success.

**MB-R-054** — Any command other than terminate that arrives while the client is
disconnected and backing off shall be dropped with a log line, not queued for
delivery after reconnect.

**MB-R-055** — With `reconnect` disabled, a failed connection attempt or a
transport error shall end the client task with that error, after emitting a
client-disconnected status.

**MB-R-056** — The connection settings (`reconnect`, `timeout_ms`, `delay_ms`,
`interval_ms`, and the transport endpoint) shall be re-read from the shared
configuration on every connection attempt, so an edit to them takes effect on the
next reconnect.

---

## Server

**MB-R-057** — A server shall answer every inbound request directly from the
shared register store, with no request queue and no simulated device logic of its
own.

**MB-R-058** — A server shall answer read coils, read discrete inputs, read
holding registers, read input registers, write single coil, write single register,
write multiple coils, write multiple registers, and read/write multiple registers.

**MB-R-059** — A server shall reject report-server-id, mask-write-register,
read-device-identification, and any custom function code with the Modbus exception
`IllegalFunction`.

**MB-R-060** — A server read whose range is not fully covered by declared regions,
or whose cells are not readable as the requested cell type, shall be answered with
the Modbus exception `IllegalDataAddress`. The same shall hold for a write whose
range is not writable. `IllegalFunction` shall be reserved for an unsupported
function code (MB-R-059) and shall never be used to report an addressing or access
failure.

**MB-R-061** — Coil reads shall report a stored word as set when it is non-zero;
coil writes shall store a set coil as `1` and a clear coil as `0`.

**MB-R-062** — A multi-register or multi-coil write shall be answered with the
address written and the number of values written.

**MB-R-063** — A read/write-multiple-registers request shall perform its read
check, write check, read, and write under a single exclusive hold on the store, so
no concurrent request can interleave between them; the response shall carry the
values read *before* the write is applied.

**MB-R-064** — A read/write-multiple-registers request whose read range is not
readable, or whose write range is not writable, shall be answered with the Modbus
exception `IllegalDataAddress` and shall apply no write.

**MB-R-065** — A server shall serve any slave id for which memory regions are
declared; it shall not filter requests by a configured slave id.

**MB-R-066** — A server shall log a "request received" line for every inbound
request, including for rejected function codes.

**MB-R-067** — A TCP server shall additionally log the per-request outcome
(success or failure). An RTU server shall not.

---

## Transport — TCP

**MB-R-068** — A TCP client shall connect to `ip:port`, and the connect attempt
shall be bounded by `timeout_ms`.

**MB-R-069** — An `ip`/`port` pair that does not parse as a socket address shall
fail with a TCP address error, for both the client and the server.

**MB-R-070** — A TCP server shall bind `ip:port` and accept connections in a loop,
serving each accepted connection concurrently against the same shared store.

**MB-R-071** — Failure to bind the TCP listen address shall fail the server's
start, and the error shall be surfaced to the caller rather than retried.

---

## Transport — RTU

**MB-R-072** — An RTU client shall open the serial port at `path` with the
configured `baud_rate`, and shall apply `parity`, `data_bits`, and `stop_bits`
when they are set, leaving the serial library's own default in place when they are
not.

**MB-R-073** — `data_bits` shall accept exactly 5, 6, 7, or 8; `stop_bits` shall
accept exactly 1 or 2; `parity` shall accept exactly `even`, `odd`, or `none`,
case-insensitively. Any other value shall fail with a serial configuration error
before the port is opened.

**MB-R-074** — An RTU server shall open the serial port once and serve it as a
single persistent point-to-point connection, with no accept loop.

**MB-R-075** — Failure to open the serial port shall fail the server's start with a
serial error. For a client it shall be treated as a failed connection attempt and
be subject to the reconnect rules (MB-R-050 – MB-R-055).

---

## Module lifecycle and device configuration

**MB-R-076** — Each Modbus module instance shall be either a client or a server
(never both), over either TCP or RTU, and shall own one shared register store, one
register set, and one log.

**MB-R-077** — A module's register store shall be built from its device config's
register definitions: each fixed-address register shall declare the range
`[address, address + format width)` under the key (slave id, kind).

**MB-R-078** — Coil and holding-register definitions shall declare read/write
cells; discrete-input and input-register definitions shall declare read-only
cells. The register's own `access` shall not change the declared cell direction.

**MB-R-079** — A register definition with a `default` value shall have that value
encoded and written into the store at module construction, bypassing cell access
checks.

**MB-R-080** — A virtual register shall never occupy store memory; its value shall
live in a per-module, name-keyed virtual store. A virtual register with no
`default` shall be seeded with its format's decoding of all-zero words.

**MB-R-081** — A client's poll operations shall be derived from the register
definitions: write-only and virtual registers shall be excluded; the remaining
registers shall be grouped by (slave id, read function code).

**MB-R-082** — With no explicit `read_ranges` configured for a function code, each
register in that group shall be read by its own request; registers shall not be
merged across gaps.

**MB-R-083** — With explicit `read_ranges` configured for a function code, all
registers falling inside one configured range shall be read by a single request,
bridging the gaps between them, but trimmed to the first and last register's
actual extent — leading and trailing empty space inside the configured range shall
not be read. Registers outside every configured range shall be read by their own
requests.

**MB-R-084** — Address gaps that are inside a configured `read_range` but backed by
no register shall be declared as read-only cells, so a batched read spanning them
can be stored.

**MB-R-085** — No generated read request shall exceed the Modbus per-request limit
of 125 registers, or 2000 bits for coils and discrete inputs. A batch exceeding
the limit shall be split into multiple requests.

**MB-R-086** — A split point that would fall inside a register shall be moved back
to that register's start address, so no request ever reads a register in half.

**MB-R-087** — Effective timing shall be resolved as: the device config's
`timeout_ms` / `delay_ms` / `interval_ms` / `reconnect` when set, otherwise the
built-in defaults 3000 ms / 1000 ms / 1000 ms / enabled. Timing shall be a
property of the device config, never of the session's per-instance spec.

**MB-R-088** — Adding, editing, or deleting a register at runtime shall rebuild the
shared operation list; the running client shall pick the change up on its next
poll cycle without a reconnect.

**MB-R-089** — Reconfiguring a module's endpoint or role shall stop the running
instance, rebuild it against the same register store and register set, and preserve
the stored register values across the change.

**MB-R-090** — Writing a value to a fixed-address register on a **server** shall
read-modify-write the register's words into the store per MB-R-009, bypassing cell
access checks.

**MB-R-091** — Writing a value to a fixed-address register on a **client** shall
read-modify-write the register's words per MB-R-009 and send them as a Modbus write
command: a single-coil/single-register write when the encoded value is one word, a
multiple-coil/multiple-register write otherwise. It shall not update the store
directly, except for a write-only register, whose value is not otherwise
observable.

**MB-R-092** — Writing a value to a virtual register shall be accepted on a server
(updating the virtual store) and rejected on a client.

**MB-R-093** — Sending a write command to a module whose instance is a server, or
is not running, shall fail with an error rather than being silently dropped.

**MB-R-094** — Stopping a client shall first request graceful termination and only
abort the task if it has not finished within the grace period; a stopped instance
shall be restartable.
