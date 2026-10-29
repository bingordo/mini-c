//! Single source of truth for the supported C integer character constants.
//! C character constants have type int. Only ASCII execution characters,
//! standard single-byte escapes, 1-3 digit octal and hex escapes up to 0xff
//! are currently accepted. Multicharacter/wide/unicode literals are rejected.
//! The caller provides the entire quoted token including its delimiters.

pub fn parse_ascii_character(text: &str) -> Result<u64, &'static str> {
    let bytes=text.as_bytes();
    if bytes.len()<3 || bytes.first()!=Some(&b'\'') || bytes.last()!=Some(&b'\'') {
        return Err("invalid or unterminated character constant");
    }
    let inner=&bytes[1..bytes.len()-1];
    if inner.is_empty() || !inner.is_ascii(){
        return Err("character constant requires a single ASCII character");
    }
    if inner[0]!=b'\\' {
        if inner.len()==1 && inner[0]!=b'\'' && inner[0]!=b'\n' && inner[0]!=b'\r' {
            return Ok(u64::from(inner[0]));
        }
        return Err("multicharacter and unescaped character constants are unsupported");
    }
    if inner.len()<2 {return Err("truncated character escape")}
    let value=match inner[1] {
        b'0'..=b'7' => {
            if inner.len()>4 || inner[1..].iter().any(|b|!matches!(b,b'0'..=b'7')){
                return Err("octal character escape requires one to three octal digits");
            }
            let mut result=0u32;
            for &d in &inner[1..] { result=result*8+u32::from(d-b'0'); }
            result
        }
        b'x' => {
            if inner.len()<3 {return Err("hex character escape requires a hex digit")}
            let mut result=0u32;
            for &d in &inner[2..]{
                let value=match d {
                    b'0'..=b'9'=>u32::from(d-b'0'),
                    b'a'..=b'f'=>u32::from(d-b'a'+10),
                    b'A'..=b'F'=>u32::from(d-b'A'+10),
                    _=>return Err("invalid hex character escape"),
                };
                result=result.checked_mul(16).and_then(|x|x.checked_add(value))
                    .ok_or("hex character escape exceeds supported byte value")?;
                if result>255 {return Err("hex character escape exceeds supported byte value")}
            }
            result
        }
        b'a' if inner.len()==2=>7,
        b'b' if inner.len()==2=>8,
        b'f' if inner.len()==2=>12,
        b'n' if inner.len()==2=>10,
        b'r' if inner.len()==2=>13,
        b't' if inner.len()==2=>9,
        b'v' if inner.len()==2=>11,
        b'\\' if inner.len()==2=>92,
        b'\'' if inner.len()==2=>39,
        b'"' if inner.len()==2=>34,
        b'?' if inner.len()==2=>63,
        _=>return Err("unsupported character escape"),
    };
    if value>255 {return Err("octal character escape exceeds supported byte value")}
    Ok(u64::from(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn c_character_constants() {
        for (text,value) in [
            ("' '",32),("'a'",97),("'0'",48),("'+'",43),
            (r"'\n'",10),(r"'\t'",9),(r"'\''",39),
            (r"'\\'",92),(r"'\0'",0),(r"'\377'",255),
            (r"'\x41'",65),(r"'\x7f'",127),(r"'\xFF'",255),
        ] {
            assert_eq!(parse_ascii_character(text),Ok(value),"{text}");
        }
    }
    #[test]
    fn malformed_constants_fail_without_panic(){
        for text in [
            "''","'ab'","'unterminated",r"'\x'",r"'\777'",
            r"'\x100'",r"'\q'",r"'\999'",r"'\400'",r"'\8'",
            "'β'",r"'\xFFFFFFFFFFFFFFFFFFFFFFFF'",
            "'\n'",
        ] {
            assert!(parse_ascii_character(text).is_err(),"{text}");
        }
    }
}
