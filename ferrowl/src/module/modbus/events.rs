//! Register events: what the store tap and the virtual-register path publish for scripts.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use ferrowl_codec::codec::mask_words;
use ferrowl_codec::{NumericPrimitive, Value};
use ferrowl_lua::module::ValueType;
use ferrowl_modbus::{Key, SlaveKey};
use ferrowl_store::{Memory, StoreWrite, WriteSource, WriteTap};
use mlua::IntoLua;

use crate::config::script::{ModbusTrigger, Selection};
use crate::lua::SharedRegisters;
use crate::lua::value_conv::value_to_type;
use crate::module::modbus::registers::register_mem_binding;
use crate::sim::events::{EventSelector, EventStream};
use crate::sim::script_id::{ScriptId, executing_script};

/// Where a register write came from.
#[derive(Debug, Clone)]
pub enum RegisterEventSource {
    Remote,
    Poll,
    Set,
    Lua { script: Option<ScriptId> },
}

/// One write to a register, as a script sees it.
#[derive(Debug, Clone)]
pub struct RegisterEvent {
    pub seq: u64,
    pub timestamp_ms: u64,
    pub name: Arc<str>,
    pub old: Option<Value>,
    pub new: Option<Value>,
    pub source: RegisterEventSource,
    pub changed: bool,
}

impl IntoLua for RegisterEvent {
    fn into_lua(self, lua: &mlua::Lua) -> mlua::Result<mlua::Value> {
        let table = lua.create_table()?;
        table.set("name", &*self.name)?;
        table.set("old", self.old.map_or(ValueType::Nil, value_to_type))?;
        table.set("new", self.new.map_or(ValueType::Nil, value_to_type))?;
        table.set(
            "source",
            match self.source {
                RegisterEventSource::Remote => "remote",
                RegisterEventSource::Poll => "poll",
                RegisterEventSource::Set => "set",
                RegisterEventSource::Lua { .. } => "lua",
            },
        )?;
        table.set("seq", self.seq)?;
        table.set("timestamp", self.timestamp_ms)?;
        Ok(mlua::Value::Table(table))
    }
}

/// Publishes an event for every register a store write touches.
pub struct RegisterTap {
    registers: SharedRegisters,
    events: EventStream<RegisterEvent>,
}

impl RegisterTap {
    pub fn new(registers: SharedRegisters, events: EventStream<RegisterEvent>) -> Self {
        Self { registers, events }
    }
}

impl WriteTap<Key<SlaveKey>> for RegisterTap {
    fn written(&self, write: &StoreWrite<'_, Key<SlaveKey>>, memory: &Memory<Key<SlaveKey>>) {
        let registers = self.registers.read();
        for (name, register) in registers.iter() {
            let Some((_, key, reg_range)) = register_mem_binding(register) else {
                continue;
            };
            if key != *write.id {
                continue;
            }
            let Some(overlap) = reg_range.intersect(write.range) else {
                continue;
            };
            self.events.publish_with(|seq| {
                let in_register = overlap.start() - reg_range.start();
                let in_write = overlap.start() - write.range.start();
                let span = overlap.length();
                let new_words = memory.read_unchecked(key.clone(), &reg_range);
                let old_words = new_words.as_ref().map(|words| {
                    let mut old = words.clone();
                    old[in_register..in_register + span]
                        .copy_from_slice(&write.old[in_write..in_write + span]);
                    old
                });
                let mask = mask_words(register.format());
                let changed = (0..span).any(|i| {
                    let bits = mask.get(in_register + i).copied().unwrap_or(u16::MAX);
                    (write.old[in_write + i] ^ write.new[in_write + i]) & bits != 0
                });
                RegisterEvent {
                    seq,
                    timestamp_ms: now_ms(),
                    name: name.as_str().into(),
                    old: old_words.and_then(|w| register.decode(&w).ok()),
                    new: new_words.and_then(|w| register.decode(&w).ok()),
                    source: match write.source {
                        WriteSource::Remote => RegisterEventSource::Remote,
                        WriteSource::Poll => RegisterEventSource::Poll,
                        WriteSource::Command => RegisterEventSource::Set,
                        WriteSource::Script => RegisterEventSource::Lua {
                            script: executing_script(),
                        },
                    },
                    changed,
                }
            });
        }
    }
}

