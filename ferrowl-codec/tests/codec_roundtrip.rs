//! Integration coverage for `ferrowl-codec`'s public API: building a [`Register`], and
//! round-tripping typed [`Value`]s through raw register words with `encode_value`/`decode`.
//! These drive the crate exactly as a consumer (the Modbus module) does, over public paths only.

use ferrowl_codec::format::{Resolution, Width};
use ferrowl_codec::{
    Access, Alignment, BitField, Endian, Format, Kind, RegisterBuilder, Value, decode, encode,
    encode_value,
};

fn res() -> Resolution {
    Resolution(1.0)
}

fn int(endian: Endian) -> Format {
    Format::U16((endian, res(), BitField::default()))
}

#[test]
fn it_register_builder_applies_documented_defaults() {
    let reg = RegisterBuilder::default()
        .format(int(Endian::Big))
        .build()
        .expect("only `format` is required to build a Register");
    assert_eq!(*reg.slave_id(), 0);
    assert_eq!(*reg.access(), Access::ReadWrite);
    assert_eq!(*reg.kind(), Kind::InputRegister);
}

#[test]
fn it_u16_value_roundtrips_through_words() {
    let fmt = int(Endian::Big);
    let words = encode_value(&fmt, &Value::U16((300, res()))).expect("300 fits a u16 register");
    let back = decode(&fmt, &words).expect("freshly encoded words decode");
    match back {
        Value::U16((v, _)) => assert_eq!(v, 300),
        other => panic!("expected U16, got {other:?}"),
    }
}

#[test]
fn it_u32_big_and_little_endian_swap_word_order_but_decode_equal() {
    let value = Value::U32((0x0001_0002, res()));
    let be = encode_value(
        &Format::U32((Endian::Big, res(), BitField::default())),
        &value,
    )
    .expect("value encodes big-endian");
    let le = encode_value(
        &Format::U32((Endian::Little, res(), BitField::default())),
        &value,
    )
    .expect("value encodes little-endian");
    assert_ne!(be, le, "the two byte orders must differ for this value");

    let decoded = |fmt: Format, words: &[u16]| match decode(&fmt, words).expect("decodes") {
        Value::U32((v, _)) => v,
        other => panic!("expected U32, got {other:?}"),
    };
    assert_eq!(
        decoded(Format::U32((Endian::Big, res(), BitField::default())), &be),
        0x0001_0002
    );
    assert_eq!(
        decoded(
            Format::U32((Endian::Little, res(), BitField::default())),
            &le
        ),
        0x0001_0002
    );
}

#[test]
fn it_signed_value_roundtrips_negative() {
    let fmt = Format::I16((Endian::Big, res(), BitField::default()));
    let words = encode_value(&fmt, &Value::I16((-5, res()))).expect("encodes -5");
    match decode(&fmt, &words).expect("decodes") {
        Value::I16((v, _)) => assert_eq!(v, -5),
        other => panic!("expected I16, got {other:?}"),
    }
}

#[test]
fn it_float_value_roundtrips() {
    let fmt = Format::F32((Endian::Big, res()));
    let words = encode_value(&fmt, &Value::F32((1.5, res()))).expect("encodes 1.5");
    match decode(&fmt, &words).expect("decodes") {
        Value::F32((v, _)) => assert_eq!(v, 1.5),
        other => panic!("expected F32, got {other:?}"),
    }
}

#[test]
fn it_ascii_value_roundtrips_through_string_encoding() {
    let fmt = Format::Ascii((Alignment::Left, Width(4)));
    let words = encode(&fmt, "Hi").expect("ASCII text encodes into 4 registers");
    match decode(&fmt, &words).expect("decodes") {
        Value::Ascii(s) => assert_eq!(s.trim_end_matches('\0').trim_end(), "Hi"),
        other => panic!("expected Ascii, got {other:?}"),
    }
}

#[test]
fn it_decode_rejects_too_few_words() {
    let fmt = Format::U32((Endian::Big, res(), BitField::default()));
    assert!(
        decode(&fmt, &[0x0001]).is_err(),
        "a u32 needs two words; one must be rejected"
    );
}

#[test]
fn it_encode_rejects_unparseable_string() {
    let fmt = int(Endian::Big);
    assert!(
        encode(&fmt, "not-a-number").is_err(),
        "a non-numeric string must not encode into a numeric register"
    );
}
