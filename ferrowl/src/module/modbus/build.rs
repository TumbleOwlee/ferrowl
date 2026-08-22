//! Register/memory construction from device config: default & parsed values, batched read
//! operation planning, and network-instance construction.

use std::sync::Arc;

use ferrowl_codec::{Access, Address, Kind, Register};
use ferrowl_modbus::{FunctionCode, Key, Operation, SlaveKey, Transport as NetConfig, UnitId};
use ferrowl_store::{CellKind as MemKind, CellType, Memory, Range};
use tokio::sync::RwLock;

use crate::config::{Endpoint, Role, device::NamedValue, device::ReadRanges};
use crate::instance::Instance;
use crate::instance::config::{ClientConfig, ServerConfig};

use super::module::ModuleMemory;

/// Initial display value for a register: its format decoded from all-zero words (e.g. "0").
/// Used to seed virtual registers so the table isn't blank before a script or `:set` runs.
pub(crate) fn default_value(register: &Register) -> ferrowl_codec::Value {
    let zeros = vec![0u16; register.format().width()];
    register
        .decode(&zeros)
        .unwrap_or(ferrowl_codec::Value::Ascii(String::new()))
}

/// Parse user input (`:set`, Lua `write`) into a typed [`Value`](ferrowl_codec::Value),
/// attaching the register's display resolution (1.0 when the format has none).
pub(crate) fn str_to_value(s: &str, register: &Register) -> ferrowl_codec::Value {
    let res = register.format().resolution().map(|r| r.0).unwrap_or(1.0);
    crate::config::device::Scalar::from_input(s).to_value(res)
}

fn function_code(register: &Register) -> FunctionCode {
    use ferrowl_codec::Kind;
    match register.kind() {
        Kind::Coil => FunctionCode::ReadCoils,
        Kind::DiscreteInput => FunctionCode::ReadDiscreteInputs,
        Kind::HoldingRegister => FunctionCode::ReadHoldingRegisters,
        Kind::InputRegister => FunctionCode::ReadInputRegisters,
    }
}

/// Modbus per-request limits: 2000 bits for coils/discrete inputs, 125 words for registers.
const MAX_COILS_PER_READ: usize = 2000;
const MAX_REGISTERS_PER_READ: usize = 125;

fn read_limit(fc: FunctionCode) -> usize {
    match fc {
        FunctionCode::ReadCoils | FunctionCode::ReadDiscreteInputs => MAX_COILS_PER_READ,
        _ => MAX_REGISTERS_PER_READ,
    }
}

/// Stable grouping/sort key for the four readable function codes (others are not read).
fn fn_code_key(fc: FunctionCode) -> u8 {
    match fc {
        FunctionCode::ReadCoils => 1,
        FunctionCode::ReadDiscreteInputs => 2,
        FunctionCode::ReadHoldingRegisters => 3,
        FunctionCode::ReadInputRegisters => 4,
        _ => 0,
    }
}

/// Readable register spans grouped by `(slave, function-code key)`, each value carrying the
/// function code and a list of `(start, end)` spans. Used for both operation and memory planning.
type ReadableSpanGroups =
    std::collections::BTreeMap<(UnitId, u8), (FunctionCode, Kind, Vec<(usize, usize)>)>;

fn group_readable_spans(
    registers: &[(String, String, Register, Vec<NamedValue>)],
    include_write_only: bool,
) -> ReadableSpanGroups {
    let mut groups = std::collections::BTreeMap::new();
    for (_, _, register, _) in registers {
        if let Address::Fixed(addr) = register.address() {
            if !include_write_only && *register.access() == Access::WriteOnly {
                continue;
            }
            let fc = function_code(register);
            let start = *addr as usize;
            groups
                .entry((*register.slave_id(), fn_code_key(fc)))
                .or_insert_with(|| (fc, register.kind().clone(), Vec::new()))
                .2
                .push((start, start + register.format().width()));
        }
    }
    groups
}

