use crate::LogFn;
use crate::bridge::{UnitIdFilter, downstream::DownstreamHandle};
use crate::tcp::tls::ClientStream;
use rust_modbus::{
    ClientFraming, ClientTransport, Connection, ExceptionCode, FrameTransport, RequestPdu,
    ResponsePdu, Rtu, SerialStream, Service, Tcp, UnitId,
};

/// The upstream-facing `Service`: applies BR-R-015's filter, then forwards to the
/// downstream link and relays its answer verbatim (BR-R-007).
pub(crate) struct BridgeService<S, F, L> {
    downstream: DownstreamHandle<S, F>,
    unit_filter: Option<UnitIdFilter>,
    log: L,
}

impl<S, F, L> BridgeService<S, F, L> {
    pub(crate) fn new(
        downstream: DownstreamHandle<S, F>,
        unit_filter: Option<UnitIdFilter>,
        log: L,
    ) -> Self {
        Self {
            downstream,
            unit_filter,
            log,
        }
    }
}

// `Service::on_request`'s trait-declared return type is `impl Future<..> + Send`, and
// `rust_modbus::ClientTransport<F>`'s own methods return a bare `impl Future<..>` with no
// `+ Send` — so an `impl<S, F, L> Service for BridgeService<S, F, L>` block generic over an
// *arbitrary* `S: ClientTransport<F>` can never prove that bound abstractly (a trait impl's
// obligations are checked once, for all valid substitutions of its own generic parameters,
// unlike an ordinary generic function whose Send-ness is only checked lazily per call site).
// The forwarding logic itself stays generic (`forward_and_relay`, an ordinary async fn, whose
// Send-ness *is* resolved per monomorphized call site); only the trait impl is written once
// per concrete transport this crate actually drives a `BridgeService` over — the two
// production shapes (TCP, RTU) plus, in this module's own tests, an in-memory duplex link.
async fn forward_and_relay<S, F, L>(
    downstream: &DownstreamHandle<S, F>,
    unit_filter: &Option<UnitIdFilter>,
    log: &L,
    unit: UnitId,
    request: RequestPdu,
) -> Result<Option<ResponsePdu>, ExceptionCode>
where
    S: ClientTransport<F> + Send + 'static,
    F: ClientFraming + Send + 'static,
    L: LogFn + Clone,
{
    // BR-R-015 — an unlisted unit id is ignored entirely: no forward, no answer.
    if let Some(filter) = unit_filter
        && !filter.allows(unit)
    {
        return Ok(None);
    }
    log.invoke(format!("relaying request for unit {unit} downstream."))
        .await;
    let result = downstream.forward(unit, request).await;
    if let Err(e) = &result {
        log.invoke(format!(
            "{} request for unit {unit} answered with a gateway exception: {e:?}.",
            crate::bridge::downstream::ERROR_PREFIX
        ))
        .await;
    }
    result
}

impl<L> Service for BridgeService<FrameTransport<ClientStream, Tcp>, Tcp, L>
where
    L: LogFn + Clone + Send + Sync + 'static,
{
    async fn on_request(
        &self,
        _conn: &Connection,
        unit: UnitId,
        request: RequestPdu,
    ) -> Result<Option<ResponsePdu>, ExceptionCode> {
        forward_and_relay(
            &self.downstream,
            &self.unit_filter,
            &self.log,
            unit,
            request,
        )
        .await
    }
}

impl<L> Service for BridgeService<FrameTransport<SerialStream, Rtu>, Rtu, L>
where
    L: LogFn + Clone + Send + Sync + 'static,
{
    async fn on_request(
        &self,
        _conn: &Connection,
        unit: UnitId,
        request: RequestPdu,
    ) -> Result<Option<ResponsePdu>, ExceptionCode> {
        forward_and_relay(
            &self.downstream,
            &self.unit_filter,
            &self.log,
            unit,
            request,
        )
        .await
    }
}

