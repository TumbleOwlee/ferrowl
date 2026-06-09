//! Shared-state bundles handed to an [`Instance`](crate::instance::Instance)
//! constructor.

use ferrowl_mem::Memory;
use ferrowl_net::KeyParams;

use std::sync::Arc;
use tokio::sync::RwLock;

/// Shared state for a client instance: transport `Config`, the poll
/// operations, and the register memory.
#[derive(Clone)]
pub struct ClientConfig<T: KeyParams, Config> {
    pub config: Arc<RwLock<Config>>,
    pub operations: Arc<RwLock<Vec<ferrowl_net::Operation>>>,
    pub memory: Arc<RwLock<Memory<ferrowl_net::Key<T>>>>,
}

/// Shared state for a server instance: transport `Config` and the register
/// memory requests are answered from.
#[derive(Clone)]
pub struct ServerConfig<T: KeyParams, Config> {
    pub config: Arc<RwLock<Config>>,
    pub memory: Arc<RwLock<Memory<ferrowl_net::Key<T>>>>,
}
