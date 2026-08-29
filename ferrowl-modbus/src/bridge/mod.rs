//! Modbus relay ("bridge") mode: BR-R-001..BR-R-015.
mod config;
mod downstream;
mod downstream_ascii_over_tcp;
mod downstream_rtu;
mod downstream_rtu_over_tcp;
mod downstream_tcp;
mod downstream_tcp_family;
mod service;
mod upstream_rtu;
mod upstream_tcp;
pub use config::{BridgeEndpointKind, BridgeEndpointSpec, UnitIdFilter};
pub use downstream::{DownstreamHandle, ERROR_PREFIX};
pub use downstream_ascii_over_tcp::{
    AsciiOverTcpDownstream, spawn as spawn_ascii_over_tcp_downstream,
};
pub use downstream_rtu::{RtuDownstream, spawn as spawn_rtu_downstream};
pub use downstream_rtu_over_tcp::{RtuOverTcpDownstream, spawn as spawn_rtu_over_tcp_downstream};
pub use downstream_tcp::{TcpDownstream, spawn as spawn_tcp_downstream};

use crate::{Error, LogFn};
use tokio::task::JoinHandle;

/// One bridge's worth of configuration: an upstream (server-facing) endpoint and a
/// downstream (client-facing) endpoint, each independently TCP or RTU (BR-R-004).
pub struct BridgeConfig {
    pub upstream: BridgeEndpointSpec,
    pub downstream: BridgeEndpointSpec,
}

/// BR-R-002 — the whole of bridge mode: no TUI, no session file, no Lua/`C_*`/sim
/// framework, no register store of its own. Spawns the downstream link (BR-R-006), builds
/// the relay `Service`, and spawns the upstream link (BR-R-005) over whichever of the two
/// independent transport axes `config` selects — `Tcp`/`Rtu`/`RtuOverTcp`/`AsciiOverTcp` on
/// each side (BR-R-004).
pub async fn run<L>(config: BridgeConfig, log: L) -> Result<JoinHandle<Result<(), Error>>, Error>
where
    L: LogFn + Clone + Send + Sync + 'static,
{
    use crate::bridge::config::BridgeEndpointKind::{AsciiOverTcp, Rtu, RtuOverTcp, Tcp};
    // BR-R-015 — downstream.unit_ids is never read.
    let unit_filter = config.upstream.unit_ids.clone();

    // Builds the downstream link and the `BridgeService` relaying onto it, then hands the
    // service to the given upstream continuation. A macro rather than a helper function
    // returning `BridgeService`: the four downstream kinds produce four different concrete
    // `BridgeService<S, F, L>` instantiations (`service.rs`'s per-transport `Service` impls
    // are not generic over `S, F`), so a single `match` expression can't unify them into one
    // return type — each downstream arm must call onward to the upstream runner itself.
    macro_rules! with_downstream_service {
        ($down:expr, |$service:ident| $upstream_call:expr) => {
            match $down {
                Tcp(down) => {
                    let downstream = downstream_tcp::spawn(down, log.clone());
                    let $service = service::BridgeService::new(
                        downstream.into_handle(),
                        unit_filter,
                        log.clone(),
                    );
                    $upstream_call
                }
                Rtu(down) => {
                    let downstream = downstream_rtu::spawn(down, log.clone());
                    let $service =
                        service::BridgeService::new(downstream, unit_filter, log.clone());
                    $upstream_call
                }
                RtuOverTcp(down) => {
                    let downstream = downstream_rtu_over_tcp::spawn(down, log.clone());
                    let $service = service::BridgeService::new(
                        downstream.into_handle(),
                        unit_filter,
                        log.clone(),
                    );
                    $upstream_call
                }
                AsciiOverTcp(down) => {
                    let downstream = downstream_ascii_over_tcp::spawn(down, log.clone());
                    let $service = service::BridgeService::new(
                        downstream.into_handle(),
                        unit_filter,
                        log.clone(),
                    );
                    $upstream_call
                }
            }
        };
    }

    match config.upstream.kind {
        Tcp(up) => {
            with_downstream_service!(config.downstream.kind, |service| upstream_tcp::run(
                &up, service, log
            )
            .await)
        }
        Rtu(up) => {
            with_downstream_service!(config.downstream.kind, |service| upstream_rtu::run(
                &up, service
            )
            .await)
        }
        RtuOverTcp(up) => {
            with_downstream_service!(config.downstream.kind, |service| {
                upstream_tcp::run_framed::<rust_modbus::RtuOverTcp, _, _, _>(&up, service, log)
                    .await
            })
        }
        AsciiOverTcp(up) => {
            with_downstream_service!(config.downstream.kind, |service| {
                upstream_tcp::run_framed::<rust_modbus::Ascii, _, _, _>(&up, service, log).await
            })
        }
    }
}

