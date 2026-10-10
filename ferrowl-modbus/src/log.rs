//! Async logging callback used by the client/server loops.

use std::future::Future;

/// MB-R-258 — severity of one log line, chosen where it is emitted, never derived from the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
}

/// Async logging callback used by the client/server loops.
///
/// Stands in for an `AsyncFn(Level, String) -> ()` bound, whose `for<'a> L::CallRefFuture<'a>:
/// Send` clause needs the nightly `async_fn_traits` feature. Blanket-implemented for any
/// `Fn(Level, String) -> impl Future<Output = ()> + Send`, so call sites pass an ordinary closure
/// returning an async block, e.g. `move |level, s| async move { ... }`.
pub trait LogFn: Send + Sync + 'static {
    fn invoke(&self, level: Level, msg: String) -> impl Future<Output = ()> + Send;
}

impl<F, Fut> LogFn for F
where
    F: Fn(Level, String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send,
{
    fn invoke(&self, level: Level, msg: String) -> impl Future<Output = ()> + Send {
        self(level, msg)
    }
}
