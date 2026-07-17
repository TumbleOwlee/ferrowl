//! Device and session configuration loading (TOML/JSON).

pub mod script;

pub mod device {
    pub use crate::module::modbus::config::device::*;
}
pub mod session {
    pub use crate::module::modbus::config::session::*;
}
pub mod ocpp {
    pub use crate::module::ocpp::config::device::*;
    pub use crate::module::ocpp::config::session::*;
}

pub use device::DeviceConfig;
pub use ocpp::{OcppDeviceConfig, OcppModuleSpec, OcppSpec};
pub use session::{Endpoint, ModuleSpec, Role, Session};

use ferrowl_util::convert::{Converter, FileType};

/// Ferrowl version stamped into device/session files on save (see `DeviceConfig::version`,
/// `Session::version`), so older configs can be detected for compatibility shims.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Sanitize a hand-edited (or dialog-typed) sim-cycle interval in seconds into a `Duration`: a
/// non-finite or non-positive value falls back to `default_secs` (instead of panicking in
/// `Duration::from_secs_f64` or busy-waiting on zero), and an otherwise-valid value is floored to
/// `min_secs` (instead of thrashing the sim thread on a near-zero interval). Pass `0.0` for
/// `min_secs` when no floor is wanted. Shared by [`Session::interval_duration`],
/// [`DeviceConfig::script_interval_duration`], and [`OcppDeviceConfig::script_interval_duration`]
/// so the NaN/negative/zero guard and floor rule stay in exactly one place.
pub(crate) fn sanitize_interval_secs(
    value: f64,
    default_secs: f64,
    min_secs: f64,
) -> std::time::Duration {
    let secs = if value.is_finite() && value > 0.0 {
        value.max(min_secs)
    } else {
        default_secs
    };
    std::time::Duration::from_secs_f64(secs)
}

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

/// Load a device-type config file, migrating legacy per-register `update` scripts
/// into the global script list.
pub fn load_device(path: &str) -> Result<DeviceConfig, ConfigError> {
    let mut device: DeviceConfig = load(path)?;
    device.migrate_update_scripts();
    Ok(device)
}

/// Load an OCPP device-type config file.
pub fn load_ocpp_device(path: &str) -> Result<OcppDeviceConfig, ConfigError> {
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
    /// CS-R-033 — a saved device/session file reloads to an equal value (envelope round-trips).
    fn ut_load_device_and_session_roundtrip() {
        let dpath = tmp("ferrowl_cfgmod_device.toml");
        Converter::save(&DeviceConfig::default(), &dpath, FileType::Toml).unwrap();
        assert_eq!(load_device(&dpath).unwrap(), DeviceConfig::default());

        let spath = tmp("ferrowl_cfgmod_session.json");
        Converter::save(&Session::default(), &spath, FileType::Json).unwrap();
        assert_eq!(load_session(&spath).unwrap(), Session::default());
    }

    #[test]
    /// CS-R-054 — loading a device config self-heals a legacy per-register `update` snippet on every load.
    fn ut_load_device_migrates_update_scripts() {
        let path = tmp("ferrowl_cfgmod_legacy_update.toml");
        std::fs::write(
            &path,
            "[definitions.reg]\ntype = \"U16\"\nupdate = \"C_Time:Sleep(1)\"\n",
        )
        .unwrap();
        let device = load_device(&path).unwrap();
        assert!(device.definitions["reg"].update.is_none());
        assert_eq!(device.scripts.len(), 1);
        assert_eq!(device.scripts[0].name, "reg");
        assert_eq!(device.scripts[0].code, "C_Time:Sleep(1)");
        assert!(device.scripts[0].enabled);
    }

    #[test]
    /// CS-R-003 — a path with an unknown extension fails to load with an unknown-format error.
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
    /// CS-R-052 — a field present in a file but absent from the schema is ignored on load.
    fn ut_load_ignores_unknown_field() {
        let path = tmp("ferrowl_cfgmod_unknown_field.toml");
        std::fs::write(
            &path,
            "bogus_unknown_field = 42\n[definitions.reg]\ntype = \"U16\"\n",
        )
        .unwrap();
        let device = load_device(&path).unwrap();
        assert!(device.definitions.contains_key("reg"));
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