// `it_bridge_run_wires_tcp_upstream_tcp_downstream` (`tests/bridge_run.rs`) exercises the
// (Tcp, Tcp) arm above end-to-end through this module's public `run`. Every other transport
// combination involves RTU, whose downstream/upstream types are hard-pinned to a real
// `SerialStream` (no serial hardware available in a test environment, matching
// `rtu_serial.rs`'s existing convention of only exercising the open-failure path there) —
// real hardware is required to drive `downstream_rtu::spawn`/`upstream_rtu::run` for a real
// exchange, so those combinations' service/type wiring is proven here instead, one layer
// below `bridge::run`'s own dispatch, over an in-memory duplex link standing in for the
// serial link (same approach `upstream_tcp`/`upstream_rtu`'s own tests already use).
// `service::BridgeService` and `upstream_tcp::run`/`upstream_rtu::run` are `pub(crate)`,
// unreachable from an external `tests/` crate, so these live as crate-internal unit tests.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::service::BridgeService;
    use ferrowl_codec::Kind as RegKind;
    use ferrowl_store::{CellKind, CellType, Memory, Range};
    use parking_lot::RwLock as MemLock;
    use rust_modbus::{
        Address, Client as RmClient, FrameTransport, Quantity, RegisterValue, Rtu as RtuFraming,
        Server as ModbusServer, UnitId,
    };
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::DuplexStream;
    use tokio::sync::RwLock as TokioRwLock;
    use tokio::sync::mpsc;

    fn sink() -> impl LogFn + Clone {
        |_s: String| async move {}
    }

    /// Polls a `ServerBuilder::spawn`-returned `BoundAddr` until the listener actually binds,
    /// instead of racing it with a fixed sleep (MB-R-130 companion — `spawn()` only guarantees
    /// the task was scheduled, not that its first bind attempt has run).
    async fn wait_bound_addr(bound_addr: &crate::server_core::BoundAddr) {
        for _ in 0..50 {
            if bound_addr.lock().is_some() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("listener did not bind within 1s");
    }

    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    fn tcp_config(port: u16) -> crate::tcp::Config {
        crate::tcp::Config {
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
            slave_id: UnitId(1),
            kind,
        })
    }

    /// A `DownstreamHandle` standing in for an RTU downstream link, over a duplex pair whose
    /// peer answers with a fixed value — the same shape `downstream_rtu::spawn` would build
    /// against a real serial port.
    fn duplex_downstream(
        expected: RegisterValue,
        log: impl LogFn + Clone + 'static,
    ) -> DownstreamHandle<FrameTransport<DuplexStream, RtuFraming>, RtuFraming> {
        let (client_end, mut peer) = tokio::io::duplex(256);
        let mut client_end = Some(client_end);
        let handle = DownstreamHandle::spawn(
            move || {
                let client_end = client_end.take();
                async move {
                    Ok(rust_modbus::Client::new(
                        FrameTransport::<_, RtuFraming>::new(
                            client_end.expect("connect called once"),
                        ),
                    ))
                }
            },
            true,
            log,
        );
        tokio::spawn(async move {
            use rust_modbus::Framing;
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buf = [0u8; 64];
            loop {
                match peer.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let (header, _req) = RtuFraming::decode_request(&buf[..n]).unwrap();
                        let response = rust_modbus::ResponsePdu::ReadHoldingRegisters {
                            registers: vec![expected],
                        };
                        let frame = RtuFraming::encode_response(&header, &response).unwrap();
                        if peer.write_all(&frame).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });
        handle
    }

    /// BR-R-002, BR-R-005, BR-R-006 — the (Tcp upstream, Rtu downstream) wiring shape: a
    /// real TCP upstream server (`upstream_tcp::run`, same as `bridge::run`'s Tcp/Rtu arm
    /// calls) relays through to an RTU-shaped downstream (duplex standing in for serial).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn it_bridge_run_wires_tcp_upstream_rtu_downstream() {
        let downstream = duplex_downstream(RegisterValue(11), sink());
        let service = BridgeService::new(downstream, None, sink());

        let upstream_port = free_port();
        let _upstream = upstream_tcp::run(&tcp_config(upstream_port), service, sink())
            .await
            .expect("upstream failed to start");
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut client: RmClient<_, rust_modbus::Tcp> = RmClient::new(FrameTransport::new(
            tokio::net::TcpStream::connect(("127.0.0.1", upstream_port))
                .await
                .expect("client connects"),
        ));
        let registers = client
            .read_holding_registers(UnitId(1), Address(0), Quantity(1))
            .await
            .unwrap();
        assert_eq!(registers, vec![RegisterValue(11)]);
    }

    /// BR-R-002, BR-R-005, BR-R-006 — the (Rtu upstream, Tcp downstream) wiring shape: a
    /// real TCP downstream server (via `downstream_tcp::spawn`, same as `bridge::run`'s
    /// Rtu/Tcp arm calls) is relayed to over an RTU-shaped upstream link (duplex standing in
    /// for serial, served directly via `serve_link` as `upstream_rtu::run` itself would).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn it_bridge_run_wires_rtu_upstream_tcp_downstream() {
        let downstream_port = free_port();
        let mut mem = Memory::<crate::Key<crate::SlaveKey>>::default();
        mem.add_ranges(
            key(RegKind::HoldingRegister),
            &CellKind::read_write(CellType::Register),
            &[Range::new(0, 4)],
        );
        mem.write(
            key(RegKind::HoldingRegister),
            &CellType::Register,
            &Range::new(0, 1),
            &[22],
        )
        .unwrap();
        let srv_mem = Arc::new(MemLock::new(mem));
        let (_srv_tx, srv_rx) = mpsc::channel::<crate::ServerCommand>(1);
        let (_downstream_server, downstream_bound_addr) = crate::tcp::ServerBuilder::new(
            Arc::new(TokioRwLock::new(tcp_config(downstream_port))),
            srv_mem,
            crate::tcp::tls::new_self_signed_cache(),
        )
        .spawn(srv_rx, sink(), sink())
        .await
        .expect("downstream server failed to start");
        wait_bound_addr(&downstream_bound_addr).await;

        let downstream = downstream_tcp::spawn(tcp_config(downstream_port), sink());
        tokio::time::sleep(Duration::from_millis(50)).await;
        let service = BridgeService::new(downstream.into_handle(), None, sink());

        let (server_end, client_end) = tokio::io::duplex(256);
        let modbus = ModbusServer::new(service);
        let handle = modbus.handle();
        let serving =
            tokio::spawn(modbus.serve_link(FrameTransport::<_, RtuFraming>::new(server_end)));

        let mut client: RmClient<_, RtuFraming> = RmClient::new(FrameTransport::new(client_end));
        let registers = client
            .read_holding_registers(UnitId(1), Address(0), Quantity(1))
            .await
            .unwrap();
        assert_eq!(registers, vec![RegisterValue(22)]);

        handle.shutdown().await;
        let _ = serving.await;
    }

    /// BR-R-002, BR-R-005, BR-R-006 — the (Rtu upstream, Rtu downstream) wiring shape: both
    /// links are RTU-shaped (duplex standing in for serial on each side), matching
    /// `bridge::run`'s Rtu/Rtu arm's construction exactly.
    #[tokio::test]
    async fn it_bridge_run_wires_rtu_upstream_rtu_downstream() {
        let downstream = duplex_downstream(RegisterValue(33), sink());
        let service = BridgeService::new(downstream, None, sink());

        let (server_end, client_end) = tokio::io::duplex(256);
        let modbus = ModbusServer::new(service);
        let handle = modbus.handle();
        let serving =
            tokio::spawn(modbus.serve_link(FrameTransport::<_, RtuFraming>::new(server_end)));

        let mut client: RmClient<_, RtuFraming> = RmClient::new(FrameTransport::new(client_end));
        let registers = client
            .read_holding_registers(UnitId(1), Address(0), Quantity(1))
            .await
            .unwrap();
        assert_eq!(registers, vec![RegisterValue(33)]);

        handle.shutdown().await;
        let _ = serving.await;
    }
}
