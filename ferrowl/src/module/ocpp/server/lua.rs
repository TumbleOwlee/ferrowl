//! Single per-module Lua sim for the CSMS **server** view. Unlike the client (one charging station)
//! the server spans many connected stations, so one Lua context backs the whole module and the
//! `C_OCPP` global is the multi-station [`OcppServer`] shape: `GetChargingStations()` /
//! `GetConnectors(cs)` enumerate, `ChargingStation(cs)` / `Connector(cs, id)` return per-scope
//! accessors. State is reached through a [`SharedServerStates`] registry the view keeps in step with
//! its entries; dispatched actions land on a [`ServerActionQueue`] the view drains and routes.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use parking_lot::{Mutex, RwLock};

use ferrowl_lua::module::{
    LogModule, OcppActions, OcppServer, OcppServerHost, Read, TestModule, TimeModule, ValueType,
    Write,
};
use ferrowl_lua::{ContextBuilder, Error};

use crate::app::Level;
use crate::module::ocpp::client::lua_sim::{
    LuaLogSink, OcppFields, OcppSimHandle, emit, sleep_responsive, vt_to_json,
};
use crate::module::ocpp::lock::{with_state, with_state_mut};
use crate::module::ocpp::scope::Scope;
use crate::module::ocpp::server::view::ServerVersion;
use crate::module::view::SharedLog;

/// Actions enqueued by the server Lua sim: `(identity, scope, action, overrides-json)`, drained by
/// the view each refresh and routed to the matching connection/entry.
pub type ServerActionQueue = Arc<Mutex<VecDeque<(String, Scope, String, serde_json::Value)>>>;

/// One charging station's shared observed states, by level.
pub struct Station<V: ServerVersion> {
    pub cs: Option<Arc<RwLock<V::Cs>>>,
    /// `(scope, state)` per connector entry; the scope carries the EVSE for 2.0.1.
    pub conns: Vec<(Scope, Arc<RwLock<V::Conn>>)>,
}

impl<V: ServerVersion> Default for Station<V> {
    fn default() -> Self {
        Self {
            cs: None,
            conns: Vec::new(),
        }
    }
}

/// Registry of every connected station's shared states, written by the view as entries appear and
/// read by the sim thread.
pub struct ServerStates<V: ServerVersion> {
    pub stations: HashMap<String, Station<V>>,
}

impl<V: ServerVersion> Default for ServerStates<V> {
    fn default() -> Self {
        Self {
            stations: HashMap::new(),
        }
    }
}

pub type SharedServerStates<V> = Arc<RwLock<ServerStates<V>>>;

/// Push a `(identity, scope, action, overrides-json)` item onto the server action queue.
fn enqueue(
    queue: &ServerActionQueue,
    identity: String,
    scope: Scope,
    action: &str,
    args: Vec<(String, ValueType)>,
) {
    let mut overrides = serde_json::Map::new();
    for (key, value) in args {
        overrides.insert(key, vt_to_json(value));
    }
    queue.lock().push_back((
        identity,
        scope,
        action.to_string(),
        serde_json::Value::Object(overrides),
    ));
}

/// Accessor handle for a station's CS-level state.
pub struct CsHandle<V: ServerVersion> {
    identity: String,
    state: Arc<RwLock<V::Cs>>,
    queue: ServerActionQueue,
}

impl<V: ServerVersion> Read for CsHandle<V> {
    fn read(&self, name: String) -> ferrowl_lua::Result<ValueType> {
        with_state(&self.state, |s| s.get_field(&name))
            .ok_or_else(|| Error::RuntimeError(format!("unknown CS field '{name}'")))
    }
}
impl<V: ServerVersion> Write for CsHandle<V> {
    fn write(&self, name: String, value: ValueType) -> ferrowl_lua::Result<()> {
        if with_state_mut(&self.state, |s| s.set_field(&name, value)) {
            Ok(())
        } else {
            Err(Error::RuntimeError(format!("cannot set CS field '{name}'")))
        }
    }
}
impl<V: ServerVersion> OcppActions for CsHandle<V> {
    fn actions() -> Vec<&'static str> {
        <V::Cs as OcppFields>::actions()
    }
    fn dispatch(&self, action: &str, args: Vec<(String, ValueType)>) -> bool {
        enqueue(&self.queue, self.identity.clone(), Scope::CS, action, args);
        true
    }
}