#[cfg(test)]
impl<L> Service for BridgeService<FrameTransport<tokio::io::DuplexStream, Rtu>, Rtu, L>
where
    L: LogFn + Clone + Send + Sync + 'static,
{
    async fn on_request(
        &self,
        _conn: &Connection,
        unit: UnitId,
        request: RequestPdu,
    ) -> Result<Option<ResponsePdu>, ExceptionCode> {
        forward_and_relay(
            &self.downstream,
            &self.unit_filter,
            &self.log,
            unit,
            request,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_modbus::{
        Address, Client as RmClient, Framing, Quantity, RegisterValue, Server as ModbusServer,
    };
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

    /// A `LogFn` that records every line into a shared buffer for assertions.
    fn recording_log() -> (impl LogFn + Clone, Arc<parking_lot::Mutex<Vec<String>>>) {
        let lines = Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
        let sink = lines.clone();
        let log = move |s: String| {
            let sink = sink.clone();
            async move {
                sink.lock().push(s);
            }
        };
        (log, lines)
    }

    /// A downstream handle whose connect closure builds a `Client` over one end of a duplex
    /// link with a fixed responder driving the other end.
    fn downstream_with_fixed_responder(
        expected: ResponsePdu,
        log: impl LogFn + Clone + Send + 'static,
    ) -> DownstreamHandle<FrameTransport<DuplexStream, Rtu>, Rtu> {
        let (client_end, mut peer) = tokio::io::duplex(256);
        let mut client_end = Some(client_end);
        let handle = DownstreamHandle::spawn(
            move || {
                let client_end = client_end.take();
                async move {
                    Ok(rust_modbus::Client::new(FrameTransport::<_, Rtu>::new(
                        client_end.expect("connect called once"),
                    )))
                }
            },
            true,
            log,
        );
        tokio::spawn(async move {
            let mut buf = [0u8; 64];
            loop {
                match peer.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let (header, _req) = Rtu::decode_request(&buf[..n]).unwrap();
                        let frame = Rtu::encode_response(&header, &expected).unwrap();
                        if peer.write_all(&frame).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });
        handle
    }

    /// BR-R-007 — a `BridgeService` forwards an upstream request downstream and relays the
    /// downstream response back to the upstream client unmodified.
    #[tokio::test]
    async fn ut_service_forwards_and_relays_response_unmodified() {
        let expected = ResponsePdu::ReadHoldingRegisters {
            registers: vec![RegisterValue(99)],
        };
        let (log, _lines) = recording_log();
        let downstream = downstream_with_fixed_responder(expected.clone(), log.clone());
        tokio::time::sleep(Duration::from_millis(20)).await;

        let service = BridgeService::new(downstream, None, log);

        let (server_end, client_end) = tokio::io::duplex(256);
        let modbus = ModbusServer::new(service);
        let handle = modbus.handle();
        let serving = tokio::spawn(modbus.serve_link(FrameTransport::<_, Rtu>::new(server_end)));

        let mut client: RmClient<_, Rtu> = RmClient::new(FrameTransport::new(client_end));
        let registers = client
            .read_holding_registers(UnitId(1), Address(0), Quantity(1))
            .await
            .unwrap();
        assert_eq!(registers, vec![RegisterValue(99)]);

        handle.shutdown().await;
        let _ = serving.await;
    }

    /// BR-R-015 — a unit id not on the filter's allow-list is ignored entirely: no downstream
    /// forward, no upstream response.
    #[tokio::test]
    async fn ut_service_unit_ids_filter_ignores_unlisted_unit_with_no_response() {
        let expected = ResponsePdu::ReadHoldingRegisters {
            registers: vec![RegisterValue(1)],
        };
        let (log, _lines) = recording_log();
        let downstream = downstream_with_fixed_responder(expected, log.clone());
        tokio::time::sleep(Duration::from_millis(20)).await;

        let filter = UnitIdFilter::parse("1").unwrap();
        let service = BridgeService::new(downstream, Some(filter), log);

        let (server_end, client_end) = tokio::io::duplex(256);
        let modbus = ModbusServer::new(service);
        let handle = modbus.handle();
        let serving = tokio::spawn(modbus.serve_link(FrameTransport::<_, Rtu>::new(server_end)));

        let mut client: RmClient<_, Rtu> = RmClient::new(FrameTransport::new(client_end));
        let result = tokio::time::timeout(
            Duration::from_millis(150),
            client.read_holding_registers(UnitId(2), Address(0), Quantity(1)),
        )
        .await;
        assert!(
            result.is_err(),
            "expected no response for an unlisted unit id, got {result:?}"
        );

        handle.shutdown().await;
        let _ = serving.await;
    }

    /// BR-R-015 — with no filter set, every unit id is forwarded (the no-filter default).
    #[tokio::test]
    async fn ut_service_no_filter_forwards_every_unit_id() {
        let expected = ResponsePdu::ReadHoldingRegisters {
            registers: vec![RegisterValue(7)],
        };
        let (log, _lines) = recording_log();
        let downstream = downstream_with_fixed_responder(expected.clone(), log.clone());
        tokio::time::sleep(Duration::from_millis(20)).await;

        let service = BridgeService::new(downstream, None, log);

        let (server_end, client_end) = tokio::io::duplex(256);
        let modbus = ModbusServer::new(service);
        let handle = modbus.handle();
        let serving = tokio::spawn(modbus.serve_link(FrameTransport::<_, Rtu>::new(server_end)));

        let mut client: RmClient<_, Rtu> = RmClient::new(FrameTransport::new(client_end));
        for unit in [1u8, 2] {
            let registers = client
                .read_holding_registers(UnitId(unit), Address(0), Quantity(1))
                .await
                .unwrap();
            assert_eq!(registers, vec![RegisterValue(7)]);
        }

        handle.shutdown().await;
        let _ = serving.await;
    }

    /// BR-R-012 (design decision 3) — a downstream gateway exception is logged with the
    /// `[bridge]` prefix, naming the unit id.
    #[tokio::test]
    async fn ut_service_gateway_exception_logged_with_bridge_prefix() {
        let (log, lines) = recording_log();
        let downstream: DownstreamHandle<FrameTransport<DuplexStream, Rtu>, Rtu> =
            DownstreamHandle::spawn(
                || async {
                    std::future::pending::<
                        Result<
                            rust_modbus::Client<FrameTransport<DuplexStream, Rtu>, Rtu>,
                            crate::Error,
                        >,
                    >()
                    .await
                },
                true,
                log.clone(),
            );

        let service = BridgeService::new(downstream, None, log);

        let (server_end, client_end) = tokio::io::duplex(256);
        let modbus = ModbusServer::new(service);
        let handle = modbus.handle();
        let serving = tokio::spawn(modbus.serve_link(FrameTransport::<_, Rtu>::new(server_end)));

        let mut client: RmClient<_, Rtu> = RmClient::new(FrameTransport::new(client_end));
        let result = client
            .read_holding_registers(UnitId(1), Address(0), Quantity(1))
            .await;
        assert!(result.is_err());

        assert!(
            lines
                .lock()
                .iter()
                .any(|l| l.starts_with(crate::bridge::ERROR_PREFIX) && l.contains("unit 1")),
            "expected a [bridge]-prefixed line naming the unit id: {:?}",
            lines.lock()
        );

        handle.shutdown().await;
        let _ = serving.await;
    }
}
