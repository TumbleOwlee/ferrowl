use clap::ValueEnum;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::File,
    io::{BufReader, Write},
};

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum FileType {
    Toml,
    Json,
}

#[derive(Debug)]
pub enum Error {
    Serialize(String),
    Deserialize(String),
}

impl FileType {
    /// Infer the file type from a path's extension (`.toml` / `.json`).
    pub fn from_path(path: &str) -> Option<FileType> {
        match std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref()
        {
            Some("toml") => Some(FileType::Toml),
            Some("json") => Some(FileType::Json),
            _ => None,
        }
    }
}

pub struct Converter {}

impl Converter {
    /// Deserialize a value from a file of the given type.
    pub fn load<T: DeserializeOwned>(path: &str, ty: FileType) -> Result<T, Error> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| Error::Deserialize(format!("Failed to read {} [{}].", path, e)))?;
        match ty {
            FileType::Toml => toml::from_str::<T>(&content)
                .map_err(|e| Error::Deserialize(format!("Failed to deserialize TOML [{}].", e))),
            FileType::Json => serde_json::from_str::<T>(&content)
                .map_err(|e| Error::Deserialize(format!("Failed to deserialize JSON [{}].", e))),
        }
    }

    /// Serialize a value to a file of the given type.
    pub fn save<T: Serialize>(value: &T, path: &str, ty: FileType) -> Result<(), Error> {
        let content = match ty {
            FileType::Toml => toml::to_string_pretty(value)
                .map_err(|e| Error::Serialize(format!("Failed to serialize TOML [{}].", e)))?,
            FileType::Json => serde_json::to_string_pretty(value)
                .map_err(|e| Error::Serialize(format!("Failed to serialize JSON [{}].", e)))?,
        };
        let mut file = File::create(path)
            .map_err(|e| Error::Serialize(format!("Failed to create {} [{}].", path, e)))?;
        write!(file, "{}", content)
            .map_err(|e| Error::Serialize(format!("Failed to write {} [{}].", path, e)))
    }
    pub fn convert<T: Serialize + DeserializeOwned>(
        src: &str,
        src_type: FileType,
        dest: &str,
        dest_type: FileType,
    ) -> Result<(), Error> {
        let data: T = match src_type {
            FileType::Toml => {
                let content = std::fs::read_to_string(src)
                    .map_err(|e| Error::Serialize(format!("Failed to read TOML file [{}].", e)))?;
                toml::from_str::<T>(&content)
                    .map_err(|e| Error::Serialize(format!("Failed to deserialize TOML [{}].", e)))?
            }
            FileType::Json => {
                let file = File::open(src)
                    .map_err(|e| Error::Serialize(format!("Failed to open JSON file [{}].", e)))?;
                let reader = BufReader::new(file);
                serde_json::from_reader(reader)
                    .map_err(|e| Error::Serialize(format!("Failed to deserialize JSON [{}].", e)))?
            }
        };

        match dest_type {
            FileType::Toml => {
                let content = toml::to_string::<T>(&data)
                    .map_err(|e| Error::Serialize(format!("Failed to serialize TOML [{}].", e)))?;
                let mut file = File::create(dest).map_err(|e| {
                    Error::Serialize(format!("Failed to create TOML file [{}].", e))
                })?;
                write!(file, "{}", content)
                    .map_err(|e| Error::Serialize(format!("Failed to serialize TOML [{}].", e)))
            }
            FileType::Json => {
                let content = serde_json::to_string_pretty::<T>(&data)
                    .map_err(|e| Error::Serialize(format!("Failed to serialize JSON [{}].", e)))?;
                let mut file = File::create(dest).map_err(|e| {
                    Error::Serialize(format!("Failed to create JSON file [{}].", e))
                })?;
                write!(file, "{}", content)
                    .map_err(|e| Error::Serialize(format!("Failed to serialize JSON [{}].", e)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::sync::atomic::{AtomicU32, Ordering};

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Sample {
        a: u32,
        b: String,
    }

    /// Unique scratch path in the temp dir (process id + monotonic counter avoids collisions
    /// across parallel test threads).
    fn tmp_path(ext: &str) -> String {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir()
            .join(format!(
                "ferrowl_convert_{}_{}.{}",
                std::process::id(),
                n,
                ext
            ))
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn ut_filetype_from_path() {
        assert_eq!(FileType::from_path("cfg.toml"), Some(FileType::Toml));
        assert_eq!(FileType::from_path("cfg.json"), Some(FileType::Json));
        // Extension matching is case-insensitive.
        assert_eq!(FileType::from_path("CFG.TOML"), Some(FileType::Toml));
        assert_eq!(FileType::from_path("CFG.Json"), Some(FileType::Json));
        assert_eq!(FileType::from_path("cfg.txt"), None);
        assert_eq!(FileType::from_path("noext"), None);
    }

    #[test]
    fn ut_toml_save_load_round_trip() {
        let path = tmp_path("toml");
        let value = Sample {
            a: 7,
            b: "hello".into(),
        };
        Converter::save(&value, &path, FileType::Toml).unwrap();
        let loaded: Sample = Converter::load(&path, FileType::Toml).unwrap();
        assert_eq!(loaded, value);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ut_json_save_load_round_trip() {
        let path = tmp_path("json");
        let value = Sample {
            a: 42,
            b: "world".into(),
        };
        Converter::save(&value, &path, FileType::Json).unwrap();
        let loaded: Sample = Converter::load(&path, FileType::Json).unwrap();
        assert_eq!(loaded, value);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ut_convert_toml_to_json_preserves_data() {
        let src = tmp_path("toml");
        let dst = tmp_path("json");
        let value = Sample {
            a: 3,
            b: "x".into(),
        };
        Converter::save(&value, &src, FileType::Toml).unwrap();
        Converter::convert::<Sample>(&src, FileType::Toml, &dst, FileType::Json).unwrap();
        let loaded: Sample = Converter::load(&dst, FileType::Json).unwrap();
        assert_eq!(loaded, value);
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&dst);
    }

    #[test]
    fn ut_load_missing_file_errors() {
        let r = Converter::load::<Sample>("/no/such/ferrowl/file.toml", FileType::Toml);
        assert!(matches!(r, Err(Error::Deserialize(_))));
    }

    #[test]
    fn ut_load_malformed_content_errors() {
        let path = tmp_path("json");
        std::fs::write(&path, "{ not valid json ").unwrap();
        let r = Converter::load::<Sample>(&path, FileType::Json);
        assert!(matches!(r, Err(Error::Deserialize(_))));
        let _ = std::fs::remove_file(&path);
    }
}
