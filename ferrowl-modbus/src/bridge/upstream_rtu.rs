use crate::bridge::service::BridgeService;
use crate::common::serial_config_from;
use crate::rtu::Config;
use crate::{Error, LogFn, SerialError};
use rust_modbus::{
    ClientFraming, ClientTransport, Rtu, Server as ModbusServer, Service, open_serial,
};
use tokio::task::JoinHandle;

/// Open the configured upstream serial port and spawn the RTU serve loop, answering from
/// `service` (BR-R-005 — upstream acts as an ordinary server). One port, one link, no accept
/// loop, no reconnect (edge-cases.md: "Upstream RTU serial loss ends the bridge task with an
/// error; there is no reconnect for the upstream side").
#[allow(dead_code)] // wired into bridge::run in a later stage
pub(crate) async fn run<S, F, L>(
    config: &Config,
    service: BridgeService<S, F, L>,
) -> Result<JoinHandle<Result<(), Error>>, Error>
where
    S: ClientTransport<F> + Send + Sync + 'static,
    F: ClientFraming + Send + Sync + 'static,
    L: LogFn + Clone + Send + Sync + 'static,
    BridgeService<S, F, L>: Service,
{
    let serial = serial_config_from(
        config.baud_rate,
        config.data_bits,
        config.stop_bits,
        config.parity.as_deref(),
    )?;
    match open_serial::<Rtu>(&config.path, serial) {
        Ok(transport) => {
            let server = ModbusServer::new(service);
            Ok(tokio::task::spawn(async move {
                server.serve_link(transport).await.map_err(Error::Server)
            }))
        }
        Err(e) => Err(SerialError::Error(e).into()),
    }
}

// This stage's own tests live as crate-internal unit tests (see s6's upstream_tcp.rs for the
// same reasoning): `run` and `BridgeService` are both `pub(crate)`, unreachable from an
// external `tests/` crate. That includes the open-failure test below — it needs `run` and
// `BridgeService` directly, so it cannot live in `tests/rtu_serial.rs` either.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::downstream::DownstreamHandle;
    use rust_modbus::{
        Address, Client as RmClient, FrameTransport, Framing, Quantity, RegisterValue, ResponsePdu,
        UnitId,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

    fn sink() -> impl LogFn + Clone {
        |_s: String| async move {}
    }

    /// BR-R-008 — an upstream broadcast (RTU unit id 0) is forwarded downstream (fire and
    /// forget) and answered with silence on the upstream link: no stray response frame is
    /// left queued ahead of a follow-up, ordinary request.
    #[tokio::test]
    async fn ut_upstream_rtu_broadcast_request_forwarded_and_unanswered() {
        // The downstream link: a duplex pair whose peer notes the broadcast write it sees
        // (never answering it — nothing is awaited for a broadcast, BR-R-009/MB-R-102), then
        // answers a follow-up ordinary read with a fixed value.
        let (downstream_client_end, mut downstream_peer) = tokio::io::duplex(256);
        let mut downstream_client_end = Some(downstream_client_end);
        let downstream: DownstreamHandle<FrameTransport<DuplexStream, Rtu>, Rtu> =
            DownstreamHandle::spawn(
                move || {
                    let end = downstream_client_end.take();
                    async move {
                        Ok(rust_modbus::Client::new(FrameTransport::<_, Rtu>::new(
                            end.expect("connect called once"),
                        )))
                    }
                },
                true,
                sink(),
            );
        tokio::time::sleep(Duration::from_millis(20)).await;

        let downstream_reached = Arc::new(AtomicBool::new(false));
        {
            let downstream_reached = downstream_reached.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 64];
                // First message: the forwarded broadcast write. Note it, answer nothing.
                let Ok(n) = downstream_peer.read(&mut buf).await else {
                    return;
                };
                if n > 0 {
                    downstream_reached.store(true, Ordering::SeqCst);
                }
                // Second message: the follow-up ordinary read. Answer it for real.
                let expected = ResponsePdu::ReadHoldingRegisters {
                    registers: vec![RegisterValue(20)],
                };
                if let Ok(n) = downstream_peer.read(&mut buf).await {
                    let (header, _req) = Rtu::decode_request(&buf[..n]).unwrap();
                    let frame = Rtu::encode_response(&header, &expected).unwrap();
                    let _ = downstream_peer.write_all(&frame).await;
                }
            });
        }

        let service = BridgeService::new(downstream, None, sink());

        // The upstream link: a duplex pair served by BridgeService, driven by an ordinary
        // RmClient the same way `server_core.rs`'s own broadcast test drives its server.
        let (server_end, client_end) = tokio::io::duplex(256);
        let server = ModbusServer::new(service);
        let handle = server.handle();
        let serving = tokio::spawn(server.serve_link(FrameTransport::<_, Rtu>::new(server_end)));

        let mut client: RmClient<_, Rtu> = RmClient::new(FrameTransport::new(client_end));
        // Returns as soon as the frame is written — nothing is awaited, since nothing answers.
        client
            .write_single_register(UnitId(0), Address(1), RegisterValue(7))
            .await
            .expect("broadcast write is not itself an error");

        // Let the broadcast fully round-trip through the server/downstream forward before
        // issuing the follow-up: a raw in-memory duplex is a byte stream, not message-framed,
        // so two writes issued back-to-back without this gap could coalesce into one `read()`
        // on the downstream peer's side, which is a test-harness artifact, not something
        // BR-R-008 makes any claim about.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // The follow-up read lines up cleanly, proving the broadcast put no stray frame on
        // the upstream wire.
        let registers = client
            .read_holding_registers(UnitId(1), Address(0), Quantity(1))
            .await
            .unwrap();
        assert_eq!(registers, vec![RegisterValue(20)]);

        assert!(
            downstream_reached.load(Ordering::SeqCst),
            "the broadcast write must be forwarded downstream (BR-R-008)"
        );

        handle.shutdown().await;
        let _ = serving.await;
    }

    fn bad_config() -> Config {
        Config {
            path: "/nonexistent/ferrowl-no-such-serial-port".to_string(),
            baud_rate: 9600,
            slave: 1,
            data_bits: None,
            stop_bits: None,
            parity: None,
            timeout_ms: 1000,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
        }
    }

    /// BR-R-013 — an upstream serial open failure (nonexistent device path) is reported as an
    /// error, not silently swallowed; there is no reconnect for the upstream side
    /// (edge-cases.md).
    #[tokio::test]
    async fn it_upstream_rtu_open_failure_is_reported() {
        let downstream: DownstreamHandle<FrameTransport<DuplexStream, Rtu>, Rtu> =
            DownstreamHandle::spawn(
                || async {
                    std::future::pending::<
                        Result<rust_modbus::Client<FrameTransport<DuplexStream, Rtu>, Rtu>, Error>,
                    >()
                    .await
                },
                true,
                sink(),
            );
        let service = BridgeService::new(downstream, None, sink());

        let result = run(&bad_config(), service).await;
        assert!(result.is_err());
    }
}
