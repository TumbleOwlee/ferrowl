//! Identity of the script running on the current sim thread.

use std::cell::RefCell;
use std::sync::Arc;

/// Which sim a script belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ScriptOwner {
    Module,
    Session,
}

/// A script's owner and name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScriptId {
    pub owner: ScriptOwner,
    pub name: Arc<str>,
}

thread_local! {
    static EXECUTING: RefCell<Option<ScriptId>> = const { RefCell::new(None) };
}

/// Marks `id` as the executing script of this thread until dropped, then restores the previous
/// one.
pub struct ExecutionGuard {
    previous: Option<ScriptId>,
}

impl ExecutionGuard {
    pub fn enter(id: ScriptId) -> Self {
        let previous = EXECUTING.with(|cell| cell.borrow_mut().replace(id));
        Self { previous }
    }
}

impl Drop for ExecutionGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        EXECUTING.with(|cell| *cell.borrow_mut() = previous);
    }
}

/// The script running on this thread, if any.
pub fn executing_script() -> Option<ScriptId> {
    EXECUTING.with(|cell| cell.borrow().clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> ScriptId {
        ScriptId {
            owner: ScriptOwner::Module,
            name: name.into(),
        }
    }

    #[test]
    fn ut_guard_restores_previous_on_drop() {
        assert_eq!(executing_script(), None);
        let outer = ExecutionGuard::enter(id("outer"));
        assert_eq!(executing_script(), Some(id("outer")));
        {
            let _inner = ExecutionGuard::enter(id("inner"));
            assert_eq!(executing_script(), Some(id("inner")));
        }
        assert_eq!(executing_script(), Some(id("outer")));
        drop(outer);
        assert_eq!(executing_script(), None);
    }
}
