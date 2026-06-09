//! Data formats describing how raw register words are interpreted.

use serde::{Deserialize, Serialize};

/// Text alignment of an ASCII value inside its fixed-width register block.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Alignment {
    Left,
    Right,
}

impl std::fmt::Display for Alignment {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Alignment::Left => {
                write!(fmt, "Left")
            }
            Alignment::Right => {
                write!(fmt, "Right")
            }
        }
    }
}

/// Width of an ASCII value, in 16-bit registers (2 characters each).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Width(pub usize);

/// Byte order of a multi-byte value across registers.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Endian {
    Little,
    Big,
}

impl std::fmt::Display for Endian {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Endian::Little => {
                write!(fmt, "Little Endian")
            }
            Endian::Big => {
                write!(fmt, "Big Endian")
            }
        }
    }
}

/// Scale factor applied when displaying a numeric value
/// (`displayed = raw * resolution`).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Resolution(pub f64);

impl std::fmt::Display for Resolution {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(fmt, "{}", self.0)
    }
}

/// How the raw register words of a value are interpreted.
///
/// Numeric variants carry the byte order ([`Endian`]) and display scale
/// ([`Resolution`]); [`Ascii`](Self::Ascii) carries its [`Alignment`] and
/// [`Width`] in registers.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Format {
    Ascii((Alignment, Width)),
    U8((Endian, Resolution)),
    U16((Endian, Resolution)),
    U32((Endian, Resolution)),
    U64((Endian, Resolution)),
    U128((Endian, Resolution)),
    I8((Endian, Resolution)),
    I16((Endian, Resolution)),
    I32((Endian, Resolution)),
    I64((Endian, Resolution)),
    I128((Endian, Resolution)),
    F32((Endian, Resolution)),
    F64((Endian, Resolution)),
}

impl std::fmt::Display for Format {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Format::Ascii((alignment, _)) => {
                write!(fmt, "ASCII ({})", alignment)
            }
            Format::U8((e, _)) => write!(fmt, "U8 ({})", e),
            Format::U16((e, _)) => write!(fmt, "U16 ({})", e),
            Format::U32((e, _)) => write!(fmt, "U32 ({})", e),
            Format::U64((e, _)) => write!(fmt, "U64 ({})", e),
            Format::U128((e, _)) => write!(fmt, "U128 ({})", e),
            Format::I8((e, _)) => write!(fmt, "I8 ({})", e),
            Format::I16((e, _)) => write!(fmt, "I16 ({})", e),
            Format::I32((e, _)) => write!(fmt, "I32 ({})", e),
            Format::I64((e, _)) => write!(fmt, "I64 ({})", e),
            Format::I128((e, _)) => write!(fmt, "I128 ({})", e),
            Format::F32((e, _)) => write!(fmt, "F32 ({})", e),
            Format::F64((e, _)) => write!(fmt, "F64 ({})", e),
        }
    }
}

impl Format {
    /// The width of the format in Modbus registers (`u16` words).
    pub fn width(&self) -> usize {
        match self {
            Self::Ascii((_, w)) => w.0,
            Self::U8(_) | Self::U16(_) | Self::I8(_) | Self::I16(_) => 1,
            Self::U32(_) | Self::I32(_) | Self::F32(_) => 2,
            Self::U64(_) | Self::I64(_) | Self::F64(_) => 4,
            Self::U128(_) | Self::I128(_) => 8,
        }
    }

    /// The display scale factor, or `None` for ASCII formats.
    pub fn resolution(&self) -> Option<Resolution> {
        match self {
            Self::Ascii((_, _)) => None,
            Self::U8((_, resolution))
            | Self::U16((_, resolution))
            | Self::I8((_, resolution))
            | Self::I16((_, resolution))
            | Self::U32((_, resolution))
            | Self::I32((_, resolution))
            | Self::F32((_, resolution))
            | Self::U64((_, resolution))
            | Self::I64((_, resolution))
            | Self::F64((_, resolution))
            | Self::U128((_, resolution))
            | Self::I128((_, resolution)) => Some(resolution.clone()),
        }
    }

    /// The length of the format in bytes (two per register).
    pub fn length(&self) -> usize {
        self.width() * 2
    }
}

#[cfg(test)]
mod tests {
    use super::{Alignment, Endian, Format, Resolution, Width};

    fn res() -> Resolution {
        Resolution(1.0)
    }

    #[test]
    fn ut_format_width() {
        assert_eq!(Format::Ascii((Alignment::Left, Width(4))).width(), 4);
        assert_eq!(Format::U8((Endian::Big, res())).width(), 1);
        assert_eq!(Format::U16((Endian::Big, res())).width(), 1);
        assert_eq!(Format::I8((Endian::Big, res())).width(), 1);
        assert_eq!(Format::I16((Endian::Big, res())).width(), 1);
        assert_eq!(Format::U32((Endian::Big, res())).width(), 2);
        assert_eq!(Format::I32((Endian::Big, res())).width(), 2);
        assert_eq!(Format::F32((Endian::Big, res())).width(), 2);
        assert_eq!(Format::U64((Endian::Big, res())).width(), 4);
        assert_eq!(Format::I64((Endian::Big, res())).width(), 4);
        assert_eq!(Format::F64((Endian::Big, res())).width(), 4);
        assert_eq!(Format::U128((Endian::Big, res())).width(), 8);
        assert_eq!(Format::I128((Endian::Big, res())).width(), 8);
    }

    #[test]
    fn ut_format_length() {
        assert_eq!(Format::U8((Endian::Big, res())).length(), 2);
        assert_eq!(Format::U32((Endian::Big, res())).length(), 4);
        assert_eq!(Format::U64((Endian::Big, res())).length(), 8);
        assert_eq!(Format::U128((Endian::Big, res())).length(), 16);
        assert_eq!(Format::Ascii((Alignment::Left, Width(3))).length(), 6);
    }

    #[test]
    fn ut_format_resolution() {
        let r = Resolution(0.5);
        assert!(
            Format::Ascii((Alignment::Left, Width(1)))
                .resolution()
                .is_none()
        );
        assert_eq!(
            Format::U8((Endian::Big, r.clone())).resolution().unwrap().0,
            0.5
        );
        assert_eq!(
            Format::I16((Endian::Little, r.clone()))
                .resolution()
                .unwrap()
                .0,
            0.5
        );
        assert_eq!(
            Format::F32((Endian::Big, r.clone()))
                .resolution()
                .unwrap()
                .0,
            0.5
        );
    }
}
