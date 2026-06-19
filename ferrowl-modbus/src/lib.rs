//! Modbus client and server implementations over TCP and RTU (serial).
//!
//! Both transports expose the same shape: a `ClientBuilder`/`ServerBuilder`
//! that spawns a background tokio task working against a shared
//! [`Memory`](ferrowl_store::Memory). Clients poll the [`Operation`]s they are
//! given and accept write [`Command`]s over a channel; servers answer
//! incoming requests directly from memory. Memory is keyed by [`Key`]
//! parameterized over [`KeyParams`] (default: [`SlaveKey`]).

mod client_core;
mod command;
mod common;
mod error;
mod key;
mod log;
mod operation;
pub mod rtu;
mod run_config;
mod scalar;
mod server_core;
pub mod tcp;
mod transport;

pub use command::Command;
pub use error::{Error, ModbusError, SerialError, TcpError};
pub use key::{Key, KeyParams, SlaveKey};
pub use log::LogFn;
pub use operation::Operation;
pub use run_config::RunConfig;
pub use scalar::{Address, Coil, Word};
pub use transport::Transport;

pub use tokio_modbus::{FunctionCode, SlaveId};
