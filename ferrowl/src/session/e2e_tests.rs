//! Cross-component end-to-end tests for session-level Lua scripts
//! (`SessionSim` + `ModuleRegistry`): real modbus/OCPP module fixtures wired into a real
//! `ModuleRegistry`, driven through the actual `SessionSim` sim thread (no mocks, no networking),
//! asserting effects land in real memory/state across module boundaries. Complements the
//! mock-directory unit tests in `session_sim.rs` and the single-module roundtrip tests in
//! `registry.rs`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;

use ferrowl_codec::format::{BitField, Endian, Resolution, WordOrder};
use ferrowl_codec::{Access, Format, Kind, Register, RegisterBuilder};
use ferrowl_lua::module::{ModuleDirectory, ModuleHost, ValueType};
use ferrowl_modbus::{Key, SlaveKey, UnitId};
use ferrowl_ocpp::V1_6;
use ferrowl_store::{CellKind, Memory, Range};
use ferrowl_test_support::wait_until_blocking;

use crate::app::LOG_SIZE;
use crate::config::script::ScriptDef;
use crate::lua::SharedRegisters;
use crate::module::modbus::VirtualStore;
use crate::module::ocpp::client::lua_sim::OcppFields;
use crate::module::ocpp::client::lua_sim::ScopedActionQueue;
use crate::module::ocpp::client::v1_6::state::CsState as Cs16;
use crate::module::ocpp::lock::with_state_mut;
use crate::module::ocpp::scope::Scope;
use crate::module::ocpp::server::lua::{ServerActionQueue, ServerStates, SharedServerStates};
use crate::module::ocpp::server::view::ServerVersion;
use crate::module::view::SharedLog;
use crate::registry::next_instance_id;
use crate::registry::{ModbusHost, ModuleRegistry, OcppClientEntry, OcppServerEntry};
use crate::session::SessionSim;

fn log() -> SharedLog {
    Arc::new(tokio::sync::RwLock::new(crate::app::LogRing::init()))
}

fn log_lines(log: &SharedLog) -> Vec<String> {
    log.blocking_read()
        .peek_n(LOG_SIZE)
        .into_iter()
        .map(|(_, _, l)| l)
        .collect()
}

fn script(name: &str, code: &str) -> ScriptDef {
    ScriptDef {
        name: name.to_string(),
        code: code.to_string(),
        enabled: true,
    }
}

fn holding(addr: u16) -> Register {
    RegisterBuilder::default()
        .slave_id(UnitId(1))
        .access(Access::ReadWrite)
        .kind(Kind::HoldingRegister)
        .address(ferrowl_codec::Address::Fixed(addr))
        .format(Format::u16(
            Endian::Big,
            WordOrder::Normal,
            Resolution(1.0),
            BitField::default(),
        ))
        .build()
        .unwrap()
}

fn modbus_memory_key() -> Key<SlaveKey> {
    Key {
        id: SlaveKey {
            slave_id: UnitId(1),
            kind: Kind::HoldingRegister,
        },
    }
}

/// Memory holding two U16 registers ("setpoint" @ 0, "power" @ 1), read/write.
fn evse_memory() -> Arc<RwLock<Memory<Key<SlaveKey>>>> {
    let mut memory: Memory<Key<SlaveKey>> = Memory::default();
    memory.add_ranges(
        modbus_memory_key(),
        &CellKind::read_write(ferrowl_store::CellType::Register),
        &[Range::new(0, 2)],
    );
    Arc::new(RwLock::new(memory))
}

fn read_register(memory: &Arc<RwLock<Memory<Key<SlaveKey>>>>, addr: u16) -> u16 {
    memory
        .read()
        .read_unchecked(modbus_memory_key(), &Range::new(addr as usize, 1))
        .expect("register readable")[0]
}

fn modbus_host(memory: Arc<RwLock<Memory<Key<SlaveKey>>>>, role: &'static str) -> ModbusHost {
    let mut registers = HashMap::new();
    registers.insert("setpoint".to_string(), holding(0));
    registers.insert("power".to_string(), holding(1));
    registers.insert("counter".to_string(), holding(1));
    modbus_host_with(memory, role, Arc::new(RwLock::new(registers)))
}

