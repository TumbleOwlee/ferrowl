//! Data formats describing how raw register words are interpreted.

mod alignment;
mod bitfield;
mod endian;
mod scalar;

pub use alignment::Alignment;
pub use bitfield::BitField;
pub use endian::Endian;
pub use scalar::{Resolution, Width};

use serde::{Deserialize, Serialize};

/// How the raw register words of a value are interpreted.
///
/// Integer variants carry the byte order ([`Endian`]), display scale
/// ([`Resolution`]) and a [`BitField`] selector; float variants carry just
/// endian and resolution; [`Ascii`](Self::Ascii) carries its [`Alignment`] and
/// [`Width`] in registers.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
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

    #[test]
    fn ut_format_resolution_all_variants() {
        let r = Resolution(0.25);
        let e = Endian::Big;
        for f in [
            Format::U16((e.clone(), r.clone(), bf())),
            Format::I8((e.clone(), r.clone(), bf())),
            Format::U32((e.clone(), r.clone(), bf())),
            Format::I32((e.clone(), r.clone(), bf())),
            Format::U64((e.clone(), r.clone(), bf())),
            Format::I64((e.clone(), r.clone(), bf())),
            Format::U128((e.clone(), r.clone(), bf())),
            Format::I128((e.clone(), r.clone(), bf())),
            Format::F64((e.clone(), r.clone())),
        ] {
            assert_eq!(f.resolution().unwrap().0, 0.25);
        }
    }

    #[test]
    fn ut_format_bitfield_all_variants() {
        let m = BitField { mask: 0x0FF0 };
        let e = Endian::Big;
        for f in [
            Format::U8((e.clone(), res(), m.clone())),
            Format::U32((e.clone(), res(), m.clone())),
            Format::U64((e.clone(), res(), m.clone())),
            Format::U128((e.clone(), res(), m.clone())),
            Format::I8((e.clone(), res(), m.clone())),
            Format::I16((e.clone(), res(), m.clone())),
            Format::I32((e.clone(), res(), m.clone())),
            Format::I64((e.clone(), res(), m.clone())),
            Format::I128((e.clone(), res(), m.clone())),
        ] {
            assert_eq!(f.bitfield(), m);
        }
        // Float variant reports the no-op default.
        assert!(Format::F64((e, res())).bitfield().is_full());
    }

    #[test]
    fn ut_format_display_all_variants() {
        assert_eq!(
            Format::Ascii((Alignment::Left, Width(2))).to_string(),
            "ASCII (Left)"
        );
        let e = Endian::Big;
        assert_eq!(
            Format::U8((e.clone(), res(), bf())).to_string(),
            "U8 (Big Endian)"
        );
        assert_eq!(
            Format::U16((e.clone(), res(), bf())).to_string(),
            "U16 (Big Endian)"
        );
        assert_eq!(
            Format::U32((e.clone(), res(), bf())).to_string(),
            "U32 (Big Endian)"
        );
        assert_eq!(
            Format::U64((e.clone(), res(), bf())).to_string(),
            "U64 (Big Endian)"
        );
        assert_eq!(
            Format::U128((e.clone(), res(), bf())).to_string(),
            "U128 (Big Endian)"
        );
        assert_eq!(
            Format::I8((e.clone(), res(), bf())).to_string(),
            "I8 (Big Endian)"
        );
        assert_eq!(
            Format::I16((e.clone(), res(), bf())).to_string(),
            "I16 (Big Endian)"
        );
        assert_eq!(
            Format::I32((e.clone(), res(), bf())).to_string(),
            "I32 (Big Endian)"
        );
        assert_eq!(
            Format::I64((e.clone(), res(), bf())).to_string(),
            "I64 (Big Endian)"
        );
        assert_eq!(
            Format::I128((e.clone(), res(), bf())).to_string(),
            "I128 (Big Endian)"
        );
        assert_eq!(
            Format::F32((e.clone(), res())).to_string(),
            "F32 (Big Endian)"
        );
        assert_eq!(Format::F64((e, res())).to_string(), "F64 (Big Endian)");
    }
}
