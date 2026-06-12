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

impl Default for Resolution {
    fn default() -> Self {
        Resolution(1.0)
    }
}

impl std::fmt::Display for Resolution {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(fmt, "{}", self.0)
    }
}

/// Bit-field selector for an integer value: the raw word is masked and the
/// resulting field shifted down by the mask's trailing-zero count.
///
/// Read: `field = (raw & mask) >> shift`. Write: `raw = (value << shift) & mask`.
/// The shift is *derived* from `mask` (the bit position of its least-significant
/// set bit), so only the mask is ever configured. The default (`u128::MAX`,
/// shift 0) is a no-op and is narrowed to each format's own width when applied.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct BitField {
    pub mask: u128,
}

impl Default for BitField {
    fn default() -> Self {
        Self { mask: u128::MAX }
    }
}

impl BitField {
    /// Bit offset of the field, i.e. the mask's trailing-zero count (0 for a
    /// zero mask, which is otherwise degenerate).
    pub fn shift(&self) -> u32 {
        if self.mask == 0 {
            0
        } else {
            self.mask.trailing_zeros()
        }
    }

    /// Whether this is the no-op full mask (all bits selected, shift 0).
    pub fn is_full(&self) -> bool {
        self.mask == u128::MAX
    }
}

impl std::fmt::Display for BitField {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(fmt, "0x{:X}", self.mask)
    }
}

/// How the raw register words of a value are interpreted.
///
/// Integer variants carry the byte order ([`Endian`]), display scale
/// ([`Resolution`]) and a [`BitField`] selector; float variants carry just
/// endian and resolution; [`Ascii`](Self::Ascii) carries its [`Alignment`] and
/// [`Width`] in registers.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Format {
    Ascii((Alignment, Width)),
    U8((Endian, Resolution, BitField)),
    U16((Endian, Resolution, BitField)),
    U32((Endian, Resolution, BitField)),
    U64((Endian, Resolution, BitField)),
    U128((Endian, Resolution, BitField)),
    I8((Endian, Resolution, BitField)),
    I16((Endian, Resolution, BitField)),
    I32((Endian, Resolution, BitField)),
    I64((Endian, Resolution, BitField)),
    I128((Endian, Resolution, BitField)),
    F32((Endian, Resolution)),
    F64((Endian, Resolution)),
}

impl std::fmt::Display for Format {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Format::Ascii((alignment, _)) => {
                write!(fmt, "ASCII ({})", alignment)
            }
            Format::U8((e, _, _)) => write!(fmt, "U8 ({})", e),
            Format::U16((e, _, _)) => write!(fmt, "U16 ({})", e),
            Format::U32((e, _, _)) => write!(fmt, "U32 ({})", e),
            Format::U64((e, _, _)) => write!(fmt, "U64 ({})", e),
            Format::U128((e, _, _)) => write!(fmt, "U128 ({})", e),
            Format::I8((e, _, _)) => write!(fmt, "I8 ({})", e),
            Format::I16((e, _, _)) => write!(fmt, "I16 ({})", e),
            Format::I32((e, _, _)) => write!(fmt, "I32 ({})", e),
            Format::I64((e, _, _)) => write!(fmt, "I64 ({})", e),
            Format::I128((e, _, _)) => write!(fmt, "I128 ({})", e),
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
            Self::U8((_, resolution, _))
            | Self::U16((_, resolution, _))
            | Self::I8((_, resolution, _))
            | Self::I16((_, resolution, _))
            | Self::U32((_, resolution, _))
            | Self::I32((_, resolution, _))
            | Self::U64((_, resolution, _))
            | Self::I64((_, resolution, _))
            | Self::U128((_, resolution, _))
            | Self::I128((_, resolution, _)) => Some(resolution.clone()),
            Self::F32((_, resolution)) | Self::F64((_, resolution)) => Some(resolution.clone()),
        }
    }

    /// The [`BitField`] selector for integer formats, or the no-op default
    /// (full mask, shift 0) for float and ASCII formats.
    pub fn bitfield(&self) -> BitField {
        match self {
            Self::U8((_, _, bf))
            | Self::U16((_, _, bf))
            | Self::U32((_, _, bf))
            | Self::U64((_, _, bf))
            | Self::U128((_, _, bf))
            | Self::I8((_, _, bf))
            | Self::I16((_, _, bf))
            | Self::I32((_, _, bf))
            | Self::I64((_, _, bf))
            | Self::I128((_, _, bf)) => bf.clone(),
            Self::F32(_) | Self::F64(_) | Self::Ascii(_) => BitField::default(),
        }
    }

    /// The length of the format in bytes (two per register).
    pub fn length(&self) -> usize {
        self.width() * 2
    }
}

#[cfg(test)]
mod tests {
    use super::{Alignment, BitField, Endian, Format, Resolution, Width};

    fn res() -> Resolution {
        Resolution(1.0)
    }

    fn bf() -> BitField {
        BitField::default()
    }

    #[test]
    fn ut_format_width() {
        assert_eq!(Format::Ascii((Alignment::Left, Width(4))).width(), 4);
        assert_eq!(Format::U8((Endian::Big, res(), bf())).width(), 1);
        assert_eq!(Format::U16((Endian::Big, res(), bf())).width(), 1);
        assert_eq!(Format::I8((Endian::Big, res(), bf())).width(), 1);
        assert_eq!(Format::I16((Endian::Big, res(), bf())).width(), 1);
        assert_eq!(Format::U32((Endian::Big, res(), bf())).width(), 2);
        assert_eq!(Format::I32((Endian::Big, res(), bf())).width(), 2);
        assert_eq!(Format::F32((Endian::Big, res())).width(), 2);
        assert_eq!(Format::U64((Endian::Big, res(), bf())).width(), 4);
        assert_eq!(Format::I64((Endian::Big, res(), bf())).width(), 4);
        assert_eq!(Format::F64((Endian::Big, res())).width(), 4);
        assert_eq!(Format::U128((Endian::Big, res(), bf())).width(), 8);
        assert_eq!(Format::I128((Endian::Big, res(), bf())).width(), 8);
    }

    #[test]
    fn ut_format_length() {
        assert_eq!(Format::U8((Endian::Big, res(), bf())).length(), 2);
        assert_eq!(Format::U32((Endian::Big, res(), bf())).length(), 4);
        assert_eq!(Format::U64((Endian::Big, res(), bf())).length(), 8);
        assert_eq!(Format::U128((Endian::Big, res(), bf())).length(), 16);
        assert_eq!(Format::Ascii((Alignment::Left, Width(3))).length(), 6);
    }

    #[test]
    fn ut_format_bitfield() {
        // Integer carries its BitField; float/ASCII report the no-op default.
        let bitfield = BitField { mask: 0xFF00 };
        assert_eq!(
            Format::U16((Endian::Big, res(), bitfield.clone())).bitfield(),
            bitfield
        );
        assert_eq!(
            Format::U16((Endian::Big, res(), bitfield))
                .bitfield()
                .shift(),
            8
        );
        assert!(Format::F32((Endian::Big, res())).bitfield().is_full());
        assert!(
            Format::Ascii((Alignment::Left, Width(1)))
                .bitfield()
                .is_full()
        );
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
            Format::U8((Endian::Big, r.clone(), bf()))
                .resolution()
                .unwrap()
                .0,
            0.5
        );
        assert_eq!(
            Format::I16((Endian::Little, r.clone(), bf()))
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