fn modbus_host_with(
    memory: Arc<RwLock<Memory<Key<SlaveKey>>>>,
    role: &'static str,
    registers: SharedRegisters,
) -> ModbusHost {
    ModbusHost {
        memory,
        virtual_store: Arc::new(tokio::sync::RwLock::new(HashMap::new())) as VirtualStore,
        registers,
        role,
        instance_id: next_instance_id(),
    }
}

fn registry_from(modules: Vec<(&str, Arc<dyn ModuleHost>)>) -> ModuleRegistry {
    let registry = ModuleRegistry::new();
    let map: HashMap<String, Arc<dyn ModuleHost>> = modules
        .into_iter()
        .map(|(name, host)| (name.to_string(), host))
        .collect();
    registry.replace_all(map);
    registry
}

fn as_directory(registry: ModuleRegistry) -> Arc<dyn ModuleDirectory> {
    Arc::new(registry) as Arc<dyn ModuleDirectory>
}

fn client_entry() -> (Arc<RwLock<Cs16>>, ScopedActionQueue, OcppClientEntry<Cs16>) {
    client_entry_with_id(next_instance_id())
}

fn client_entry_with_id(
    instance_id: u64,
) -> (Arc<RwLock<Cs16>>, ScopedActionQueue, OcppClientEntry<Cs16>) {
    let state: Arc<RwLock<Cs16>> = Arc::new(RwLock::new(Cs16::default()));
    let queue: ScopedActionQueue = Arc::new(parking_lot::Mutex::new(Default::default()));
    let entry = OcppClientEntry {
        state: state.clone(),
        queue: queue.clone(),
        instance_id,
    };
    (state, queue, entry)
}

type ServerCs = <V1_6 as ServerVersion>::Cs;
type ServerConn = <V1_6 as ServerVersion>::Conn;

fn server_entry_with_station(
    identity: &str,
    connector: i64,
) -> (
    SharedServerStates<V1_6>,
    ServerActionQueue,
    OcppServerEntry<V1_6>,
) {
    let states: SharedServerStates<V1_6> = Arc::new(RwLock::new(ServerStates::default()));
    with_state_mut(&states, |reg| {
        let st = reg.stations.entry(identity.to_string()).or_default();
        st.cs = Some(Arc::new(RwLock::new(ServerCs::default())));
        st.conns.push((
            Scope::connector(connector),
            Arc::new(RwLock::new(ServerConn::default())),
        ));
    });
    let queue: ServerActionQueue = Arc::new(parking_lot::Mutex::new(Default::default()));
    let entry = OcppServerEntry {
        states: states.clone(),
        queue: queue.clone(),
        instance_id: next_instance_id(),
    };
    (states, queue, entry)
}

// --- 1. Two modbus modules, session mirror ----------------------------------

#[test]
/// SC-R-020 — the session sim mirrors state between two modbus modules via C_Module.
fn it_two_modbus_modules_session_mirror() {
    let mem_a = evse_memory();
    let mem_b = evse_memory();
    // seed "power" @ evse_b to 7 before the sim starts.
    assert!(
        mem_b
            .write()
            .write_unchecked(modbus_memory_key(), &Range::new(1, 1), &[7])
    );

    let registry = registry_from(vec![
        ("evse_a", Arc::new(modbus_host(mem_a.clone(), "client"))),
        ("evse_b", Arc::new(modbus_host(mem_b, "client"))),
    ]);
    let log = log();
    let mut sim = SessionSim::new(as_directory(registry), log);
    sim.set_interval(Duration::from_millis(20));
    sim.set_scripts(vec![script(
        "mirror",
        r#"
        local a = C_Module:Get("evse_a")
        local b = C_Module:Get("evse_b")
        a:Register():Set("setpoint", b:Register():Get("power"))
        "#,
    )]);

    wait_until_blocking(
        "two modbus modules session mirror",
        Duration::from_millis(10),
        Duration::from_secs(10),
        || (read_register(&mem_a, 0) == 7).then_some(()),
    );
}

// --- 2. OCPP client -> modbus mirror -----------------------------------------