/// Publishes a write to a virtual register; `changed` is false only for an equal old value.
pub fn publish_virtual(
    events: &EventStream<RegisterEvent>,
    name: &str,
    old: Option<Value>,
    new: Value,
    source: RegisterEventSource,
) {
    events.publish_with(|seq| RegisterEvent {
        seq,
        timestamp_ms: now_ms(),
        name: name.into(),
        changed: old.as_ref().is_none_or(|old| !values_equal(old, &new)),
        old,
        new: Some(new),
        source,
    });
}

/// Selects the events a script's trigger asks for.
pub fn register_selector(trigger: &ModbusTrigger, me: ScriptId) -> EventSelector<RegisterEvent> {
    let ModbusTrigger::Register {
        registers,
        every_write,
    } = trigger.clone();
    Arc::new(move |event| {
        let named = match &registers {
            Selection::All(_) => true,
            Selection::Names(names) => names.iter().any(|n| **n == *event.name),
        };
        let own_write =
            matches!(&event.source, RegisterEventSource::Lua { script: Some(id) } if *id == me);
        named && (every_write || event.changed) && !own_write
    })
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Numeric(p, rp), Value::Numeric(q, rq)) => rp.0 == rq.0 && primitives_equal(p, q),
        (Value::Ascii(x), Value::Ascii(y)) => x == y,
        _ => false,
    }
}

