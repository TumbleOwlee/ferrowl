//! Selection value-types specific to the Modbus setup dialog: the transport and role choices,
//! each rendered via [`ToLabel`]. Separated from the dialog widget/state logic in the parent
//! module; the parity/reconnect/dialog-mode/numeric-serial choices shared with the monitor and
//! ocpp dialogs live in `crate::dialog::choices`.

use ferrowl_ui::traits::ToLabel;

use crate::config::ClientOrServer;

/// Transport selection value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transport {
    Tcp,
    Rtu,
    /// RTU framing carried over a TCP socket (MB-R-113) — shares the `Tcp` field set
    /// (ip/port, TLS), not RTU's serial fields.
    RtuOverTcp,
    /// MB-R-116 — shares `Tcp`'s ip/port field set, no TLS. Appended last (index 3) to
    /// keep `Tcp=0, Rtu=1, RtuOverTcp=2` stable, same convention as `RtuOverTcp`'s addition.
    Udp,
    /// ASCII framing over a serial line (MB-R-121) — shares `Rtu`'s field set (path/
    /// baud/parity/data_bits/stop_bits). Appended last (index 4) to keep prior indices stable.
    Ascii,
    /// ASCII framing carried over a TCP socket (MB-R-125) — shares `Tcp`/`RtuOverTcp`/
    /// `Udp`'s ip/port field set, with TLS (MB-R-127) like `Tcp`/`RtuOverTcp`. Appended
    /// last (index 5).
    AsciiOverTcp,
}

impl ToLabel for Transport {
    fn to_label(&self) -> String {
        match self {
            Transport::Tcp => "TCP",
            Transport::Rtu => "RTU",
            Transport::RtuOverTcp => "RTU over TCP",
            Transport::Udp => "UDP",
            Transport::Ascii => "ASCII",
            Transport::AsciiOverTcp => "ASCII over TCP",
        }
        .to_string()
    }
}

impl ToLabel for ClientOrServer {
    fn to_label(&self) -> String {
        format!("{self}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_labels() {
        assert_eq!(Transport::Tcp.to_label(), "TCP");
        assert_eq!(Transport::Rtu.to_label(), "RTU");
        assert_eq!(Transport::RtuOverTcp.to_label(), "RTU over TCP");
        assert_eq!(Transport::Udp.to_label(), "UDP");
        assert_eq!(Transport::Ascii.to_label(), "ASCII");
        assert_eq!(Transport::AsciiOverTcp.to_label(), "ASCII over TCP");
        assert_eq!(
            ClientOrServer::Server.to_label(),
            format!("{}", ClientOrServer::Server)
        );
    }
}
