//! Scoped accessors for the `parking_lot::RwLock`-backed shared state (client `CsState`, server
//! `RfidStore`/`V::Cs`/`V::Conn`). Every guard is taken inside the closure passed here and dropped
//! when it returns — a guard can never outlive the call, which makes the reentrancy-deadlock class
//! (a read guard still held while the same thread takes a write guard) unrepresentable. parking_lot
//! is still non-reentrant: never call `with_state_mut` from inside a `with_state`/`with_state_mut`
//! closure over the *same* lock.

use std::sync::Arc;

use parking_lot::RwLock;

/// Run `f` with a read guard on `state`, dropping the guard before returning.
pub fn with_state<T, R>(state: &Arc<RwLock<T>>, f: impl FnOnce(&T) -> R) -> R {
    f(&state.read())
}

/// Run `f` with a write guard on `state`, dropping the guard before returning.
pub fn with_state_mut<T, R>(state: &Arc<RwLock<T>>, f: impl FnOnce(&mut T) -> R) -> R {
    f(&mut state.write())
}

/// A type holding a shared `Arc<RwLock<Self::State>>`, giving it the `with_state`/`with_state_mut`
/// scoped-guard accessors above for free from just a `state()` accessor.
pub(crate) trait HasState {
    type State;

    /// The shared state this type wraps.
    fn state(&self) -> &Arc<RwLock<Self::State>>;

    /// Run `f` with a read guard on the shared state, dropped before returning.
    fn with_state<R>(&self, f: impl FnOnce(&Self::State) -> R) -> R {
        with_state(self.state(), f)
    }

    /// Run `f` with a write guard on the shared state, dropped before returning.
    fn with_state_mut<R>(&self, f: impl FnOnce(&mut Self::State) -> R) -> R {
        with_state_mut(self.state(), f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The intended pattern: a `with_state` call's *result* (a plain value, not a guard) feeds a
    /// later, separate `with_state_mut` call over the same lock. This is the safe shape — the read
    /// guard from the first call is fully dropped before the second call ever asks for a write
    /// guard. Nesting the closures themselves (calling `with_state_mut` from *inside* a `with_state`
    /// closure over the same lock) would instead reproduce the fixed reentrancy deadlock; that
    /// shape must never appear in this codebase.
    #[test]
    fn ut_sequential_with_state_result_then_with_state_mut() {
        let state = Arc::new(RwLock::new(vec![1, 2, 3]));
        let sum: i32 = with_state(&state, |v| v.iter().sum());
        with_state_mut(&state, |v| v.push(sum));
        assert_eq!(with_state(&state, |v| v.clone()), vec![1, 2, 3, 6]);
    }
}
