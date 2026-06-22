//! Per-OCPP-module Lua simulation. A version-generic bridge exposes a charging-station `CsState`
//! to Lua as the `C_OCPP` global: `Get`/`Set` read and write state fields by name, and
//! `C_OCPP:<Action>(overrides?)` enqueues an action onto a shared queue that the view drains and
//! sends. Enabled scripts run every ~100ms on a dedicated thread (the `mlua` VM is `!Send`),
//! mirroring the Modbus simulation.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use ferrowl_lua::module::{OcppActions, OcppModule, Read, TimeModule, ValueType, Write};
use ferrowl_lua::{ContextBuilder, Error};
use ferrowl_ocpp::{V1_6, V2_0_1, Version};

use crate::module::view::SharedLog;

/// Actions enqueued by Lua (`action` name + flat JSON override args), drained by the view each
/// refresh and turned into outbound sends.
pub type ActionQueue = Arc<Mutex<VecDeque<(String, serde_json::Value)>>>;

/// A version's charging-station state, addressable by field name and exposing its action set, so
/// the generic bridge can serve `C_OCPP` for either OCPP version. Implemented by both `CsState`s.
pub trait OcppFields {
    /// The `C_OCPP:<Action>` method names exposed for this version.
    fn actions() -> Vec<&'static str>
    where
        Self: Sized;
    fn get_field(&self, name: &str) -> Option<ValueType>;
    fn set_field(&mut self, name: &str, value: ValueType) -> bool;
}

impl OcppFields for crate::module::ocpp::client::v1_6::state::CsState {
    fn actions() -> Vec<&'static str> {
        V1_6::cs_actions().to_vec()
    }
    fn get_field(&self, name: &str) -> Option<ValueType> {
        self.get_field(name)
    }
    fn set_field(&mut self, name: &str, value: ValueType) -> bool {
        self.set_field(name, value)
    }
}

impl OcppFields for crate::module::ocpp::client::v2_0_1::state::CsState {
    fn actions() -> Vec<&'static str> {
        // The two transaction shortcuts (mapped to TransactionEvent by the view) precede the real
        // CS-originated actions, matching the action-button list.
        let mut actions = vec!["StartTransaction", "StopTransaction"];
        actions.extend_from_slice(V2_0_1::cs_actions());
        actions
    }
    fn get_field(&self, name: &str) -> Option<ValueType> {
        self.get_field(name)
    }
    fn set_field(&mut self, name: &str, value: ValueType) -> bool {
        self.set_field(name, value)
    }
}

/// Host handle backing `C_OCPP` for one module: shared state (for `Get`/`Set`) + the action queue.
struct OcppBridge<S: OcppFields> {
    state: Arc<RwLock<S>>,
    queue: ActionQueue,
}

impl<S: OcppFields + 'static> Read for OcppBridge<S> {
    fn read(&self, name: String) -> ferrowl_lua::Result<ValueType> {
        self.state
            .read()
            .unwrap()
            .get_field(&name)
            .ok_or_else(|| Error::RuntimeError(format!("unknown OCPP field '{name}'")))
    }
}

impl<S: OcppFields + 'static> Write for OcppBridge<S> {
    fn write(&self, name: String, value: ValueType) -> ferrowl_lua::Result<()> {
        if self.state.write().unwrap().set_field(&name, value) {
            Ok(())
        } else {
            Err(Error::RuntimeError(format!(
                "cannot set OCPP field '{name}'"
            )))
        }
    }
}

impl<S: OcppFields + 'static> OcppActions for OcppBridge<S> {
    fn actions() -> Vec<&'static str> {
        S::actions()
    }
    fn dispatch(&self, action: &str, args: Vec<(String, ValueType)>) -> bool {
        let mut overrides = serde_json::Map::new();
        for (key, value) in args {
            overrides.insert(key, vt_to_json(value));
        }
        self.queue
            .lock()
            .unwrap()
            .push_back((action.to_string(), serde_json::Value::Object(overrides)));
        true
    }
}

/// Map a Lua `ValueType` to JSON for the override args (integers narrow to i64 — override values
/// are small scalars like ports and currents).
fn vt_to_json(value: ValueType) -> serde_json::Value {
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
}

impl Drop for OcppSimHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Spawn the simulation thread for one OCPP module. Returns `None` when there are no scripts. The
/// Lua `Context` is built inside the thread because it is `!Send`. Each cycle runs every enabled
/// script at most every ~100ms (`refresh_all`); script errors go to the module log.
pub fn run_ocpp_sim<S>(
    state: Arc<RwLock<S>>,
    queue: ActionQueue,
    scripts: Vec<(String, String)>,
    log: SharedLog,
) -> Option<OcppSimHandle>
where
    S: OcppFields + Send + Sync + 'static,
{
    if scripts.is_empty() {
        return None;
    }

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let handle = std::thread::spawn(move || {
        let bridge = OcppBridge { state, queue };
        let mut builder = ContextBuilder::<String>::default()
            .with_stdlib()
            .with_module(OcppModule::init(bridge))
            .with_module(TimeModule::default());
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
            if let Err(errors) = context.refresh_all(Duration::from_millis(100)) {
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

/// Append a line to the module's ring log from the (non-runtime) sim thread.
fn emit(log: &SharedLog, line: &str) {
    log.blocking_write().write(line);
}

/// Sleep up to `interval` in small chunks so the stop flag is observed promptly.
fn sleep_responsive(interval: Duration, stop: &AtomicBool) {
    let chunk = Duration::from_millis(25);
    let mut slept = Duration::ZERO;
    while slept < interval && !stop.load(Ordering::Relaxed) {
        let step = chunk.min(interval - slept);
        std::thread::sleep(step);
        slept += step;
    }
}