/// Floats equal when both are NaN; integers compare by their decimal text.
fn primitives_equal(a: &NumericPrimitive, b: &NumericPrimitive) -> bool {
    match (a, b) {
        (NumericPrimitive::F32(x), NumericPrimitive::F32(y)) => {
            x == y || (x.is_nan() && y.is_nan())
        }
        (NumericPrimitive::F64(x), NumericPrimitive::F64(y)) => {
            x == y || (x.is_nan() && y.is_nan())
        }
        _ => a.to_string() == b.to_string(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Attaches an always-selecting queue to `stream`; tests assert by popping.
#[cfg(test)]
pub(crate) fn capture_events(
    stream: &EventStream<RegisterEvent>,
) -> crate::sim::events::ScriptQueue<RegisterEvent> {
    let queue = crate::sim::events::ScriptQueue::new(
        Arc::new(|_| true),
        crate::sim::events::SimWake::default(),
    );
    stream.attach(&queue);
    queue
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::RegisterBridge;
    use crate::sim::events::ScriptQueue;
    use crate::sim::script_id::ScriptOwner;
    use crate::view::status_bar::StatusCell;
    use ferrowl_codec::format::{Alignment, BitField, Endian, Resolution, Width, WordOrder};
    use ferrowl_codec::{Access, Address, Format, Kind, Register, RegisterBuilder};
    use ferrowl_lua::module::{Read, Write};
    use ferrowl_modbus::UnitId;
    use ferrowl_store::{CellKind, CellType, Range, WriteSource};
    use parking_lot::RwLock;
    use std::collections::HashMap;

    fn key() -> Key<SlaveKey> {
        Key {
            id: SlaveKey {
                slave_id: UnitId(1),
                kind: Kind::HoldingRegister,
            },
        }
    }

    fn register(addr: Address, format: Format) -> Register {
        RegisterBuilder::default()
            .slave_id(UnitId(1))
            .access(Access::ReadWrite)
            .kind(Kind::HoldingRegister)
            .address(addr)
            .format(format)
            .build()
            .unwrap()
    }

    fn u16_at(addr: u16, mask: u128) -> Register {
        register(
            Address::Fixed(addr),
            Format::u16(
                Endian::Big,
                WordOrder::Normal,
                Resolution(1.0),
                BitField { mask },
            ),
        )
    }

    struct Fixture {
        memory: Arc<RwLock<Memory<Key<SlaveKey>>>>,
        stream: EventStream<RegisterEvent>,
        queue: ScriptQueue<RegisterEvent>,
        registers: SharedRegisters,
    }

    /// Real memory declaring words `declared` of the holding key, with the tap installed.
    fn fixture(registers: &[(&str, Register)], declared: Range) -> Fixture {
        let shared: SharedRegisters = Arc::new(RwLock::new(
            registers
                .iter()
                .map(|(n, r)| (n.to_string(), r.clone()))
                .collect::<HashMap<_, _>>(),
        ));
        let stream = EventStream::new();
        let mut memory = Memory::default();
        memory.add_ranges(
            key(),
            &CellKind::read_write(CellType::Register),
            &[declared],
        );
        memory.set_write_tap(Arc::new(RegisterTap::new(shared.clone(), stream.clone())));
        let queue = capture_events(&stream);
        Fixture {
            memory: Arc::new(RwLock::new(memory)),
            stream,
            queue,
            registers: shared,
        }
    }

    impl Fixture {
        fn write(&self, source: WriteSource, start: usize, words: &[u16]) {
            let range = Range::new(start, words.len());
            assert!(
                self.memory
                    .write()
                    .write_unchecked_from(source, key(), &range, words)
            );
        }

        fn drain(&self) -> Vec<RegisterEvent> {
            std::iter::from_fn(|| self.queue.pop()).collect()
        }
    }

    fn u16_of(value: &Option<Value>) -> Option<u16> {
        match value {
            Some(Value::Numeric(NumericPrimitive::U16(v), _)) => Some(*v),
            _ => None,
        }
    }

    #[test]
    /// MB-R-303 — an event carries the register name, old value, new value, source, sequence
    /// number and a wall-clock millisecond timestamp.
    fn ut_event_carries_all_fields() {
        let fx = fixture(&[("a", u16_at(0, u128::MAX))], Range::new(0, 4));
        let before = now_ms();
        fx.write(WriteSource::Remote, 0, &[7]);
        let after = now_ms();
        let events = fx.drain();
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(&*e.name, "a");
        assert_eq!(u16_of(&e.old), Some(0));
        assert_eq!(u16_of(&e.new), Some(7));
        assert!(matches!(e.source, RegisterEventSource::Remote));
        assert_eq!(e.seq, 1);
        assert!(e.changed);
        assert!((before..=after).contains(&e.timestamp_ms));
    }

    fn event(source: RegisterEventSource) -> RegisterEvent {
        RegisterEvent {
            seq: 9,
            timestamp_ms: 1234,
            name: "reg".into(),
            old: None,
            new: Some(Value::Numeric(NumericPrimitive::U16(3), Resolution(1.0))),
            source,
            changed: true,
        }
    }

    #[test]
    /// MB-R-303 — the Lua table holds name, old, new, source, seq and timestamp; `source` is one
    /// of `remote`, `poll`, `set`, `lua`.
    fn ut_event_converts_to_lua_table() {
        let lua = mlua::Lua::new();
        let table = match event(RegisterEventSource::Remote).into_lua(&lua).unwrap() {
            mlua::Value::Table(t) => t,
            other => panic!("expected a table, got {other:?}"),
        };
        assert_eq!(table.get::<String>("name").unwrap(), "reg");
        assert!(table.get::<mlua::Value>("old").unwrap().is_nil());
        assert_eq!(table.get::<i64>("new").unwrap(), 3);
        assert_eq!(table.get::<u64>("seq").unwrap(), 9);
        assert_eq!(table.get::<u64>("timestamp").unwrap(), 1234);
        let lua_source = RegisterEventSource::Lua {
            script: Some(ScriptId {
                owner: ScriptOwner::Module,
                name: "s".into(),
            }),
        };
        for (source, text) in [
            (RegisterEventSource::Remote, "remote"),
            (RegisterEventSource::Poll, "poll"),
            (RegisterEventSource::Set, "set"),
            (lua_source, "lua"),
        ] {
            let table = match event(source).into_lua(&lua).unwrap() {
                mlua::Value::Table(t) => t,
                other => panic!("expected a table, got {other:?}"),
            };
            assert_eq!(table.get::<String>("source").unwrap(), text);
        }
    }

    #[test]
    /// MB-R-304 — a sequence number is taken for every write, with or without an attached
    /// queue, and strictly increases.
    fn ut_seq_strictly_increases_with_no_queue_attached() {
        let fx = fixture(&[("a", u16_at(0, u128::MAX))], Range::new(0, 4));
        fx.stream.detach(&fx.queue);
        fx.write(WriteSource::Remote, 0, &[1]);
        fx.write(WriteSource::Remote, 0, &[2]);
        fx.stream.attach(&fx.queue);
        fx.write(WriteSource::Remote, 0, &[3]);
        fx.write(WriteSource::Remote, 0, &[4]);
        let seqs: Vec<u64> = fx.drain().iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![3, 4]);
    }

    #[test]
    /// MB-R-302 — one event per register the write overlaps, none for a register it misses.
    fn ut_one_event_per_overlapped_register() {
        let fx = fixture(
            &[
                ("a", u16_at(0, u128::MAX)),
                ("b", u16_at(1, u128::MAX)),
                ("c", u16_at(2, u128::MAX)),
            ],
            Range::new(0, 4),
        );
        fx.write(WriteSource::Remote, 0, &[5, 6]);
        let mut names: Vec<String> = fx.drain().iter().map(|e| e.name.to_string()).collect();
        names.sort();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    /// MB-R-302, MB-R-303 — a write covering only the second word of a two-word register reports the whole
    /// register's old and new values.
    fn ut_partial_overlap_reports_whole_register_values() {
        let wide = register(
            Address::Fixed(2),
            Format::u32(
                Endian::Big,
                WordOrder::Normal,
                Resolution(1.0),
                BitField::default(),
            ),
        );
        let fx = fixture(&[("wide", wide)], Range::new(0, 8));
        fx.write(WriteSource::Remote, 2, &[0, 5]);
        fx.drain();
        fx.write(WriteSource::Remote, 3, &[7]);
        let events = fx.drain();
        assert_eq!(events.len(), 1);
        let u32_of = |v: &Option<Value>| match v {
            Some(Value::Numeric(NumericPrimitive::U32(n), _)) => Some(*n),
            _ => None,
        };
        assert_eq!(u32_of(&events[0].old), Some(5));
        assert_eq!(u32_of(&events[0].new), Some(7));
        assert!(events[0].changed);
    }

    #[test]
    /// MB-E-110 — a change confined to bits outside the register's bit field is not a change,
    /// and an unchanged word is not one either.
    fn ut_bits_outside_the_field_are_not_a_change() {
        let fx = fixture(&[("flags", u16_at(0, 0x00FF))], Range::new(0, 2));
        fx.write(WriteSource::Remote, 0, &[0x0100]);
        fx.write(WriteSource::Remote, 0, &[0x0101]);
        fx.write(WriteSource::Remote, 0, &[0x0101]);
        let changed: Vec<bool> = fx.drain().iter().map(|e| e.changed).collect();
        assert_eq!(changed, vec![false, true, false]);
    }

    #[test]
    /// MB-E-111 — a register whose words are not all declared reports nil old and new values.
    fn ut_undeclared_words_give_nil_values() {
        let wide = register(
            Address::Fixed(0),
            Format::f32(Endian::Big, WordOrder::Normal, Resolution(1.0)),
        );
        let fx = fixture(&[("wide", wide)], Range::new(0, 1));
        fx.write(WriteSource::Remote, 0, &[1]);
        let events = fx.drain();
        assert_eq!(events.len(), 1);
        assert!(events[0].old.is_none() && events[0].new.is_none());
        assert!(events[0].changed);
    }

    #[test]
    /// MB-E-111 — words that do not decode give a nil value.
    fn ut_invalid_ascii_words_give_nil_values() {
        let text = register(Address::Fixed(0), Format::Ascii(Alignment::Left, Width(1)));
        let fx = fixture(&[("text", text)], Range::new(0, 1));
        fx.write(WriteSource::Remote, 0, &[0xFFFF]);
        let events = fx.drain();
        assert_eq!(events.len(), 1);
        assert!(events[0].new.is_none());
    }

    fn f32_value(v: f32) -> Value {
        Value::Numeric(NumericPrimitive::F32(v), Resolution(1.0))
    }

    #[test]
    /// MB-E-115 — a virtual write of NaN over NaN is not a change; a different value is.
    fn ut_virtual_nan_equals_itself() {
        let stream = EventStream::new();
        let queue = capture_events(&stream);
        publish_virtual(
            &stream,
            "v",
            Some(f32_value(f32::NAN)),
            f32_value(f32::NAN),
            RegisterEventSource::Set,
        );
        publish_virtual(
            &stream,
            "v",
            Some(f32_value(f32::NAN)),
            f32_value(1.0),
            RegisterEventSource::Set,
        );
        let changed: Vec<bool> = std::iter::from_fn(|| queue.pop())
            .map(|e| e.changed)
            .collect();
        assert_eq!(changed, vec![false, true]);
    }

    #[test]
    /// MB-E-116 — a virtual register's first write has a nil old value and counts as a change.
    fn ut_virtual_first_write_has_nil_old() {
        let stream = EventStream::new();
        let queue = capture_events(&stream);
        publish_virtual(&stream, "v", None, f32_value(2.0), RegisterEventSource::Set);
        let e = queue.pop().unwrap();
        assert!(e.old.is_none());
        assert!(e.changed);
        assert!(e.new.is_some());
    }

    #[test]
    /// MB-E-109 — a Lua write to a polled register is one event, and the poll restoring the
    /// device's value is another.
    fn ut_client_lua_write_then_poll_gives_two_events() {
        let fx = fixture(&[("a", u16_at(0, u128::MAX))], Range::new(0, 2));
        let bridge = RegisterBridge::new(
            fx.memory.clone(),
            Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            fx.registers.clone(),
            StatusCell::default(),
            fx.stream.clone(),
        );
        bridge.write("a".to_string(), ValueType::Int(9)).unwrap();
        fx.write(WriteSource::Poll, 0, &[0]);
        let events = fx.drain();
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0].source, RegisterEventSource::Lua { .. }));
        assert_eq!(u16_of(&events[0].new), Some(9));
        assert!(matches!(events[1].source, RegisterEventSource::Poll));
        assert_eq!(u16_of(&events[1].new), Some(0));
        assert!(events.iter().all(|e| e.changed));
        assert!(matches!(
            bridge.read("a".to_string()).unwrap(),
            ValueType::Int(0)
        ));
    }

    fn sel_event(name: &str, changed: bool, source: RegisterEventSource) -> RegisterEvent {
        RegisterEvent {
            seq: 1,
            timestamp_ms: 0,
            name: name.into(),
            old: None,
            new: None,
            source,
            changed,
        }
    }

    fn trigger(names: &[&str], every_write: bool) -> ModbusTrigger {
        ModbusTrigger::Register {
            registers: Selection::Names(names.iter().map(|n| n.to_string()).collect()),
            every_write,
        }
    }

    fn me() -> ScriptId {
        ScriptId {
            owner: ScriptOwner::Module,
            name: "me".into(),
        }
    }

    #[test]
    /// SC-R-091 — a selection holds a register by name; `all` holds every register.
    fn ut_selector_matches_names_or_all() {
        let named = register_selector(&trigger(&["temp"], false), me());
        assert!(named(&sel_event("temp", true, RegisterEventSource::Remote)));
        assert!(!named(&sel_event(
            "other",
            true,
            RegisterEventSource::Remote
        )));
        let all = register_selector(
            &ModbusTrigger::Register {
                registers: Selection::All(crate::config::script::AllMarker::All),
                every_write: false,
            },
            me(),
        );
        assert!(all(&sel_event(
            "anything",
            true,
            RegisterEventSource::Remote
        )));
    }

    #[test]
    /// SC-R-092 — an unchanged write selects only for an every-write trigger.
    fn ut_selector_requires_change_unless_every_write() {
        let changes = register_selector(&trigger(&["temp"], false), me());
        let every = register_selector(&trigger(&["temp"], true), me());
        let same = sel_event("temp", false, RegisterEventSource::Poll);
        assert!(!changes(&same));
        assert!(every(&same));
    }

    #[test]
    /// SC-R-093 — a script's own write is the only one its trigger leaves out.
    fn ut_selector_excludes_only_the_writing_script() {
        let selector = register_selector(&trigger(&["temp"], false), me());
        let from =
            |id: Option<ScriptId>| sel_event("temp", true, RegisterEventSource::Lua { script: id });
        assert!(!selector(&from(Some(me()))));
        assert!(selector(&from(Some(ScriptId {
            owner: ScriptOwner::Module,
            name: "other".into(),
        }))));
        assert!(selector(&from(None)));
        assert!(selector(&sel_event("temp", true, RegisterEventSource::Set)));
    }

    #[test]
    /// SC-R-094 — a same-named script of another owner still fires.
    fn ut_selector_same_name_other_owner_still_fires() {
        let selector = register_selector(&trigger(&["temp"], false), me());
        let session = ScriptId {
            owner: ScriptOwner::Session,
            name: "me".into(),
        };
        assert!(selector(&sel_event(
            "temp",
            true,
            RegisterEventSource::Lua {
                script: Some(session)
            }
        )));
    }
}
