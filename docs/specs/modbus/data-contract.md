# Modbus — Data Contract

The register model: the four register tables, the thirteen data formats,
bit-fields, display scaling, virtual registers, and the address-range rules of the
in-memory store.

---

## 1. The four register tables

| Table | Element | Width | Direction on the wire | Read code | Write codes |
|---|---|---|---|---|---|
| `Coil` | bit | 1 bit | read/write | 1 | 5, 15 |
| `DiscreteInput` | bit | 1 bit | read-only | 2 | — |
| `HoldingRegister` | word | 16 bit | read/write | 3 | 6, 16 |
| `InputRegister` | word | 16 bit | read-only | 4 | — |

`HoldingRegister` is the default table for a register with no explicit kind at the
model level; the *device config* default is `InputRegister`.

Each (slave id, table) pair is an **independent address space**. Address 10 in
slave 1's holding registers and address 10 in slave 1's input registers are two
different cells; so is address 10 in slave 2's holding registers.

Storage is bit-oriented only in intent: a coil is stored as one 16-bit cell of
type "coil", holding `1` (set) or `0` (clear). Anything non-zero read back from a
coil cell reports as set.

---

## 2. Data formats

Thirteen formats. Every one of them determines a fixed width in 16-bit registers.

| Format | Width (registers) | Bytes | Signed | Endian | Bit-field | Resolution |
|---|---|---|---|---|---|---|
| `U8` | 1 | 2 | no | yes | yes | yes |
| `I8` | 1 | 2 | yes | yes | yes | yes |
| `U16` | 1 | 2 | no | yes | yes | yes |
| `I16` | 1 | 2 | yes | yes | yes | yes |
| `U32` | 2 | 4 | no | yes | yes | yes |
| `I32` | 2 | 4 | yes | yes | yes | yes |
| `U64` | 4 | 8 | no | yes | yes | yes |
| `I64` | 4 | 8 | yes | yes | yes | yes |
| `U128` | 8 | 16 | no | yes | yes | yes |
| `I128` | 8 | 16 | yes | yes | yes | yes |
| `F32` | 2 | 4 | IEEE 754 | yes | no | yes |
| `F64` | 4 | 8 | IEEE 754 | yes | no | yes |
| `Ascii` | configured `length` | 2 × length | n/a | no | no | no |

### 2.1 `U8` / `I8`

An 8-bit format still occupies a **whole 16-bit register**, never half of one. The
byte sits in the register's **high** byte under `Big` endian and in its **low**
byte under `Little` endian.

### 2.2 Byte order

`Endian` is `Big` or `Little`, and it describes the byte order of the value's
whole byte stream across its registers.

- Each individual 16-bit register word is always transmitted with its own high
  byte first — that is the Modbus wire format and is not configurable.
- `Big` interprets the concatenated byte stream in wire order (most significant
  byte first).
- `Little` interprets the *fully reversed* byte stream. This reverses both the
  order of the words and the order of the two bytes inside each word.

So for a `U32` whose two words on the wire are `0xAABB 0xCCDD`: `Big` decodes
`0xAABBCCDD`, `Little` decodes `0xDDCCBBAA`.

### 2.3 Floats

`F32` and `F64` are the raw IEEE 754 bit pattern, subject to the same byte-order
rule. They carry no bit-field.

### 2.4 ASCII

- Two characters per register; the block is exactly `2 × length` bytes.
- `Alignment` is `Left` or `Right` and governs **padding and truncation on
  encode**:
  - `Left`: the string is written from the first byte and zero-padded on the
    right. Input longer than the block keeps the **first** `2 × length` bytes.
  - `Right`: the string is zero-padded on the left. Input longer than the block
    keeps the **last** `2 × length` bytes.
- Padding is the zero byte (`0x00`), not the space character.
- Decoding does **not** trim: the decoded string is the raw byte block, including
  any zero padding, interpreted as UTF-8. A `Right`-aligned value therefore
  decodes with its leading zero bytes intact.
- ASCII has no byte order and no bit-field.

### 2.5 Odd byte counts

When a byte stream packs into registers and its length is odd, the trailing byte
becomes the **high** byte of the final register and the low byte is zero.

---

## 3. Bit-fields

Every integer format carries a bit-field selector: a single mask.

- The **shift is derived** from the mask, as the mask's trailing-zero count. It is
  never configured independently.
- Decode: `field = (raw & mask) >> shift`.
- Encode: `raw = (value << shift) & mask`, with every bit outside the mask left
  **zero**.
- The default mask is all-ones (a no-op), narrowed to the format's own width when
  applied.
- A mask that sets any bit **at or above the format's own integer width** is
  invalid and is rejected on decode and on encode (e.g. mask `0x1FF` on a `U8`).
  The all-ones default is always valid for every width.
