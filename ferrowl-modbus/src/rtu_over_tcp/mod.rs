//! Modbus RTU-over-TCP: RTU framing carried over a TCP socket (MB-R-113..MB-R-115).
//! Reuses `tcp::Config` verbatim — same connect/bind/TLS behavior as plain TCP,
//! differing only in on-wire framing (RTU ADU: unit id + CRC, no MBAP header).

mod client;
mod server;

pub use client::{Client, ClientBuilder};
pub use server::ServerBuilder;
