//! Lua module `C_OCPP`: gives scripts access to OCPP charging-station state.
//!
//! Three module shapes share one underlying `Get`/`Set`/`<Action>` surface (registered by
//! [`register_state_actions`]):
//!
//! - [`Ocpp`] — the original flat module: bare `Get`/`Set`/`<Action>` over a single state. Still
//!   used where only one scope is addressed.
//! - [`OcppClient`] — a charging station with many connectors: bare `Get`/`Set`/`<Action>` address
//!   the CS level, `Connector(id)` returns an [`Accessor`] scoped to one connector.
//! - [`OcppServer`] — a CSMS spanning many stations: `GetChargingStations()`/`GetConnectors(cs)`
//!   enumerate, `ChargingStation(cs)`/`Connector(cs, id)` return scope [`Accessor`]s.
//!
//! The exposed action set is version-specific (OCPP 1.6 vs 2.0.1) and comes from the host handle's
//! [`OcppActions::actions`], so different host handles produce logically distinct modules.

pub mod traits;

use crate::module::ValueType;
use ferrowl_lua_derive::Module;
use mlua::{Result, Table, UserData, UserDataMethods};
use traits::{OcppClientHost, OcppHandle, OcppServerHost};

/// Register the shared `Get(name)` / `Set(name, value)` / `<Action>(overrides?)` methods onto any
/// userdata `U` whose host handle `H` is reachable via the `handle` projection. Used by every
/// `C_OCPP` shape (top-level module and per-scope [`Accessor`]) so the surface stays identical.
fn register_state_actions<U, H, M>(methods: &mut M, handle: fn(&U) -> &H)
where
    U: 'static,
    H: OcppHandle,
    M: UserDataMethods<U>,
{
    // `Get(name)` / `Set(name, value)` mirror the register module.
    methods.add_method("Get", move |_, this, name: String| handle(this).read(name));
    methods.add_method("Set", move |_, this, (name, value): (String, ValueType)| {
        handle(this).write(name, value)
    });

    // One method per version-specific action: `:<Action>(overrides?)`.
    for action in H::actions() {
        methods.add_method(action, move |_, this, args: Option<Table>| {
            match table_to_overrides(args) {
                Ok(overrides) => Ok(handle(this).dispatch(action, overrides)),
                // Bad argument shape (e.g. a non-scalar override value) -> false, not a raise.
                Err(_) => Ok(false),
            }
        });
    }
}

/// A per-scope accessor returned by `Connector(..)`/`ChargingStation(..)`: it carries one host
/// handle and exposes that scope's `Get`/`Set`/`<Action>` surface.
pub struct Accessor<H: OcppHandle> {
    handle: H,
}

impl<H: OcppHandle> Accessor<H> {
    /// Wrap a resolved scope handle.
    pub fn new(handle: H) -> Self {
        Self { handle }
    }
}

impl<H: OcppHandle> UserData for Accessor<H> {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        register_state_actions(methods, |a: &Accessor<H>| &a.handle);
    }
}

/// Lua module `C_OCPP`, parameterised over a host handle providing state read/write and action
/// dispatch. Flat single-scope shape.
#[derive(Module)]
#[module = "C_OCPP"]
pub struct Ocpp<H: OcppHandle> {
    handle: H,
}

impl<H: OcppHandle> Ocpp<H> {
    /// Creates the module around the host handle.
    pub fn init(handle: H) -> Self {
        Self { handle }
    }
}

impl<H: OcppHandle> UserData for Ocpp<H> {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        register_state_actions(methods, |o: &Ocpp<H>| &o.handle);
    }
}

/// Lua module `C_OCPP` for the **client**: CS-level `Get`/`Set`/`<Action>` plus `Connector(id)`.
#[derive(Module)]
#[module = "C_OCPP"]
pub struct OcppClient<H: OcppHandle + OcppClientHost> {
    handle: H,
}

impl<H: OcppHandle + OcppClientHost> OcppClient<H> {
    /// Creates the module around the client host handle.
    pub fn init(handle: H) -> Self {
        Self { handle }
    }
}

impl<H: OcppHandle + OcppClientHost> UserData for OcppClient<H> {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        register_state_actions(methods, |o: &OcppClient<H>| &o.handle);
        methods.add_method("Connector", |_, this, id: i64| {
            Ok(Accessor::new(this.handle.connector(id)))
        });
    }
}

