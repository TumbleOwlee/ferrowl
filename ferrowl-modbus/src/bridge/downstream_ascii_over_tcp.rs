use crate::LogFn;
use crate::ascii_over_tcp;
use crate::bridge::downstream::DownstreamHandle;
use crate::tcp::{self, tls::ClientStream};
use rust_modbus::{Ascii, ExceptionCode, FrameTransport, RequestPdu, ResponsePdu, UnitId};

/// A `DownstreamHandle` connected over TCP carrying ASCII framing (plain or TLS, BR-R-011),
/// mirroring `TcpDownstream` exactly (`downstream_tcp.rs`) but for ASCII-over-TCP (BR-R-004).
#[derive(Clone)]
pub struct AsciiOverTcpDownstream(DownstreamHandle<FrameTransport<ClientStream, Ascii>, Ascii>);

impl AsciiOverTcpDownstream {
    /// BR-R-007 — forward one decoded upstream request to the downstream ASCII-over-TCP
    /// connection.
    pub async fn forward(
        &self,
        unit: UnitId,
        request: RequestPdu,
    ) -> Result<Option<ResponsePdu>, ExceptionCode> {
        self.0.forward(unit, request).await
    }

    /// The wrapped `DownstreamHandle`, for `BridgeService::new` (`bridge::mod::run`'s
    /// assembly, and this module's own crate-internal upstream tests).
    pub(crate) fn into_handle(
        self,
    ) -> DownstreamHandle<FrameTransport<ClientStream, Ascii>, Ascii> {
        self.0
    }
}

/// BR-R-006 — the downstream ASCII-over-TCP interface always acts as an ordinary client,
/// including TLS (BR-R-011) and reconnect/backoff (`config.reconnect`, MB-R-050–056).
pub fn spawn(config: tcp::Config, log: impl LogFn + Clone + 'static) -> AsciiOverTcpDownstream {
    let reconnect = config.reconnect;
    let cache = tcp::tls::new_self_signed_cache();
    AsciiOverTcpDownstream(DownstreamHandle::spawn(
        move || {
            let config = config.clone();
            let cache = cache.clone();
            async move {
                ascii_over_tcp::Client::connect(&config, &cache)
                    .await
                    .map(|c| c.core.client)
            }
        },
        reconnect,
        log,
    ))
}
