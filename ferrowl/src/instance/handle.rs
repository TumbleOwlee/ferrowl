//! Join handles for running instance tasks.

use std::net::SocketAddr;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;

/// Handle of a running client task plus the channel its commands go through.
pub struct ClientHandle {
    pub handle: JoinHandle<Result<(), ferrowl_modbus::Error>>,
    pub sender: Sender<ferrowl_modbus::Command>,
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
pub struct ServerHandle {
    pub handle: JoinHandle<Result<(), ferrowl_modbus::Error>>,
    pub sender: Sender<ferrowl_modbus::ServerCommand>,
    pub bound_addr: Arc<Mutex<Option<SocketAddr>>>,
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
