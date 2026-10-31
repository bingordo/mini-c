//! Decode the supported narrow C string literal spelling to bytes.
//! The terminating NUL is part of the resulting array object.
pub fn decode_narrow_string(text: &str) -> Result<Vec<u8>, &'static str> {
    let b = text.as_bytes();
    if b.len() < 2 || b[0] != b'"' || b[b.len()-1] != b'"' {
        return Err("invalid string literal delimiters");
    }
    let mut out = Vec::new();
    let mut i = 1;
    let end = b.len()-1;
    while i < end {
        if b[i] == b'"' || b[i] == b'\n' || b[i] == b'\r' {
            return Err("unescaped quote or newline in string literal");
        }
        if b[i] != b'\\' {
            if !b[i].is_ascii() { return Err("non-ASCII narrow string unsupported"); }
            out.push(b[i]);
            i += 1;
            continue;
        }
        i += 1;
        if i == end { return Err("truncated string escape"); }
        let start = i;
        if b[i] == b'x' {
            i += 1;
            while i < end && b[i].is_ascii_hexdigit() { i += 1; }
            if i == start + 1 { return Err("hex escape needs a digit"); }
        } else if matches!(b[i], b'0'..=b'7') {
            i += 1;
            while i < end && i < start+3 && matches!(b[i], b'0'..=b'7') { i += 1; }
        } else {
            i += 1;
        }
        // Reuse the character constant decoder for identical C escape rules.
        let mut quoted = Vec::with_capacity(i-start+3);
        quoted.push(b'\'');
        quoted.push(b'\\');
        quoted.extend_from_slice(&b[start..i]);
        quoted.push(b'\'');
        let spelling = std::str::from_utf8(&quoted).map_err(|_| "invalid escape encoding")?;
        let value = crate::char_literal::parse_ascii_character(spelling)?;
        out.push(value as u8);
    }
    out.push(0);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_narrow_strings_with_nul_terminator() {
        assert_eq!(decode_narrow_string(r#""hello""#), Ok(b"hello\0".to_vec()));
        assert_eq!(decode_narrow_string(r#""a\n\x42\101\0""#), Ok(b"a\nBA\0\0".to_vec()));
        assert_eq!(decode_narrow_string(r#""\"\\\?""#), Ok(b"\"\\?\0".to_vec()));
    }
    #[test]
    fn rejects_invalid_escapes() {
        for s in [r#""\x""#, r#""\q""#, r#""\400""#, r#""\x100""#, "\"unterminated", "\"line\nbreak\""] {
            assert!(decode_narrow_string(s).is_err(), "{s:?}");
        }
    }
}
