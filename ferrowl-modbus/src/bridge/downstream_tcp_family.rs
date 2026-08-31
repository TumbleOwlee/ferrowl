use crate::LogFn;
use crate::bridge::downstream::DownstreamHandle;
use crate::tcp::{
    self,
    tls::{ClientStream, SelfSignedCache},
};
use crate::{ascii_over_tcp, rtu_over_tcp};
use rust_modbus::{Ascii, Client, ClientFraming, ExceptionCode, FrameTransport};
use rust_modbus::{RequestPdu, ResponsePdu, RtuOverTcp, Tcp, UnitId};
use std::future::Future;

/// Connects a TCP-family transport (BR-R-004) and hands back the connected client,
/// erasing which of `tcp`/`rtu_over_tcp`/`ascii_over_tcp`'s `Client::connect` was used —
/// the three differ only in which framing they carry, not in connection setup.
pub(crate) trait FamilyConnect: ClientFraming + Sized {
    fn connect_family(
        config: &tcp::Config,
        cache: &SelfSignedCache,
    ) -> impl Future<Output = Result<Client<FrameTransport<ClientStream, Self>, Self>, crate::Error>>
    + Send;
}

impl FamilyConnect for Tcp {
    async fn connect_family(
        config: &tcp::Config,
        cache: &SelfSignedCache,
    ) -> Result<Client<FrameTransport<ClientStream, Self>, Self>, crate::Error> {
        tcp::Client::connect(config, cache)
            .await
            .map(|c| c.core.client)
    }
}

impl FamilyConnect for RtuOverTcp {
    async fn connect_family(
        config: &tcp::Config,
        cache: &SelfSignedCache,
    ) -> Result<Client<FrameTransport<ClientStream, Self>, Self>, crate::Error> {
        rtu_over_tcp::Client::connect(config, cache)
            .await
            .map(|c| c.core.client)
    }
}

impl FamilyConnect for Ascii {
    async fn connect_family(
        config: &tcp::Config,
        cache: &SelfSignedCache,
    ) -> Result<Client<FrameTransport<ClientStream, Self>, Self>, crate::Error> {
        ascii_over_tcp::Client::connect(config, cache)
            .await
            .map(|c| c.core.client)
    }
}

/// A `DownstreamHandle` connected over TCP with framing `F` (plain or TLS, BR-R-011),
/// generic over `Tcp`, `RtuOverTcp` and `Ascii` framing (BR-R-004). Wraps the handle rather
/// than aliasing it directly: the underlying `ClientStream` (plain-or-TLS socket type) is
/// `pub(crate)` in `tcp::tls` and never needs naming outside this crate, so this newtype
/// keeps it out of the public API surface while still letting external integration tests
/// hold and forward through a handle.
#[derive(Clone)]
pub struct TcpFamilyDownstream<F>(DownstreamHandle<FrameTransport<ClientStream, F>, F>);

impl<F> TcpFamilyDownstream<F>
where
    F: ClientFraming + Send + 'static,
    F::Header: Sync,
{
    /// BR-R-007 — forward one decoded upstream request to the downstream connection.
    pub async fn forward(
        &self,
        unit: UnitId,
        request: RequestPdu,
    ) -> Result<Option<ResponsePdu>, ExceptionCode> {
        self.0.forward(unit, request).await
    }

    /// The wrapped `DownstreamHandle`, for `BridgeService::new` (`bridge::mod::run`'s
    /// assembly, and this module's own crate-internal upstream tests).
    pub(crate) fn into_handle(self) -> DownstreamHandle<FrameTransport<ClientStream, F>, F> {
        self.0
    }
}

/// BR-R-006 — the downstream TCP-family interface always acts as an ordinary client,
/// including TLS (BR-R-011, via `tcp::Client::connect`/its `rtu_over_tcp`/`ascii_over_tcp`
/// siblings, unchanged) and reconnect/backoff (`config.reconnect`, MB-R-050–056).
pub fn spawn<F>(config: tcp::Config, log: impl LogFn + Clone + 'static) -> TcpFamilyDownstream<F>
where
    F: FamilyConnect + Send + 'static,
    F::Header: Sync,
{
    let reconnect = config.reconnect;
    // One cache for this downstream's whole lifetime (reused across reconnect attempts,
    // mirroring the module-instance-scoped cache lifetime used elsewhere — MB-R-138).
    let cache = tcp::tls::new_self_signed_cache();
    TcpFamilyDownstream(DownstreamHandle::spawn(
        move || {
            let config = config.clone();
            let cache = cache.clone();
            async move { F::connect_family(&config, &cache).await }
        },
        reconnect,
        log,
    ))
}
