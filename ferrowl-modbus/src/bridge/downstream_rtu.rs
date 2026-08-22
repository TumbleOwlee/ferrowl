use crate::LogFn;
use crate::PathConflictCell;
use crate::bridge::downstream::DownstreamHandle;
use crate::rtu;
use rust_modbus::{FrameTransport, Rtu, SerialStream};

pub type RtuDownstream = DownstreamHandle<FrameTransport<SerialStream, Rtu>, Rtu>;

/// BR-R-006 — the downstream RTU interface always acts as an ordinary client, opening the
/// serial port (via `rtu::Client::connect`, unchanged) and reconnecting on backoff
/// (`config.reconnect`, MB-R-050–056).
///
/// MB-R-150's path-conflict check is scoped to `ferrowl::module::modbus` session instances
/// (client/server/monitor tabs); bridge mode is a distinct architecture (see AGENTS.md "Bridging
/// Modbus and OCPP" scope boundary) with no session registry to consult, so a default,
/// never-conflicting cell is used here — behavior is unchanged from before this feature.
pub fn spawn(config: rtu::Config, log: impl LogFn + Clone + 'static) -> RtuDownstream {
    let reconnect = config.reconnect;
    let path_conflict = PathConflictCell::default();
    DownstreamHandle::spawn(
        move || {
            let config = config.clone();
            let path_conflict = path_conflict.clone();
            async move {
                rtu::Client::connect(&config, &path_conflict)
                    .await
                    .map(|c| c.core.client)
            }
        },
        reconnect,
        log,
    )
}
