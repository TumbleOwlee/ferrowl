//! Join handles for running instance tasks.

use std::net::SocketAddr;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;

/// Handle of a running client task plus the channel its commands go through.
///
/// `connected` (MB-R-137) is the "transport currently connected" signal every client transport's
/// `ClientBuilder::spawn` now returns alongside the task handle: `true` right after a connect
/// attempt succeeds, `false` again once that connection's run loop ends (graceful terminate,
/// transport error, or timeout) — never touched during a backoff wait.
pub struct ClientHandle {
    pub handle: JoinHandle<Result<(), ferrowl_modbus::Error>>,
    pub sender: Sender<ferrowl_modbus::Command>,
    pub connected: ferrowl_modbus::ConnectedCell,
}

/// Handle of a running server task plus the channel its commands go through
/// (MB-R-133).
///
/// `bound_addr` is the ready signal `ferrowl_modbus`'s Tcp/RtuOverTcp/AsciiOverTcp/Udp
/// `ServerBuilder::spawn` now returns alongside the task handle: `None` until the listener
/// actually binds (`start()` returning only means the task was scheduled, not that its first
/// bind attempt has run), `Some(<real addr>)` once bound, `None` again if the serve loop ends.
/// A pure-serial (Rtu/Ascii) server has no socket to report, so its `start()` arm fills this
/// with an `Arc` that is never written to — `Instance::bound_addr()` then correctly reads back
/// `None` for it, same as "never bound yet."
///
/// `open` (MB-R-153) is the "serial port open" signal the Rtu/Ascii `ServerBuilder::spawn` arms
/// return alongside the task handle; the TCP-family arms leave it at its `Default` (`false`,
/// never read for those roles since `bound_addr` is authoritative there) so
/// `Instance::connection_status()` has one formula for every server transport.
pub struct ServerHandle {
    pub handle: JoinHandle<Result<(), ferrowl_modbus::Error>>,
    pub sender: Sender<ferrowl_modbus::ServerCommand>,
    pub bound_addr: Arc<Mutex<Option<SocketAddr>>>,
    pub open: ferrowl_modbus::ConnectedCell,
}

/// Handle of a running instance, by role.
pub enum Handle {
    Server(ServerHandle),
    Client(ClientHandle),
}

impl Handle {
    pub fn is_finished(&self) -> bool {
        match self {
            Handle::Server(h) => h.handle.is_finished(),
            Handle::Client(h) => h.handle.is_finished(),
        }
    }
}
