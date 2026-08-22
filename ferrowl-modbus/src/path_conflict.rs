//! MB-R-150 — a late-bindable, session-wide check for whether another module instance is
//! already claiming the Rtu/Ascii serial path about to be opened. `PathConflictCell` lets a
//! long-lived builder (`rtu`/`ascii` `ClientBuilder`/`ServerBuilder`/`MonitorBuilder`,
//! constructed once) receive its checker *after* construction — the owning session-level
//! registry (`ferrowl::module::modbus::SerialPathRegistry`, a higher crate) is only known
//! once the module joins an `App`/headless session, which can be after the builder itself
//! was built.

use parking_lot::RwLock;
use std::sync::Arc;

/// Checked once per connect attempt, immediately before the OS-level serial-port open, with
/// the freshly `~`-expanded path for that attempt (MB-R-056: settings, including the path,
/// are re-read fresh every attempt). Returns the name of another module instance in the same
/// session currently claiming that same path, if any — `None` means proceed with the open.
pub type PathConflictCheck = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Shared, late-bindable cell holding a [`PathConflictCheck`], defaulting to "never
/// conflicts" — a builder with no checker attached (e.g. constructed and driven standalone,
/// outside any session, as most existing unit tests already do) behaves exactly as before
/// this feature. `set` may be called at any time, including after the owning task has
/// already been spawned: the attempt loop re-reads this cell fresh on every attempt.
#[derive(Clone, Default)]
pub struct PathConflictCell(Arc<RwLock<Option<PathConflictCheck>>>);

impl PathConflictCell {
    /// Attach (or replace) the checker.
    pub fn set(&self, check: PathConflictCheck) {
        *self.0.write() = Some(check);
    }

    /// Runs the currently-attached checker (if any) against `path`. `None` (no checker
    /// attached, or the checker itself found no conflict) means proceed with the open.
    pub fn check(&self, path: &str) -> Option<String> {
        self.0.read().as_ref().and_then(|f| f(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MB-R-150 — a cell with nothing attached never reports a conflict (the pre-feature
    /// behavior for a builder used standalone).
    #[test]
    fn ut_path_conflict_cell_default_never_conflicts() {
        let cell = PathConflictCell::default();
        assert_eq!(cell.check("/dev/ttyUSB0"), None);
    }

    /// MB-R-150 — after `set`, the cell runs the attached closure and returns its result.
    #[test]
    fn ut_path_conflict_cell_set_then_check_runs_closure() {
        let cell = PathConflictCell::default();
        cell.set(Arc::new(|path: &str| {
            (path == "/dev/ttyUSB0").then(|| "other-module".to_string())
        }));
        assert_eq!(cell.check("/dev/ttyUSB0"), Some("other-module".to_string()));
        assert_eq!(cell.check("/dev/ttyUSB1"), None);
    }

    /// MB-R-150 — clones share the same underlying cell: `set` on one clone is visible to
    /// `check` on another (the property the late-binding design depends on — a builder hands
    /// out a clone via a getter, the owner attaches later on that same clone).
    #[test]
    fn ut_path_conflict_cell_clone_shares_state() {
        let cell = PathConflictCell::default();
        let clone = cell.clone();
        clone.set(Arc::new(|_: &str| Some("x".to_string())));
        assert_eq!(cell.check("/any/path"), Some("x".to_string()));
    }

    /// MB-R-150 — `set` called a second time replaces the checker outright (used when a
    /// module's session registry itself is swapped, e.g. `App` startup attaching the real
    /// registry over a module's default empty one).
    #[test]
    fn ut_path_conflict_cell_set_replaces_previous_checker() {
        let cell = PathConflictCell::default();
        cell.set(Arc::new(|_: &str| Some("first".to_string())));
        cell.set(Arc::new(|_: &str| Some("second".to_string())));
        assert_eq!(cell.check("/dev/ttyUSB0"), Some("second".to_string()));
    }
}
