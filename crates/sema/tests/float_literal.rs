//! Exact target-format literal rounding tests, including halfway cases,
//! subnormals, long significands and exponent overflow.
use mini_c_sema::float_literal::{binary32_bits, binary64_bits};

#[test]
fn binary32_hex_rounding() {
    for (text, bits) in [
        ("0x1p0f", 0x3f800000),
        ("0X.8P+1F", 0x3f800000),
        ("-0x0p999999999999999999999999999999999999999999f", 0x80000000),
        ("0x1.000001p0f", 0x3f800000),
        ("0x1.0000010000000000000000000000001p0f", 0x3f800001),
        ("0x1.000003p0f", 0x3f800002),
        ("0x1p-149f", 1),
        ("0x1p-150f", 0),
        ("0x1.00000000000000000000001p-150f", 1),
        ("0x1.fffffep-127f", 0x00800000),
        ("0x1.fffffep127f", 0x7f7fffff),
        ("0x1.ffffffp127f", 0x7f800000),
        ("0x1p999999999999999999999999999999999999999999f", 0x7f800000),
        ("0x1p-999999999999999999999999999999999999999999f", 0),
        ("0.5f", 0x3f000000),
    ] {
        assert_eq!(binary32_bits(text).unwrap(), bits, "{text}");
    }
}

#[test]
fn binary64_hex_rounding() {
    for (text, bits) in [
        ("0x1p0", 0x3ff0000000000000),
        ("-0x0p0", 0x8000000000000000),
        ("0x1.00000000000008p0", 0x3ff0000000000000),
        ("0x1.000000000000080000000000000001p0", 0x3ff0000000000001),
        ("0x1.00000000000018p0", 0x3ff0000000000002),
        ("0x1p-1074", 1),
        ("0x1p-1075", 0),
        ("0x1.00000000000000000000001p-1075", 1),
        ("0x1.fffffffffffffp-1023", 0x0010000000000000),
        ("0x1.fffffffffffffp1023", 0x7fefffffffffffff),
        ("0x1.fffffffffffff8p1023", 0x7ff0000000000000),
        ("0.5", 0x3fe0000000000000),
    ] {
        assert_eq!(binary64_bits(text).unwrap(), bits, "{text}");
    }
}

#[test]
fn malformed_hex_literals_reject() {
    for text in ["0xp0", "0x.p0", "0x1", "0x1p", "0x1p+", "0x1p1p2", "0x1..0p0", "0xg.p0"] {
        assert!(binary64_bits(text).is_err(), "{text}");
        assert!(binary32_bits(text).is_err(), "{text}");
    }
}
