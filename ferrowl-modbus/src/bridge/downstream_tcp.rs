use crate::LogFn;
use crate::bridge::downstream::DownstreamHandle;
use crate::tcp::{self, tls::ClientStream};
use rust_modbus::{ExceptionCode, FrameTransport, RequestPdu, ResponsePdu, Tcp, UnitId};

/// A `DownstreamHandle` connected over TCP (plain or TLS, BR-R-011). Wraps the handle rather
/// than aliasing it directly: the underlying `ClientStream` (plain-or-TLS socket type) is
/// `pub(crate)` in `tcp::tls` and never needs naming outside this crate, so this newtype keeps
/// it out of the public API surface while still letting `bridge::mod::run` (and this module's
/// own integration tests, a separate crate under `tests/`) hold and forward through a handle.
#[derive(Clone)]
pub struct TcpDownstream(DownstreamHandle<FrameTransport<ClientStream, Tcp>, Tcp>);

impl TcpDownstream {
    /// BR-R-007 — forward one decoded upstream request to the downstream TCP connection.
    pub async fn forward(
        &self,
        unit: UnitId,
        request: RequestPdu,
    ) -> Result<Option<ResponsePdu>, ExceptionCode> {
        self.0.forward(unit, request).await
    }
}

/// BR-R-006 — the downstream TCP interface always acts as an ordinary client, including
/// TLS (BR-R-011, via `tcp::Client::connect`, unchanged) and reconnect/backoff
/// (`config.reconnect`, MB-R-050–056).
pub fn spawn(config: tcp::Config, log: impl LogFn + Clone + 'static) -> TcpDownstream {
    let reconnect = config.reconnect;
    TcpDownstream(DownstreamHandle::spawn(
        move || {
            let config = config.clone();
            async move { tcp::Client::connect(&config).await.map(|c| c.core.client) }
        },
        reconnect,
        log,
    ))
}