- Float and ASCII formats have no bit-field; theirs behaves as the no-op default.

### 3.1 Aliasing registers and the write mask

Several registers may share one address, each owning a disjoint slice of its bits.
To keep a write to one from clobbering its siblings, a register exposes:

- a **write mask**: one 16-bit word per register of the format's width, carrying
  the bits that register owns, laid out in the same byte order the value is
  encoded with. A full-width integer, a float, or an ASCII format yields all-ones
  words.
- a **merge**: `(old & !mask) | (new & mask)`, word by word. Words missing from
  `old` are treated as zero.

Every write to a fixed-address register — server-side store write or client-side
Modbus command — goes through read-modify-write with this merge.

---

## 4. Display scaling (resolution)

Every numeric format carries a `Resolution` scale factor (default `1.0`).

- **Display** applies it: `displayed = raw × resolution`, rendered as a float.
- **Encode and decode do not.** The words on the wire always carry the raw,
  unscaled value.
- Value input is therefore also raw: entering `10` on a register with
  `resolution = 0.5` stores the raw word `10`, which then *displays* as `5`.
- A value can also be rendered unscaled, and as a zero-padded hex bit pattern
  (two's complement for signed integers, IEEE 754 bits for floats, two hex digits
  per byte for ASCII).

---

## 5. Addresses and virtual registers

A register's address is either:

- **Fixed(u16)** — a concrete Modbus address, 0–65535. The register occupies
  `[address, address + format width)` in its (slave, table) address space.
- **Virtual** — no wire address at all.

A register definition is virtual when it has no `address`, or when `virtual = true`
(which wins even if an `address` is also present).

### Virtual registers

- never occupy store memory and are never read from or written to the wire;
- live in a per-module, **name-keyed** virtual value store, shared with the Lua
  sim thread so scripts can drive them;
- are seeded at construction with their `default`, or, absent one, with their
  format's decoding of all-zero words (so the table shows `0`, not a blank);
- are writable only on a **server** module; a write to a virtual register on a
  client is rejected.

---

## 6. Address ranges in the store

- Ranges are **half-open**: `[start, end)`. `length = end - start`.
- A range with `end < start` is rejected on deserialization.
- A memory region must be **declared before use**. Reads and writes only succeed
  on addresses fully covered by declared regions; a partially covered range fails
  as a whole, and no partial result is returned.
- Declared regions per key are non-overlapping and ordered by start address; a
  read or write spanning several adjacent regions walks them in address order and
  succeeds only if they cover the range completely.
- Declaring a range that overlaps an existing region of the **same cell type**
  merges into it. A read region overlapping a write cell (or vice versa) widens
  that cell to read/write. An incompatible cell type or access combination fails
  the whole declaring call, leaving the key's memory unchanged even when the call
  carried several ranges.

### Cell model

| Property | Values |
|---|---|
| cell type | `Coil` (1-bit semantics) or `Register` (16-bit) |
| access | `Read`, `Write`, or `ReadWrite` |
| value | one `u16` |

Checked access enforces both: a read must address `Read`/`ReadWrite` cells *of the
requested cell type*; a write must address `Write`/`ReadWrite` cells of that type.
A coil request against register cells fails, and vice versa.

Unchecked access ignores the access direction (but not coverage): it reads
write-only cells and writes read-only cells. It is used for the client's poll
writeback into the store, for seeding `default` values, and for server-side UI
writes.

### Cell direction from register kind

A module declares cells from its register definitions by **kind**, not by the
register's `access`:

| Kind | Declared cell |
|---|---|
| `Coil` | `ReadWrite(Coil)` |
| `DiscreteInput` | `Read(Coil)` |
| `HoldingRegister` | `ReadWrite(Register)` |
| `InputRegister` | `Read(Register)` |

The register's `access` instead governs whether it is *polled* (write-only
registers are excluded from read operations) and whether a client-side write is
mirrored back into the store.

---

## 7. Batched read planning

Client poll operations are `(slave id, read function code, [start, end))` triples,
grouped by (slave id, function code).

- **Without** `read_ranges` for that function code: each register becomes its own
  request. Contiguous registers are **not** merged.
- **With** `read_ranges`: every register falling inside one configured range is
  read by a single request that bridges the gaps between those registers, but is
  trimmed to the extent of the first and last register — empty space at the
  leading and trailing ends of the configured range is not read. Registers outside
  every configured range still get their own requests.
- Gap addresses inside a configured range that are backed by no register are
  declared as **read-only** cells, so the batched read can be stored.
- Per-request limits are enforced: **125 registers**, or **2000 bits** for coils
  and discrete inputs. Longer batches are split.
- A split that would land inside a register is moved back to that register's start
  address, so a register is never read in half.
