//! Transport selection between TCP and RTU connection settings.

use crate::{rtu, tcp};

/// Transport-specific connection settings.
#[derive(Debug, Clone)]
pub enum Transport {
    Tcp(tcp::Config),
    Rtu(rtu::Config),
}
