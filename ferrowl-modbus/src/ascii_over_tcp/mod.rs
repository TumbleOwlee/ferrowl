//! Modbus ASCII-over-TCP: ASCII framing carried over a TCP socket (MB-R-125..MB-R-127).
//! Reuses `tcp::Config` verbatim — same connect/bind/TLS behavior as plain TCP and
//! RtuOverTcp, differing only in on-wire framing (Modbus ASCII: `:` start, hex PDU, LRC,
//! CR LF terminator). Unlike RTU, ASCII framing is self-delimiting (`:`/CR LF), so it needs
//! no distinct "-over-stream" framing marker the way RTU needs `RtuOverTcp` — this module
//! uses rust_modbus's `Ascii` framing type directly, the same one the `ascii` (serial)
//! module uses, over `connect_tcp_framed::<Ascii>` / `serve_framed::<Ascii>` /
//! `serve_tls::<Ascii>` instead of `open_serial::<Ascii>`.

mod client;
mod server;

pub use client::{Client, ClientBuilder};
pub use server::ServerBuilder;
