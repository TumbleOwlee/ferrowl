# Modbus — Data Contract

Register model: four tables, thirteen formats, bit-fields, display scaling, virtual registers, store address-range rules.

---

## The four register tables

| Table | Element | Width | Direction on the wire | Read code | Write codes | Req |
|---|---|---|---|---|---|---|
| `Coil` | bit | 1 bit | read/write | 1 | 5, 15 | MB-R-004 |
| `DiscreteInput` | bit | 1 bit | read-only | 2 | — | MB-R-004 |
| `HoldingRegister` | word | 16 bit | read/write | 3 | 6, 16 | MB-R-004 |
| `InputRegister` | word | 16 bit | read-only | 4 | — | MB-R-004 |

`HoldingRegister` is the model-level default kind; the *device config* default is `InputRegister`.

Each (slave id, table) pair is an **independent address space**: address 10 in slave 1's holding registers, address 10 in slave 1's input registers, and address 10 in slave 2's holding registers are three different cells.

Storage is bit-oriented only in intent: a coil is one 16-bit cell of type "coil" holding `1` (set) or `0` (clear). Any non-zero read back from a coil cell reports as set.

---

## Data formats

Thirteen formats, each with a fixed width in 16-bit registers.

| Format | Width (registers) | Bytes | Signed | Endian | Bit-field | Resolution | Display | Req |
|---|---|---|---|---|---|---|---|---|
| `U8` | 1 | 2 | no | yes | yes | yes | `U8 (<Endian>)` | MB-R-010, MB-R-011, MB-R-012 |
| `I8` | 1 | 2 | yes | yes | yes | yes | `I8 (<Endian>)` | MB-R-010, MB-R-011, MB-R-012 |
| `U16` | 1 | 2 | no | yes | yes | yes | `U16 (<Endian>)` | MB-R-010, MB-R-011 |
| `I16` | 1 | 2 | yes | yes | yes | yes | `I16 (<Endian>)` | MB-R-010, MB-R-011 |
| `U32` | 2 | 4 | no | yes | yes | yes | `U32 (<Endian>)` | MB-R-010, MB-R-011 |
| `I32` | 2 | 4 | yes | yes | yes | yes | `I32 (<Endian>)` | MB-R-010, MB-R-011 |
| `U64` | 4 | 8 | no | yes | yes | yes | `U64 (<Endian>)` | MB-R-010, MB-R-011 |
| `I64` | 4 | 8 | yes | yes | yes | yes | `I64 (<Endian>)` | MB-R-010, MB-R-011 |
| `U128` | 8 | 16 | no | yes | yes | yes | `U128 (<Endian>)` | MB-R-010, MB-R-011 |
| `I128` | 8 | 16 | yes | yes | yes | yes | `I128 (<Endian>)` | MB-R-010, MB-R-011 |
| `F32` | 2 | 4 | IEEE 754 | yes | no | yes | `F32 (<Endian>)` | MB-R-010, MB-R-011, MB-R-017, MB-R-018 |
| `F64` | 4 | 8 | IEEE 754 | yes | no | yes | `F64 (<Endian>)` | MB-R-010, MB-R-011, MB-R-017, MB-R-018 |
| `Ascii` | configured `length` | 2 × length | n/a | no | no | no | `ASCII (<Alignment>)` | MB-R-010, MB-R-011, MB-R-019 |

### `U8` / `I8`

An 8-bit format occupies a **whole 16-bit register**. The byte sits in the **low** byte under `Big`, **high** byte under `Little`.

### Byte order

`Endian` (`Big` or `Little`) describes the byte order of the value's whole byte stream across its registers.

- Each 16-bit word is always transmitted high byte first — Modbus wire format, not configurable (MB-R-013).
- `Big` interprets the concatenated byte stream in wire order (most significant first).
- `Little` interprets the *fully reversed* stream — reverses both word order and the two bytes inside each word.

`U32` with wire words `0xAABB 0xCCDD`: `Big` → `0xAABBCCDD`, `Little` → `0xDDCCBBAA`.

### Register order

Independent of byte order, every integer and float format carries a **register order** `Normal` or `Reversed`, reordering the format's 16-bit *words*: `Normal` natural; `Reversed` whole sequence reversed (`U64` `[w0,w1,w2,w3]` → `[w3,w2,w1,w0]`).

Composes with byte order as a separate axis: on decode words are reordered **first**, then `### Byte order`'s byte-order rule; on encode byte-order first, reorder last. Exact inverses. Default `Normal` = byte-order rule alone.

Four layouts for a `U32` with wire words `0xAABB 0xCCDD`:

| Byte order | Register order | Decodes to | Req |
|---|---|---|---|
| `Big` | `Normal` | `0xAABBCCDD` | MB-R-013, MB-R-099 |
| `Little` | `Normal` | `0xDDCCBBAA` | MB-R-013, MB-R-099 |
| `Big` | `Reversed` | `0xCCDDAABB` | MB-R-013, MB-R-099 |
| `Little` | `Reversed` | `0xBBAADDCC` | MB-R-013, MB-R-099 |

Width-1 formats (`U8`/`I8`/`U16`/`I16`): register order is a no-op. `Ascii` has no register order, as it has no byte order.

### Floats

`F32`/`F64` are the raw IEEE 754 bit pattern, subject to the same byte-order rule. No bit-field.

### ASCII

