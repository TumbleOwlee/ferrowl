//! Runtime configuration for a client's poll loop.

/// Configuration passed to a client's `run` loop, bundling the logging and status
/// callbacks together with the polling timings.
pub struct RunConfig<L, S> {
    pub log: L,
    pub status: S,
    pub timeout_ms: usize,
    pub delay_ms: usize,
    pub interval_ms: usize,
}
