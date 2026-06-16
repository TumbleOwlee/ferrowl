//! Async logging callback used by the client/server loops.

use std::future::Future;

/// Async logging callback used by the client/server loops.
///
/// Stable replacement for the previously used `AsyncFn(String) -> ()` bound plus its unstable
/// `for<'a> L::CallRefFuture<'a>: Send` clause (which required the nightly `async_fn_traits`
/// feature). Blanket-implemented for any `Fn(String) -> impl Future<Output = ()> + Send`, so call
/// sites pass an ordinary closure returning an async block, e.g. `move |s| async move { ... }`.
pub trait LogFn: Send + Sync + 'static {
    fn invoke(&self, msg: String) -> impl Future<Output = ()> + Send;
}

impl<F, Fut> LogFn for F
where
    F: Fn(String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send,
{
    fn invoke(&self, msg: String) -> impl Future<Output = ()> + Send {
        self(msg)
    }
}