/// Build batched read operations. For each `(slave, function code)`: if the device config defines
/// explicit ranges for that code they are used verbatim (gaps included), otherwise contiguous
/// registers are auto-merged. Either way batches are split so no request exceeds the Modbus
/// per-request limit (125 words / 2000 bits).
pub(crate) fn build_read_operations(
    registers: &[(String, String, Register, Vec<NamedValue>)],
    read_ranges: &ReadRanges,
) -> Vec<Operation> {
    let mut ops = Vec::new();
    for ((slave, _), (fc, kind, mut spans)) in group_readable_spans(registers, false) {
        let limit = read_limit(fc);
        spans.sort_unstable();

        // Collect sorted register boundaries before spans is consumed, so the emit loop can
        // snap split points back to a register boundary and never cut a register in half.
        let reg_starts: Vec<usize> = spans.iter().map(|&(s, _)| s).collect();
        let reg_ends: Vec<usize> = spans.iter().map(|&(_, e)| e).collect();

        let explicit = read_ranges.ranges_for(kind);
        let batches = if explicit.is_empty() {
            // No explicit ranges: Do not merge registers at all.
            spans
        } else {
            // Each explicit range groups the registers that fall inside it into a single read,
            // bridging the gaps *between* those registers but trimmed to their actual extent
            // (leading/trailing space inside the range is not read). Registers outside every
            // explicit range are auto-merged into their own requests.
            let mut windows: Vec<(usize, usize)> =
                explicit.iter().map(|r| (r.start(), r.end())).collect();
            windows.sort_unstable();
            let windows = merge_spans(&windows);

            let mut bounds: Vec<Option<(usize, usize)>> = vec![None; windows.len()];
            let mut uncovered: Vec<(usize, usize)> = Vec::new();
            for &(s, e) in &spans {
                match windows.iter().position(|&(ws, we)| s < we && e > ws) {
                    Some(i) => {
                        let b = bounds[i].get_or_insert((s, e));
                        b.0 = b.0.min(s);
                        b.1 = b.1.max(e);
                    }
                    None => uncovered.push((s, e)),
                }
            }
            let mut batches: Vec<(usize, usize)> = bounds.into_iter().flatten().collect();
            uncovered.sort_unstable();
            batches.extend(&uncovered);
            batches.sort_unstable();
            batches
        };

        // Emit each batch, splitting so no request exceeds the protocol limit.
        // If the naive cut point falls inside a register, snap back to that register's start
        // so a register is never read in half (e.g. a U128 spanning 120-128 must not split at 125).
        // Cuts that land in gaps between registers are left as-is.
        for (start, end) in batches {
            let mut s = start;
            while s < end {
                let naive_e = (s + limit).min(end);
                let e = if naive_e < end {
                    // Find the last register whose start < naive_e.
                    let idx = reg_starts.partition_point(|&rs| rs < naive_e);
                    if idx > 0 && reg_ends[idx - 1] > naive_e {
                        // naive_e bisects register [reg_starts[idx-1], reg_ends[idx-1]); snap back.
                        reg_starts[idx - 1]
                    } else {
                        naive_e
                    }
                } else {
                    naive_e
                };
                ops.push(Operation {
                    slave_id: slave,
                    fn_code: fc,
                    range: Range::new(s, e - s),
                });
                s = e;
            }
        }
    }
    ops
}

/// MB-R-129 — declares `range` for `key`/`kind` in `memory` via `Memory::add_ranges`. On success
/// returns `Ok(())`; on rejection (an incompatible overlap) returns `Err` holding the Warning line
/// the caller should log, naming `subject` (the register name, or `gap_cell_subject`'s text for an
/// explicit-read-range gap), the rejected range, and the rejection reason. `add_ranges`'s own
/// all-or-nothing rejection semantics are unchanged; this only surfaces the failure to the caller.
pub(crate) fn declare_or_reject_msg(
    memory: &mut Memory<Key<SlaveKey>>,
    key: Key<SlaveKey>,
    kind: &MemKind,
    range: &Range,
    subject: &str,
) -> Result<(), String> {
    if memory.add_ranges(key, kind, std::slice::from_ref(range)) {
        Ok(())
    } else {
        Err(format!(
            "{subject}: declaration of {range} rejected — overlaps existing memory with an incompatible type/access combination"
        ))
    }
}

/// MB-R-129 — subject text for a `read_ranges` gap-cell declaration, which has no single register
/// name: identifies it by slave id and register kind instead, e.g. `"read-range gap cell (slave 1,
/// HoldingRegister)"`.
pub(crate) fn gap_cell_subject(key: &Key<SlaveKey>) -> String {
    format!(
        "read-range gap cell (slave {}, {:?})",
        key.id.slave_id, key.id.kind
    )
}

/// For every function code with explicit read ranges, the gap cells (inside those ranges but not
/// backed by a register) that must be added to memory as `Read` so a batched read can be stored.
pub(crate) fn explicit_read_coverage(
    registers: &[(String, String, Register, Vec<NamedValue>)],
    read_ranges: &ReadRanges,
) -> Vec<(Key<SlaveKey>, MemKind, Range)> {
    let mut out = Vec::new();
    for ((slave, _), (_, kind, mut spans)) in group_readable_spans(registers, true) {
        let explicit = read_ranges.ranges_for(kind.clone());
        if explicit.is_empty() {
            continue;
        }
        spans.sort_unstable();
        let covered = merge_spans(&spans);
        let mem_type = match kind {
            Kind::Coil | Kind::DiscreteInput => CellType::Coil,
            Kind::HoldingRegister | Kind::InputRegister => CellType::Register,
        };
        let key = Key {
            id: SlaveKey {
                slave_id: slave,
                kind: kind.clone(),
            },
        };
        for r in &explicit {
            for gap in subtract_spans(r.start(), r.end(), &covered) {
                out.push((key.clone(), MemKind::Read(mem_type), gap));
            }
        }
    }
    out
}