#[test]
/// SC-R-020 — the session sim mirrors OCPP client state into a modbus module via C_Module.
fn it_ocpp_client_to_modbus_mirror() {
    let (state, _queue, entry) = client_entry();
    // seed connector 1 power before the sim starts.
    state.write().connector_mut(1).unwrap().power = 42.0;

    let mem = evse_memory();
    let registry = registry_from(vec![
        ("cs1", Arc::new(entry) as Arc<dyn ModuleHost>),
        ("evse", Arc::new(modbus_host(mem.clone(), "client"))),
    ]);

    let log = log();
    let mut sim = SessionSim::new(as_directory(registry), log);
    sim.set_interval(Duration::from_millis(20));
    sim.set_scripts(vec![script(
        "mirror",
        r#"
        local power = C_Module:Get("cs1"):OCPP():Connector(1):Get("Power")
        C_Module:Get("evse"):Register():Set("power", power)
        "#,
    )]);

    wait_until_blocking(
        "ocpp client to modbus mirror",
        Duration::from_millis(10),
        Duration::from_secs(10),
        || (read_register(&mem, 1) == 42).then_some(()),
    );
}

#[test]
/// SC-R-071 — a session `Register()` accessor sees a register added to the module while the sim runs.
fn it_session_register_accessor_sees_runtime_add() {
    let mem = evse_memory();
    let mut map = HashMap::new();
    map.insert("setpoint".to_string(), holding(0));
    let shared: SharedRegisters = Arc::new(RwLock::new(map));
    let registry = registry_from(vec![(
        "evse",
        Arc::new(modbus_host_with(mem.clone(), "client", shared.clone())) as Arc<dyn ModuleHost>,
    )]);

    let mut sim = SessionSim::new(as_directory(registry), log());
    sim.set_interval(Duration::from_millis(20));
    sim.set_scripts(vec![script(
        "late",
        r#"r = r or C_Module:Get("evse"):Register(); r:Set("setpoint", 1); if r:Has("extra") then r:Set("extra", 9) end"#,
    )]);

    wait_until_blocking(
        "register 0 set to 1",
        Duration::from_millis(10),
        Duration::from_secs(10),
        || (read_register(&mem, 0) == 1).then_some(()),
    );
    shared.write().insert("extra".to_string(), holding(1));
    wait_until_blocking(
        "register 1 set to 9 after the runtime add",
        Duration::from_millis(10),
        Duration::from_secs(10),
        || (read_register(&mem, 1) == 9).then_some(()),
    );
}

// --- 3. OCPP action dispatch cross-module -------------------------------------

#[test]
/// SC-R-020 — the session sim dispatches an OCPP action on another module via C_Module.
fn it_ocpp_action_dispatch_cross_module() {
    let (_state, queue, entry) = client_entry();
    let registry = registry_from(vec![("cs1", Arc::new(entry) as Arc<dyn ModuleHost>)]);

    let log = log();
    let mut sim = SessionSim::new(as_directory(registry), log);
    sim.set_interval(Duration::from_millis(300));
    sim.set_scripts(vec![script(
        "dispatch",
        r#"
        local o = C_Module:Get("cs1"):OCPP()
        o:BootNotification()
        o:Connector(1):StartTransaction()
        "#,
    )]);

    wait_until_blocking(
        "ocpp action dispatch cross module",
        Duration::from_millis(10),
        Duration::from_secs(10),
        || (queue.lock().len() >= 2).then_some(()),
    );
    let items: Vec<_> = queue.lock().drain(..).collect();
    assert!(
        items
            .iter()
            .any(|(scope, action, _)| *scope == Scope::CS && action == "BootNotification")
    );
    assert!(items.iter().any(
        |(scope, action, _)| *scope == Scope::connector(1) && action == "StartTransaction"
    ));
}

// --- 4. OCPP server enumeration -----------------------------------------------

