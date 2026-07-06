//! Per-OCPP-module Lua simulation for the **client** (charging-station) views. The `C_OCPP` global
//! is the multi-connector [`OcppClient`] shape: bare `Get`/`Set`/`<Action>` address the CS level,
//! `Connector(id)` addresses one connector. Actions enqueue onto a [`ScopedActionQueue`] (carrying
//! the target scope) that the view drains and sends. Enabled scripts run once per second on a
//! dedicated thread (the `mlua` VM is `!Send`). The [`OcppFields`] trait + the server's bridge live
//! alongside in `server/lua.rs`.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use parking_lot::{Mutex, RwLock};

use ferrowl_lua::module::{
    LogModule, LogSink, OcppActions, OcppClient, OcppClientHost, Read, TestModule, TimeModule,
    ValueType, Write,
};
use ferrowl_lua::{ContextBuilder, Error};
use ferrowl_ocpp::{V1_6, V2_0_1, Version};

use crate::module::ocpp::lock::{with_state, with_state_mut};
use crate::module::ocpp::scope::Scope;
use crate::module::view::SharedLog;

/// A version's observed-state type, addressable by field name and exposing its action set. The
/// server states implement this (CS-level / connector) and the server Lua bridge in `server/lua.rs`
/// reads/writes through it.
pub trait OcppFields {
    /// The `C_OCPP:<Action>` method names exposed for this version.
    fn actions() -> Vec<&'static str>
    where
        Self: Sized;
    fn get_field(&self, name: &str) -> Option<ValueType>;
    fn set_field(&mut self, name: &str, value: ValueType) -> bool;
}

// --- Multi-connector client bridge (split CS / connector state) ------------

/// Actions enqueued by Lua, drained by the multi-connector client view: target `scope` + action
/// name + flat JSON override args.
pub type ScopedActionQueue = Arc<Mutex<VecDeque<(Scope, String, serde_json::Value)>>>;

/// Split-level state access for the multi-connector client `C_OCPP` bridge: bare `Get`/`Set`/
/// `<Action>` hit the CS level, `Connector(id)` hits one connector. Implemented by the client
/// `CsState`s.
pub trait ClientFields {
    /// CS-level action method names (bare `C_OCPP:<Action>`).
    fn cs_actions() -> Vec<&'static str>
    where
        Self: Sized;
    /// Connector-level action method names (`C_OCPP:Connector(id):<Action>`).
    fn conn_actions() -> Vec<&'static str>
    where
        Self: Sized;
    fn cs_get(&self, name: &str) -> Option<ValueType>;
    fn cs_set(&mut self, name: &str, value: ValueType) -> bool;
    fn conn_get(&self, id: i64, name: &str) -> Option<ValueType>;
    fn conn_set(&mut self, id: i64, name: &str, value: ValueType) -> bool;
    /// The dispatch scope for `Connector(id)` actions (version-specific: 1.6 connector / 2.0.1 evse).
    fn conn_scope(&self, id: i64) -> Scope;
}

/// Host handle for the CS level of one client module: shared state + the scoped queue. Bare
/// `Get`/`Set`/`<Action>` route here; `Connector(id)` produces a [`ClientConnHandle`].
struct ClientCsHandle<S: ClientFields> {
    state: Arc<RwLock<S>>,
    queue: ScopedActionQueue,
}

impl<S: ClientFields + 'static> Read for ClientCsHandle<S> {
    fn read(&self, name: String) -> ferrowl_lua::Result<ValueType> {
        with_state(&self.state, |s| s.cs_get(&name))
            .ok_or_else(|| Error::RuntimeError(format!("unknown CS field '{name}'")))
    }
}
impl<S: ClientFields + 'static> Write for ClientCsHandle<S> {
    fn write(&self, name: String, value: ValueType) -> ferrowl_lua::Result<()> {
        if with_state_mut(&self.state, |s| s.cs_set(&name, value)) {
            Ok(())
        } else {
            Err(Error::RuntimeError(format!("cannot set CS field '{name}'")))
        }
    }
}
impl<S: ClientFields + 'static> OcppActions for ClientCsHandle<S> {
    fn actions() -> Vec<&'static str> {
        S::cs_actions()
    }
    fn dispatch(&self, action: &str, args: Vec<(String, ValueType)>) -> bool {
        enqueue(&self.queue, Scope::CS, action, args);
        true
    }
}
impl<S: ClientFields + 'static> OcppClientHost for ClientCsHandle<S> {
    type Conn = ClientConnHandle<S>;
    fn connector(&self, id: i64) -> ClientConnHandle<S> {
        ClientConnHandle {
            state: self.state.clone(),
            queue: self.queue.clone(),
            id,
        }
    }
}

/// Host handle for one connector of a client module, addressing it by id.
struct ClientConnHandle<S: ClientFields> {
    state: Arc<RwLock<S>>,
    queue: ScopedActionQueue,
    id: i64,
}

