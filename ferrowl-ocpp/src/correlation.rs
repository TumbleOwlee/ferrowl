//! Outbound-call correlation table.
//!
//! Maps a Call's [`UniqueId`] to the oneshot that should be completed when its `CallResult` /
//! `CallError` arrives. Shared (`Arc<Mutex<_>>`) between the connection's reader task (which
//! completes entries) and the per-call awaiter tasks (which register and wait).

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::Value;
use tokio::sync::oneshot;

use crate::error::CallError;
use crate::ocppj::UniqueId;

/// Result delivered to a waiting caller: the raw CallResult payload, or a peer rejection.
pub type CallOutcome = Result<Value, CallError>;

/// Correlation table of in-flight outbound Calls.
#[derive(Clone, Default)]
pub struct PendingCalls {
    inner: Arc<Mutex<HashMap<UniqueId, oneshot::Sender<CallOutcome>>>>,
}

impl PendingCalls {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register interest in the reply to `id`, returning the receiver to await.
    pub fn register(&self, id: UniqueId) -> oneshot::Receiver<CallOutcome> {
        let (tx, rx) = oneshot::channel();
        self.inner.lock().insert(id, tx);
        rx
    }

    /// Complete the entry for `id` with `outcome`. No-op (does not panic) if `id` is unknown or
    /// its waiter already dropped.
    pub fn complete(&self, id: &UniqueId, outcome: CallOutcome) {
        if let Some(tx) = self.inner.lock().remove(id) {
            let _ = tx.send(outcome);
        }
    }

    /// Forget the entry for `id` without completing it (e.g. on timeout).
    pub fn remove(&self, id: &UniqueId) {
        self.inner.lock().remove(id);
    }

    /// Drop every pending entry, signaling each waiter that the connection is gone.
    pub fn fail_all(&self, err: &CallError) {
        let mut guard = self.inner.lock();
        for (_, tx) in guard.drain() {
            let _ = tx.send(Err(err.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocppj::CallErrorCode;

    #[tokio::test]
    /// OC-R-017 — a registered outbound Call is completed by the matching inbound reply, keyed by unique id.
    async fn ut_register_complete() {
        let p = PendingCalls::new();
        let id = UniqueId("a".into());
        let rx = p.register(id.clone());
        p.complete(&id, Ok(serde_json::json!({"ok": true})));
        let got = rx.await.unwrap().unwrap();
        assert_eq!(got, serde_json::json!({"ok": true}));
    }

    #[tokio::test]
    /// OC-R-019 — completing an id that matches no pending entry is discarded silently (no panic).
    async fn ut_complete_unknown_id_is_noop() {
        let p = PendingCalls::new();
        // Must not panic.
        p.complete(&UniqueId("missing".into()), Ok(serde_json::Value::Null));
    }

    #[tokio::test]
    /// OC-R-022 — on teardown every pending outbound Call is failed with a rejection, so no caller waits forever.
    async fn ut_fail_all_notifies_waiters() {
        let p = PendingCalls::new();
        let rx = p.register(UniqueId("x".into()));
        p.fail_all(&CallError::new(CallErrorCode::GenericError, "gone"));
        assert!(rx.await.unwrap().is_err());
    }
}
