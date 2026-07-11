//! Cross-component end-to-end regression tests for session-level Lua scripts
//! (`SessionSim` + `ModuleRegistry`): real modbus/OCPP module fixtures wired into a real
//! `ModuleRegistry`, driven through the actual `SessionSim` sim thread (no mocks, no networking),
//! asserting effects land in real memory/state across module boundaries. Complements the
//! mock-directory unit tests in `session_sim.rs` and the single-module roundtrip tests in
//! `registry.rs`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;

use ferrowl_codec::format::{BitField, Endian, Resolution};
use ferrowl_codec::{Access, Format, Kind, Register, RegisterBuilder};
use ferrowl_lua::module::{ModuleDirectory, ModuleHost, ValueType};
use ferrowl_modbus::{Key, SlaveKey};
use ferrowl_ocpp::V1_6;
use ferrowl_store::{CellKind as MemKind, Memory, Range};

use crate::app::LOG_SIZE;
use crate::config::script::ScriptDef;
use crate::module::modbus::VirtualStore;
use crate::module::ocpp::client::lua_sim::OcppFields;
use crate::module::ocpp::client::lua_sim::ScopedActionQueue;
use crate::module::ocpp::client::v1_6::state::CsState as Cs16;
use crate::module::ocpp::lock::with_state_mut;
use crate::module::ocpp::scope::Scope;
use crate::module::ocpp::server::lua::{ServerActionQueue, ServerStates, SharedServerStates};
use crate::module::ocpp::server::view::ServerVersion;
use crate::module::view::SharedLog;
use crate::registry::{ModbusHost, ModuleRegistry, OcppClientEntry, OcppServerEntry};
use crate::session_sim::SessionSim;

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

/// Polls `cond` up to `timeout`, sleeping in small steps (mirrors `session_sim.rs`'s helper).
fn wait_for(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let step = Duration::from_millis(10);
    let mut waited = Duration::ZERO;
    while waited < timeout {
        if cond() {
            return true;
        }
        std::thread::sleep(step);
        waited += step;
    }
    cond()
}

fn holding(addr: u16) -> Register {
    RegisterBuilder::default()
        .slave_id(1u8)
        .access(Access::ReadWrite)
        .kind(Kind::HoldingRegister)
        .address(ferrowl_codec::Address::Fixed(addr))
        .format(Format::U16((
            Endian::Big,
            Resolution(1.0),
            BitField::default(),
        )))
        .build()
        .unwrap()
}

fn modbus_memory_key() -> Key<SlaveKey> {
    Key {
        id: SlaveKey {
            slave_id: 1u8,
            kind: Kind::HoldingRegister,
        },
    }
}

/// Memory holding two U16 registers ("setpoint" @ 0, "power" @ 1), read/write.
fn evse_memory() -> Arc<RwLock<Memory<Key<SlaveKey>>>> {
    let mut memory: Memory<Key<SlaveKey>> = Memory::default();
    memory.add_ranges(
        modbus_memory_key(),
        &MemKind::ReadWrite(ferrowl_store::CellType::Register),
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
    ModbusHost {
        memory,
        virtual_store: Arc::new(tokio::sync::RwLock::new(HashMap::new())) as VirtualStore,
        registers: Arc::new(registers),
        role,
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
    let state: Arc<RwLock<Cs16>> = Arc::new(RwLock::new(Cs16::default()));
    let queue: ScopedActionQueue = Arc::new(parking_lot::Mutex::new(Default::default()));
    let entry = OcppClientEntry {
        state: state.clone(),
        queue: queue.clone(),
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
    };
    (states, queue, entry)
}

// --- 1. Two modbus modules, session mirror ----------------------------------

#[test]
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

    assert!(wait_for(Duration::from_millis(500), || {
        read_register(&mem_a, 0) == 7
    }));
}

// --- 2. OCPP client -> modbus mirror -----------------------------------------

#[test]
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

    assert!(wait_for(Duration::from_millis(500), || {
        read_register(&mem, 1) == 42
    }));
}

// --- 3. OCPP action dispatch cross-module -------------------------------------

#[test]
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

    assert!(wait_for(Duration::from_millis(500), || {
        queue.lock().len() >= 2
    }));
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
        C_Log:Print("stations=" .. table.concat(stations, ","))
        C_Log:Print("conns=" .. table.concat(conns, ","))
        m:OCPP():ChargingStation("CP1"):Set("Model", "X")
        "#,
    )]);

    assert!(wait_for(Duration::from_millis(500), || {
        let lines = log_lines(&log);
        lines.iter().any(|l| l == "stations=CP1") && lines.iter().any(|l| l == "conns=1")
    }));
    assert!(wait_for(Duration::from_millis(500), || {
        with_state_mut(&states, |reg| {
            reg.stations
                .get("CP1")
                .and_then(|st| st.cs.clone())
                .map(|cs| matches!(cs.read().get_field("Model"), Some(ValueType::String(ref s)) if s == "X"))
                .unwrap_or(false)
        })
    }));
}

// --- 5. Module removal mid-run -------------------------------------------------

#[test]
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
    assert!(wait_for(Duration::from_millis(300), || {
        read_register(&mem_keep, 1) >= 2
    }));
    let count_before_removal = read_register(&mem_keep, 1);

    // Drop "evse_b" from the registry without stopping the sim.
    let mut modules: HashMap<String, Arc<dyn ModuleHost>> = HashMap::new();
    modules.insert(
        "keep".to_string(),
        Arc::new(modbus_host(mem_keep.clone(), "client")),
    );
    registry.replace_all(modules);

    assert!(wait_for(Duration::from_millis(500), || {
        log_lines(&log)
            .iter()
            .any(|l| l.contains("[sim]") && l.contains("unknown module"))
    }));
    // Loop keeps running: the "keep" counter still advances past the pre-removal value.
    assert!(wait_for(Duration::from_millis(500), || {
        read_register(&mem_keep, 1) > count_before_removal
    }));
}

// --- 6. Rename -----------------------------------------------------------------

#[test]
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

    assert!(wait_for(Duration::from_millis(500), || {
        log_lines(&log)
            .iter()
            .any(|l| l.contains("[sim]") && l.contains("unknown module"))
    }));
    assert!(wait_for(Duration::from_millis(500), || {
        read_register(&mem_b, 0) == 55
    }));
}

// --- 7. Type/Role introspection -------------------------------------------------

#[test]
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
        C_Log:Print(m:Type() .. "/" .. m:Role())
        C_Log:Print(o:Type() .. "/" .. o:Role())
        "#,
    )]);

    assert!(wait_for(Duration::from_millis(500), || {
        let lines = log_lines(&log);
        lines.iter().any(|l| l == "modbus/client") && lines.iter().any(|l| l == "ocpp/client")
    }));
}
