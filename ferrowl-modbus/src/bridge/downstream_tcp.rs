use crate::LogFn;
use crate::bridge::downstream::DownstreamHandle;
use crate::tcp::{self, tls::ClientStream};
use rust_modbus::{ExceptionCode, FrameTransport, RequestPdu, ResponsePdu, Tcp, UnitId};

/// A `DownstreamHandle` connected over TCP (plain or TLS, BR-R-011). Wraps the handle rather
/// than aliasing it directly: the underlying `ClientStream` (plain-or-TLS socket type) is
/// `pub(crate)` in `tcp::tls` and never needs naming outside this crate, so this newtype keeps
/// it out of the public API surface while still letting external integration tests (this
/// module's own, under `tests/`) hold and forward through a handle.
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

    /// The wrapped `DownstreamHandle`, for `BridgeService::new` (`bridge::mod::run`'s
    /// assembly, and this module's own crate-internal upstream tests).
    pub(crate) fn into_handle(self) -> DownstreamHandle<FrameTransport<ClientStream, Tcp>, Tcp> {
        self.0
    }
}

/// BR-R-006 — the downstream TCP interface always acts as an ordinary client, including
/// TLS (BR-R-011, via `tcp::Client::connect`, unchanged) and reconnect/backoff
/// (`config.reconnect`, MB-R-050–056).
pub fn spawn(config: tcp::Config, log: impl LogFn + Clone + 'static) -> TcpDownstream {
    let reconnect = config.reconnect;
    // One cache for this downstream's whole lifetime (reused across reconnect attempts,
    // mirroring the module-instance-scoped cache lifetime used elsewhere — MB-R-138).
    let cache = tcp::tls::new_self_signed_cache();
    TcpDownstream(DownstreamHandle::spawn(
        move || {
            let config = config.clone();
            let cache = cache.clone();
            async move {
                tcp::Client::connect(&config, &cache)
                    .await
                    .map(|c| c.core.client)
            }
        },
        reconnect,
        log,
    ))
}
