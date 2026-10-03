//! NF-R-045 — dev-only fixtures for tests that need a held network port,
//! per-run scratch directory, or bounded polling for a condition a spawned
//! task will reach. Consumed only as a `dev-dependency`; no
//! production code depends on this crate.

mod port;
mod temp;
mod wait;

pub use port::{TcpPortGuard, UdpPortGuard, reserve_tcp_port, reserve_udp_port};
pub use temp::{TempDirGuard, reserve_temp_dir};
pub use wait::{wait_until, wait_until_async, wait_until_blocking};
