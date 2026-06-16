//! Error types for Modbus protocol and transport failures.

use tokio_modbus::ExceptionCode;

/// Errors from Modbus protocol operations.
#[derive(Debug, thiserror::Error)]
pub enum ModbusError {
    #[error("Modbus exception: {0:?}")]
    Exception(ExceptionCode),
    #[error("Modbus error: {0}")]
    Error(tokio_modbus::Error),
    #[error("Modbus timeout: {0}")]
    Timeout(tokio::time::error::Elapsed),
}

/// Errors from the serial (RTU) transport.
#[derive(Debug, thiserror::Error)]
pub enum SerialError {
    #[error("Serial error: {0}")]
    Error(tokio_serial::Error),
    #[error("Serial configuration error: {0}")]
    Configuration(String),
}

/// Errors from the TCP transport.
#[derive(Debug, thiserror::Error)]
pub enum TcpError {
    #[error("TCP address error: {0}")]
    Address(std::net::AddrParseError),
    #[error("TCP configuration error: {0}")]
    Configuration(String),
    #[error("TCP error: {0}")]
    Error(tokio::io::Error),
    #[error("TCP timeout: {0}")]
    Timeout(tokio::time::error::Elapsed),
}

/// Top-level error type unifying protocol, transport, and server errors.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Modbus(#[from] ModbusError),
    #[error("{0}")]
    Serial(#[from] SerialError),
    #[error("{0}")]
    Tcp(#[from] TcpError),
    #[error("Server error: {0}")]
    Server(std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::{Error, ModbusError, SerialError, TcpError};
    use tokio_modbus::ExceptionCode;

    #[test]
    fn ut_serial_error_configuration_display() {
        let e = SerialError::Configuration("bad baud rate".to_string());
        assert_eq!(e.to_string(), "Serial configuration error: bad baud rate");
    }

    #[test]
    fn ut_tcp_error_configuration_display() {
        let e = TcpError::Configuration("missing host".to_string());
        assert_eq!(e.to_string(), "TCP configuration error: missing host");
    }

    #[test]
    fn ut_modbus_error_exception_display() {
        let e = ModbusError::Exception(ExceptionCode::IllegalFunction);
        assert!(e.to_string().contains("Modbus exception"));
    }

    #[test]
    fn ut_error_wraps_serial_display() {
        let inner = SerialError::Configuration("oops".to_string());
        let e = Error::from(inner);
        assert!(e.to_string().contains("Serial configuration error: oops"));
    }

    #[test]
    fn ut_error_wraps_tcp_display() {
        let inner = TcpError::Configuration("no host".to_string());
        let e = Error::from(inner);
        assert!(e.to_string().contains("TCP configuration error: no host"));
    }
}