/// Accessor handle for one connector of a station.
pub struct ConnHandle<V: ServerVersion> {
    identity: String,
    scope: Scope,
    state: Arc<RwLock<V::Conn>>,
    queue: ServerActionQueue,
}

impl<V: ServerVersion> Read for ConnHandle<V> {
    fn read(&self, name: String) -> ferrowl_lua::Result<ValueType> {
        with_state(&self.state, |s| s.get_field(&name))
            .ok_or_else(|| Error::RuntimeError(format!("unknown connector field '{name}'")))
    }
}
impl<V: ServerVersion> Write for ConnHandle<V> {
    fn write(&self, name: String, value: ValueType) -> ferrowl_lua::Result<()> {
        if with_state_mut(&self.state, |s| s.set_field(&name, value)) {
            Ok(())
        } else {
            Err(Error::RuntimeError(format!(
                "cannot set connector field '{name}'"
            )))
        }
    }
}
impl<V: ServerVersion> OcppActions for ConnHandle<V> {
    fn actions() -> Vec<&'static str> {
        <V::Conn as OcppFields>::actions()
    }
    fn dispatch(&self, action: &str, args: Vec<(String, ValueType)>) -> bool {
        enqueue(&self.queue, self.identity.clone(), self.scope, action, args);
        true
    }
}

/// Host backing the server `C_OCPP` module over the shared state registry.
pub struct ServerHost<V: ServerVersion> {
    states: SharedServerStates<V>,
    queue: ServerActionQueue,
}

impl<V: ServerVersion> ServerHost<V> {
    /// Builds a host over the shared state registry and its action queue (used by both the
    /// running sim thread and the session-level [`crate::registry::OcppServerEntry`] `C_Module`
    /// bridge).
    pub(crate) fn new(states: SharedServerStates<V>, queue: ServerActionQueue) -> Self {
        Self { states, queue }
    }
}

impl<V: ServerVersion> OcppServerHost for ServerHost<V> {
    type Station = CsHandle<V>;
    type Conn = ConnHandle<V>;

    fn stations(&self) -> Vec<String> {
        let mut ids: Vec<String> =
            with_state(&self.states, |s| s.stations.keys().cloned().collect());
        ids.sort();
        ids
    }

    fn connectors(&self, cs: &str) -> Vec<i64> {
        let mut ids: Vec<i64> = with_state(&self.states, |states| {
            let Some(station) = states.stations.get(cs) else {
                return Vec::new();
            };
            station
                .conns
                .iter()
                .filter_map(|(scope, _)| V::lua_connector_id(*scope))
                .collect()
        });
        ids.sort();
        ids
    }

    fn station(&self, cs: &str) -> Option<CsHandle<V>> {
        let state = with_state(&self.states, |states| states.stations.get(cs)?.cs.clone())?;
        Some(CsHandle {
            identity: cs.to_string(),
            state,
            queue: self.queue.clone(),
        })
    }

    fn connector(&self, cs: &str, id: i64) -> Option<ConnHandle<V>> {
        let (scope, state) = with_state(&self.states, |states| {
            let station = states.stations.get(cs)?;
            let (scope, state) = station
                .conns
                .iter()
                .find(|(scope, _)| V::lua_connector_id(*scope) == Some(id))?;
            Some((*scope, state.clone()))
        })?;
        Some(ConnHandle {
            identity: cs.to_string(),
            scope,
            state,
            queue: self.queue.clone(),
        })
    }
}

