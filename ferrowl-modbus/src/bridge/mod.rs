//! Modbus relay ("bridge") mode: BR-R-001..BR-R-015.
mod config;
mod downstream;
mod downstream_rtu;
mod downstream_tcp;
mod service;
mod upstream_rtu;
mod upstream_tcp;
pub use config::{BridgeEndpointKind, BridgeEndpointSpec, UnitIdFilter};
pub use downstream::{DownstreamHandle, ERROR_PREFIX};
pub use downstream_rtu::{RtuDownstream, spawn as spawn_rtu_downstream};
pub use downstream_tcp::{TcpDownstream, spawn as spawn_tcp_downstream};
