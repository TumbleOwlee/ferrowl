#![feature(async_fn_traits)]

pub mod rtu;
pub mod tcp;

use ferrowl_mem::Range;
use ferrowl_reg::Kind;
use std::fmt::{Debug, Display};
use std::hash::Hash;
use tokio_modbus::ExceptionCode;
pub use tokio_modbus::{FunctionCode, SlaveId};

#[derive(Debug, Clone)]
pub enum Config {
    Tcp(tcp::Config),
    Rtu(rtu::Config),
}

#[derive(Debug, Clone)]
pub struct Operation {
    pub slave_id: SlaveId,
    pub fn_code: FunctionCode,
    pub range: Range,
}

pub trait KeyParams: Hash + Eq + Clone + Default + Debug + Send + Sync + 'static {
    fn from_slave_fn(slave_id: SlaveId, fn_code: FunctionCode) -> Self;
}

#[derive(Hash, Debug, PartialEq, Eq, Clone, Default)]
pub struct Key<T: KeyParams> {
    pub id: T,
}

impl<T: KeyParams> Key<T> {
    pub fn new(id: T) -> Self {
        Self { id }
    }
}

/// Default concrete key params: slave address + register kind.
#[derive(Hash, Debug, PartialEq, Eq, Clone, Default)]
pub struct SlaveKind {
    pub slave_id: SlaveId,
    pub kind: Kind,
}

impl KeyParams for SlaveKind {
    fn from_slave_fn(slave_id: SlaveId, fn_code: FunctionCode) -> Self {
        Self {
            slave_id,
            kind: match fn_code {
                FunctionCode::ReadCoils
                | FunctionCode::WriteSingleCoil
                | FunctionCode::WriteMultipleCoils => Kind::Coil,
                FunctionCode::ReadDiscreteInputs => Kind::DiscreteInput,
                FunctionCode::ReadHoldingRegisters
                | FunctionCode::WriteSingleRegister
                | FunctionCode::WriteMultipleRegisters => Kind::HoldingRegister,
                FunctionCode::ReadInputRegisters => Kind::InputRegister,
                _ => Kind::HoldingRegister,
            },
        }
    }
}

#[derive(Debug)]
pub enum ModbusError {
    Exception(ExceptionCode),
    Error(tokio_modbus::Error),
    Timeout(tokio::time::error::Elapsed),
}

impl Display for ModbusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModbusError::Exception(code) => write!(f, "Modbus exception: {:?}", code),
            ModbusError::Error(e) => write!(f, "Modbus error: {}", e),
            ModbusError::Timeout(e) => write!(f, "Modbus timeout: {}", e),
        }
    }
}

impl std::error::Error for ModbusError {}

#[derive(Debug)]
pub enum SerialError {
    Error(tokio_serial::Error),
    Configuration(String),
}

impl Display for SerialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SerialError::Error(e) => write!(f, "Serial error: {}", e),
            SerialError::Configuration(e) => write!(f, "Serial configuration error: {}", e),
        }
    }
}

impl std::error::Error for SerialError {}

#[derive(Debug)]
pub enum TcpError {
    Address(std::net::AddrParseError),
    Configuration(String),
    Error(tokio::io::Error),
    Timeout(tokio::time::error::Elapsed),
}

impl Display for TcpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TcpError::Address(e) => write!(f, "TCP address error: {}", e),
            TcpError::Configuration(e) => write!(f, "TCP configuration error: {}", e),
            TcpError::Error(e) => write!(f, "TCP error: {}", e),
            TcpError::Timeout(e) => write!(f, "TCP timeout: {}", e),
        }
    }
}

impl std::error::Error for TcpError {}

#[derive(Debug)]
pub enum Error {
    Modbus(ModbusError),
    Serial(SerialError),
    Tcp(TcpError),
    Server(std::io::Error),
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Modbus(e) => write!(f, "{}", e),
            Error::Serial(e) => write!(f, "{}", e),
            Error::Tcp(e) => write!(f, "{}", e),
            Error::Server(e) => write!(f, "Server error: {}", e),
        }
    }
}

impl std::error::Error for Error {}

impl From<TcpError> for Error {
    fn from(e: TcpError) -> Self {
        Error::Tcp(e)
    }
}

impl From<SerialError> for Error {
    fn from(e: SerialError) -> Self {
        Error::Serial(e)
    }
}

impl From<ModbusError> for Error {
    fn from(e: ModbusError) -> Self {
        Error::Modbus(e)
    }
}

pub type Address = u16;
pub type Value = u16;
pub type Coil = bool;

pub enum Command {
    Terminate,
    WriteSingleCoil(SlaveId, Address, Coil),
    WriteMultipleCoils(SlaveId, Address, Vec<Coil>),
    WriteSingleRegister(SlaveId, Address, Value),
    WriteMultipleRegister(SlaveId, Address, Vec<Value>),
}

/// Configuration passed to a client's `run` loop, bundling the logging and status
/// callbacks together with the polling timings.
pub struct RunConfig<L, S> {
    pub log: L,
    pub status: S,
    pub timeout_ms: usize,
    pub delay_ms: usize,
    pub interval_ms: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_key_new_stores_fields() {
        let sk = SlaveKind { slave_id: 7, kind: Kind::HoldingRegister };
        let key = Key::new(sk.clone());
        assert_eq!(key.id, sk);
    }

    #[test]
    fn ut_key_default_is_slave_kind_default() {
        let key = Key::<SlaveKind>::default();
        assert_eq!(key.id, SlaveKind::default());
    }

    #[test]
    fn ut_slave_kind_from_slave_fn_coil() {
        let sk = SlaveKind::from_slave_fn(3, FunctionCode::ReadCoils);
        assert_eq!(sk.slave_id, 3);
        assert_eq!(sk.kind, Kind::Coil);
    }

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
