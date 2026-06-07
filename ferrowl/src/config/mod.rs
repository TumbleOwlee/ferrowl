pub mod app;
pub mod device;
pub mod session;

pub use app::AppConfig;
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

/// Resolve the general app config: explicit `--config`, else `./config.toml`, else
/// `~/.config/ferrowl/config.toml`. Returns the default config when none exists.
pub fn resolve_app_config(explicit: Option<&str>) -> AppConfig {
    let candidates: Vec<String> = match explicit {
        Some(p) => vec![p.to_string()],
        None => {
            let mut c = vec!["./config.toml".to_string()];
            if let Some(home) = home_config_path() {
                c.push(home);
            }
            c
        }
    };
    for path in candidates {
        if std::path::Path::new(&path).exists()
            && let Ok(cfg) = load::<AppConfig>(&path)
        {
            return cfg;
        }
    }
    AppConfig::default()
}

fn home_config_path() -> Option<String> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
        && !xdg.is_empty()
    {
        return Some(format!("{xdg}/ferrowl/config.toml"));
    }
    std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .map(|home| format!("{home}/.config/ferrowl/config.toml"))
}