#[test]
/// SC-R-020 — the session sim enumerates an OCPP server's stations via C_Module.
fn it_ocpp_server_enumeration() {
    let (states, _queue, entry) = server_entry_with_station("CP1", 1);
    let registry = registry_from(vec![("csms", Arc::new(entry) as Arc<dyn ModuleHost>)]);

    let log = log();
    let mut sim = SessionSim::new(as_directory(registry), log.clone());
    sim.set_interval(Duration::from_millis(20));
    sim.set_scripts(vec![script(
        "enumerate",
        r#"
        local m = C_Module:Get("csms")
        local stations = m:OCPP():GetChargingStations()
        local conns = m:OCPP():GetConnectors("CP1")
        C_Log:Info("stations=" .. table.concat(stations, ","))
        C_Log:Info("conns=" .. table.concat(conns, ","))
        m:OCPP():ChargingStation("CP1"):Set("Model", "X")
        "#,
    )]);

    wait_until_blocking(
        "log has stations=CP1 and conns=1",
        Duration::from_millis(10),
        Duration::from_secs(10),
        || {
            let lines = log_lines(&log);
            (lines.iter().any(|l| l == "stations=CP1") && lines.iter().any(|l| l == "conns=1"))
                .then_some(())
        },
    );
    wait_until_blocking(
        "CP1 Model set to X",
        Duration::from_millis(10),
        Duration::from_secs(10),
        || {
            with_state_mut(&states, |reg| {
                reg.stations
                    .get("CP1")
                    .and_then(|st| st.cs.clone())
                    .is_some_and(|cs| matches!(cs.read().get_field("Model"), Some(ValueType::String(ref s)) if s == "X"))
            })
            .then_some(())
        },
    );
}

// --- 5. Module removal mid-run -------------------------------------------------

