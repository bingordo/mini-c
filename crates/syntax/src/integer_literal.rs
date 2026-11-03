//! Decode C integer constants without losing their radix or suffix.
//!
//! Typing is target-dependent and belongs in semantic analysis. The syntax
//! crate stores a value and enough spelling information to distinguish an
//! unsuffixed decimal literal from a hexadecimal/octal constant. C long long
//! and C23 binary/bit-precise literals remain explicitly unsupported.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntegerLiteral {
    pub value: u64,
    pub nondecimal: bool,
    pub unsigned: bool,
    pub long: bool,
}

pub fn decode_integer_literal(text: &str) -> Result<IntegerLiteral, &'static str> {
    if text.is_empty() || !text.as_bytes()[0].is_ascii_digit() {
        return Err("expected integer literal");
    }
    let (base, start, nondecimal) = if text.starts_with("0x") || text.starts_with("0X") {
        (16, 2, true)
    } else if text.starts_with('0') && text.len() > 1 {
        (8, 0, true)
    } else {
        (10, 0, false)
    };
    let bytes = text.as_bytes();
    let mut end = start;
    while end < text.len() && match base {
        16 => bytes[end].is_ascii_hexdigit(),
        _ => bytes[end].is_ascii_digit(),
    } {
        end += 1;
    }
    if end == start {
        return Err("hexadecimal integer constant needs at least one digit");
    }
    let value = u64::from_str_radix(&text[start..end], base)
        .map_err(|_| "invalid or overflowing integer constant")?;
    let suffix = &text[end..];
    let mut unsigned = false;
    let mut long = false;
    for ch in suffix.bytes() {
        match ch.to_ascii_lowercase() {
            b'u' if !unsigned => unsigned = true,
            b'l' if !long => long = true,
            _ => return Err("invalid or unsupported integer literal suffix"),
        }
    }
    Ok(IntegerLiteral { value, nondecimal, unsigned, long })
}

/// Backward-compatible value-only API for callers that do not type constants.
pub fn parse_integer_literal(text: &str) -> Result<u64, &'static str> {
    decode_integer_literal(text).map(|literal| literal.value)
}

/// The original AST `Int` represents an unsuffixed signed-decimal literal.
/// Preserve it for existing clients and small unsuffixed nondecimal constants
/// (which have the same C type on all supported targets), but retain\n/// spelling where C type differs.
pub fn parse_integer_expression(text: &str) -> Result<crate::ast::Expr, &'static str> {
    let literal = decode_integer_literal(text)?;
    if !literal.unsigned && !literal.long
        && (!literal.nondecimal || literal.value <= i16::MAX as u64)
    {
        Ok(crate::ast::Expr::Int(literal.value))
    } else {
        Ok(crate::ast::Expr::IntegerConstant(literal))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decode_integer_suffixes_and_radices() {
        for (text, value, nondecimal, unsigned, long) in [
            ("0", 0, false, false, false),
            ("9", 9, false, false, false),
            ("2147483648", 2147483648, false, false, false),
            ("0x7f", 127, true, false, false),
            ("0X2A", 42, true, false, false),
            ("077", 63, true, false, false),
            ("0xffffffff", 4294967295, true, false, false),
            ("0xFFFFFFFFFFFFFFFF", u64::MAX, true, false, false),
            ("123u", 123, false, true, false),
            ("123L", 123, false, false, true),
            ("123uL", 123, false, true, true),
            ("123lU", 123, false, true, true),
            ("0x80000000U", 2147483648, true, true, false),
            ("18446744073709551615UL", u64::MAX, false, true, true),
        ] {
            assert_eq!(decode_integer_literal(text), Ok(IntegerLiteral {
                value, nondecimal, unsigned, long,
            }), "{text}");
        }
    }
    #[test]
    fn reject_invalid_constants_and_suffixes() {
        for text in ["", "x", "0x", "0X", "0xg", "0x1g", "08", "09",
                     "123ll", "123llu", "123uu", "123lul", "12uz", "0b101",
                     "18446744073709551616", "0xFFFFFFFFFFFFFFFFFFFFFFFF"] {
            assert!(decode_integer_literal(text).is_err(), "{text}");
        }
    }
}
