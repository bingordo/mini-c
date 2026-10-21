//! Token adapter for the checked-in LALRPOP expression parser.
//!
//! Reuses the incremental C scanner so regular builds need no regex lexer.
use crate::{
    ast::Span,
    diag::Diagnostic,
    lexer::{Scanner, TokenKind},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExprToken<'a> {
    Ident(&'a str),
    Number(&'a str),
    Invalid(&'a str),
    Assign,
    OrOr,
    AndAnd,
    EqEq,
    NotEq,
    LessEq,
    GreaterEq,
    Less,
    Greater,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    Amp,
    LParen,
    RParen,
    Comma,
}

/// Return byte offsets and borrowed token payloads for the generated parser.
///
/// Unknown (but otherwise lexically valid) punctuation is retained as an
/// Invalid token so the parser can report an unexpected-token diagnostic at
/// the correct source location instead of silently discarding input.
pub fn tokenize(source: &str) -> Result<Vec<(usize, ExprToken<'_>, usize)>, Diagnostic> {
    let mut scanner = Scanner::new(source);
    let mut result = Vec::new();
    while let Some(token) = scanner.next_token(None)? {
        let kind = match token.kind {
            TokenKind::Number => ExprToken::Number(token.text),
            TokenKind::Identifier | TokenKind::TypedefName => ExprToken::Ident(token.text),
            TokenKind::Punctuation => match token.text {
                "=" => ExprToken::Assign,
                "||" => ExprToken::OrOr,
                "&&" => ExprToken::AndAnd,
                "==" => ExprToken::EqEq,
                "!=" => ExprToken::NotEq,
                "<=" => ExprToken::LessEq,
                ">=" => ExprToken::GreaterEq,
                "<" => ExprToken::Less,
                ">" => ExprToken::Greater,
                "+" => ExprToken::Plus,
                "-" => ExprToken::Minus,
                "*" => ExprToken::Star,
                "/" => ExprToken::Slash,
                "%" => ExprToken::Percent,
                "!" => ExprToken::Bang,
                "&" => ExprToken::Amp,
                "(" => ExprToken::LParen,
                ")" => ExprToken::RParen,
                "," => ExprToken::Comma,
                _ => ExprToken::Invalid(token.text),
            },
        };
        let Span { lo, hi } = token.span;
        result.push((lo, kind, hi));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_token_kinds_and_byte_offsets() {
        let tokens = tokenize(" a != 23; ").unwrap();
        assert_eq!(
            tokens,
            vec![
                (1, ExprToken::Ident("a"), 2),
                (3, ExprToken::NotEq, 5),
                (6, ExprToken::Number("23"), 8),
                (8, ExprToken::Invalid(";"), 9),
            ]
        );
    }

    #[test]
    fn multi_character_operators_are_not_split() {
        let tokens = tokenize("a<=1 && b>=2 || c==3").unwrap();
        assert_eq!(tokens[1].1, ExprToken::LessEq);
        assert_eq!(tokens[3].1, ExprToken::AndAnd);
        assert_eq!(tokens[5].1, ExprToken::GreaterEq);
        assert_eq!(tokens[7].1, ExprToken::OrOr);
        assert_eq!(tokens[9].1, ExprToken::EqEq);
    }

    #[test]
    fn token_spans_remain_absolute_across_comments() {
        let tokens = tokenize(" x/* skip */+ y").unwrap();
        assert_eq!(
            tokens,
            vec![
                (1, ExprToken::Ident("x"), 2),
                (12, ExprToken::Plus, 13),
                (14, ExprToken::Ident("y"), 15),
            ]
        );
    }

    #[test]
    fn lexical_errors_retain_source_spans() {
        let err = tokenize("a + β").unwrap_err();
        assert_eq!(err.span, Span { lo: 4, hi: 6 });
        assert!(tokenize("123abc").is_err());
    }
}
