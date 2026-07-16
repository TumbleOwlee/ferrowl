//! Load, save, and convert serde-serializable data as TOML or JSON files.

use clap::ValueEnum;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::File,
    io::{BufReader, Write},
};

/// Supported config file formats.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum FileType {
    Toml,
    Json,
}

/// Error raised by [`Converter`] operations, carrying a human-readable
/// message that includes the underlying cause.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Writing or serializing failed.
    #[error("{0}")]
    Serialize(String),
    /// Reading or deserializing failed.
    #[error("{0}")]
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

/// Namespace for file (de)serialization helpers; all methods are associated
/// functions.
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
            FileType::Toml => {
                // Route TOML through a `serde_json::Value` and normalize it before handing it to
                // the TOML serializer. This matters for values that embed `serde_json::Value`s
                // (e.g. session module blobs): under the `arbitrary_precision` feature — pulled in
                // transitively by `rust-ocpp`/`rust_decimal` — `serde_json::Number` serializes as a
                // `{"$serde_json::private::Number": "…"}` wrapper struct, which TOML would otherwise
                // emit as a bogus sub-table. Normalizing turns those back into plain integers/floats.
                let json = serde_json::to_value(value)
                    .map_err(|e| Error::Serialize(format!("Failed to serialize TOML [{}].", e)))?;
                toml::to_string_pretty(&json_to_toml(&json)?)
                    .map_err(|e| Error::Serialize(format!("Failed to serialize TOML [{}].", e)))?
            }
            FileType::Json => serde_json::to_string_pretty(value)
                .map_err(|e| Error::Serialize(format!("Failed to serialize JSON [{}].", e)))?,
        };
        let mut file = File::create(path)
            .map_err(|e| Error::Serialize(format!("Failed to create {} [{}].", path, e)))?;
        write!(file, "{}", content)
            .map_err(|e| Error::Serialize(format!("Failed to write {} [{}].", path, e)))
    }
    /// Re-serialize a file from `src_type` to `dest_type` by round-tripping
    /// it through `T`.
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

