//! A small, dependency-free token scanner for the C subset.
//! It reports byte spans and rejects invalid characters rather than skipping them.
use crate::ast::Span;
use crate::diag::Diagnostic;
use crate::scopes::Scopes;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Identifier,
    TypedefName,
    Number,
    Character,
    String,
    Punctuation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token<'a> {
    pub kind: TokenKind,
    pub text: &'a str,
    pub span: Span,
}

pub fn scan(source: &str) -> Result<Vec<Token<'_>>, Diagnostic> {
    Scanner::new(source).collect()
}

/// Classifies identifiers against the caller's current ordinary-identifier scope.
/// Re-scan after scope changes; parsing must not preclassify an entire translation unit.
pub fn scan_with_scopes<'a>(
    source: &'a str,
    scopes: Option<&Scopes>,
) -> Result<Vec<Token<'a>>, Diagnostic> {
    let mut scanner = Scanner::new(source);
    let mut tokens = Vec::new();
    while let Some(token) = scanner.next_token(scopes)? {
        tokens.push(token);
    }
    Ok(tokens)
}

/// Incremental scanner: the parser can change the scope between calls to next_token.
/// This avoids classifying future typedef names before declarations take effect.
pub struct Scanner<'a> {
    source: &'a str,
    offset: usize,
}