/// Merge a sorted list of `(start, end)` spans into non-overlapping spans.
fn merge_spans(spans: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for &(s, e) in spans {
        match out.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => out.push((s, e)),
        }
    }
    out
}

/// Sub-intervals of `[start, end)` not covered by the (sorted, merged) `covered` spans.
fn subtract_spans(start: usize, end: usize, covered: &[(usize, usize)]) -> Vec<Range> {
    let mut gaps = Vec::new();
    let mut cur = start;
    for &(cs, ce) in covered {
        if ce <= cur || cs >= end {
            continue;
        }
        if cs > cur {
            gaps.push(Range::new(cur, cs - cur));
        }
        cur = cur.max(ce);
        if cur >= end {
            break;
        }
    }
    if cur < end {
        gaps.push(Range::new(cur, end - cur));
    }
    gaps
}

/// Resolved per-instance timing (ms) plus the client auto-reconnect setting. Built by
/// [`super::ModbusModule::resolve_timing`].
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub timeout_ms: usize,
    pub delay_ms: usize,
    pub interval_ms: usize,
    /// Client-only: automatically reconnect (with backoff) instead of ending the client task on
    /// a lost or refused connection. Ignored by servers.
    pub reconnect: bool,
}

pub(crate) fn endpoint_to_config(
    endpoint: &Endpoint,
    timing: &Timing,
    tls: Option<ferrowl_modbus::tcp::ModbusTlsConfig>,
) -> NetConfig {
    match endpoint {
        Endpoint::Tcp { ip, port } => NetConfig::Tcp(ferrowl_modbus::tcp::Config {
            ip: ip.clone(),
            port: *port,
            timeout_ms: timing.timeout_ms,
            delay_ms: timing.delay_ms,
            interval_ms: timing.interval_ms,
            reconnect: timing.reconnect,
            tls,
        }),
        Endpoint::RtuOverTcp { ip, port } => NetConfig::RtuOverTcp(ferrowl_modbus::tcp::Config {
            ip: ip.clone(),
            port: *port,
            timeout_ms: timing.timeout_ms,
            delay_ms: timing.delay_ms,
            interval_ms: timing.interval_ms,
            reconnect: timing.reconnect,
            tls,
        }),
        Endpoint::Rtu {
            path,
            baud_rate,
            parity,
            data_bits,
            stop_bits,
        } => NetConfig::Rtu(ferrowl_modbus::rtu::Config {
            path: path.clone(),
            baud_rate: *baud_rate,
            slave: 0,
            parity: parity.clone(),
            data_bits: *data_bits,
            stop_bits: *stop_bits,
            timeout_ms: timing.timeout_ms,
            delay_ms: timing.delay_ms,
            interval_ms: timing.interval_ms,
            reconnect: timing.reconnect,
        }),
        Endpoint::Udp { ip, port } => NetConfig::Udp(ferrowl_modbus::udp::Config {
            ip: ip.clone(),
            port: *port,
            timeout_ms: timing.timeout_ms,
            delay_ms: timing.delay_ms,
            interval_ms: timing.interval_ms,
            reconnect: timing.reconnect,
        }),
        Endpoint::AsciiOverTcp { ip, port } => {
            NetConfig::AsciiOverTcp(ferrowl_modbus::tcp::Config {
                ip: ip.clone(),
                port: *port,
                timeout_ms: timing.timeout_ms,
                delay_ms: timing.delay_ms,
                interval_ms: timing.interval_ms,
                reconnect: timing.reconnect,
                tls,
            })
        }
        Endpoint::Ascii {
            path,
            baud_rate,
            parity,
            data_bits,
            stop_bits,
        } => NetConfig::Ascii(ferrowl_modbus::rtu::Config {
            path: path.clone(),
            baud_rate: *baud_rate,
            slave: 0,
            parity: parity.clone(),
            data_bits: *data_bits,
            stop_bits: *stop_bits,
            timeout_ms: timing.timeout_ms,
            delay_ms: timing.delay_ms,
            interval_ms: timing.interval_ms,
            reconnect: timing.reconnect,
        }),
    }
}

