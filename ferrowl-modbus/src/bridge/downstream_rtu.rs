use crate::LogFn;
use crate::bridge::downstream::DownstreamHandle;
use crate::rtu;
use rust_modbus::{FrameTransport, Rtu, SerialStream};

pub type RtuDownstream = DownstreamHandle<FrameTransport<SerialStream, Rtu>, Rtu>;

/// BR-R-006 — the downstream RTU interface always acts as an ordinary client, opening the
/// serial port (via `rtu::Client::connect`, unchanged) and reconnecting on backoff
/// (`config.reconnect`, MB-R-050–056).
pub fn spawn(config: rtu::Config, log: impl LogFn + Clone + 'static) -> RtuDownstream {
    let reconnect = config.reconnect;
    DownstreamHandle::spawn(
        move || {
            let config = config.clone();
            async move { rtu::Client::connect(&config).await.map(|c| c.core.client) }
        },
        reconnect,
        log,
    )
}