impl<'a> Scanner<'a> {
    pub fn new(source: &'a str) -> Self {
        Self { source, offset: 0 }
    }
    pub fn next_token(&mut self, scopes: Option<&Scopes>) -> Result<Option<Token<'a>>, Diagnostic> {
        let source = self.source;
        let mut offset = self.offset;
        while offset < source.len() {
            let rest = &source[offset..];
            let ch = rest.chars().next().expect("nonempty remainder");
            if ch.is_ascii_whitespace() {
                offset += ch.len_utf8();
                continue;
            }
            if ch == '#' && source[..offset].rsplit('\n').next().is_some_and(|prefix| prefix.trim().is_empty()) {
                offset += rest.find('\n').unwrap_or(rest.len());
                continue;
            }
            if rest.starts_with("//") {
                offset += rest.find('\n').unwrap_or(rest.len());
                continue;
            }
            if rest.starts_with("/*") {
                match rest.find("*/") {
                    Some(end) => {
                        offset += end + 2;
                        continue;
                    }
                    None => {
                        self.offset = source.len();
                        return Err(Diagnostic::coded(
                            Span {
                                lo: offset,
                                hi: source.len(),
                            },
                            crate::diag::DiagnosticCode::LexicalUnterminatedComment,
                            "unterminated block comment",
                        ));
                    }
                }
            }
            let start = offset;
            let kind = if ch.is_ascii_alphabetic() || ch == '_' {
                offset += 1;
                while offset < source.len()
                    && (source.as_bytes()[offset].is_ascii_alphanumeric()
                        || source.as_bytes()[offset] == b'_')
                {
                    offset += 1;
                }
                if scopes.is_some_and(|scope| scope.is_typedef(&source[start..offset])) {
                    TokenKind::TypedefName
                } else {
                    TokenKind::Identifier
                }
            } else if ch == '\'' {
                // Keep the original spelling and byte span. The shared
                // decoder enforces one ASCII execution character (including
                // C escapes), instead of a benchmark-specific rewrite.
                offset += 1;
                let mut closed=false;
                while offset<source.len() {
                    let b=source.as_bytes()[offset];
                    if b==b'\n' || b==b'\r' {break}
                    if b==b'\\' {
                        offset+=1;
                        if offset<source.len() && !matches!(source.as_bytes()[offset],b'\n'|b'\r'){
                            offset+=1;
                        }
                    } else if b==b'\'' {
                        offset+=1;
                        closed=true;
                        break;
                    } else {
                        offset+=1;
                    }
                }
                let literal=&source[start..offset];
                if !closed {
                    self.offset=offset;
                    return Err(Diagnostic::coded(
                        Span {lo:start,hi:offset},
                        crate::diag::DiagnosticCode::LexicalInvalidCharacter,
                        "unterminated character constant",
                    ));
                }
                if let Err(message)=crate::char_literal::parse_ascii_character(literal) {
                    self.offset=offset;
                    return Err(Diagnostic::coded(
                        Span {lo:start,hi:offset},
                        crate::diag::DiagnosticCode::LexicalInvalidCharacter,
                        message,
                    ));
                }
                TokenKind::Character
            } else if ch == '"' {
                // Preserve the spelling for the parser's later escape decoding.
                // A backslash shields the following byte from ending the token.
                offset += 1;
                let mut closed = false;
                while offset < source.len() {
                    match source.as_bytes()[offset] {
                        b'\n' | b'\r' => break,
                        b'\\' => {
                            offset += 1;
                            if offset < source.len()
                                && !matches!(source.as_bytes()[offset], b'\n' | b'\r')
                            {
                                offset += 1;
                            }
                        }
                        b'"' => {
                            offset += 1;
                            closed = true;
                            break;
                        }
                        _ => offset += 1,
                    }
                }
                if !closed {
                    self.offset = offset;
                    return Err(Diagnostic::coded(
                        Span { lo: start, hi: offset },
                        crate::diag::DiagnosticCode::LexicalInvalidCharacter,
                        "unterminated string literal",
                    ));
                }
                TokenKind::String
            } else if ch.is_ascii_digit() {
                offset += 1;
                while offset < source.len() && source.as_bytes()[offset].is_ascii_digit() {
                    offset += 1;
                }
                if offset < source.len()
                    && (source.as_bytes()[offset].is_ascii_alphabetic()
                        || source.as_bytes()[offset] == b'_')
                {
                    // Advance past the whole unsupported suffix. Recovery must
                    // never encounter the same error at the same source offset.
                    while offset < source.len()
                        && (source.as_bytes()[offset].is_ascii_alphanumeric()
                            || source.as_bytes()[offset] == b'_')
                    {
                        offset += 1;
                    }
                    self.offset = offset;
                    return Err(Diagnostic::coded(
                        Span {
                            lo: start,
                            hi: offset,
                        },
                        crate::diag::DiagnosticCode::LexicalNumericSuffix,
                        "unsupported numeric literal or suffix",
                    ));
                }
                TokenKind::Number
            } else {
                let op = [
                    "<<=", ">>=", "...", "==", "!=", "<=", ">=", "&&", "||", "++", "--", "->",
                    "+=", "-=", "*=", "/=", "%=", "<<", ">>", "&=", "|=", "^=",
                ]
                .iter()
                .find(|&&op| rest.starts_with(op))
                .copied();
                if let Some(op) = op {
                    offset += op.len();
                } else if "{}()[];,?:~!+-*/%<>=&|^.#".contains(ch) {
                    offset += 1;
                } else {
                    self.offset = start + ch.len_utf8();
                    return Err(Diagnostic::coded(
                        Span {
                            lo: start,
                            hi: self.offset,
                        },
                        crate::diag::DiagnosticCode::LexicalInvalidCharacter,
                        "invalid character",
                    ));
                }
                TokenKind::Punctuation
            };
            self.offset = offset;
            return Ok(Some(Token {
                kind,
                text: &source[start..offset],
                span: Span {
                    lo: start,
                    hi: offset,
                },
            }));
        }
        self.offset = offset;
        Ok(None)
    }

    pub fn collect(mut self) -> Result<Vec<Token<'a>>, Diagnostic> {
        let mut tokens = Vec::new();
        while let Some(token) = self.next_token(None)? {
            tokens.push(token);
        }
        Ok(tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tokens_and_comments() {
        let tokens = scan("int x /*comment*/ = 42; // tail\n x+=2;").unwrap();
        assert_eq!(
            tokens.iter().map(|t| t.text).collect::<Vec<_>>(),
            ["int", "x", "=", "42", ";", "x", "+=", "2", ";"]
        );
    }
    #[test]
    fn ignores_preprocessor_line_markers() {
        let tokens = scan("# 1 \"header.h\" 1\nint value;\n#line 9 \"main.c\"\nint next;").unwrap();
        assert_eq!(tokens.iter().map(|t| t.text).collect::<Vec<_>>(), ["int", "value", ";", "int", "next", ";"]);
    }
    #[test]
    fn invalid_utf8_character_span() {
        let error = scan("int β;").unwrap_err();
        assert_eq!(error.span, Span { lo: 4, hi: 6 });
    }
    #[test]
    fn typedef_shadowing_classification() {
        use crate::scopes::Binding;
        let mut scopes = Scopes::new();
        scopes.declare("T", Binding::Typedef);
        assert_eq!(
            scan_with_scopes("T x;", Some(&scopes)).unwrap()[0].kind,
            TokenKind::TypedefName
        );
        scopes.enter();
        scopes.declare("T", Binding::Ordinary);
        assert_eq!(
            scan_with_scopes("T x;", Some(&scopes)).unwrap()[0].kind,
            TokenKind::Identifier
        );
    }
    #[test]
    fn incremental_typedef_scope_changes() {
        use crate::scopes::Binding;
        let mut scopes = Scopes::new();
        let mut scanner = Scanner::new("T T T");
        assert_eq!(
            scanner.next_token(Some(&scopes)).unwrap().unwrap().kind,
            TokenKind::Identifier
        );
        scopes.declare("T", Binding::Typedef);
        assert_eq!(
            scanner.next_token(Some(&scopes)).unwrap().unwrap().kind,
            TokenKind::TypedefName
        );
        scopes.enter();
        scopes.declare("T", Binding::Ordinary);
        assert_eq!(
            scanner.next_token(Some(&scopes)).unwrap().unwrap().kind,
            TokenKind::Identifier
        );
        assert!(scanner.next_token(Some(&scopes)).unwrap().is_none());
    }
    #[test]
    fn c_character_tokens_have_original_spans() {
        let tokens=scan("x='a'; y='\\n'; z='\\'';").unwrap();
        let literals:Vec<_>=tokens.into_iter().filter(|t|t.kind==TokenKind::Character).collect();
        assert_eq!(literals.iter().map(|t|t.text).collect::<Vec<_>>(),
            ["'a'",r"'\n'",r"'\''"]);
        assert_eq!(literals[0].span,Span {lo:2,hi:5});
        for text in ["''","'ab'",r"'\x'",r"'\q'","'unterminated"]{
            assert!(scan(text).is_err(),"{text}");
        }
    }
    #[test]
    fn unsupported_suffix_rejected() {
        assert!(scan("123abc").is_err());
    }
    #[test]
    fn unterminated_comment() {
        assert!(scan("x /* oops")
            .unwrap_err()
            .message
            .contains("unterminated"));
    }
    #[test]
    fn string_tokens_preserve_escapes_and_spans() {
        let tokens = scan(r#"char *p="a\"b";"#).unwrap();
        let literal = tokens.iter().find(|t| t.kind == TokenKind::String).unwrap();
        assert_eq!(literal.text, r#""a\"b""#);
        assert_eq!(literal.span, Span { lo: 8, hi: 14 });
        for bad in ["\"unfinished", "\"line\nbreak\"", "\"trailing\\"] {
            assert!(scan(bad).is_err(), "{bad:?}");
        }
    }

}
