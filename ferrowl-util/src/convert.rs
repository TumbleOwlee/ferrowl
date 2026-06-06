use clap::ValueEnum;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::File,
    io::{BufReader, Write},
};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
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
