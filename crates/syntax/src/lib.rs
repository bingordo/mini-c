//! Parser entry points; downstream crates depend on the AST, not LALRPOP.
pub mod ast;
pub mod diag;
pub mod lexer;
pub mod parser;
pub mod scopes;
pub mod source;
pub mod stream;
#[path = "grammar.rs"]
#[allow(clippy::all)]
pub mod grammar;

/// Parse an expression and return a structured, byte-accurate diagnostic.
/// This API is suitable for embedding; it never prints or exits.
pub fn parse_expression_diagnostic(source: &str) -> Result<ast::Expr, diag::Diagnostic> {
    use lalrpop_util::ParseError;
    grammar::ExpressionParser::new()
        .parse(source)
        .map_err(|error| {
            let (span, message) = match error {
                ParseError::InvalidToken { location } => (
                    ast::Span {
                        lo: location,
                        hi: location,
                    },
                    "invalid token".to_owned(),
                ),
                ParseError::UnrecognizedEof { location, expected } => (
                    ast::Span {
                        lo: location,
                        hi: location,
                    },
                    format!("unexpected end of input; expected {}", expected.join(", ")),
                ),
                ParseError::UnrecognizedToken {
                    token: (lo, _, hi),
                    expected,
                } => (
                    ast::Span { lo, hi },
                    format!("unexpected token; expected {}", expected.join(", ")),
                ),
                ParseError::ExtraToken { token: (lo, _, hi) } => {
                    (ast::Span { lo, hi }, "extra token".to_owned())
                }
                ParseError::User { error } => (
                    ast::Span {
                        lo: 0,
                        hi: source.len(),
                    },
                    error.to_string(),
                ),
            };
            diag::Diagnostic::new(span, message)
        })
}

pub fn parse_expression(source: &str) -> Result<ast::Expr, String> {
    parse_expression_diagnostic(source).map_err(|e| e.message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast::{BinOp, Expr};
    #[test]
    fn structured_error_location() {
        let e = parse_expression_diagnostic("1 + )").unwrap_err();
        assert_eq!(e.span, ast::Span { lo: 4, hi: 5 });
        assert!(e.message.contains("unexpected"));
    }
    #[test]
    fn structured_eof_location() {
        let e = parse_expression_diagnostic("1 +").unwrap_err();
        assert_eq!(e.span, ast::Span { lo: 3, hi: 3 });
    }
    #[test]
    fn precedence() {
        let result = parse_expression("1 + 2 * 3").unwrap();
        assert_eq!(
            result,
            Expr::Binary(
                BinOp::Add,
                Box::new(Expr::Int(1)),
                Box::new(Expr::Binary(
                    BinOp::Mul,
                    Box::new(Expr::Int(2)),
                    Box::new(Expr::Int(3))
                ))
            )
        );
    }
    #[test]
    fn overflowing_integer_is_error_not_panic() {
        assert!(parse_expression("18446744073709551616").is_err());
    }
    #[test]
    fn function_calls_and_arguments() {
        assert_eq!(
            parse_expression("f(1, 2 + 3)").unwrap(),
            Expr::Call(
                Box::new(Expr::Name("f".into())),
                vec![
                    Expr::Int(1),
                    Expr::Binary(BinOp::Add, Box::new(Expr::Int(2)), Box::new(Expr::Int(3))),
                ],
            ),
        );
        assert_eq!(
            parse_expression("factory()(x)").unwrap(),
            Expr::Call(
                Box::new(Expr::Call(Box::new(Expr::Name("factory".into())), vec![])),
                vec![Expr::Name("x".into())],
            ),
        );
        assert!(parse_expression("f(1,)").is_err());
    }
    #[test]
    fn names_and_parentheses() {
        assert!(parse_expression("(alpha + 4) / 2").is_ok());
        assert!(parse_expression("1 +").is_err());
    }
}
