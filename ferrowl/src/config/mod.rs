//! Device and session configuration loading (TOML/JSON).

pub mod device;
pub mod session;

pub use device::DeviceConfig;
pub use session::{Endpoint, ModuleSpec, Role, Session};

use ferrowl_util::convert::{Converter, FileType};

/// Ferrowl version stamped into device/session files on save (see `DeviceConfig::version`,
/// `Session::version`), so older configs can be detected for compatibility shims.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Error type for config loading.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid file (JSON/TOML): {0}")]
    UnknownFormat(String),
    #[error("{0}")]
    Io(String),
}

fn file_type(path: &str) -> Result<FileType, ConfigError> {
    FileType::from_path(path).ok_or_else(|| ConfigError::UnknownFormat(path.to_string()))
}

fn load<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, ConfigError> {
    let ty = file_type(path)?;
    Converter::load(path, ty).map_err(|e| ConfigError::Io(format!("{:?}", e)))
}

/// Load a device-type config file.
pub fn load_device(path: &str) -> Result<DeviceConfig, ConfigError> {
    load(path)
}

/// Load a session file.
pub fn load_session(path: &str) -> Result<Session, ConfigError> {
    load(path)
}
