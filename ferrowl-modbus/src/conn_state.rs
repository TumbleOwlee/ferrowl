//! MB-R-137/152/153 — a shared boolean flag reporting whether a client/server/monitor task is
//! currently connected (transport dialed, listener bound, or serial port open), independent of
//! whether the task itself is still running. Mirrors `PathConflictCell`'s shape
//! (`path_conflict.rs`) but carries a plain flag, not a checker closure.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Default)]
pub struct ConnectedCell(Arc<AtomicBool>);

impl ConnectedCell {
    pub fn set(&self, connected: bool) {
        self.0.store(connected, Ordering::Relaxed);
    }

    pub fn get(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// MB-R-137 — a fresh cell reports "not connected", matching a task that hasn't completed
    /// its first connect/bind/open attempt yet.
    fn ut_conn_state_default_is_false() {
        assert!(!ConnectedCell::default().get());
    }

    #[test]
    /// MB-R-137 — `set`/`get` round-trip.
    fn ut_conn_state_set_then_get_reflects_value() {
        let cell = ConnectedCell::default();
        cell.set(true);
        assert!(cell.get());
        cell.set(false);
        assert!(!cell.get());
    }

    #[test]
    /// MB-R-137 — clones share the same underlying flag: a `spawn()` call keeps one clone while
    /// handing another out to the caller, and both must observe the same state.
    fn ut_conn_state_clone_shares_state() {
        let cell = ConnectedCell::default();
        let clone = cell.clone();
        clone.set(true);
        assert!(cell.get());
    }
}