/// Convert a `serde_json::Value` into a `toml::Value`, normalizing numbers so they serialize as
/// plain TOML integers/floats instead of the `arbitrary_precision` wrapper struct. JSON `null`s
/// are dropped when they appear as an object field's value; TOML has no null type, so a `null`
/// appearing at the top level or inside an array (where there's no key to omit) is an error.
/// `u64` values that overflow `i64` are represented as a TOML float rather than silently wrapping.
fn json_to_toml(value: &serde_json::Value) -> Result<toml::Value, Error> {
    use serde_json::Value as J;
    match value {
        J::Null => Err(Error::Serialize(
            "Cannot represent JSON null as TOML outside of an object field.".to_string(),
        )),
        J::Bool(b) => Ok(toml::Value::Boolean(*b)),
        J::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(toml::Value::Integer(i))
            } else if let Some(u) = n.as_u64() {
                if u > i64::MAX as u64 {
                    Ok(toml::Value::Float(u as f64))
                } else {
                    Ok(toml::Value::Integer(u as i64))
                }
            } else if let Some(f) = n.as_f64() {
                Ok(toml::Value::Float(f))
            } else {
                Ok(toml::Value::String(n.to_string()))
            }
        }
        J::String(s) => Ok(toml::Value::String(s.clone())),
        J::Array(a) => Ok(toml::Value::Array(
            a.iter().map(json_to_toml).collect::<Result<Vec<_>, _>>()?,
        )),
        J::Object(o) => {
            let mut table = toml::value::Table::new();
            for (k, v) in o {
                if v.is_null() {
                    continue;
                }
                table.insert(k.clone(), json_to_toml(v)?);
            }
            Ok(toml::Value::Table(table))
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
    /// CS-R-002 — the encoding is selected from the file-path extension.
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
    /// CS-R-004 — a value saves and loads through TOML to an equal value.
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
    /// CS-R-005 — a numeric value serializes as a plain TOML number, not a wrapper table.
    fn ut_toml_embedded_json_value_number_is_plain() {
        // A struct embedding a `serde_json::Value` with numbers must serialize to plain TOML
        // integers — not the `$serde_json::private::Number` wrapper that `arbitrary_precision`
        // would otherwise produce.
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Wrap {
            blob: serde_json::Value,
        }
        let value = Wrap {
            blob: serde_json::json!({ "ip": "127.0.0.1", "port": 9000 }),
        };
        let path = tmp_path("toml");
        Converter::save(&value, &path, FileType::Toml).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("port = 9000"), "got:\n{text}");
        assert!(!text.contains("private::Number"), "got:\n{text}");
        let loaded: Wrap = Converter::load(&path, FileType::Toml).unwrap();
        assert_eq!(loaded, value);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    /// CS-R-004 — a value saves and loads through JSON to an equal value.
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
    /// CS-R-004 — converting TOML to JSON preserves the data model.
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
    /// CS-R-004 — converting JSON to TOML preserves the data model.
    fn ut_convert_json_to_toml_preserves_data() {
        // Exercises the JSON-source read path and the TOML-destination write path.
        let src = tmp_path("json");
        let dst = tmp_path("toml");
        let value = Sample {
            a: 11,
            b: "y".into(),
        };
        Converter::save(&value, &src, FileType::Json).unwrap();
        Converter::convert::<Sample>(&src, FileType::Json, &dst, FileType::Toml).unwrap();
        let loaded: Sample = Converter::load(&dst, FileType::Toml).unwrap();
        assert_eq!(loaded, value);
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&dst);
    }

    #[test]
    fn ut_convert_json_source_open_error() {
        let dst = tmp_path("toml");
        let r = Converter::convert::<Sample>(
            "/no/such/ferrowl/src.json",
            FileType::Json,
            &dst,
            FileType::Toml,
        );
        assert!(matches!(r, Err(Error::Serialize(_))));
    }

    #[test]
    /// CS-R-050 — a malformed source fails to convert with a deserialize error.
    fn ut_convert_json_source_malformed_error() {
        let src = tmp_path("json");
        std::fs::write(&src, "{ not valid json ").unwrap();
        let dst = tmp_path("toml");
        let r = Converter::convert::<Sample>(&src, FileType::Json, &dst, FileType::Toml);
        assert!(matches!(r, Err(Error::Serialize(_))));
        let _ = std::fs::remove_file(&src);
    }

    #[test]
    fn ut_convert_toml_dest_create_error() {
        // Valid source, but the destination directory does not exist -> create fails.
        let src = tmp_path("toml");
        Converter::save(
            &Sample {
                a: 1,
                b: "z".into(),
            },
            &src,
            FileType::Toml,
        )
        .unwrap();
        let r = Converter::convert::<Sample>(
            &src,
            FileType::Toml,
            "/no/such/ferrowl/dir/out.toml",
            FileType::Toml,
        );
        assert!(matches!(r, Err(Error::Serialize(_))));
        let _ = std::fs::remove_file(&src);
    }

    #[test]
    fn ut_convert_json_dest_create_error() {
        let src = tmp_path("toml");
        Converter::save(
            &Sample {
                a: 1,
                b: "z".into(),
            },
            &src,
            FileType::Toml,
        )
        .unwrap();
        let r = Converter::convert::<Sample>(
            &src,
            FileType::Toml,
            "/no/such/ferrowl/dir/out.json",
            FileType::Json,
        );
        assert!(matches!(r, Err(Error::Serialize(_))));
        let _ = std::fs::remove_file(&src);
    }

    #[test]
    fn ut_save_create_error() {
        // Destination directory missing -> File::create fails in save().
        let r = Converter::save(
            &Sample {
                a: 1,
                b: "z".into(),
            },
            "/no/such/ferrowl/dir/out.toml",
            FileType::Toml,
        );
        assert!(matches!(r, Err(Error::Serialize(_))));
    }

    #[test]
    fn ut_load_missing_file_errors() {
        let r = Converter::load::<Sample>("/no/such/ferrowl/file.toml", FileType::Toml);
        assert!(matches!(r, Err(Error::Deserialize(_))));
    }

    #[test]
    /// CS-R-006 — a top-level JSON null is not representable in TOML and fails serialization.
    fn ut_json_to_toml_null_top_level_errors() {
        let r = json_to_toml(&serde_json::Value::Null);
        assert!(matches!(r, Err(Error::Serialize(_))));
    }

    #[test]
    /// CS-R-006 — a JSON null inside an array is not representable in TOML and fails.
    fn ut_json_to_toml_null_in_array_errors() {
        let r = json_to_toml(&serde_json::json!([1, null, 3]));
        assert!(matches!(r, Err(Error::Serialize(_))));
    }

    #[test]
    /// CS-R-006 — a JSON null at an object key is dropped (field omitted) in TOML.
    fn ut_json_to_toml_null_in_object_is_dropped() {
        let v = json_to_toml(&serde_json::json!({ "a": 1, "b": null })).unwrap();
        let table = v.as_table().unwrap();
        assert_eq!(table.get("a").unwrap().as_integer(), Some(1));
        assert!(!table.contains_key("b"));
    }

    #[test]
    /// CS-R-005 — a u64 exceeding the signed-64 range serializes as a TOML float.
    fn ut_json_to_toml_u64_overflow_falls_back_to_float() {
        let big = u64::MAX;
        let v = json_to_toml(&serde_json::json!(big)).unwrap();
        assert_eq!(v.as_float(), Some(big as f64));
    }

    #[test]
    /// CS-R-005 — a u64 within range stays a TOML integer.
    fn ut_json_to_toml_u64_in_range_stays_integer() {
        let v = json_to_toml(&serde_json::json!(42u64)).unwrap();
        assert_eq!(v.as_integer(), Some(42));
    }

    #[test]
    /// CS-R-050 — malformed file content fails to load with a deserialize error.
    fn ut_load_malformed_content_errors() {
        let path = tmp_path("json");
        std::fs::write(&path, "{ not valid json ").unwrap();
        let r = Converter::load::<Sample>(&path, FileType::Json);
        assert!(matches!(r, Err(Error::Deserialize(_))));
        let _ = std::fs::remove_file(&path);
    }
}
