//! Statement-attribute parsing keeps musttail as a source guarantee, not
//! an ordinary return plus optimization hint.
use mini_c_syntax::{parser::Parser,stream::{ItemSource,ExternalItem},ast::{Stmt,Expr}};

#[test]
fn parses_clang_musttail_for_nested_direct_and_void_returns() {
    for src in [
        "int f(int x){ [[clang::musttail]] return f(x); }",
        "void f(int x){ { [[clang::musttail]] return f(x); } }",
    ] {
        let Some(ExternalItem::Function(f))=Parser::new(src).next_item().unwrap()
        else {panic!("function expected")};
        let Stmt::Block(mut stmts)=f.body else {panic!("body expected")};
        let node=stmts.pop().unwrap();
        let annotated=match node {
            Stmt::Block(nested)=>nested.into_iter().next().unwrap(),
            other=>other,
        };
        assert!(matches!(annotated,Stmt::MustTailReturn(Expr::Call(..))),
            "annotation dropped: {src}");
    }
}

#[test]
fn rejects_invalid_or_unknown_musttail_attribute_syntax() {
    for src in [
        "int f(void){ [[clang::musttail]] return; }",
        "int f(void){ [[unknown::musttail]] return f(); }",
        "int f(void){ [[clang::unknown]] return f(); }",
        "int f(void){ [[clang::musttail] return f(); }",
        "int f(void){ [[clang::musttail]] f(); }",
    ] {
        assert!(Parser::new(src).next_item().is_err(),"invalid attribute accepted: {src}");
    }
}

#[test]
fn unannotated_return_keeps_ordinary_ast_semantics() {
    let Some(ExternalItem::Function(f))=Parser::new(
        "int f(int a){ return f(a); }").next_item().unwrap()
    else {panic!("function expected")};
    assert!(matches!(f.body,Stmt::Block(ref body)
        if matches!(&body[0],Stmt::Return(Some(Expr::Call(..))))));
}
