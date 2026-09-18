//! Small general-purpose helpers shared across the ferrowl crates:
//! exponential-backoff retry driving ([`backoff`]), user-supplied filesystem path expansion
//! ([`path`]), and the shared TLS policy enums ([`tls`]).

pub mod backoff;
pub mod path;
pub mod tls;
