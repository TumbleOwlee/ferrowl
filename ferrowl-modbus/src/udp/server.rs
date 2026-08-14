use crate::server_core::Server;
use crate::udp::Config;
use crate::{Error, Key, KeyParams, LogFn, TcpError};

use ferrowl_store::Memory;
use parking_lot::RwLock as MemLock;
use rust_modbus::Server as ModbusServer;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

/// Builds and spawns a Modbus UDP server task answering requests from the
/// shared `memory`.
pub struct ServerBuilder<T: KeyParams> {
    config: Arc<RwLock<Config>>,
    memory: Arc<MemLock<Memory<Key<T>>>>,
}

impl<T: KeyParams> ServerBuilder<T> {
    pub fn new(config: Arc<RwLock<Config>>, memory: Arc<MemLock<Memory<Key<T>>>>) -> Self {
        Self { config, memory }
    }

    /// Binds the configured address and spawns `serve_udp` as a tokio task. `log`
    /// receives log lines.
    pub async fn spawn<L>(&self, log: L) -> Result<JoinHandle<Result<(), Error>>, Error>
    where
        L: LogFn + Clone,
    {
        let guard = self.config.read().await;
        run(&guard, self.memory.clone(), log).await
    }
}

/// Every production server logs per-request outcomes (MB-R-067); UDP is no exception.
const VERBOSE: bool = true;

/// MB-R-128 — this transport is never physical Rtu/Ascii serial (RtuOverTcp/AsciiOverTcp ride
/// TCP; Tcp and Udp have no serial concept at all): an unmapped slave id keeps the ordinary
/// exception, same as MB-R-065/MB-R-060.
const PHYSICAL_SERIAL: bool = false;

/// Bind the configured UDP address and spawn `serve_udp` (MB-R-119); each datagram answers
/// from the shared `memory` via a [`Server`] (verbose logging on, MB-R-067, mirroring the TCP
/// server). No accept loop, no TLS branch (MB-R-116 — `udp::Config` carries no `tls` field at
/// all).
async fn run<T, L>(
    config: &Config,
    memory: Arc<MemLock<Memory<Key<T>>>>,
    log: L,
) -> Result<JoinHandle<Result<(), Error>>, Error>
where
    T: KeyParams,
    L: LogFn + Clone,
{
    let addr: SocketAddr = format!("{}:{}", config.ip, config.port)
        .parse()
        .map_err(|e| Error::Tcp(TcpError::Address(e)))?;
    let server = ModbusServer::new(Server::new(memory, log.clone(), VERBOSE, PHYSICAL_SERIAL));
    match UdpSocket::bind(addr).await {
        Ok(socket) => Ok(tokio::task::spawn(async move {
            server.serve_udp(socket).await.map_err(Error::Server)
        })),
        // MB-R-120 — a bind failure surfaces as an error from `spawn`, not retried.
        Err(e) => Err(Error::Server(e.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::{PHYSICAL_SERIAL, VERBOSE};

    /// MB-R-067 — the UDP server logs per-request outcomes exactly like every other transport.
    #[test]
    fn ut_udp_server_is_verbose() {
        assert!(VERBOSE);
    }

    /// MB-R-128 — Udp is never physical-serial: an unmapped slave id keeps the ordinary
    /// exception.
    #[test]
    fn ut_udp_server_is_not_physical_serial() {
        assert!(!PHYSICAL_SERIAL);
    }
}
