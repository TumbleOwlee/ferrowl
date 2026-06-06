//! General application config (globals not tied to a single device type).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    /// Number of log lines kept in the on-screen ring buffer per module.
    #[serde(default = "default_history")]
    pub history_length: usize,
    #[serde(default = "default_timeout")]
    pub timeout_ms: usize,
    #[serde(default = "default_delay")]
    pub delay_ms: usize,
    #[serde(default = "default_interval")]
    pub interval_ms: usize,
    /// Base path for per-module log files (tab name appended as suffix). `None` disables.
    #[serde(default)]
    pub log_file: Option<String>,
}

fn default_history() -> usize {
    80
}
fn default_timeout() -> usize {
    3000
}
fn default_delay() -> usize {
    1000
}
fn default_interval() -> usize {
    1000
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            history_length: default_history(),
            timeout_ms: default_timeout(),
            delay_ms: default_delay(),
            interval_ms: default_interval(),
            log_file: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrowl_util::convert::{Converter, FileType};

    #[test]
    fn ut_app_config_roundtrip() {
        let original = AppConfig {
            history_length: 30,
            timeout_ms: 500,
            delay_ms: 250,
            interval_ms: 500,
            log_file: Some("ferrowl.log".to_string()),
        };
        for (ty, ext) in [(FileType::Toml, "toml"), (FileType::Json, "json")] {
            let path = std::env::temp_dir().join(format!("ferrowl_app_test.{ext}"));
            let path = path.to_str().unwrap();
            Converter::save(&original, path, ty).expect("save");
            let back: AppConfig = Converter::load(path, ty).expect("load");
            assert_eq!(original, back);
        }
    }

    #[test]
    fn ut_app_config_defaults_from_empty_toml() {
        let path = std::env::temp_dir().join("ferrowl_app_empty.toml");
        std::fs::write(&path, "").unwrap();
        let cfg: AppConfig = Converter::load(path.to_str().unwrap(), FileType::Toml).unwrap();
        assert_eq!(cfg, AppConfig::default());
    }
}
