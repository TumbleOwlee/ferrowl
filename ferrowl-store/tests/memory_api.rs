//! Integration coverage for `ferrowl-store`'s public [`Memory`] API: declaring regions, the
//! access-checked read/write paths, the error variants they return, and the merge/widen
//! semantics of [`Memory::add_ranges`]. Exercised through public paths as a consumer would.

use ferrowl_store::{CellKind, CellType, Memory, MemoryError, Range};

fn mem() -> Memory<u8> {
    Memory::default()
}

/// MB-R-029 — reads and writes succeed on addresses fully covered by a declared region.
#[test]
fn it_declared_readwrite_region_roundtrips_values() {
    let mut m = mem();
    assert!(m.add_ranges(
        1,
        &CellKind::ReadWrite(CellType::Register),
        &[Range::new(0, 4)]
    ));
    m.write(1, &CellType::Register, &Range::new(0, 4), &[10, 20, 30, 40])
        .expect("writing a declared read/write range succeeds");
    let got = m
        .read(1, &CellType::Register, &Range::new(0, 4))
        .expect("reading a declared read/write range succeeds");
    assert_eq!(got, vec![10, 20, 30, 40]);
}

/// MB-R-033 — a checked read on an unregistered key fails with `UnknownKey`.
#[test]
fn it_read_on_unregistered_key_is_unknown_key() {
    let m = mem();
    let err = m
        .read(9, &CellType::Register, &Range::new(0, 1))
        .expect_err("a key with no declared regions cannot be read");
    assert_eq!(err, MemoryError::UnknownKey);
}

/// MB-R-033 — a checked write whose value count differs from the range length fails with `LengthMismatch`.
#[test]
fn it_write_with_wrong_length_is_length_mismatch() {
    let mut m = mem();
    m.add_ranges(
        1,
        &CellKind::ReadWrite(CellType::Register),
        &[Range::new(0, 4)],
    );
    let err = m
        .write(1, &CellType::Register, &Range::new(0, 4), &[1, 2])
        .expect_err("value count must match the range length");
    assert_eq!(
        err,
        MemoryError::LengthMismatch {
            expected: 4,
            got: 2
        }
    );
}

/// MB-R-033 — a checked write to a read-only region fails with `AddressNotWritable`.
#[test]
fn it_write_to_read_only_region_is_not_writable() {
    let mut m = mem();
    m.add_ranges(1, &CellKind::Read(CellType::Register), &[Range::new(0, 2)]);
    let err = m
        .write(1, &CellType::Register, &Range::new(0, 2), &[1, 2])
        .expect_err("a read-only region rejects writes");
    assert_eq!(err, MemoryError::AddressNotWritable);
}

/// MB-R-033 — a checked read from a write-only region fails with `AddressNotReadable`.
#[test]
fn it_read_from_write_only_region_is_not_readable() {
    let mut m = mem();
    m.add_ranges(1, &CellKind::Write(CellType::Register), &[Range::new(0, 2)]);
    let err = m
        .read(1, &CellType::Register, &Range::new(0, 2))
        .expect_err("a write-only region rejects reads");
    assert_eq!(err, MemoryError::AddressNotReadable);
}

/// MB-R-031 — a write range overlapping a read region of the same type widens it to read/write.
#[test]
fn it_overlapping_read_and_write_ranges_widen_to_readwrite() {
    let mut m = mem();
    assert!(m.add_ranges(1, &CellKind::Read(CellType::Register), &[Range::new(0, 4)]));
    // A write range over the same cells widens their access rather than conflicting.
    assert!(m.add_ranges(1, &CellKind::Write(CellType::Register), &[Range::new(0, 4)]));
    m.write(1, &CellType::Register, &Range::new(0, 4), &[5, 6, 7, 8])
        .expect("widened cells accept writes");
    let got = m
        .read(1, &CellType::Register, &Range::new(0, 4))
        .expect("widened cells accept reads");
    assert_eq!(got, vec![5, 6, 7, 8]);
}

/// MB-R-032 — an incompatible cell-type overlap is rejected and leaves the store's memory unchanged.
#[test]
fn it_incompatible_cell_type_overlap_is_rejected_and_leaves_memory_unchanged() {
    let mut m = mem();
    assert!(m.add_ranges(
        1,
        &CellKind::ReadWrite(CellType::Register),
        &[Range::new(0, 4)]
    ));
    m.write(1, &CellType::Register, &Range::new(0, 4), &[1, 2, 3, 4])
        .expect("initial write succeeds");
    // A coil range over register cells is an incompatible type: all-or-nothing rejection.
    assert!(!m.add_ranges(1, &CellKind::ReadWrite(CellType::Coil), &[Range::new(2, 4)]));
    let got = m
        .read(1, &CellType::Register, &Range::new(0, 4))
        .expect("the rejected declaration left the register region intact");
    assert_eq!(got, vec![1, 2, 3, 4]);
}
