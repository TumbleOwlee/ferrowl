//! MB-R-150 — session-wide directory of which module instance is currently running against
//! which Rtu/Ascii serial path, so a connect attempt (via the `PathConflictCell` each
//! `ferrowl-modbus` rtu/ascii builder exposes) can check for another instance already
//! claiming the same path before the OS-level open. "Currently running" (claimed between a
//! successful `start()` and the matching `stop()`), not "currently present in session
//! config" — a stopped instance must stop counting as a conflict (MB-R-150: "recovers…once
//! the conflicting instance stops").

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct SerialPathRegistry {
    claims: Arc<RwLock<HashMap<String, String>>>, // instance name -> ~-expanded path
}

impl SerialPathRegistry {
    // `#[allow(dead_code)]`: implemented and tested below; App does not construct the
    // session-wide registry yet.
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record `name` as currently running against `expanded_path` (already `~`-expanded).
    pub fn claim(&self, name: &str, expanded_path: &str) {
        self.claims
            .write()
            .insert(name.to_string(), expanded_path.to_string());
    }

    /// Remove `name`'s claim, if any. Safe to call even when `name` never claimed anything.
    pub fn release(&self, name: &str) {
        self.claims.write().remove(name);
    }

    /// Name of some *other* instance currently claiming `expanded_path`, if any.
    pub fn conflict(&self, name: &str, expanded_path: &str) -> Option<String> {
        self.claims
            .read()
            .iter()
            .find(|(n, p)| n.as_str() != name && p.as_str() == expanded_path)
            .map(|(n, _)| n.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MB-R-150 — two different instances on the same path see each other as a conflict;
    /// neither sees itself.
    #[test]
    fn ut_registry_symmetric_conflict_on_shared_path() {
        let reg = SerialPathRegistry::new();
        reg.claim("A", "/dev/ttyUSB0");
        reg.claim("B", "/dev/ttyUSB0");
        assert_eq!(reg.conflict("A", "/dev/ttyUSB0"), Some("B".to_string()));
        assert_eq!(reg.conflict("B", "/dev/ttyUSB0"), Some("A".to_string()));
    }

    /// MB-R-150 — different paths never conflict.
    #[test]
    fn ut_registry_no_conflict_on_different_paths() {
        let reg = SerialPathRegistry::new();
        reg.claim("A", "/dev/ttyUSB0");
        reg.claim("B", "/dev/ttyUSB1");
        assert_eq!(reg.conflict("A", "/dev/ttyUSB0"), None);
    }

    /// MB-R-150 — releasing a claim clears the conflict for the remaining instance
    /// ("recovers automatically once the conflicting instance stops").
    #[test]
    fn ut_registry_release_clears_conflict() {
        let reg = SerialPathRegistry::new();
        reg.claim("A", "/dev/ttyUSB0");
        reg.claim("B", "/dev/ttyUSB0");
        reg.release("A");
        assert_eq!(reg.conflict("B", "/dev/ttyUSB0"), None);
    }

    /// MB-R-150 — clones share the same underlying map.
    #[test]
    fn ut_registry_clone_shares_state() {
        let reg = SerialPathRegistry::new();
        let clone = reg.clone();
        clone.claim("A", "/dev/ttyUSB0");
        assert_eq!(reg.conflict("B", "/dev/ttyUSB0"), Some("A".to_string()));
    }
}