pub(crate) fn build_instance(
    role: Role,
    config: NetConfig,
    operations: Arc<RwLock<Vec<Operation>>>,
    memory: ModuleMemory,
    cache: ferrowl_modbus::tcp::SelfSignedCache,
) -> Instance<SlaveKey> {
    match (role, config) {
        (Role::Client, NetConfig::Tcp(cfg)) => Instance::with_tcp_client(
            ClientConfig {
                config: Arc::new(RwLock::new(cfg)),
                operations,
                memory,
            },
            cache,
        ),
        (Role::Server, NetConfig::Tcp(cfg)) => Instance::with_tcp_server(
            ServerConfig {
                config: Arc::new(RwLock::new(cfg)),
                memory,
            },
            cache,
        ),
        (Role::Client, NetConfig::Rtu(cfg)) => Instance::with_rtu_client(ClientConfig {
            config: Arc::new(RwLock::new(cfg)),
            operations,
            memory,
        }),
        (Role::Server, NetConfig::Rtu(cfg)) => Instance::with_rtu_server(ServerConfig {
            config: Arc::new(RwLock::new(cfg)),
            memory,
        }),
        (Role::Client, NetConfig::RtuOverTcp(cfg)) => Instance::with_rtu_over_tcp_client(
            ClientConfig {
                config: Arc::new(RwLock::new(cfg)),
                operations,
                memory,
            },
            cache,
        ),
        (Role::Server, NetConfig::RtuOverTcp(cfg)) => Instance::with_rtu_over_tcp_server(
            ServerConfig {
                config: Arc::new(RwLock::new(cfg)),
                memory,
            },
            cache,
        ),
        (Role::Client, NetConfig::Udp(cfg)) => Instance::with_udp_client(ClientConfig {
            config: Arc::new(RwLock::new(cfg)),
            operations,
            memory,
        }),
        (Role::Server, NetConfig::Udp(cfg)) => Instance::with_udp_server(ServerConfig {
            config: Arc::new(RwLock::new(cfg)),
            memory,
        }),
        (Role::Client, NetConfig::Ascii(cfg)) => Instance::with_ascii_client(ClientConfig {
            config: Arc::new(RwLock::new(cfg)),
            operations,
            memory,
        }),
        (Role::Server, NetConfig::Ascii(cfg)) => Instance::with_ascii_server(ServerConfig {
            config: Arc::new(RwLock::new(cfg)),
            memory,
        }),
        (Role::Client, NetConfig::AsciiOverTcp(cfg)) => Instance::with_ascii_over_tcp_client(
            ClientConfig {
                config: Arc::new(RwLock::new(cfg)),
                operations,
                memory,
            },
            cache,
        ),
        (Role::Server, NetConfig::AsciiOverTcp(cfg)) => Instance::with_ascii_over_tcp_server(
            ServerConfig {
                config: Arc::new(RwLock::new(cfg)),
                memory,
            },
            cache,
        ),
        (Role::Monitor, _) => unreachable!(
            "a monitor is never built via build_instance/NetConfig — its construction lands in \
             s4 of the modbus-bus-monitor plan (ModbusMonitorModule lifecycle wrapper), which \
             uses MonitorNetConfig, not NetConfig, and never calls this function"
        ),
    }
}

#[cfg(test)]
mod tests {
    use ferrowl_codec::format::{BitField, Endian, Resolution, WordOrder};
    use ferrowl_codec::{Access, Address, Format, Kind, RegisterBuilder};
    use ferrowl_modbus::UnitId;
    use ferrowl_modbus::{Key, SlaveKey};
    use ferrowl_store::{CellKind as MemKind, CellType, Memory, Range};

    fn entry(
        slave: u8,
        kind: Kind,
        addr: u16,
        format: Format,
        access: Access,
    ) -> (
        String,
        String,
        ferrowl_codec::Register,
        Vec<crate::config::device::NamedValue>,
    ) {
        let register = RegisterBuilder::default()
            .slave_id(UnitId(slave))
            .access(access)
            .kind(kind)
            .address(Address::Fixed(addr))
            .format(format)
            .build()
            .unwrap();
        (String::new(), String::new(), register, vec![])
    }

    fn u16reg(
        slave: u8,
        kind: Kind,
        addr: u16,
        access: Access,
    ) -> (
        String,
        String,
        ferrowl_codec::Register,
        Vec<crate::config::device::NamedValue>,
    ) {
        entry(
            slave,
            kind,
            addr,
            Format::U16((
                Endian::Big,
                WordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            )),
            access,
        )
    }

