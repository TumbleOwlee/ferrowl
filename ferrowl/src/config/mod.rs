//! Device and session configuration loading (TOML/JSON).

pub mod device {
    pub use crate::module::modbus::config::device::*;
}
pub mod session {
    pub use crate::module::modbus::config::session::*;
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use ferrowl_util::convert::{Converter, FileType};

    fn tmp(name: &str) -> String {
        std::env::temp_dir()
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn ut_load_device_and_session_roundtrip() {
        let dpath = tmp("ferrowl_cfgmod_device.toml");
        Converter::save(&DeviceConfig::default(), &dpath, FileType::Toml).unwrap();
        assert_eq!(load_device(&dpath).unwrap(), DeviceConfig::default());

        let spath = tmp("ferrowl_cfgmod_session.json");
        Converter::save(&Session::default(), &spath, FileType::Json).unwrap();
        assert_eq!(load_session(&spath).unwrap(), Session::default());
    }

    #[test]
    fn ut_load_unknown_format_errors() {
        let e = load_session("/tmp/ferrowl_cfg.bin");
        assert!(matches!(e, Err(ConfigError::UnknownFormat(_))));
    }

    #[test]
    fn ut_load_io_error() {
        let e = load_device("/no/such/ferrowl/device.toml");
        assert!(matches!(e, Err(ConfigError::Io(_))));
    }

    #[test]
    fn ut_config_error_display() {
        assert!(
            ConfigError::UnknownFormat("p".into())
                .to_string()
                .contains("invalid file")
        );
        assert_eq!(ConfigError::Io("boom".into()).to_string(), "boom");
    }
}