/// Spawn the single Lua sim thread for the whole server module. Returns `None` when there are no
/// scripts. The Lua `Context` is built inside the thread (it is `!Send`); script errors go to the
/// module log. Mirrors the client [`run_client_sim`](crate::module::ocpp::client::lua_sim::run_client_sim).
/// Run one server script exactly once on a short-lived detached thread, outside the sim
/// (SC-R-035). Fresh context, same `C_*` modules as [`run_server_sim`], one script, no enabled
/// filter and no loop. Errors log under `[run]`, distinct from the sim's `[lua]`/`[sim]` lines.
pub fn run_server_script_once<V: ServerVersion>(
    states: SharedServerStates<V>,
    queue: ServerActionQueue,
    name: String,
    code: String,
    log: SharedLog,
) {
    std::thread::spawn(move || {
        let host = ServerHost { states, queue };
        let context = ContextBuilder::<String>::default()
            .with_stdlib()
            .with_module(OcppServer::init(host))
            .with_module(TimeModule::default())
            .with_module(TestModule)
            .with_module(LogModule::init(LuaLogSink(log.clone())))
            .with_print_sink(LuaLogSink(log.clone()))
            .with_script(name.clone(), &code)
            .build();
        match context {
            Ok(mut context) => {
                if let Err(e) = context.call(&name) {
                    emit(&log, Level::Error, &format!("[run] {e}"));
                }
            }
            Err(e) => emit(
                &log,
                Level::Error,
                &format!("[run] failed to build context: {e}"),
            ),
        }
    });
}

pub fn run_server_sim<V: ServerVersion>(
    states: SharedServerStates<V>,
    queue: ServerActionQueue,
    scripts: Vec<(String, String)>,
    interval: Duration,
    log: SharedLog,
) -> Option<OcppSimHandle> {
    if scripts.is_empty() {
        return None;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let handle = std::thread::spawn(move || {
        let host = ServerHost { states, queue };
        let mut builder = ContextBuilder::<String>::default()
            .with_stdlib()
            .with_module(OcppServer::init(host))
            .with_module(TimeModule::default())
            .with_module(TestModule)
            .with_module(LogModule::init(LuaLogSink(log.clone())))
            .with_print_sink(LuaLogSink(log.clone()));
        for (name, code) in &scripts {
            builder = builder.with_script(name.clone(), code);
        }
        let mut context = match builder.build() {
            Ok(context) => context,
            Err(e) => {
                emit(
                    &log,
                    Level::Error,
                    &format!("[lua] failed to build context: {e}"),
                );
                return;
            }
        };
        while !thread_stop.load(Ordering::Relaxed) {
            if let Err(errors) = context.refresh_all(interval) {
                for e in errors {
                    emit(&log, Level::Error, &format!("[lua] {e}"));
                }
            }
            sleep_responsive(Duration::from_millis(50), &thread_stop);
        }
    });
    Some(OcppSimHandle::from_parts(stop, handle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrowl_ocpp::V1_6;

    type Cs = <V1_6 as ServerVersion>::Cs;
    type Conn = <V1_6 as ServerVersion>::Conn;

    /// SC-R-021 — a server module's C_OCPP bridge enumerates stations and routes Get/Set/action by identity.
    #[test]
    fn ut_server_host_resolves_and_routes_by_identity() {
        let states: SharedServerStates<V1_6> = Arc::new(RwLock::new(ServerStates::default()));
        with_state_mut(&states, |reg| {
            let st = reg.stations.entry("CP1".to_string()).or_default();
            st.cs = Some(Arc::new(RwLock::new(Cs::default())));
            st.conns
                .push((Scope::connector(1), Arc::new(RwLock::new(Conn::default()))));
        });
        let queue: ServerActionQueue = Arc::new(Mutex::new(VecDeque::new()));
        let host = ServerHost {
            states,
            queue: queue.clone(),
        };

        // Enumeration + resolution.
        assert_eq!(host.stations(), vec!["CP1".to_string()]);
        assert_eq!(host.connectors("CP1"), vec![1]);
        assert!(host.station("CP1").is_some());
        assert!(host.station("nope").is_none());
        assert!(host.connector("CP1", 9).is_none());

        // A connector action enqueues with the station identity + connector scope.
        let conn = host.connector("CP1", 1).expect("connector handle");
        assert!(conn.dispatch("RemoteStartTransaction", vec![]));
        let (identity, scope, action, _) = queue.lock().pop_front().unwrap();
        assert_eq!(identity, "CP1");
        assert_eq!(scope, Scope::connector(1));
        assert_eq!(action, "RemoteStartTransaction");

        // CS-level Get/Set round-trips through the shared state.
        let cs = host.station("CP1").unwrap();
        cs.write("Model".to_string(), ValueType::String("X".into()))
            .unwrap();
        assert!(matches!(cs.read("Model".to_string()).unwrap(), ValueType::String(s) if s == "X"));
    }
}
