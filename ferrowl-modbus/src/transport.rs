//! Transport selection between TCP, RTU, and RtuOverTcp connection settings.

use crate::{rtu, tcp};

/// Transport-specific connection settings.
#[derive(Debug, Clone)]
pub enum Transport {
    Tcp(tcp::Config),
    Rtu(rtu::Config),
    /// RTU framing carried over a TCP socket (MB-R-113); carries exactly the same
    /// connection parameters as `Tcp` — no separate config type.
    RtuOverTcp(tcp::Config),
}

#[cfg(test)]
mod tests {
    use super::Transport;

    /// MB-R-113 — `RtuOverTcp` carries exactly a `tcp::Config`, the same type (and
    /// so the same fields) the `Tcp` variant carries — no separate struct.
    #[test]
    fn ut_rtu_over_tcp_variant_carries_tcp_config() {
        let cfg = crate::tcp::Config::default();
        let _: Transport = Transport::RtuOverTcp(cfg.clone());
        // Compiles iff both variants take the identical `tcp::Config` type.
        let _: Transport = Transport::Tcp(cfg);
    }
}