#[test]
/// SC-R-020 — a module removed mid-run makes its C_Module handle error while the sim keeps looping.
fn it_module_removal_mid_run_logs_error_and_keeps_looping() {
    let mem_b = evse_memory();
    let mem_keep = evse_memory();
    let registry = ModuleRegistry::new();
    let mut modules: HashMap<String, Arc<dyn ModuleHost>> = HashMap::new();
    modules.insert("evse_b".to_string(), Arc::new(modbus_host(mem_b, "client")));
    modules.insert(
        "keep".to_string(),
        Arc::new(modbus_host(mem_keep.clone(), "client")),
    );
    registry.replace_all(modules);

    let log = log();
    let mut sim = SessionSim::new(
        Arc::new(registry.clone()) as Arc<dyn ModuleDirectory>,
        log.clone(),
    );
    sim.set_interval(Duration::from_millis(20));
    sim.set_scripts(vec![
        script("mirror", r#"C_Module:Get("evse_b"):Register():Get("power")"#),
        script(
            "counter",
            r#"C_Module:Get("keep"):Register():Set("counter", (C_Module:Get("keep"):Register():Get("counter") or 0) + 1)"#,
        ),
    ]);

    // Let it run cleanly for a bit first.
    wait_until_blocking(
        "keep counter reaches 2",
        Duration::from_millis(10),
        Duration::from_secs(10),
        || (read_register(&mem_keep, 1) >= 2).then_some(()),
    );
    let count_before_removal = read_register(&mem_keep, 1);

    // Drop "evse_b" from the registry without stopping the sim.
    let mut modules: HashMap<String, Arc<dyn ModuleHost>> = HashMap::new();
    modules.insert(
        "keep".to_string(),
        Arc::new(modbus_host(mem_keep.clone(), "client")),
    );
    registry.replace_all(modules);

    wait_until_blocking(
        "unknown module error logged",
        Duration::from_millis(10),
        Duration::from_secs(10),
        || {
            (log_lines(&log)
                .iter()
                .any(|l| l.contains("[sim]") && l.contains("unknown module")))
            .then_some(())
        },
    );
    // Loop keeps running: the "keep" counter still advances past the pre-removal value.
    wait_until_blocking(
        "keep counter advances past its pre-removal value",
        Duration::from_millis(10),
        Duration::from_secs(10),
        || (read_register(&mem_keep, 1) > count_before_removal).then_some(()),
    );
}

// --- 6. Rename -----------------------------------------------------------------

#[test]
/// SC-R-020 — after a rename, the old C_Module name errors and the new name resolves.
fn it_module_rename_old_name_errors_new_name_resolves() {
    let mem_b = evse_memory();
    let registry = ModuleRegistry::new();
    let mut modules: HashMap<String, Arc<dyn ModuleHost>> = HashMap::new();
    modules.insert(
        "evse_b".to_string(),
        Arc::new(modbus_host(mem_b.clone(), "client")),
    );
    registry.replace_all(modules);

    let log = log();
    let mut sim = SessionSim::new(
        Arc::new(registry.clone()) as Arc<dyn ModuleDirectory>,
        log.clone(),
    );
    sim.set_interval(Duration::from_millis(20));
    sim.set_scripts(vec![
        script(
            "target_b",
            r#"C_Module:Get("evse_b"):Register():Get("power")"#,
        ),
        script(
            "branch",
            r#"
            for _, n in ipairs(C_Module:List()) do
                if n == "evse_c" then
                    C_Module:Get("evse_c"):Register():Set("setpoint", 55)
                end
            end
            "#,
        ),
    ]);

    // Rename "evse_b" -> "evse_c" (same underlying memory, new key).
    let mut modules: HashMap<String, Arc<dyn ModuleHost>> = HashMap::new();
    modules.insert(
        "evse_c".to_string(),
        Arc::new(modbus_host(mem_b.clone(), "client")),
    );
    registry.replace_all(modules);

    wait_until_blocking(
        "unknown module error logged for the old name",
        Duration::from_millis(10),
        Duration::from_secs(10),
        || {
            (log_lines(&log)
                .iter()
                .any(|l| l.contains("[sim]") && l.contains("unknown module")))
            .then_some(())
        },
    );
    wait_until_blocking(
        "register 0 set to 55 via the new name",
        Duration::from_millis(10),
        Duration::from_secs(10),
        || (read_register(&mem_b, 0) == 55).then_some(()),
    );
}

// --- 7. Type/Role introspection -------------------------------------------------

#[test]
/// SC-R-020 — C_Module reports each module's type and role.
fn it_type_role_introspection() {
    let mem = evse_memory();
    let (_state, _queue, entry) = client_entry();
    let registry = registry_from(vec![
        ("evse", Arc::new(modbus_host(mem, "client"))),
        ("cs1", Arc::new(entry) as Arc<dyn ModuleHost>),
    ]);

    let log = log();
    let mut sim = SessionSim::new(as_directory(registry), log.clone());
    sim.set_interval(Duration::from_millis(20));
    sim.set_scripts(vec![script(
        "introspect",
        r#"
        local m = C_Module:Get("evse")
        local o = C_Module:Get("cs1")
        C_Log:Info(m:Type() .. "/" .. m:Role())
        C_Log:Info(o:Type() .. "/" .. o:Role())
        "#,
    )]);

    wait_until_blocking(
        "type role introspection",
        Duration::from_millis(10),
        Duration::from_secs(10),
        || {
            let lines = log_lines(&log);
            (lines.iter().any(|l| l == "modbus/client") && lines.iter().any(|l| l == "ocpp/client"))
                .then_some(())
        },
    );
}

fn registry_map(modules: Vec<(&str, Arc<dyn ModuleHost>)>) -> HashMap<String, Arc<dyn ModuleHost>> {
    modules
        .into_iter()
        .map(|(name, host)| (name.to_string(), host))
        .collect()
}

#[test]
/// SC-R-078 — a `Register()` accessor held across cycles reaches a module rebuilt under the same name.
fn it_held_modbus_accessor_reaches_rebuilt_module() {
    let old_mem = evse_memory();
    let registry = registry_from(vec![(
        "evse",
        Arc::new(modbus_host(old_mem.clone(), "client")) as Arc<dyn ModuleHost>,
    )]);
    let mut sim = SessionSim::new(as_directory(registry.clone()), log());
    sim.set_interval(Duration::from_millis(20));
    sim.set_scripts(vec![script(
        "held",
        r#"r = r or C_Module:Get("evse"):Register(); r:Set("setpoint", 5)"#,
    )]);
    wait_until_blocking(
        "old module register 0 set to 5",
        Duration::from_millis(10),
        Duration::from_secs(10),
        || (read_register(&old_mem, 0) == 5).then_some(()),
    );

    let new_mem = evse_memory();
    registry.replace_all(registry_map(vec![(
        "evse",
        Arc::new(modbus_host(new_mem.clone(), "client")),
    )]));
    wait_until_blocking(
        "rebuilt module register 0 set to 5",
        Duration::from_millis(10),
        Duration::from_secs(10),
        || (read_register(&new_mem, 0) == 5).then_some(()),
    );
}

#[test]
/// SC-R-080 — a held `OCPP()` accessor raises `was replaced` once its module is replaced, and the sim loop keeps running.
fn it_held_ocpp_accessor_raises_after_replacement() {
    let (_state, _queue, entry) = client_entry();
    let registry = registry_from(vec![("cs1", Arc::new(entry) as Arc<dyn ModuleHost>)]);
    let log = log();
    let mut sim = SessionSim::new(as_directory(registry.clone()), log.clone());
    sim.set_interval(Duration::from_millis(20));
    sim.set_scripts(vec![script(
        "held",
        r#"o = o or C_Module:Get("cs1"):OCPP(); o:Get("Model")"#,
    )]);
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        !log_lines(&log).iter().any(|l| l.contains("was replaced")),
        "no error before the replacement"
    );

    let (_state2, _queue2, entry2) = client_entry();
    registry.replace_all(registry_map(vec![("cs1", Arc::new(entry2))]));
    wait_until_blocking(
        "held ocpp accessor raises after replacement",
        Duration::from_millis(10),
        Duration::from_secs(10),
        || {
            log_lines(&log)
                .iter()
                .any(|l| l.contains("module 'cs1' was replaced; call OCPP() again"))
                .then_some(())
        },
    );
    let n = log_lines(&log).len();
    wait_until_blocking(
        "sim loop must keep running",
        Duration::from_millis(10),
        Duration::from_secs(10),
        || (log_lines(&log).len() > n).then_some(()),
    );
}

