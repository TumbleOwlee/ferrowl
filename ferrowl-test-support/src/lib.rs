//! NF-R-045 — dev-only fixtures for tests that need a held network port or
//! per-run scratch directory. Consumed only as a `dev-dependency`; no
//! production code depends on this crate.

mod port;
mod temp;

pub use port::{TcpPortGuard, UdpPortGuard, reserve_tcp_port, reserve_udp_port};
pub use temp::{TempDirGuard, reserve_temp_dir};
