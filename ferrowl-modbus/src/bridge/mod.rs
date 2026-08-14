//! Modbus relay ("bridge") mode: BR-R-001..BR-R-015.
mod config;
mod downstream;
pub use config::{BridgeEndpointKind, BridgeEndpointSpec, UnitIdFilter};
pub use downstream::{DownstreamHandle, ERROR_PREFIX};
