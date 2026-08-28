use crate::bridge::downstream_tcp_family::{TcpFamilyDownstream, spawn as spawn_family};
use crate::{LogFn, tcp};
use rust_modbus::Tcp;

/// A `DownstreamHandle` connected over plain TCP framing (BR-R-004), see
/// `TcpFamilyDownstream` (`downstream_tcp_family.rs`) for the shared implementation.
pub type TcpDownstream = TcpFamilyDownstream<Tcp>;

/// BR-R-006 — the downstream TCP interface always acts as an ordinary client, including
/// TLS (BR-R-011) and reconnect/backoff (`config.reconnect`, MB-R-050–056).
pub fn spawn(config: tcp::Config, log: impl LogFn + Clone + 'static) -> TcpDownstream {
    spawn_family::<Tcp>(config, log)
}
