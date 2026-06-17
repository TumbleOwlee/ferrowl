//! Async logging callback used by the CS/CSMS core loops.

use std::future::Future;

/// Async logging callback used by the CS/CSMS core loops.
///
/// Mirrors `ferrowl-modbus`'s `LogFn` idiom verbatim: a stable RPITIT trait blanket-implemented
/// for any `Fn(String) -> impl Future<Output = ()> + Send`, so call sites pass an ordinary
/// closure returning an async block, e.g. `move |s| async move { ... }`. No `async-trait`
/// dependency.
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