/// Lua module `C_OCPP` for the **server**: enumeration plus per-station / per-connector accessors.
#[derive(Module)]
#[module = "C_OCPP"]
pub struct OcppServer<H: OcppServerHost + 'static> {
    handle: H,
}

impl<H: OcppServerHost + 'static> OcppServer<H> {
    /// Creates the module around the server host handle.
    pub fn init(handle: H) -> Self {
        Self { handle }
    }
}

impl<H: OcppServerHost + 'static> UserData for OcppServer<H> {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("GetChargingStations", |_, this, ()| {
            Ok(this.handle.stations())
        });
        methods.add_method("GetConnectors", |_, this, cs: String| {
            Ok(this.handle.connectors(&cs))
        });
        methods.add_method("ChargingStation", |_, this, cs: String| {
            Ok(this.handle.station(&cs).map(Accessor::new))
        });
        methods.add_method("Connector", |_, this, (cs, id): (String, i64)| {
            Ok(this.handle.connector(&cs, id).map(Accessor::new))
        });
    }
}

/// Flatten an optional Lua override table into `(key, value)` scalar pairs. A missing table is no
/// overrides; a non-string key or non-scalar value surfaces as an error (handled as `false`).
fn table_to_overrides(table: Option<Table>) -> Result<Vec<(String, ValueType)>> {
    let mut out = Vec::new();
    if let Some(table) = table {
        for pair in table.pairs::<String, ValueType>() {
            out.push(pair?);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ContextBuilder;
    use crate::module::{OcppActions, Read, Write};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    /// A mock host: an in-memory key/value store plus a record of dispatched actions.
    #[derive(Clone, Default)]
    struct MockHandle {
        store: Rc<RefCell<HashMap<String, ValueType>>>,
        dispatched: Rc<RefCell<Vec<(String, Vec<(String, ValueType)>)>>>,
    }

    impl Read for MockHandle {
        fn read(&self, name: String) -> mlua::Result<ValueType> {
            self.store
                .borrow()
                .get(&name)
                .cloned()
                .ok_or_else(|| mlua::Error::RuntimeError(format!("unknown '{name}'")))
        }
    }
    impl Write for MockHandle {
        fn write(&self, name: String, value: ValueType) -> mlua::Result<()> {
            self.store.borrow_mut().insert(name, value);
            Ok(())
        }
    }
    impl OcppActions for MockHandle {
        fn actions() -> Vec<&'static str> {
            vec!["StartTransaction"]
        }
        fn dispatch(&self, action: &str, args: Vec<(String, ValueType)>) -> bool {
            self.dispatched
                .borrow_mut()
                .push((action.to_string(), args));
            true
        }
    }

    #[test]
    fn ut_get_set_roundtrip_and_dispatch() {
        let handle = MockHandle::default();
        let mut ctx = ContextBuilder::<String>::default()
            .with_stdlib()
            .with_module(Ocpp::init(handle.clone()))
            .with_script(
                "s".to_string(),
                r#"
                C_OCPP:Set("Power", 42)
                C_OCPP:Set("Power", C_OCPP:Get("Power") + 1)
                ok = C_OCPP:StartTransaction({ idTag = "ABC" })
                bad = C_OCPP.Get == nil
                "#,
            )
            .build()
            .expect("build context");

        ctx.call_all(std::time::Duration::ZERO).expect("run");

        // Set/Get round-tripped through the host store.
        match handle.store.borrow().get("Power") {
            Some(ValueType::Int(v)) => assert_eq!(*v, 43),
            other => panic!("expected Int(43), got {other:?}"),
        }
        // The action was enqueued with its override arg.
        let dispatched = handle.dispatched.borrow();
        assert_eq!(dispatched.len(), 1);
        assert_eq!(dispatched[0].0, "StartTransaction");
        assert_eq!(dispatched[0].1.len(), 1);
        assert_eq!(dispatched[0].1[0].0, "idTag");
    }

    type Store = Rc<RefCell<HashMap<String, ValueType>>>;
    /// Records dispatched actions as `(scope_label, action)`.
    type DispatchLog = Rc<RefCell<Vec<(String, String)>>>;
    /// One station: its CS-level store plus per-connector stores.
    type StationData = (Store, HashMap<i64, Store>);

    fn store_get(store: &Store, key: &str) -> Option<ValueType> {
        store.borrow().get(key).cloned()
    }

    /// A handle scoped to one state store, tagging dispatched actions with `scope`.
    #[derive(Clone)]
    struct ScopeHandle {
        scope: String,
        store: Store,
        dispatched: DispatchLog,
    }
    impl Read for ScopeHandle {
        fn read(&self, name: String) -> mlua::Result<ValueType> {
            self.store
                .borrow()
                .get(&name)
                .cloned()
                .ok_or_else(|| mlua::Error::RuntimeError(format!("unknown '{name}'")))
        }
    }
    impl Write for ScopeHandle {
        fn write(&self, name: String, value: ValueType) -> mlua::Result<()> {
            self.store.borrow_mut().insert(name, value);
            Ok(())
        }
    }
    impl OcppActions for ScopeHandle {
        fn actions() -> Vec<&'static str> {
            vec!["BootNotification", "StartTransaction"]
        }
        fn dispatch(&self, action: &str, _args: Vec<(String, ValueType)>) -> bool {
            self.dispatched
                .borrow_mut()
                .push((self.scope.clone(), action.to_string()));
            true
        }
    }

    /// Client host: a CS-level store plus lazily-created per-connector stores.
    #[derive(Clone, Default)]
    struct ClientHost {
        cs: Store,
        conns: Rc<RefCell<HashMap<i64, Store>>>,
        dispatched: DispatchLog,
    }
    impl Read for ClientHost {
        fn read(&self, name: String) -> mlua::Result<ValueType> {
            store_get(&self.cs, &name)
                .ok_or_else(|| mlua::Error::RuntimeError(format!("unknown '{name}'")))
        }
    }
    impl Write for ClientHost {
        fn write(&self, name: String, value: ValueType) -> mlua::Result<()> {
            self.cs.borrow_mut().insert(name, value);
            Ok(())
        }
    }
    impl OcppActions for ClientHost {
        fn actions() -> Vec<&'static str> {
            vec!["BootNotification", "StartTransaction"]
        }
        fn dispatch(&self, action: &str, _args: Vec<(String, ValueType)>) -> bool {
            self.dispatched
                .borrow_mut()
                .push(("cs".to_string(), action.to_string()));
            true
        }
    }
    impl OcppClientHost for ClientHost {
        type Conn = ScopeHandle;
        fn connector(&self, id: i64) -> ScopeHandle {
            let store = self.conns.borrow_mut().entry(id).or_default().clone();
            ScopeHandle {
                scope: format!("c{id}"),
                store,
                dispatched: self.dispatched.clone(),
            }
        }
    }

    #[test]
    fn ut_client_bare_is_cs_connector_is_scoped() {
        let host = ClientHost::default();
        let mut ctx = ContextBuilder::<String>::default()
            .with_stdlib()
            .with_module(OcppClient::init(host.clone()))
            .with_script(
                "s".to_string(),
                r#"
                C_OCPP:Set("Model", "M")
                C_OCPP:Connector(1):Set("Power", 11)
                C_OCPP:BootNotification()
                C_OCPP:Connector(2):StartTransaction({ idTag = "ABC" })
                "#,
            )
            .build()
            .expect("build context");
        ctx.call_all(std::time::Duration::ZERO).expect("run");

        // Bare Get/Set hit CS-level state; Connector(id) hits that connector's store.
        assert!(matches!(store_get(&host.cs, "Model"), Some(ValueType::String(s)) if s == "M"));
        let conn1 = host.conns.borrow()[&1].clone();
        assert!(matches!(
            store_get(&conn1, "Power"),
            Some(ValueType::Int(11))
        ));

        // Actions dispatch at the scope they were called on.
        let log = host.dispatched.borrow();
        assert!(log.contains(&("cs".to_string(), "BootNotification".to_string())));
        assert!(log.contains(&("c2".to_string(), "StartTransaction".to_string())));
    }

    /// Server host: a fixed map of stations, each with a CS store and per-connector stores.
    #[derive(Clone)]
    struct ServerHost {
        stations: Rc<RefCell<HashMap<String, StationData>>>,
        dispatched: DispatchLog,
    }
    impl ServerHost {
        /// Two stations: cp001 with connectors 1,2 and cp002 with connector 1.
        fn fixture() -> Self {
            let mut stations = HashMap::new();
            let mk = |conns: &[i64]| -> StationData {
                let map = conns.iter().map(|c| (*c, Store::default())).collect();
                (Store::default(), map)
            };
            stations.insert("cp001".to_string(), mk(&[1, 2]));
            stations.insert("cp002".to_string(), mk(&[1]));
            Self {
                stations: Rc::new(RefCell::new(stations)),
                dispatched: Rc::new(RefCell::new(Vec::new())),
            }
        }
    }
    impl OcppServerHost for ServerHost {
        type Station = ScopeHandle;
        type Conn = ScopeHandle;
        fn stations(&self) -> Vec<String> {
            let mut s: Vec<String> = self.stations.borrow().keys().cloned().collect();
            s.sort();
            s
        }
        fn connectors(&self, cs: &str) -> Vec<i64> {
            let stations = self.stations.borrow();
            let Some((_, conns)) = stations.get(cs) else {
                return Vec::new();
            };
            let mut ids: Vec<i64> = conns.keys().copied().collect();
            ids.sort();
            ids
        }
        fn station(&self, cs: &str) -> Option<ScopeHandle> {
            let stations = self.stations.borrow();
            let (store, _) = stations.get(cs)?;
            Some(ScopeHandle {
                scope: cs.to_string(),
                store: store.clone(),
                dispatched: self.dispatched.clone(),
            })
        }
        fn connector(&self, cs: &str, id: i64) -> Option<ScopeHandle> {
            let stations = self.stations.borrow();
            let (_, conns) = stations.get(cs)?;
            Some(ScopeHandle {
                scope: format!("{cs}/{id}"),
                store: conns.get(&id)?.clone(),
                dispatched: self.dispatched.clone(),
            })
        }
    }

    #[test]
    fn ut_server_enumerates_and_routes_by_identity() {
        let host = ServerHost::fixture();
        let mut ctx = ContextBuilder::<String>::default()
            .with_stdlib()
            .with_module(OcppServer::init(host.clone()))
            .with_script(
                "s".to_string(),
                r#"
                stations = C_OCPP:GetChargingStations()
                conns = C_OCPP:GetConnectors("cp001")
                C_OCPP:ChargingStation("cp001"):Set("Model", "X")
                C_OCPP:Connector("cp001", 1):Set("Power", 7)
                C_OCPP:Connector("cp002", 1):StartTransaction({ idTag = "T" })
                -- Stash enumeration results in a known store to assert from the host side.
                local probe = C_OCPP:Connector("cp001", 2)
                probe:Set("nstations", #stations)
                probe:Set("nconns", #conns)
                if C_OCPP:ChargingStation("nope") == nil then probe:Set("missing_ok", true) end
                "#,
            )
            .build()
            .expect("build context");
        ctx.call_all(std::time::Duration::ZERO).expect("run");

        let stations = host.stations.borrow();
        let (cp001_cs, cp001_conns) = &stations["cp001"];
        // Scope accessors route Set to the right station/connector store.
        assert!(matches!(store_get(cp001_cs, "Model"), Some(ValueType::String(s)) if s == "X"));
        assert!(matches!(
            store_get(&cp001_conns[&1], "Power"),
            Some(ValueType::Int(7))
        ));

        // Enumeration: 2 stations, cp001 has 2 connectors; unknown station resolves to nil.
        let probe = &cp001_conns[&2];
        assert!(matches!(
            store_get(probe, "nstations"),
            Some(ValueType::Int(2))
        ));
        assert!(matches!(
            store_get(probe, "nconns"),
            Some(ValueType::Int(2))
        ));
        assert!(matches!(
            store_get(probe, "missing_ok"),
            Some(ValueType::Bool(true))
        ));

        // Action dispatched on the cp002/1 connector accessor.
        assert!(
            host.dispatched
                .borrow()
                .contains(&("cp002/1".to_string(), "StartTransaction".to_string()))
        );
    }
}