#[test]
/// SC-R-078 — a held `OCPP()` accessor follows an in-place rebuild that keeps state, queue and instance id.
fn it_held_ocpp_accessor_follows_in_place_rebuild() {
    let id = next_instance_id();
    let (state, queue, entry) = client_entry_with_id(id);
    let registry = registry_from(vec![("cs1", Arc::new(entry) as Arc<dyn ModuleHost>)]);
    let log = log();
    let mut sim = SessionSim::new(as_directory(registry.clone()), log.clone());
    sim.set_interval(Duration::from_millis(20));
    sim.set_scripts(vec![script(
        "held",
        r#"o = o or C_Module:Get("cs1"):OCPP(); o:Set("Model", "x")"#,
    )]);
    wait_until_blocking(
        "state model set to x",
        Duration::from_millis(10),
        Duration::from_secs(10),
        || (state.read().model == "x").then_some(()),
    );

    state.write().model = String::new();
    registry.replace_all(registry_map(vec![(
        "cs1",
        Arc::new(OcppClientEntry {
            state: state.clone(),
            queue: queue.clone(),
            instance_id: id,
        }),
    )]));
    wait_until_blocking(
        "state model set to x again after the rebuild",
        Duration::from_millis(10),
        Duration::from_secs(10),
        || (state.read().model == "x").then_some(()),
    );
    assert!(!log_lines(&log).iter().any(|l| l.contains("was replaced")));
}

#[test]
/// SC-R-079 — module-set changes never restart the session sim nor reset its globals.
fn it_session_sim_survives_module_set_changes() {
    let mem = evse_memory();
    let registry = registry_from(vec![(
        "evse",
        Arc::new(modbus_host(mem.clone(), "client")) as Arc<dyn ModuleHost>,
    )]);
    let mut sim = SessionSim::new(as_directory(registry.clone()), log());
    sim.set_interval(Duration::from_millis(20));
    sim.set_scripts(vec![script(
        "count",
        r#"n = (n or 0) + 1; C_Module:Get("evse"):Register():Set("setpoint", n)"#,
    )]);
    wait_until_blocking(
        "session sim survives module set changes",
        Duration::from_millis(10),
        Duration::from_secs(10),
        || (read_register(&mem, 0) >= 10).then_some(()),
    );
    let before = read_register(&mem, 0);

    let (_s, _q, cs) = client_entry();
    registry.replace_all(registry_map(vec![
        ("evse", Arc::new(modbus_host(mem.clone(), "client"))),
        ("cs1", Arc::new(cs)),
    ]));
    std::thread::sleep(Duration::from_millis(60));
    registry.replace_all(registry_map(vec![
        ("evse", Arc::new(modbus_host(mem.clone(), "client"))),
        ("cs1", Arc::new(client_entry().2)),
    ]));
    let mut max = before;
    for _ in 0..30 {
        let v = read_register(&mem, 0);
        assert!(
            v >= before,
            "counter dropped to {v} (before {before}): sim restarted"
        );
        max = max.max(v);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(max > before, "counter stopped advancing");
}