impl<S: ClientFields + 'static> Read for ClientConnHandle<S> {
    fn read(&self, name: String) -> ferrowl_lua::Result<ValueType> {
        with_state(&self.state, |s| s.conn_get(self.id, &name))
            .ok_or_else(|| Error::RuntimeError(format!("unknown connector field '{name}'")))
    }
}
impl<S: ClientFields + 'static> Write for ClientConnHandle<S> {
    fn write(&self, name: String, value: ValueType) -> ferrowl_lua::Result<()> {
        if with_state_mut(&self.state, |s| s.conn_set(self.id, &name, value)) {
            Ok(())
        } else {
            Err(Error::RuntimeError(format!(
                "cannot set connector field '{name}'"
            )))
        }
    }
}
impl<S: ClientFields + 'static> OcppActions for ClientConnHandle<S> {
    fn actions() -> Vec<&'static str> {
        S::conn_actions()
    }
    fn dispatch(&self, action: &str, args: Vec<(String, ValueType)>) -> bool {
        let scope = with_state(&self.state, |s| s.conn_scope(self.id));
        enqueue(&self.queue, scope, action, args);
        true
    }
}

/// Push a `(scope, action, overrides-json)` item onto a scoped queue.
fn enqueue(queue: &ScopedActionQueue, scope: Scope, action: &str, args: Vec<(String, ValueType)>) {
    let mut overrides = serde_json::Map::new();
    for (key, value) in args {
        overrides.insert(key, vt_to_json(value));
    }
    queue.lock().push_back((
        scope,
        action.to_string(),
        serde_json::Value::Object(overrides),
    ));
}

/// Connector-scoped action names for OCPP 1.6 (everything else is CS-level).
const CONNECTOR_ACTIONS_V16: &[&str] = &[
    "StatusNotification",
    "MeterValues",
    "StartTransaction",
    "StopTransaction",
];

impl ClientFields for crate::module::ocpp::client::v1_6::state::CsState {
    fn cs_actions() -> Vec<&'static str> {
        V1_6::cs_actions()
            .iter()
            .copied()
            .filter(|a| !CONNECTOR_ACTIONS_V16.contains(a))
            .collect()
    }
    fn conn_actions() -> Vec<&'static str> {
        V1_6::cs_actions()
            .iter()
            .copied()
            .filter(|a| CONNECTOR_ACTIONS_V16.contains(a))
            .collect()
    }
    fn cs_get(&self, name: &str) -> Option<ValueType> {
        self.cs_get_field(name)
    }
    fn cs_set(&mut self, name: &str, value: ValueType) -> bool {
        self.cs_set_field(name, value)
    }
    fn conn_get(&self, id: i64, name: &str) -> Option<ValueType> {
        self.connector(id).and_then(|c| c.get_field(name))
    }
    fn conn_set(&mut self, id: i64, name: &str, value: ValueType) -> bool {
        self.connector_mut(id)
            .map(|c| c.set_field(name, value))
            .unwrap_or(false)
    }
    fn conn_scope(&self, id: i64) -> Scope {
        Scope::connector(id)
    }
}

/// Spawn the simulation thread for one multi-connector client module. Like [`run_ocpp_sim`] but
/// over split CS/connector state via the [`OcppClient`] module (bare `Get/Set/<Action>` = CS,
/// `Connector(id)` = one connector).
pub fn run_client_sim<S>(
    state: Arc<RwLock<S>>,
    queue: ScopedActionQueue,
    scripts: Vec<(String, String)>,
    log: SharedLog,
) -> Option<OcppSimHandle>
where
    S: ClientFields + Send + Sync + 'static,
{
    if scripts.is_empty() {
        return None;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let handle = std::thread::spawn(move || {
        let bridge = ClientCsHandle { state, queue };
        let mut builder = ContextBuilder::<String>::default()
            .with_stdlib()
            .with_module(OcppClient::init(bridge))
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
                emit(&log, &format!("[lua] failed to build context: {e}"));
                return;
            }
        };
        while !thread_stop.load(Ordering::Relaxed) {
            if let Err(errors) = context.refresh_all(Duration::from_secs(1)) {
                for e in errors {
                    emit(&log, &format!("[lua] {e}"));
                }
            }
            sleep_responsive(Duration::from_millis(50), &thread_stop);
        }
    });
    Some(OcppSimHandle {
        stop,
        handle: Some(handle),
    })
}

/// Connector-scoped action names for OCPP 2.0.1 (everything else is CS-level). The
/// `StartTransaction`/`StopTransaction` shortcuts (mapped to TransactionEvent by the view) are also
/// connector-scoped and are prepended by `conn_actions`.
const CONNECTOR_ACTIONS_V201: &[&str] = &["MeterValues", "StatusNotification", "TransactionEvent"];