- Two characters per register; block is exactly `2 × length` bytes (MB-R-019).
- `Alignment` `Left` or `Right` governs **padding and truncation on encode**: `Left` writes from the first byte, zero-pads right, over-long input keeps the **first** `2 × length` bytes; `Right` zero-pads left, keeps the **last** `2 × length` bytes (MB-R-020).
- Padding is `0x00`, not space.
- Decoding does **not** trim: the raw byte block including zero padding, as UTF-8. A `Right`-aligned value decodes with leading zero bytes intact.
- No byte order, no bit-field.

### Odd byte counts

A byte stream of odd length packed into registers: the trailing byte becomes the **high** byte of the final register, low byte zero.

---

## Bit-fields

Every integer format carries a bit-field selector: a single mask.

- **Shift derived** from the mask as its trailing-zero count; never configured independently (MB-R-014).
- Decode: `field = (raw & mask) >> shift` (MB-R-015).
- Encode: `raw = (value << shift) & mask`, bits outside the mask **zero**.
- Default mask all-ones (no-op), narrowed to the format's width when applied.
- A mask setting any bit **at or above the format's integer width** is invalid, rejected on decode and encode (e.g. `0x1FF` on `U8`) (MB-R-016). All-ones default always valid.
- Float and ASCII formats have no bit-field; theirs behaves as the no-op default.

### Aliasing registers and the write mask

Several registers may share one address, each owning a disjoint bit slice. To keep a write to one from clobbering siblings, a register exposes:

- **write mask**: one 16-bit word per register of the format's width, carrying the bits it owns, laid out in the same byte order the value is encoded with. Full-width integer, float, or ASCII yields all-ones words (MB-R-009).
- **merge**: `(old & !mask) | (new & mask)`, word by word. Words missing from `old` are zero.

Every write to a fixed-address register — server-side store write or client-side Modbus command — goes through read-modify-write with this merge.

---

## Display scaling (resolution)

Every numeric format carries a `Resolution` scale factor (default `1.0`).

- **Display** applies it: `displayed = raw × resolution`, rendered as a float (MB-R-021).
- **Encode and decode do not.** Wire words always carry the raw value.
- Input is therefore raw: entering `10` with `resolution = 0.5` stores raw `10`, *displays* `5`.
- A value can also render unscaled, and as a zero-padded hex bit pattern (two's complement for signed integers, IEEE 754 bits for floats, two hex digits per byte for ASCII).

---

## Addresses and virtual registers

A register's address is either:

- **Fixed(u16)** — 0–65535. Occupies `[address, address + format width)` in its (slave, table) space (MB-R-003).
- **Virtual** — no wire address.

A definition is virtual when it has no `address`, or when `virtual = true` (wins even with `address` present).

### Virtual registers

- never occupy store memory, never read from or written to the wire (MB-R-080);
- live in a per-module, **name-keyed** virtual store shared with the Lua sim thread;
- seeded at construction with `default`, or, absent one, the format's decoding of all-zero words (table shows `0`, not blank);
- writable only on a **server**; a client write is rejected.

---

## Address ranges in the store

- Ranges **half-open**: `[start, end)`, `length = end - start` (MB-R-028).
- `end < start` rejected on deserialization.
- A region must be **declared before use**. Reads and writes succeed only on fully covered addresses; a partially covered range fails as a whole, no partial result (MB-R-029).
- Declared regions per key are non-overlapping, ordered by start; a read/write spanning adjacent regions walks them in order and succeeds only if they cover the range completely.
- Declaring a range overlapping an existing region of the **same cell type** merges into it. A read region overlapping a write cell (or vice versa) widens it to read/write. An incompatible cell type or access combination fails the whole call, key's memory unchanged even with several ranges in the call.

### Cell model

| Property | Values | Req |
|---|---|---|
| cell type | `Coil` (1-bit semantics) or `Register` (16-bit) | MB-R-030 |
| access | `Read`, `Write`, or `ReadWrite` | MB-R-030 |
| value | one `u16` | MB-R-030 |

Checked access enforces both: a read must address `Read`/`ReadWrite` cells *of the requested type*; a write `Write`/`ReadWrite` cells of that type. Coil request against register cells fails, and vice versa.

Unchecked access ignores direction (not coverage): reads write-only cells, writes read-only cells. Used for the client's poll writeback, seeding `default` values, server-side UI writes.

### Cell direction from register kind

Cells are declared by **kind**, not `access`:

| Kind | Declared cell | Req |
|---|---|---|
| `Coil` | `ReadWrite(Coil)` | MB-R-078 |
| `DiscreteInput` | `Read(Coil)` | MB-R-078 |
| `HoldingRegister` | `ReadWrite(Register)` | MB-R-078 |
| `InputRegister` | `Read(Register)` | MB-R-078 |

`access` instead governs whether the register is *polled* (write-only excluded from reads) and whether a client-side write is mirrored into the store.

---

## Batched read planning

Client poll operations are `(slave id, read function code, [start, end))` triples, grouped by (slave id, function code).

- **Without** `read_ranges` for that code: each register its own request. Contiguous registers **not** merged (MB-R-082).
- **With** `read_ranges`: every register inside one configured range is read by a single request bridging their gaps, trimmed to the first and last register's extent — leading/trailing empty space not read. Registers outside every range get their own requests (MB-R-083).
- Gap addresses inside a configured range backed by no register are declared **read-only** cells, so the batched read can be stored (MB-R-084).
- Per-request limits: **125 registers**, or **2000 bits** for coils/discrete inputs. Longer batches split (MB-R-085).
- A split landing inside a register moves back to that register's start; a register is never read in half (MB-R-086).
