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

#[test]
fn ascii_character_literals_keep_expression_semantics() {
    assert_eq!(parse_expression("'a' + 1").unwrap(),
        Expr::Binary(BinOp::Add,Box::new(Expr::Int(97)),Box::new(Expr::Int(1))));
    assert_eq!(parse_expression(r"'\t'").unwrap(),Expr::Int(9));
    for source in ["''","'ab'",r"'\q'",r"'\x'"] {
        assert!(parse_expression_diagnostic(source).is_err(),"{source}");
    }
}

#[test]
fn musl_isblank_body_parses_without_character_literal_rewrites(){
    use mini_c_syntax::{parser::Parser,stream::{ItemSource,ExternalItem},ast::Stmt};
    let original="int isblank(int c)\n{\n\treturn (c == ' ' || c == '\\t');\n}\n";
    let mut parser=Parser::new(original);
    let Some(ExternalItem::Function(f))=parser.next_item().unwrap() else {
        panic!("musl isblank was not parsed as a function");
    };
    assert_eq!(f.name,"isblank");
    let Stmt::Block(statements)=f.body else {panic!("expected function body")};
    assert!(matches!(statements.as_slice(),[Stmt::Return(Some(_))]));
    assert!(parser.next_item().unwrap().is_none());
}


#[test]
fn bitwise_c_precedence_and_unary_not() {
    use mini_c_syntax::ast::UnOp;
    let binary=|op,l,r| Expr::Binary(op,Box::new(l),Box::new(r));
    assert_eq!(parse_expression("a | b ^ c & 7 == 3").unwrap(),
        binary(BinOp::BitOr,name("a"),
            binary(BinOp::BitXor,name("b"),
                binary(BinOp::BitAnd,name("c"),
                    binary(BinOp::Eq,Expr::Int(7),Expr::Int(3))))));
    assert_eq!(parse_expression("~a & 255").unwrap(),
        binary(BinOp::BitAnd,Expr::Unary(UnOp::BitNot,Box::new(name("a"))),Expr::Int(255)));
    // C bitwise operators are eager and distinct from logical && and ||.
    assert_eq!(parse_expression("a && b | c").unwrap(),
        binary(BinOp::And,name("a"),binary(BinOp::BitOr,name("b"),name("c"))));
}
