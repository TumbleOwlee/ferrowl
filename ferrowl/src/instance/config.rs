use ferrowl_mem::Memory;
use ferrowl_net::KeyParams;

use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct ClientConfig<T: KeyParams, Config> {
    pub config: Arc<RwLock<Config>>,
    pub operations: Arc<RwLock<Vec<ferrowl_net::Operation>>>,
    pub memory: Arc<RwLock<Memory<ferrowl_net::Key<T>>>>,
}

#[derive(Clone)]
pub struct ServerConfig<T: KeyParams, Config> {
    pub config: Arc<RwLock<Config>>,
    pub memory: Arc<RwLock<Memory<ferrowl_net::Key<T>>>>,
}
