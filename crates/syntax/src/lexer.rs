//! A small, allocation-free token scanner for the C subset.
//! It reports byte spans and rejects invalid characters rather than skipping them.
use crate::ast::Span;
use crate::diag::Diagnostic;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind { Identifier, Number, Punctuation }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token<'a> { pub kind: TokenKind, pub text: &'a str, pub span: Span }

pub fn scan(source: &str) -> Result<Vec<Token<'_>>, Diagnostic> {
    let mut tokens = Vec::new();
    let mut offset = 0;
    while offset < source.len() {
        let rest = &source[offset..];
        let ch = rest.chars().next().expect("nonempty remainder");
        if ch.is_ascii_whitespace() {
            offset += ch.len_utf8();
            continue;
        }
        if rest.starts_with("//") {
            offset += rest.find('\n').unwrap_or(rest.len());
            continue;
        }
        if rest.starts_with("/*") {
            match rest.find("*/") {
                Some(end) => { offset += end + 2; continue; }
                None => return Err(Diagnostic::new(Span { lo: offset, hi: source.len() }, "unterminated block comment")),
            }
        }
        let start = offset;
        let kind = if ch.is_ascii_alphabetic() || ch == '_' {
            offset += 1;
            while offset < source.len() && (source.as_bytes()[offset].is_ascii_alphanumeric() || source.as_bytes()[offset] == b'_') { offset += 1; }
            TokenKind::Identifier
        } else if ch.is_ascii_digit() {
            offset += 1;
            while offset < source.len() && source.as_bytes()[offset].is_ascii_digit() { offset += 1; }
            TokenKind::Number
        } else {
            let op = ["<<=", ">>=", "...", "==", "!=", "<=", ">=", "&&", "||", "++", "--", "->", "+=", "-=", "*=", "/=", "%=", "<<", ">>", "&=", "|=", "^="]
                .iter().find(|&&op| rest.starts_with(op)).copied();
            if let Some(op) = op { offset += op.len(); }
            else if "{}()[];,?:~!+-*/%<>=&|^.#".contains(ch) { offset += 1; }
            else { return Err(Diagnostic::new(Span { lo: start, hi: start + ch.len_utf8() }, "invalid character")); }
            TokenKind::Punctuation
        };
        tokens.push(Token { kind, text: &source[start..offset], span: Span { lo: start, hi: offset } });
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tokens_and_comments() {
        let tokens = scan("int x /*comment*/ = 42; // tail\n x+=2;").unwrap();
        assert_eq!(tokens.iter().map(|t| t.text).collect::<Vec<_>>(), ["int", "x", "=", "42", ";", "x", "+=", "2", ";"]);
    }
    #[test]
    fn invalid_utf8_character_span() {
        let error = scan("int β;").unwrap_err();
        assert_eq!(error.span, Span { lo: 4, hi: 6 });
    }
    #[test]
    fn unterminated_comment() {
        assert!(scan("x /* oops").unwrap_err().message.contains("unterminated"));
    }
}
