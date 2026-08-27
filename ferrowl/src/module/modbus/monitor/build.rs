//! MB-R-140 — the monitor role/transport compatibility check and its resulting
//! `ferrowl_modbus` config, mirroring `module/modbus/build.rs::endpoint_to_config`'s shape but
//! restricted to the two transports a monitor can observe.

use crate::config::Endpoint;

/// A monitor's resolved network config: `Rtu`/`Ascii` framing over the same
/// `ferrowl_modbus::rtu::Config` shape (the two endpoint kinds share identical fields —
/// path/baud_rate/parity/data_bits/stop_bits — differing only in on-wire framing, exactly as
/// `ferrowl_modbus::ascii` reuses `rtu::Config` verbatim for the client/server case).
// `#[allow(dead_code)]`: implemented and tested here; nothing constructs a
// `ModbusMonitorModule` from it yet.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub(crate) enum MonitorNetConfig {
    Rtu(ferrowl_modbus::rtu::Config),
    Ascii(ferrowl_modbus::rtu::Config),
}

/// MB-R-140 — a monitor is configurable only on the `Rtu` or `Ascii` transport.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("monitor role requires transport rtu or ascii, got {0}")]
#[allow(dead_code)] // forward-declared; see MonitorNetConfig's note
pub(crate) struct MonitorTransportError(pub &'static str);

/// MB-R-140 — the role/transport compatibility check, same failure class as MB-R-107's
/// resolve-time TLS check (a typed error returned from a resolution step, not a panic or a
/// silent fallback). `ferrowl_modbus::rtu::Config`'s `timeout_ms`/`delay_ms`/`interval_ms`
/// fields are structurally present but unused by the monitor engine (`MonitorBuilder` never
/// reads them) — left at `0`.
#[allow(dead_code)] // see `MonitorNetConfig`'s note
pub(crate) fn endpoint_to_monitor_config(
    endpoint: &Endpoint,
    reconnect: bool,
) -> Result<MonitorNetConfig, MonitorTransportError> {
    match endpoint {
        Endpoint::Rtu {
            path,
            baud_rate,
            parity,
            data_bits,
            stop_bits,
        } => Ok(MonitorNetConfig::Rtu(ferrowl_modbus::rtu::Config {
            path: path.clone(),
            baud_rate: *baud_rate,
            slave: 0, // inert: a monitor addresses no request of its own
            parity: parity.clone(),
            data_bits: *data_bits,
            stop_bits: *stop_bits,
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect,
        })),
        Endpoint::Ascii {
            path,
            baud_rate,
            parity,
            data_bits,
            stop_bits,
        } => Ok(MonitorNetConfig::Ascii(ferrowl_modbus::rtu::Config {
            path: path.clone(),
            baud_rate: *baud_rate,
            slave: 0,
            parity: parity.clone(),
            data_bits: *data_bits,
            stop_bits: *stop_bits,
            timeout_ms: 0,
            delay_ms: 0,
            interval_ms: 0,
            reconnect,
        })),
        Endpoint::Tcp { .. } => Err(MonitorTransportError("tcp")),
        Endpoint::RtuOverTcp { .. } => Err(MonitorTransportError("rtu_over_tcp")),
        Endpoint::Udp { .. } => Err(MonitorTransportError("udp")),
        Endpoint::AsciiOverTcp { .. } => Err(MonitorTransportError("ascii_over_tcp")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rtu_endpoint() -> Endpoint {
        Endpoint::Rtu {
            path: "/dev/ttyUSB0".to_string(),
            baud_rate: 9600,
            parity: Some("even".to_string()),
            data_bits: Some(8),
            stop_bits: Some(1),
        }
    }

    fn ascii_endpoint() -> Endpoint {
        Endpoint::Ascii {
            path: "/dev/ttyUSB1".to_string(),
            baud_rate: 4800,
            parity: None,
            data_bits: None,
            stop_bits: None,
        }
    }

    /// MB-R-140 — `Rtu`/`Ascii` endpoints resolve to a monitor config, carrying the endpoint's
    /// serial fields through unchanged.
    #[test]
    fn ut_endpoint_to_monitor_config_accepts_rtu_and_ascii() {
        let cfg = endpoint_to_monitor_config(&rtu_endpoint(), true).unwrap();
        assert_eq!(
            cfg,
            MonitorNetConfig::Rtu(ferrowl_modbus::rtu::Config {
                path: "/dev/ttyUSB0".to_string(),
                baud_rate: 9600,
                slave: 0,
                parity: Some("even".to_string()),
                data_bits: Some(8),
                stop_bits: Some(1),
                timeout_ms: 0,
                delay_ms: 0,
                interval_ms: 0,
                reconnect: true,
            })
        );

        let cfg = endpoint_to_monitor_config(&ascii_endpoint(), false).unwrap();
        assert_eq!(
            cfg,
            MonitorNetConfig::Ascii(ferrowl_modbus::rtu::Config {
                path: "/dev/ttyUSB1".to_string(),
                baud_rate: 4800,
                slave: 0,
                parity: None,
                data_bits: None,
                stop_bits: None,
                timeout_ms: 0,
                delay_ms: 0,
                interval_ms: 0,
                reconnect: false,
            })
        );
    }

    /// MB-R-140 — every non-serial transport is rejected with a role/transport compatibility
    /// error, naming the rejected transport.
    #[test]
    fn ut_endpoint_to_monitor_config_rejects_tcp_rtu_over_tcp_udp_ascii_over_tcp() {
        let cases = [
            (
                Endpoint::Tcp {
                    ip: "127.0.0.1".to_string(),
                    port: 502,
                },
                "tcp",
            ),
            (
                Endpoint::RtuOverTcp {
                    ip: "127.0.0.1".to_string(),
                    port: 502,
                },
                "rtu_over_tcp",
            ),
            (
                Endpoint::Udp {
                    ip: "127.0.0.1".to_string(),
                    port: 502,
                },
                "udp",
            ),
            (
                Endpoint::AsciiOverTcp {
                    ip: "127.0.0.1".to_string(),
                    port: 502,
                },
                "ascii_over_tcp",
            ),
        ];
        for (endpoint, label) in cases {
            let err = endpoint_to_monitor_config(&endpoint, true).unwrap_err();
            assert_eq!(err, MonitorTransportError(label));
        }
    }
}
