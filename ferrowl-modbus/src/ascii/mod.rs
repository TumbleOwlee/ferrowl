//! Modbus ASCII (serial) client and server: `:` start character, hexadecimal-encoded PDU,
//! LRC checksum, CR LF terminator (MB-R-121..MB-R-124). Reuses `rtu::Config` verbatim — same
//! serial connect/open behavior as RTU (MB-R-072/073), differing only in on-wire framing.

mod client;
mod server;

pub use client::{Client, ClientBuilder};
pub use server::ServerBuilder;
