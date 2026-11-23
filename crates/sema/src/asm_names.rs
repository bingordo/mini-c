//! Resolve GNU extended-asm symbolic operand names to the positional ABI.
//!
//! The C checker owns operand declarations and source names, but never parses
//! template instructions, guesses clobbers or chooses register allocations.
//! Positional binding lets existing TypeId, RA, GAS and ELF paths stay shared.
use std::collections::HashMap;

/// Replace symbolic references with their output-first positional indices.
/// Width modifiers are copied opaquely; machine syntax remains backend-owned.
pub(crate) fn positional_template(template: &str, names: &[Option<&str>]) -> Result<String, String> {
    let mut indices = HashMap::new();
    for (index, name) in names.iter().enumerate() {
        let Some(name) = name else { continue };
        let valid = name.as_bytes().first()
            .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        if !valid {
            return Err(format!("invalid GNU asm operand name [{name}]"));
        }
        if indices.insert(*name, index).is_some() {
            return Err(format!("duplicate GNU asm operand name [{name}]"));
        }
    }

    let mut result = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            result.push(ch);
            continue;
        }
        result.push('%');
        let Some(token) = chars.next() else { continue };
        if token == '%' {
            result.push('%');
            continue;
        }
        let named = if token == '[' {
            true
        } else if token.is_ascii_alphabetic() && chars.peek() == Some(&'[') {
            result.push(token);
            chars.next();
            true
        } else {
            result.push(token);
            false
        };
        if !named { continue; }

        let mut name = String::new();
        loop {
            match chars.next() {
                Some(']') => break,
                Some(c) if c.is_ascii_alphanumeric() || c == '_' => name.push(c),
                Some(_) => return Err("invalid GNU asm symbolic operand reference".into()),
                None => return Err("unterminated GNU asm symbolic operand reference".into()),
            }
        }
        let index = indices.get(name.as_str())
            .ok_or_else(||format!("unknown GNU asm symbolic operand [{name}]"))?;
        result.push_str(&index.to_string());
    }
    Ok(result)
}