impl ClientFields for crate::module::ocpp::client::v2_0_1::state::CsState {
    fn cs_actions() -> Vec<&'static str> {
        V2_0_1::cs_actions()
            .iter()
            .copied()
            .filter(|a| !CONNECTOR_ACTIONS_V201.contains(a))
            .collect()
    }
    fn conn_actions() -> Vec<&'static str> {
        let mut actions = vec!["StartTransaction", "StopTransaction"];
        actions.extend(
            V2_0_1::cs_actions()
                .iter()
                .copied()
                .filter(|a| CONNECTOR_ACTIONS_V201.contains(a)),
        );
        actions
    }
    fn cs_get(&self, name: &str) -> Option<ValueType> {
        self.cs_get_field(name)
    }
    fn cs_set(&mut self, name: &str, value: ValueType) -> bool {
        self.cs_set_field(name, value)
    }
    fn conn_get(&self, id: i64, name: &str) -> Option<ValueType> {
        self.connector(id).and_then(|c| c.get_field(name))
    }
    fn conn_set(&mut self, id: i64, name: &str, value: ValueType) -> bool {
        self.connector_mut(id)
            .map(|c| c.set_field(name, value))
            .unwrap_or(false)
    }
    fn conn_scope(&self, id: i64) -> Scope {
        match self.connector(id) {
            Some(c) => Scope::evse(c.evse_id, Some(c.connector_id)),
            None => Scope::evse(1, Some(id)),
        }
    }
}

/// Map a Lua `ValueType` to JSON for the override args (integers narrow to i64 — override values
/// are small scalars like ports and currents).
pub(crate) fn vt_to_json(value: ValueType) -> serde_json::Value {
    match value {
        ValueType::Int(v) => serde_json::Value::from(v as i64),
        ValueType::Float(v) => serde_json::json!(v),
        ValueType::String(v) => serde_json::Value::String(v),
        ValueType::Bool(v) => serde_json::Value::Bool(v),
        ValueType::Nil => serde_json::Value::Null,
    }
}

/// Shallow-merge a JSON object of override fields into a base payload object (overrides win).
/// No-op when either side is not an object.
pub fn merge_overrides(base: &mut serde_json::Value, overrides: serde_json::Value) {
    if let (Some(base), Some(overrides)) = (base.as_object_mut(), overrides.as_object()) {
        for (key, value) in overrides {
            base.insert(key.clone(), value.clone());
        }
    }
}

/// Controls a running OCPP simulation thread: setting the flag and joining stops it.
pub struct OcppSimHandle {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl OcppSimHandle {
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    /// Construct a handle from its stop flag and join handle (for sibling sim runners, e.g. the
    /// server's single-sim runner in `server/lua.rs`).
    pub(crate) fn from_parts(stop: Arc<AtomicBool>, handle: std::thread::JoinHandle<()>) -> Self {
        Self {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for OcppSimHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Append a line to the module's ring log from the (non-runtime) sim thread.
pub(crate) fn emit(log: &SharedLog, line: &str) {
    log.blocking_write().write(line);
}

/// Routes `C_Log:Print(..)` lines from a Lua sim into the module's ring log. Used to build the
/// `C_Log` module for both the client and server sims.
pub(crate) struct LuaLogSink(pub SharedLog);

impl LogSink for LuaLogSink {
    fn print(&self, line: &str) {
        emit(&self.0, line);
    }
}

/// Sleep up to `interval` in small chunks so the stop flag is observed promptly.
pub(crate) fn sleep_responsive(interval: Duration, stop: &AtomicBool) {
    let chunk = Duration::from_millis(25);
    let mut slept = Duration::ZERO;
    while slept < interval && !stop.load(Ordering::Relaxed) {
        let step = chunk.min(interval - slept);
        std::thread::sleep(step);
        slept += step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::ocpp::client::v1_6::state::CsState as Cs16;
    use crate::module::ocpp::client::v2_0_1::state::CsState as Cs201;

    #[test]
    fn ut_v16_actions_partition_disjoint_and_covers() {
        let cs = <Cs16 as ClientFields>::cs_actions();
        let conn = <Cs16 as ClientFields>::conn_actions();
        // The two levels are disjoint and together cover every 1.6 CS action.
        for a in &conn {
            assert!(!cs.contains(a), "{a} in both levels");
        }
        assert_eq!(cs.len() + conn.len(), V1_6::cs_actions().len());
        assert!(conn.contains(&"StartTransaction"));
        assert!(cs.contains(&"BootNotification"));
    }

    #[test]
    fn ut_conn_scope_is_version_specific() {
        // 1.6: connector-only scope.
        let s16 = Cs16::default();
        assert_eq!(
            <Cs16 as ClientFields>::conn_scope(&s16, 1),
            Scope::connector(1)
        );
        // 2.0.1: scope carries the connector's EVSE.
        let mut s201 = Cs201::default();
        s201.add_connector(2, 5);
        assert_eq!(
            <Cs201 as ClientFields>::conn_scope(&s201, 5),
            Scope::evse(2, Some(5))
        );
    }

    #[test]
    fn ut_v201_conn_actions_include_shortcuts() {
        let conn = <Cs201 as ClientFields>::conn_actions();
        assert!(conn.contains(&"StartTransaction"));
        assert!(conn.contains(&"StopTransaction"));
        // CS-level excludes connector-scoped reals like MeterValues.
        let cs = <Cs201 as ClientFields>::cs_actions();
        assert!(!cs.contains(&"MeterValues"));
        assert!(cs.contains(&"BootNotification"));
    }
}
