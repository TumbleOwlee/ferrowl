use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;

pub struct ClientHandle {
    pub handle: JoinHandle<Result<(), ferrowl_net::Error>>,
    pub sender: Sender<ferrowl_net::Command>,
}

pub struct ServerHandle {
    pub handle: JoinHandle<Result<(), ferrowl_net::Error>>,
}

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
