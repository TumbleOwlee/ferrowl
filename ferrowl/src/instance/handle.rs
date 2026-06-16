//! Join handles for running instance tasks.

use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;

/// Handle of a running client task plus the channel its commands go through.
pub struct ClientHandle {
    pub handle: JoinHandle<Result<(), ferrowl_modbus::Error>>,
    pub sender: Sender<ferrowl_modbus::Command>,
}

/// Handle of a running server task.
pub struct ServerHandle {
    pub handle: JoinHandle<Result<(), ferrowl_modbus::Error>>,
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
