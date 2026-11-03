//! C integer constant decoding shared by the incremental parser and LALRPOP.
//!
//! Unsuffixed hexadecimal and octal constants fit in `int` only for this
//! initial subset. Larger nondecimal constants need C's unsigned type-selection
//! rules and MUST be rejected rather than misrepresented as decimal literals.
//! Decimal literals retain the existing target-dependent int/long behavior.

pub fn parse_integer_literal(text: &str) -> Result<u64, &'static str> {
    if text.is_empty() || !text.as_bytes()[0].is_ascii_digit() {
        return Err("expected integer literal");
    }
    let (digits, base, nondecimal) =
        if text.starts_with("0x") || text.starts_with("0X") {
            (&text[2..], 16, true)
        } else if text.starts_with('0') && text.len() > 1 {
            (text, 8, true)
        } else {
            (text, 10, false)
        };
    if digits.is_empty() {
        return Err("hexadecimal integer constant needs at least one digit");
    }
    let value = u64::from_str_radix(digits, base)
        .map_err(|_| "invalid or overflowing integer constant")?;
    // The AST currently holds only value, not literal base. The semantic
    // checker therefore cannot select unsigned int for larger hex/octal
    // constants. Restrict this slice to the unambiguous signed-int range.
    if nondecimal && value > i32::MAX as u64 {
        return Err("nondecimal integer constant needs unsigned literal typing (unsupported)");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn c_decimal_hex_and_octal() {
        for (text, value) in [
            ("0", 0), ("9", 9), ("127", 127), ("2147483648", 2147483648),
            ("0x7f", 127), ("0X2A", 42), ("0xff", 255), ("0xABC", 2748),
            ("0377", 255), ("077", 63),
        ] {
            assert_eq!(parse_integer_literal(text), Ok(value), "{text}");
        }
    }
    #[test]
    fn reject_malformed_or_semantically_unrepresentable_literals() {
        for text in ["", "x", "0x", "0X", "0xg", "0x1g", "08", "09",
                     "123u", "0xFFFFFFFF", "0x80000000", "037777777777",
                     "18446744073709551616", "0xFFFFFFFFFFFFFFFFFFFFFFFF"] {
            assert!(parse_integer_literal(text).is_err(), "{text}");
        }
    }
}
