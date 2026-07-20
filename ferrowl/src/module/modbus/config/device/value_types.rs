//! Config-file spellings of register value types (`Scalar`, `ValueType`, and the access,
//! endian and alignment enums) plus their conversions into the corresponding `ferrowl-codec`
//! types. Separated from the device/register layout structs in the parent module.

use ferrowl_codec::{
    Access,
    format::{Alignment, BitField, Endian, Resolution, WordOrder},
};
use serde::{Deserialize, Serialize};

/// A named-value payload. Untagged so the config file can write `value = 10`, `value = 1.5`
/// or `value = "text"` without quoting numbers. Written to a register via its `Display` string,
/// which `Register::encode` then interprets per the register's own format.
///
/// Serialization is derived untagged. Deserialization is hand-written via a `Visitor` rather than
/// `#[serde(untagged)]` because that buffers content into an intermediate that `serde_json`'s
/// `arbitrary_precision` mode represents numbers as — making untagged number variants fail to
/// match. A direct visitor avoids the buffering and also accepts the `arbitrary_precision` number
/// wrapper, so it works under any serde_json feature set as well as for TOML.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Scalar {
    Int(i64),
    Float(f64),
    Text(String),
}

impl<'de> Deserialize<'de> for Scalar {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ScalarVisitor;

        impl<'de> serde::de::Visitor<'de> for ScalarVisitor {
            type Value = Scalar;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an integer, float, or string")
            }

            fn visit_i64<E>(self, v: i64) -> Result<Scalar, E> {
                Ok(Scalar::Int(v))
            }

            fn visit_u64<E>(self, v: u64) -> Result<Scalar, E> {
                Ok(i64::try_from(v).map_or(Scalar::Float(v as f64), Scalar::Int))
            }

            fn visit_f64<E>(self, v: f64) -> Result<Scalar, E> {
                Ok(Scalar::Float(v))
            }

            fn visit_str<E>(self, v: &str) -> Result<Scalar, E> {
                Ok(Scalar::Text(v.to_owned()))
            }

            fn visit_string<E>(self, v: String) -> Result<Scalar, E> {
                Ok(Scalar::Text(v))
            }

            // `serde_json`'s `arbitrary_precision` mode (enabled transitively by `rust-ocpp`'s
            // `jsonschema`/`rust_decimal` deps when this crate shares a workspace build) encodes a
            // number as a single-entry map `{ "$serde_json::private::Number": "<digits>" }`. Parse
            // that back into the appropriate variant.
            fn visit_map<A>(self, mut map: A) -> Result<Scalar, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let _key: String = map
                    .next_key()?
                    .ok_or_else(|| serde::de::Error::custom("empty number map"))?;
                let raw: String = map.next_value()?;
                if let Ok(i) = raw.parse::<i64>() {
                    Ok(Scalar::Int(i))
                } else if let Ok(f) = raw.parse::<f64>() {
                    Ok(Scalar::Float(f))
                } else {
                    Err(serde::de::Error::custom(format!("invalid number: {raw}")))
                }
            }
        }

        deserializer.deserialize_any(ScalarVisitor)
    }
}

impl std::fmt::Display for Scalar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Scalar::Int(v) => write!(f, "{v}"),
            Scalar::Float(v) => write!(f, "{v}"),
            Scalar::Text(v) => write!(f, "{v}"),
        }
    }
}

impl Scalar {
    /// Infer a scalar from user input: integer first, then float, else verbatim text.
    pub fn from_input(s: &str) -> Self {
        let t = s.trim();
        if let Ok(i) = t.parse::<i64>() {
            Scalar::Int(i)
        } else if let Ok(f) = t.parse::<f64>() {
            Scalar::Float(f)
        } else {
            Scalar::Text(s.to_string())
        }
    }

    pub fn to_value(&self, res: f64) -> ferrowl_codec::Value {
        match self {
            Scalar::Int(i) => ferrowl_codec::Value::I64((*i, Resolution(res))),
            Scalar::Float(f) => ferrowl_codec::Value::F64((*f, Resolution(res))),
            Scalar::Text(s) => ferrowl_codec::Value::Ascii(s.clone()),
        }
    }
}

/// The value encoding of a register.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueType {
    U8,
    U16,
    U32,
    U64,
    U128,
    I8,
    I16,
    I32,
    I64,
    I128,
    F32,
    F64,
    Ascii,
}

/// Config-file spelling of register access rights.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum AccessCfg {
    ReadOnly,
    WriteOnly,
    #[default]
    ReadWrite,
}

/// Config-file spelling of byte order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum EndianCfg {
    #[default]
    Big,
    Little,
}

/// Config-file spelling of register (word) order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum WordOrderCfg {
    #[default]
    Normal,
    Reversed,
}

/// Config-file spelling of ASCII alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum AlignmentCfg {
    #[default]
    Left,
    Right,
}

/// Parse a config `bitmask` string into a [`BitField`]. Accepts `0x`-prefixed hex
/// or decimal; `None`, empty, or an unparseable value yields the full mask (no-op).
pub fn parse_bitmask(s: Option<&str>) -> BitField {
    let mask = match s.map(str::trim).filter(|s| !s.is_empty()) {
        None => u128::MAX,
        Some(s) => s
            .strip_prefix("0x")
            .or_else(|| s.strip_prefix("0X"))
            .map(|hex| u128::from_str_radix(hex, 16))
            .unwrap_or_else(|| s.parse::<u128>())
            .unwrap_or(u128::MAX),
    };
    BitField { mask }
}

impl From<AccessCfg> for Access {
    fn from(a: AccessCfg) -> Self {
        match a {
            AccessCfg::ReadOnly => Access::ReadOnly,
            AccessCfg::WriteOnly => Access::WriteOnly,
            AccessCfg::ReadWrite => Access::ReadWrite,
        }
    }
}

impl From<EndianCfg> for Endian {
    fn from(e: EndianCfg) -> Self {
        match e {
            EndianCfg::Big => Endian::Big,
            EndianCfg::Little => Endian::Little,
        }
    }
}

impl From<WordOrderCfg> for WordOrder {
    fn from(w: WordOrderCfg) -> Self {
        match w {
            WordOrderCfg::Normal => WordOrder::Normal,
            WordOrderCfg::Reversed => WordOrder::Reversed,
        }
    }
}

impl From<AlignmentCfg> for Alignment {
    fn from(a: AlignmentCfg) -> Self {
        match a {
            AlignmentCfg::Left => Alignment::Left,
            AlignmentCfg::Right => Alignment::Right,
        }
    }
}
