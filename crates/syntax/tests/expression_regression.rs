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
fn bitwise_operator_precedence_and_complement() {
    use mini_c_syntax::ast::UnOp;
    let binary = |op, l, r| Expr::Binary(op, Box::new(l), Box::new(r));
    assert_eq!(parse_expression("a | b ^ c & 7 == 3").unwrap(),
        binary(BinOp::BitOr, name("a"),
            binary(BinOp::BitXor, name("b"),
                binary(BinOp::BitAnd, name("c"),
                    binary(BinOp::Eq, Expr::Int(7), Expr::Int(3))))));
    assert_eq!(parse_expression("~a & 255").unwrap(),
        binary(BinOp::BitAnd, Expr::Unary(UnOp::BitNot, Box::new(name("a"))), Expr::Int(255)));
    assert_eq!(parse_expression("a && b | c").unwrap(),
        binary(BinOp::And, name("a"), binary(BinOp::BitOr, name("b"), name("c"))));
}


#[test]
fn c_shift_precedence_between_additive_and_relational() {
    let bin = |op, a, b| Expr::Binary(op, Box::new(a), Box::new(b));
    assert_eq!(parse_expression("a + b << c < d").unwrap(),
        bin(BinOp::Lt,
            bin(BinOp::Shl, bin(BinOp::Add, name("a"), name("b")), name("c")),
            name("d")));
    assert_eq!(parse_expression("a >> b + c").unwrap(),
        bin(BinOp::Shr, name("a"), bin(BinOp::Add, name("b"), name("c"))));
    assert_eq!(parse_expression("a << b << c").unwrap(),
        bin(BinOp::Shl, bin(BinOp::Shl, name("a"), name("b")), name("c")));
}


#[test]
fn compound_assignments_have_assignment_precedence_and_right_associativity() {
    for (operator, binop) in [
        ("&=", BinOp::BitAnd), ("|=", BinOp::BitOr),
        ("^=", BinOp::BitXor), ("<<=", BinOp::Shl), (">>=", BinOp::Shr),
    ] {
        let parsed = parse_expression(&format!("a {operator} b = 3")).unwrap();
        assert_eq!(parsed, Expr::CompoundAssign(binop, Box::new(name("a")),
            Box::new(Expr::Assign(Box::new(name("b")), Box::new(Expr::Int(3))))));
    }
    assert_eq!(parse_expression("a &= b |= 7").unwrap(),
        Expr::CompoundAssign(BinOp::BitAnd, Box::new(name("a")),
            Box::new(Expr::CompoundAssign(BinOp::BitOr,
                Box::new(name("b")), Box::new(Expr::Int(7))))));
    assert_eq!(parse_expression("a + b &= 3").unwrap(),
        Expr::CompoundAssign(BinOp::BitAnd,
            Box::new(Expr::Binary(BinOp::Add, Box::new(name("a")), Box::new(name("b")))),
            Box::new(Expr::Int(3))));
}


#[test]
fn arithmetic_compound_assignments_parse_with_right_associativity() {
    let operators = [
        ("+=", BinOp::Add), ("-=", BinOp::Sub),
        ("*=", BinOp::Mul), ("/=", BinOp::Div), ("%=", BinOp::Mod),
    ];
    for (token, op) in operators {
        assert_eq!(parse_expression(&format!("a {token} b = 2")).unwrap(),
            Expr::CompoundAssign(op, Box::new(name("a")),
                Box::new(Expr::Assign(Box::new(name("b")), Box::new(Expr::Int(2))))));
    }
    assert_eq!(parse_expression("a += b *= 3").unwrap(),
        Expr::CompoundAssign(BinOp::Add, Box::new(name("a")),
            Box::new(Expr::CompoundAssign(BinOp::Mul,
                Box::new(name("b")), Box::new(Expr::Int(3))))));
}


#[test]
fn prefix_and_postfix_updates_have_correct_precedence() {
    let parse = |s: &str| parse_expression(s).unwrap();
    assert_eq!(parse("++a"), Expr::Update {
        target: Box::new(name("a")), increment: true, postfix: false,
    });
    assert_eq!(parse("--a"), Expr::Update {
        target: Box::new(name("a")), increment: false, postfix: false,
    });
    assert_eq!(parse("a++"), Expr::Update {
        target: Box::new(name("a")), increment: true, postfix: true,
    });
    assert_eq!(parse("a--"), Expr::Update {
        target: Box::new(name("a")), increment: false, postfix: true,
    });
    assert_eq!(parse("++a * b--"), Expr::Binary(BinOp::Mul,
        Box::new(parse("++a")), Box::new(parse("b--"))));
    assert_eq!(parse("*p++"), Expr::Unary(
        mini_c_syntax::ast::UnOp::Deref, Box::new(parse("p++"))));
    assert_eq!(parse("(*p)++"), Expr::Update {
        target: Box::new(parse("*p")), increment: true, postfix: true,
    });
}


#[test]
fn comma_is_left_associative_and_below_assignment() {
    let comma = |a, b| Expr::Comma(Box::new(a), Box::new(b));
    assert_eq!(parse_expression("a, b, c").unwrap(),
        comma(comma(name("a"), name("b")), name("c")));
    assert_eq!(parse_expression("a = b, c = d").unwrap(),
        comma(
            Expr::Assign(Box::new(name("a")), Box::new(name("b"))),
            Expr::Assign(Box::new(name("c")), Box::new(name("d"))),
        ));
    assert_eq!(parse_expression("f(1, 2)").unwrap(),
        Expr::Call(Box::new(name("f")), vec![Expr::Int(1), Expr::Int(2)]));
    assert_eq!(parse_expression("f((a = 1, a), 2)").unwrap(),
        Expr::Call(Box::new(name("f")), vec![
            comma(Expr::Assign(Box::new(name("a")), Box::new(Expr::Int(1))), name("a")),
            Expr::Int(2),
        ]));
    assert!(parse_expression("a,").is_err());
}


#[test]
fn conditional_precedence_associativity_and_full_true_expression() {
    let ternary = |cond, yes, no| Expr::Conditional(
        Box::new(cond), Box::new(yes), Box::new(no));
    assert_eq!(parse_expression("a ? b : c ? d : e").unwrap(),
        ternary(name("a"), name("b"),
            ternary(name("c"), name("d"), name("e"))));
    assert_eq!(parse_expression("a || b ? c : d").unwrap(),
        ternary(Expr::Binary(BinOp::Or, Box::new(name("a")), Box::new(name("b"))),
            name("c"), name("d")));
    assert_eq!(parse_expression("a ? b, c : d").unwrap(),
        ternary(name("a"),
            Expr::Comma(Box::new(name("b")), Box::new(name("c"))),
            name("d")));
    assert_eq!(parse_expression("x = c ? a : b").unwrap(),
        Expr::Assign(Box::new(name("x")), Box::new(ternary(
            name("c"), name("a"), name("b")))));
    assert_eq!(parse_expression("f(a ? b : c, d)").unwrap(),
        Expr::Call(Box::new(name("f")), vec![
            ternary(name("a"), name("b"), name("c")), name("d")
        ]));
    for incomplete in ["a ? b", "a ? : c", "a ? b :", "a ? b : c ? d"] {
        assert!(parse_expression(incomplete).is_err(), "accepted {incomplete}");
    }
}
