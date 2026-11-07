//! Decimal C floating constants: lexical syntax and source-preserving rank.
//!
//! These constants are not native floating operations yet. Preserve the text
//! verbatim so a later backend can round according to the target format.
use crate::ast::FloatRank;

/// Validate the complete decimal floating token, including an optional C
/// f/F or l/L suffix. Hexadecimal floating constants are deliberately
/// unsupported until their p-exponent and target rounding are implemented.
pub fn decimal_float_rank(text: &str) -> Result<FloatRank, &'static str> {
    let bytes = text.as_bytes();
    if bytes.is_empty() || !text.is_ascii() || text.starts_with("0x") || text.starts_with("0X") {
        return Err("unsupported floating constant");
    }
    let (limit, rank) = match bytes.last().copied() {
        Some(b'f' | b'F') => (bytes.len() - 1, FloatRank::Float),
        Some(b'l' | b'L') => (bytes.len() - 1, FloatRank::LongDouble),
        _ => (bytes.len(), FloatRank::Double),
    };
    let mut i = 0;
    while i < limit && bytes[i].is_ascii_digit() { i += 1; }
    let int_digits = i;
    let mut dot = false;
    let mut fractional_digits = 0;
    if i < limit && bytes[i] == b'.' {
        dot = true;
        i += 1;
        let start = i;
        while i < limit && bytes[i].is_ascii_digit() { i += 1; }
        fractional_digits = i - start;
    }
    if int_digits == 0 && fractional_digits == 0 {
        return Err("floating constant requires decimal digits");
    }
    let mut exponent = false;
    if i < limit && matches!(bytes[i], b'e' | b'E') {
        exponent = true;
        i += 1;
        if i < limit && matches!(bytes[i], b'+' | b'-') { i += 1; }
        let start = i;
        while i < limit && bytes[i].is_ascii_digit() { i += 1; }
        if i == start { return Err("floating exponent requires decimal digits"); }
    }
    if i != limit || !(dot || exponent) {
        return Err("unsupported floating constant or suffix");
    }
    Ok(rank)
}

/// Full C99 hexadecimal float spelling (not decimal FP arithmetic).
/// The original lexeme is retained for target-specific rounding later.
pub fn floating_rank(text: &str) -> Result<FloatRank, &'static str> {
    if text.starts_with("0x") || text.starts_with("0X") {
        return hex_float_rank(text);
    }
    decimal_float_rank(text)
}

fn hex_float_rank(text: &str) -> Result<FloatRank, &'static str> {
    let bytes = text.as_bytes();
    let (limit, rank) = match bytes.last().copied() {
        Some(b'f' | b'F') => (bytes.len() - 1, FloatRank::Float),
        Some(b'l' | b'L') => (bytes.len() - 1, FloatRank::LongDouble),
        _ => (bytes.len(), FloatRank::Double),
    };
    let mut i = 2; // required 0x or 0X prefix, already checked above
    let start = i;
    while i < limit && bytes[i].is_ascii_hexdigit() { i += 1; }
    let mut digits = i - start;
    if i < limit && bytes[i] == b'.' {
        i += 1;
        let begin = i;
        while i < limit && bytes[i].is_ascii_hexdigit() { i += 1; }
        digits += i - begin;
    }
    if digits == 0 || i >= limit || !matches!(bytes[i], b'p' | b'P') {
        return Err("hexadecimal floating constant requires a significand and p exponent");
    }
    i += 1;
    if i < limit && matches!(bytes[i], b'+' | b'-') { i += 1; }
    let begin = i;
    while i < limit && bytes[i].is_ascii_digit() { i += 1; }
    if begin == i || i != limit {
        return Err("hexadecimal floating exponent requires decimal digits");
    }
    Ok(rank)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csmith_hex_floating_tokens_parse_without_numeric_rounding() {
        for (text, rank) in [
            ("0x1.0p-100f", FloatRank::Float),
            ("0x1.0p-28f", FloatRank::Float),
            ("0x1.0p100f", FloatRank::Float),
            ("0x1p+2", FloatRank::Double),
            ("0X.8P+3L", FloatRank::LongDouble),
            ("0x1.fp-2", FloatRank::Double),
        ] {
            assert_eq!(floating_rank(text), Ok(rank), "{text}");
        }
        for invalid in ["0x1.0", "0x.p1", "0x1p", "0x1p+", "0x1p-2ff",
                        "0x1p+3u", "0x1p+1.0", "0x1p+1e2"] {
            assert!(floating_rank(invalid).is_err(), "{invalid}");
        }
        // Integer constants must remain integer tokens (the lexer tests this).
        assert!(floating_rank("0x1f").is_err());
    }

    #[test]
    fn accepts_complete_decimal_float_spelling_and_retains_rank() {
        for (literal, rank) in [
            ("0.5f", FloatRank::Float), ("0.5F", FloatRank::Float),
            (".5f", FloatRank::Float), ("1.f", FloatRank::Float),
            ("0.0", FloatRank::Double), ("1.", FloatRank::Double),
            ("1e+12", FloatRank::Double), ("1E-12", FloatRank::Double),
            ("1.2E+3L", FloatRank::LongDouble),
            ("3.40282346638528859811704183484516925e+38F", FloatRank::Float),
        ] {
            assert_eq!(decimal_float_rank(literal), Ok(rank), "{literal}");
        }
    }

    #[test]
    fn rejects_malformed_pp_numbers_without_partial_numeric_acceptance() {
        for literal in [
            "1f", "1L", ".f", "1e", "1e+", "1e-",
            "0.5ff", "0.5u", "1.2.3", "1e++3", "1e-+2",
            "1._2", "0.5f_suffix",
        ] {
            assert!(decimal_float_rank(literal).is_err(), "{literal}");
        }
    }
}
