//! Parity tests for the generated expression parser's external-token API.
use mini_c_syntax::{
    ast::{BinOp, Expr, Span},
    parse_expression,
    parse_expression_diagnostic,
};

fn name(s: &str) -> Expr {
    Expr::Name(s.to_owned())
}

#[test]
fn right_associative_assignment() {
    assert_eq!(
        parse_expression("a = b = 7").unwrap(),
        Expr::Assign(
            Box::new(name("a")),
            Box::new(Expr::Assign(Box::new(name("b")), Box::new(Expr::Int(7))))
        )
    );
}

#[test]
fn nested_calls_and_precedence() {
    assert_eq!(
        parse_expression("f(1, 2 * 3)").unwrap(),
        Expr::Call(
            Box::new(name("f")),
            vec![
                Expr::Int(1),
                Expr::Binary(
                    BinOp::Mul,
                    Box::new(Expr::Int(2)),
                    Box::new(Expr::Int(3))
                )
            ]
        )
    );
}

#[test]
fn lexical_and_grammar_errors_do_not_accept_partial_input() {
    for source in ["a; b", "f(1,)", "123abc", "a @ b", "a +", "1 2"] {
        assert!(
            parse_expression_diagnostic(source).is_err(),
            "accepted invalid expression {source:?}"
        );
    }
}

#[test]
fn source_spans_and_overflow_are_preserved() {
    assert_eq!(
        parse_expression_diagnostic("a + )").unwrap_err().span,
        Span { lo: 4, hi: 5 }
    );
    assert_eq!(
        parse_expression_diagnostic("a +").unwrap_err().span,
        Span { lo: 3, hi: 3 }
    );
    assert_eq!(
        parse_expression("18446744073709551615").unwrap(),
        Expr::Int(u64::MAX)
    );
    assert!(parse_expression_diagnostic("18446744073709551616").is_err());
}

#[test]
fn comments_do_not_change_expression_tokenization() {
    assert_eq!(
        parse_expression("  a /*comment*/ + b").unwrap(),
        Expr::Binary(
            BinOp::Add,
            Box::new(name("a")),
            Box::new(name("b"))
        )
    );
}
