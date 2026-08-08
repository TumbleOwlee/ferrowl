//! Transport selection between TCP, RTU, and RtuOverTcp connection settings.

use crate::{rtu, tcp, udp};

/// Transport-specific connection settings.
#[derive(Debug, Clone)]
pub enum Transport {
    Tcp(tcp::Config),
    Rtu(rtu::Config),
    /// RTU framing carried over a TCP socket (MB-R-113); carries exactly the same
    /// connection parameters as `Tcp` — no separate config type.
    RtuOverTcp(tcp::Config),
    /// MB-R-116 — same field set as `Tcp` minus `tls`; its own `udp::Config` type, not a
    /// reuse of `tcp::Config` (unlike `RtuOverTcp`).
    Udp(udp::Config),
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

    /// MB-R-116 — `Transport::Udp` carries a `udp::Config`, not `tcp::Config` — the two
    /// types are structurally similar (minus `tls`) but distinct.
    #[test]
    fn ut_udp_variant_carries_udp_config() {
        let cfg = crate::udp::Config::default();
        let _: Transport = Transport::Udp(cfg);
    }
}
