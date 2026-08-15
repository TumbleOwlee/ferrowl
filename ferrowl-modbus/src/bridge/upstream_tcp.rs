use crate::bridge::service::BridgeService;
use crate::tcp::Config;
use crate::tcp::tls::build_server_tls_config;
use crate::{Error, LogFn, TcpError};
use rust_modbus::{
    ClientFraming, ClientTransport, Server as ModbusServer, Service, TcpListener, TlsListener,
};
use std::net::SocketAddr;
use tokio::task::JoinHandle;

/// Bind the configured upstream TCP address (BR-R-005 — upstream acts as an ordinary server)
/// and spawn the accept loop, answering every connection via `service`. Plain TCP unless
/// `config.tls` is set (BR-R-011), mirroring `tcp/server.rs::run`'s bind/TLS shape exactly
/// (duplicated rather than shared: the store-based server and the bridge server share no
/// state type to factor a common helper around).
#[allow(dead_code)] // wired into bridge::run in a later stage
pub(crate) async fn run<S, F, L>(
    config: &Config,
    service: BridgeService<S, F, L>,
    log: L,
) -> Result<JoinHandle<Result<(), Error>>, Error>
where
    S: ClientTransport<F> + Send + Sync + 'static,
    F: ClientFraming + Send + Sync + 'static,
    L: LogFn + Clone + Send + Sync + 'static,
    BridgeService<S, F, L>: Service,
{
    let addr: SocketAddr = format!("{}:{}", config.ip, config.port)
        .parse()
        .map_err(|e| Error::Tcp(TcpError::Address(e)))?;
    let server = ModbusServer::new(service);
    match &config.tls {
        None => match TcpListener::bind(addr).await {
            Ok(listener) => Ok(tokio::task::spawn(async move {
                server.serve(listener).await.map_err(Error::Server)
            })),
            Err(e) => Err(Error::Server(e)),
        },
        Some(tls) => {
            let (tls_config, used_fallback) =
                build_server_tls_config(tls, &config.ip).map_err(Error::Tcp)?;
            if used_fallback {
                log.invoke(
                    "No cert_file/key_file/self_signed configured for this TLS server; \
                     falling back to an ephemeral self-signed certificate."
                        .to_string(),
                )
                .await;
            }
            match TlsListener::bind(addr, tls_config).await {
                Ok(listener) => Ok(tokio::task::spawn(async move {
                    server
                        .serve_tls::<rust_modbus::Tcp>(listener)
                        .await
                        .map_err(Error::Server)
                })),
                Err(e) => Err(Error::Server(e)),
            }
        }
    }
}

// This stage's own tests live as crate-internal unit tests, not `tests/` integration tests:
// both `upstream_tcp::run` and `BridgeService` are `pub(crate)` (the plan's own stated
// intent — "never part of the crate's public surface", only `bridge::run`, added in a later
// stage, constructs/calls them) and so are unreachable from an external `tests/` crate. Real
// TCP loopback (ephemeral ports on both sides) is still exercised, just from inside the crate.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::spawn_tcp_downstream;
    use ferrowl_codec::Kind as RegKind;
    use ferrowl_store::{CellKind as MemKind, CellType, Memory, Range};
    use parking_lot::RwLock as MemLock;
    use rust_modbus::{Address, Client as RmClient, FrameTransport, Quantity, RegisterValue};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::RwLock as TokioRwLock;

    fn sink() -> impl LogFn + Clone {
        |_s: String| async move {}
    }

    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    fn config(port: u16) -> Config {
        Config {
            ip: "127.0.0.1".to_string(),
            port,
            timeout_ms: 1000,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
            tls: None,
        }
    }

    fn key(kind: RegKind) -> crate::Key<crate::SlaveKey> {
        crate::Key::new(crate::SlaveKey {
            slave_id: rust_modbus::UnitId(1),
            kind,
        })
    }

    /// BR-R-013 — an upstream bind failure (address already in use) is reported as an error,
    /// not silently swallowed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn it_upstream_tcp_bind_failure_is_reported() {
        let port = free_port();
        let _occupier = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();

        let downstream_port = free_port();
        let downstream = spawn_tcp_downstream(config(downstream_port), sink());
        let service = BridgeService::new(downstream.into_handle(), None, sink());

        let res = run(&config(port), service, sink()).await;
        assert!(res.is_err());
    }

    /// BR-R-005, BR-R-007 — a real upstream TCP server relays a request through to a real
    /// downstream TCP server and returns its answer unmodified.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn it_upstream_tcp_relays_a_real_request_end_to_end() {
        let downstream_port = free_port();
        let mut mem = Memory::<crate::Key<crate::SlaveKey>>::default();
        mem.add_ranges(
            key(RegKind::HoldingRegister),
            &MemKind::ReadWrite(CellType::Register),
            &[Range::new(0, 4)],
        );
        mem.write(
            key(RegKind::HoldingRegister),
            &CellType::Register,
            &Range::new(0, 1),
            &[55],
        )
        .unwrap();
        let srv_mem = Arc::new(MemLock::new(mem));
        let _downstream_server = crate::tcp::ServerBuilder::new(
            Arc::new(TokioRwLock::new(config(downstream_port))),
            srv_mem,
        )
        .spawn(sink())
        .await
        .expect("downstream server failed to start");

        let downstream = spawn_tcp_downstream(config(downstream_port), sink());
        tokio::time::sleep(Duration::from_millis(50)).await;

        let upstream_port = free_port();
        let service = BridgeService::new(downstream.into_handle(), None, sink());
        let _upstream = run(&config(upstream_port), service, sink())
            .await
            .expect("upstream failed to start");
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut client: RmClient<_, rust_modbus::Tcp> = RmClient::new(FrameTransport::new(
            tokio::net::TcpStream::connect(("127.0.0.1", upstream_port))
                .await
                .expect("client connects"),
        ));
        let registers = client
            .read_holding_registers(rust_modbus::UnitId(1), Address(0), Quantity(1))
            .await
            .unwrap();
        assert_eq!(registers, vec![RegisterValue(55)]);
    }
}