    #[test]
    /// MB-R-021 — parsed input carries the register's display resolution (non-numeric input falls through to ASCII).
    fn ut_str_to_value_uses_register_resolution() {
        use super::str_to_value;
        use ferrowl_codec::Value;

        let reg = entry(
            1,
            Kind::HoldingRegister,
            0,
            Format::U16((
                Endian::Big,
                WordOrder::Normal,
                Resolution(0.5),
                BitField::default(),
            )),
            Access::ReadWrite,
        )
        .2;

        // Integer and float inputs carry the register's resolution.
        match str_to_value("10", &reg) {
            Value::I64((v, r)) => {
                assert_eq!(v, 10);
                assert_eq!(r.0, 0.5);
            }
            other => panic!("expected I64, got {other:?}"),
        }
        match str_to_value("1.5", &reg) {
            Value::F64((v, r)) => {
                assert_eq!(v, 1.5);
                assert_eq!(r.0, 0.5);
            }
            other => panic!("expected F64, got {other:?}"),
        }
        // Non-numeric input falls through to ASCII.
        match str_to_value("idle", &reg) {
            Value::Ascii(s) => assert_eq!(s, "idle"),
            other => panic!("expected Ascii, got {other:?}"),
        }
    }

    #[test]
    /// MB-R-081 — poll operations are derived from the registers, excluding write-only ones and grouping
    /// by (slave, read function code); different function codes never merge, and batches split at the
    /// per-request limit (MB-R-085) snapped to a register boundary (MB-R-086).
    fn ut_build_read_operations_batches() {
        use super::build_read_operations;
        use crate::config::device::ReadRanges;
        use ferrowl_modbus::FunctionCode;
        let mut read_ranges = ReadRanges {
            holding: Some("0-2".to_string()),
            ..Default::default()
        };

        // Contiguous holding registers 0,1,2 merge into one request because of read ranges;
        // a 4th at 5 stays separate (gap is never read). A write-only register is excluded entirely.
        let regs = vec![
            u16reg(1, Kind::HoldingRegister, 0, Access::ReadWrite),
            u16reg(1, Kind::HoldingRegister, 1, Access::ReadOnly),
            u16reg(1, Kind::HoldingRegister, 2, Access::ReadWrite),
            u16reg(1, Kind::HoldingRegister, 5, Access::ReadOnly),
            u16reg(1, Kind::HoldingRegister, 3, Access::WriteOnly),
        ];
        let ops = build_read_operations(&regs, &read_ranges);
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].fn_code, FunctionCode::ReadHoldingRegisters);
        assert_eq!((ops[0].range.start(), ops[0].range.end()), (0, 3));
        assert_eq!((ops[1].range.start(), ops[1].range.end()), (5, 6));

        // Different function codes never merge even at the same address.
        let regs = vec![
            u16reg(1, Kind::Coil, 0, Access::ReadOnly),
            u16reg(1, Kind::HoldingRegister, 0, Access::ReadOnly),
        ];
        let ops = build_read_operations(&regs, &read_ranges);
        assert_eq!(ops.len(), 2);

        read_ranges.holding = Some("0-150".to_string());
        // Contiguous span exceeding the 125-register limit splits. 16 contiguous U128 (8 words
        // each) cover [0,128): 15 fit in [0,120), the 16th opens a new request at [120,128).
        let regs: Vec<_> = (0..16)
            .map(|i| {
                entry(
                    1,
                    Kind::HoldingRegister,
                    i * 8,
                    Format::U128((
                        Endian::Big,
                        WordOrder::Normal,
                        Resolution(1.0),
                        BitField::default(),
                    )),
                    Access::ReadOnly,
                )
            })
            .collect();
        let ops = build_read_operations(&regs, &read_ranges);
        assert_eq!(ops.len(), 2);
        assert_eq!((ops[0].range.start(), ops[0].range.end()), (0, 120));
        assert_eq!((ops[1].range.start(), ops[1].range.end()), (120, 128));
    }

    #[test]
    /// MB-R-083 — with explicit `read_ranges`, registers inside one range are read by a single request
    /// trimmed to their extent (bridging gaps), and registers outside every range are read on their own.
    fn ut_explicit_read_ranges() {
        use super::build_read_operations;
        use crate::config::device::ReadRanges;

        // Registers at 20-25 and 30-35 inside range "0-100": one read trimmed to the registers'
        // extent (20-35), bridging the gap between them but not the empty 0-20 / 35-100.
        let regs = vec![
            entry(
                1,
                Kind::HoldingRegister,
                20,
                Format::U128((
                    Endian::Big,
                    WordOrder::Normal,
                    Resolution(1.0),
                    BitField::default(),
                )), // width 8 -> 20..28
                Access::ReadWrite,
            ),
            entry(
                1,
                Kind::HoldingRegister,
                30,
                Format::U16((
                    Endian::Big,
                    WordOrder::Normal,
                    Resolution(1.0),
                    BitField::default(),
                )), // 30..31
                Access::ReadWrite,
            ),
        ];
        let ranges = ReadRanges {
            holding: Some("0-100".to_string()),
            ..Default::default()
        };
        let ops = build_read_operations(&regs, &ranges);
        assert_eq!(ops.len(), 1);
        assert_eq!((ops[0].range.start(), ops[0].range.end()), (20, 31));

        // A bridged bounding span exceeding the limit is split into limit-sized requests.
        let regs = vec![
            u16reg(1, Kind::HoldingRegister, 0, Access::ReadWrite),
            u16reg(1, Kind::HoldingRegister, 200, Access::ReadWrite),
        ];
        let wide = ReadRanges {
            holding: Some("0-300".to_string()),
            ..Default::default()
        };
        let ops = build_read_operations(&regs, &wide);
        assert_eq!(ops.len(), 2);
        assert_eq!((ops[0].range.start(), ops[0].range.end()), (0, 125));
        assert_eq!((ops[1].range.start(), ops[1].range.end()), (125, 201));

        // A register outside every explicit range is still read, in its own request.
        let regs = vec![
            u16reg(1, Kind::HoldingRegister, 0, Access::ReadWrite),
            u16reg(1, Kind::HoldingRegister, 2, Access::ReadWrite),
            u16reg(1, Kind::HoldingRegister, 50, Access::ReadWrite),
        ];
        let small = ReadRanges {
            holding: Some("0-3".to_string()),
            ..Default::default()
        };
        let ops = build_read_operations(&regs, &small);
        let mut got: Vec<_> = ops
            .iter()
            .map(|o| (o.range.start(), o.range.end()))
            .collect();
        got.sort_unstable();
        // Registers 0 and 2 bridge to [0,3); register 50 reads alone.
        assert_eq!(got, vec![(0, 3), (50, 51)]);
    }

    #[test]
    /// MB-R-082 — with no explicit read_ranges for a function code, each register is read by its own request; registers are not merged across gaps.
    fn ut_no_read_ranges_each_register_own_request() {
        use super::build_read_operations;
        use crate::config::device::ReadRanges;

        // Three contiguous holding registers, no explicit ranges → three separate requests.
        let regs = vec![
            u16reg(1, Kind::HoldingRegister, 0, Access::ReadWrite),
            u16reg(1, Kind::HoldingRegister, 1, Access::ReadWrite),
            u16reg(1, Kind::HoldingRegister, 2, Access::ReadWrite),
        ];
        let ops = build_read_operations(&regs, &ReadRanges::default());
        let mut got: Vec<_> = ops
            .iter()
            .map(|o| (o.range.start(), o.range.end()))
            .collect();
        got.sort_unstable();
        assert_eq!(got, vec![(0, 1), (1, 2), (2, 3)]);
    }

    #[test]
    /// MB-R-084 — address gaps inside a configured read_range but backed by no register are declared as read-only cells.
    fn ut_explicit_read_coverage_declares_gap_cells() {
        use super::explicit_read_coverage;
        use crate::config::device::ReadRanges;

        // Registers at 0 and 5 (each one word) inside holding range "0-10": the uncovered gaps
        // between/around them become Read cells so a batched read spanning them can be stored.
        let regs = vec![
            u16reg(1, Kind::HoldingRegister, 0, Access::ReadWrite),
            u16reg(1, Kind::HoldingRegister, 5, Access::ReadWrite),
        ];
        let ranges = ReadRanges {
            holding: Some("0-10".to_string()),
            ..Default::default()
        };
        let cov = explicit_read_coverage(&regs, &ranges);
        // Every declared gap is a read-only cell.
        assert!(
            cov.iter()
                .all(|(_, kind, _)| matches!(kind, MemKind::Read(_)))
        );
        let mut gaps: Vec<_> = cov.iter().map(|(_, _, r)| (r.start(), r.end())).collect();
        gaps.sort_unstable();
        assert_eq!(gaps, vec![(1, 5), (6, 11)]);
    }

    // Replicates the server `:set`/edit write path + the table decode read path.
    #[test]
    /// MB-R-090 — writing a value to a fixed-address register on a server read-modify-writes it into the store, observable on read-back.
    fn ut_server_value_write_roundtrip() {
        let mut memory: Memory<Key<SlaveKey>> = Memory::default();
        let key = Key {
            id: SlaveKey {
                slave_id: UnitId(1),
                kind: Kind::HoldingRegister,
            },
        };
        memory.add_ranges(
            key.clone(),
            &MemKind::ReadWrite(CellType::Register),
            &[Range::new(0, 1)],
        );

        let register = RegisterBuilder::default()
            .slave_id(UnitId(1))
            .access(Access::ReadWrite)
            .kind(Kind::HoldingRegister)
            .address(Address::Fixed(0))
            .format(Format::U16((
                Endian::Big,
                WordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            )))
            .build()
            .unwrap();

        let raw = register.encode("50").unwrap();
        assert!(
            memory
                .write(
                    key.clone(),
                    &CellType::Register,
                    &Range::new(0, raw.len()),
                    &raw
                )
                .is_ok(),
            "write should succeed for a Combined register cell"
        );

        let read = memory
            .read(
                key,
                &CellType::Register,
                &Range::new(0, register.format().width()),
            )
            .expect("read should succeed");
        assert_eq!(read, vec![50]);
        assert_eq!(format!("{}", register.decode(&read).unwrap()), "50");
    }

    /// MB-R-104 — `endpoint_to_config` threads the `tls` value through for a TCP
    /// endpoint (and never reads it for RTU — MB-R-112).
    #[test]
    fn ut_endpoint_to_config_carries_tls_for_tcp_endpoint() {
        use super::{Timing, endpoint_to_config};
        use crate::config::Endpoint;
        use ferrowl_modbus::Transport as NetConfig;
        use ferrowl_modbus::tcp::ModbusTlsConfig;

        let timing = Timing {
            timeout_ms: 1000,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        };
        let tls = Some(ModbusTlsConfig {
            server: ferrowl_util::tls::ServerTlsPolicy::Tls {
                server_cert: ferrowl_util::tls::ServerCertSource::SelfSigned,
            },
            ..Default::default()
        });
        let endpoint = Endpoint::Tcp {
            ip: "127.0.0.1".to_string(),
            port: 502,
        };
        let net_config = endpoint_to_config(&endpoint, &timing, tls.clone());
        match net_config {
            NetConfig::Tcp(cfg) => assert_eq!(cfg.tls, tls),
            _ => panic!("expected a TCP config"),
        }
    }

    #[test]
    /// MB-R-115 — `endpoint_to_config` threads `tls` through for an RtuOverTcp
    /// endpoint exactly as it does for Tcp (MB-R-104), producing
    /// `NetConfig::RtuOverTcp(tcp::Config { .. })`.
    fn ut_endpoint_to_config_carries_tls_for_rtu_over_tcp_endpoint() {
        use super::{Timing, endpoint_to_config};
        use crate::config::Endpoint;
        use ferrowl_modbus::Transport as NetConfig;
        use ferrowl_modbus::tcp::ModbusTlsConfig;

        let timing = Timing {
            timeout_ms: 1000,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        };
        let tls = Some(ModbusTlsConfig {
            server: ferrowl_util::tls::ServerTlsPolicy::Tls {
                server_cert: ferrowl_util::tls::ServerCertSource::SelfSigned,
            },
            ..Default::default()
        });
        let endpoint = Endpoint::RtuOverTcp {
            ip: "127.0.0.1".to_string(),
            port: 502,
        };
        let net_config = endpoint_to_config(&endpoint, &timing, tls.clone());
        match net_config {
            NetConfig::RtuOverTcp(cfg) => assert_eq!(cfg.tls, tls),
            _ => panic!("expected an RtuOverTcp config"),
        }
    }

    /// MB-R-116 — a `Udp` endpoint resolves to a `Transport::Udp` config carrying the same
    /// timing fields every other transport gets from `Timing`, and no `tls`.
    #[test]
    fn ut_endpoint_to_config_udp() {
        use super::{Timing, endpoint_to_config};
        use crate::config::Endpoint;
        use ferrowl_modbus::Transport as NetConfig;

        let timing = Timing {
            timeout_ms: 1000,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        };
        let endpoint = Endpoint::Udp {
            ip: "127.0.0.1".to_string(),
            port: 502,
        };
        let net_config = endpoint_to_config(&endpoint, &timing, None);
        match net_config {
            NetConfig::Udp(cfg) => {
                assert_eq!(cfg.ip, "127.0.0.1");
                assert_eq!(cfg.port, 502);
                assert_eq!(cfg.timeout_ms, 1000);
            }
            _ => panic!("expected a Udp config"),
        }
    }

    #[test]
    /// MB-R-127 — `endpoint_to_config` threads `tls` through for an AsciiOverTcp endpoint
    /// exactly as it does for Tcp/RtuOverTcp, producing `NetConfig::AsciiOverTcp(tcp::Config { .. })`.
    fn ut_endpoint_to_config_carries_tls_for_ascii_over_tcp_endpoint() {
        use super::{Timing, endpoint_to_config};
        use crate::config::Endpoint;
        use ferrowl_modbus::Transport as NetConfig;
        use ferrowl_modbus::tcp::ModbusTlsConfig;

        let timing = Timing {
            timeout_ms: 1000,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        };
        let tls = Some(ModbusTlsConfig {
            server: ferrowl_util::tls::ServerTlsPolicy::Tls {
                server_cert: ferrowl_util::tls::ServerCertSource::SelfSigned,
            },
            ..Default::default()
        });
        let endpoint = Endpoint::AsciiOverTcp {
            ip: "127.0.0.1".to_string(),
            port: 502,
        };
        let net_config = endpoint_to_config(&endpoint, &timing, tls.clone());
        match net_config {
            NetConfig::AsciiOverTcp(cfg) => assert_eq!(cfg.tls, tls),
            _ => panic!("expected an AsciiOverTcp config"),
        }
    }

    /// MB-R-121 — an `Ascii` endpoint resolves to a `Transport::Ascii` config carrying the
    /// same timing fields every other transport gets from `Timing`, and (MB-R-112-equivalent)
    /// no `tls` — `rtu::Config` has no such field.
    #[test]
    fn ut_endpoint_to_config_ascii() {
        use super::{Timing, endpoint_to_config};
        use crate::config::Endpoint;
        use ferrowl_modbus::Transport as NetConfig;

        let timing = Timing {
            timeout_ms: 1000,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        };
        let endpoint = Endpoint::Ascii {
            path: "/dev/ttyUSB0".to_string(),
            baud_rate: 9600,
            parity: None,
            data_bits: None,
            stop_bits: None,
        };
        let net_config = endpoint_to_config(&endpoint, &timing, None);
        match net_config {
            NetConfig::Ascii(cfg) => {
                assert_eq!(cfg.path, "/dev/ttyUSB0");
                assert_eq!(cfg.baud_rate, 9600);
                assert_eq!(cfg.timeout_ms, 1000);
            }
            _ => panic!("expected an Ascii config"),
        }
    }

    #[test]
    /// MB-R-129 — `declare_or_reject_msg` returns `Ok(())` and leaves `Memory::add_ranges`'s
    /// acceptance unaffected when the range is compatible.
    fn ut_declare_or_reject_msg_ok_on_accepted_range() {
        let mut memory = Memory::<Key<SlaveKey>>::default();
        let key = Key {
            id: SlaveKey {
                slave_id: UnitId(1),
                kind: Kind::HoldingRegister,
            },
        };
        let result = super::declare_or_reject_msg(
            &mut memory,
            key,
            &MemKind::ReadWrite(CellType::Register),
            &Range::new(0, 2),
            "register 'r'",
        );
        assert!(result.is_ok());
    }

    #[test]
    /// MB-R-129 — on an incompatible overlap, `declare_or_reject_msg` returns `Err` naming the
    /// subject, the rejected range, and the incompatible-overlap reason; `add_ranges`'s
    /// all-or-nothing rejection (the range stays undeclared) is unchanged.
    fn ut_declare_or_reject_msg_err_on_incompatible_overlap() {
        let mut memory = Memory::<Key<SlaveKey>>::default();
        let key = Key {
            id: SlaveKey {
                slave_id: UnitId(1),
                kind: Kind::HoldingRegister,
            },
        };
        // Pre-populate a Read-only cell at [0,2) — mirrors a `read_ranges` gap declaration.
        assert!(memory.add_ranges(
            key.clone(),
            &MemKind::Read(CellType::Register),
            &[Range::new(0, 2)]
        ));

        let range = Range::new(0, 2);
        let result = super::declare_or_reject_msg(
            &mut memory,
            key,
            &MemKind::ReadWrite(CellType::Register),
            &range,
            "register 'r'",
        );
        let msg = result.expect_err("ReadWrite over an existing Read cell is incompatible");
        assert!(msg.contains("register 'r'"));
        assert!(msg.contains(&range.to_string()));
        assert!(msg.to_lowercase().contains("incompatible"));
    }

    #[test]
    /// MB-R-129 — a read-range gap cell's rejection subject identifies it by slave id and
    /// register kind, since it has no single register name.
    fn ut_gap_cell_subject_names_slave_and_kind() {
        let key = Key {
            id: SlaveKey {
                slave_id: UnitId(3),
                kind: Kind::InputRegister,
            },
        };
        let subject = super::gap_cell_subject(&key);
        assert!(subject.contains("gap cell"));
        assert!(subject.contains('3'));
        assert!(subject.contains("InputRegister"));
    }
}
