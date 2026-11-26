//! Conversion of checked C floating literal spellings to IEEE binary32/64
//! encodings. Both native and reference backends can use this API; hex
//! literals are rounded directly to the requested precision (never through
//! an intermediate host f64). Only a bounded leading significand and sticky
//! bit are retained, so long literals require linear time and constant space.

/// Decode a C float literal at binary32 precision.
pub fn binary32_bits(text: &str) -> Result<u32, String> {
    let digits = text.strip_suffix(['f', 'F']).unwrap_or(text);
    if is_hex(digits) {
        Ok(hex_bits(digits, 24, -126, 127, 8)? as u32)
    } else {
        digits.parse::<f32>().map(f32::to_bits)
            .map_err(|e| format!("invalid binary32 literal: {e}"))
    }
}

/// Decode a C double literal at binary64 precision.
pub fn binary64_bits(text: &str) -> Result<u64, String> {
    if is_hex(text) {
        hex_bits(text, 53, -1022, 1023, 11)
    } else {
        text.parse::<f64>().map(f64::to_bits)
            .map_err(|e| format!("invalid binary64 literal: {e}"))
    }
}

fn is_hex(text: &str) -> bool {
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    unsigned.starts_with("0x") || unsigned.starts_with("0X")
}

/// Parse the mandatory signed decimal p-exponent. Saturation is exact for
/// the final IEEE result: a source string cannot contain enough significand
/// digits to cancel an exponent outside the i128 range.
fn exponent(text: &str) -> Result<i128, String> {
    let negative = text.starts_with('-');
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return Err("invalid hexadecimal floating exponent".into());
    }
    let mut value = 0i128;
    for c in digits.bytes() {
        value = value.saturating_mul(10).saturating_add(i128::from(c-b'0'));
    }
    Ok(if negative { -value } else { value })
}

/// Round an exact hexadecimal significand times a power of two directly
/// into the target format, including subnormals and ties-to-even. Keep one
/// guard bit beyond the widest supported precision and OR all lower bits.
fn hex_bits(text: &str, precision: u32, emin: i128, emax: i128,
            exponent_bits: u32) -> Result<u64, String> {
    let negative = text.starts_with('-');
    let sign = u64::from(negative) << (precision - 1 + exponent_bits);
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    let digits = unsigned.strip_prefix("0x").or_else(|| unsigned.strip_prefix("0X"))
        .ok_or("missing hexadecimal prefix")?;
    let (significand, exp) = digits.split_once(['p', 'P'])
        .ok_or("missing hexadecimal floating exponent")?;
    let exp = exponent(exp)?;
    let mut after_point = false;
    let mut fractional = 0i128;
    let mut seen_digit = false;
    let mut length = 0i128;
    const PREFIX_BITS: u32 = 54;
    let mut prefix = 0u64;
    let mut sticky = false;
    for c in significand.bytes() {
        if c == b'.' && !after_point {
            after_point = true;
            continue;
        }
        let nibble = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => return Err("invalid hexadecimal floating significand".into()),
        };
        seen_digit = true;
        if after_point { fractional += 1; }
        for shift in (0..4).rev() {
            let bit = u64::from((nibble >> shift) & 1);
            if length == 0 && bit == 0 { continue; }
            if length < i128::from(PREFIX_BITS) {
                prefix = (prefix << 1) | bit;
            } else {
                sticky |= bit != 0;
            }
            length += 1;
        }
    }
    if !seen_digit { return Err("empty hexadecimal significand".into()); }
    if length == 0 { return Ok(sign); }
    if length < i128::from(PREFIX_BITS) {
        prefix <<= PREFIX_BITS - length as u32;
    }
    let mut e = exp.saturating_add(length - 1 - 4 * fractional);
    let infinity = sign | (((1u64 << exponent_bits) - 1) << (precision - 1));
    if e > emax { return Ok(infinity); }
    let quantum = e.max(emin) - i128::from(precision - 1);
    let keep = e.saturating_sub(quantum).saturating_add(1);
    if keep < 0 { return Ok(sign); }
    let keep = keep as u32;
    let discard = PREFIX_BITS - keep;
    let mut mantissa = prefix >> discard;
    let guard = 1u64 << (discard - 1);
    if prefix & guard != 0 &&
        (sticky || prefix & (guard - 1) != 0 || mantissa & 1 != 0) {
        mantissa += 1;
    }
    let implicit = 1u64 << (precision - 1);
    if e < emin {
        // Rounding can carry the largest subnormal into the minimum normal;
        // its bit pattern is exactly the same as the carry bit.
        return Ok(sign | mantissa);
    }
    if mantissa == implicit << 1 {
        mantissa >>= 1;
        e += 1;
        if e > emax { return Ok(infinity); }
    }
    let biased = (e - emin + 1) as u64;
    Ok(sign | (biased << (precision - 1)) | (mantissa - implicit))
}
