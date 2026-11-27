use mini_c_syntax::{parser::{Parser,GnuExtensions},stream::ItemSource};
fn parse(source:&str,mode:GnuExtensions)->Result<(),String>{
    let mut p=Parser::new(source).with_gnu_extensions(mode);
    while p.next_item().map_err(|e|e.message)?.is_some(){}
    Ok(())
}
#[test]
fn extension_permissions_are_lexically_scoped(){
    use GnuExtensions::*;
    for (raw,marked) in [
        ("int f(void){__auto_type a=1;return a;}", "int f(void){__extension__ __auto_type a=1;return a;}"),
        ("__thread int a;", "__extension__ __thread int a;"),
        ("int f(void){asm(\"nop\");return 0;}", "int f(void){__extension__ asm(\"nop\");return 0;}"),
        ("int f(void) __attribute__((unused));","__extension__ int f(void) __attribute__((unused));"),
        ("struct S { int x __attribute__((aligned(4))); };","struct S { __extension__ int x __attribute__((aligned(4))); };"),
    ] {
        assert!(parse(raw,Disabled).is_err(),"{raw}");
        assert!(parse(raw,Enabled).is_ok(),"{raw}");
        assert!(parse(marked,Disabled).is_ok(),"{marked}");
    }
    assert!(parse("int f(void){__extension__ __auto_type x=1;__auto_type y=2;return x+y;}",Disabled).is_err());
    assert!(parse("__extension__ __thread int a,b; __thread int c;",Disabled).is_err());
    assert!(parse("int f(void){return __extension__ (1+2);}",Disabled).is_ok());
}

#[test]
fn statement_expression_permissions_and_scope() {
    use GnuExtensions::*;
    let raw = "int f(void){return ({ __auto_type x=1; x+2; });}";
    let marked = "int f(void){return __extension__ ({ __auto_type x=1; x+2; });}";
    assert!(parse(raw, Disabled).is_err());
    assert!(parse(raw, Enabled).is_ok());
    assert!(parse(marked, Disabled).is_ok());
    assert!(parse("int f(void){int a=__extension__ ({1;}); return ({a;});}", Disabled).is_err());
    assert!(parse("typedef int T; int f(void){int x=({int T=1; T;}); T y=2; return x+y;}", Enabled).is_ok());
    assert!(parse("int f(void){return sizeof ({int x=1; x;});}", Enabled).is_ok());
    assert!(parse("int f(void){return sizeof ({1;});}", Disabled).is_err());
    for bad in ["int f(void){return ({1;};}", "int f(void){return ({1;);}", "int f(void){return ({1});}"] {
        assert!(parse(bad, Enabled).is_err(), "{bad}");
    }
}

#[test]
fn statement_expression_ast_preserves_scope_and_result() {
    use mini_c_syntax::{ast::{Expr, Stmt}, stream::ExternalItem};
    let src = "int f(void){return ({int x=1; x+=2; ({x;});});}";
    let mut parser = Parser::new_with_spans(src);
    let Some(ExternalItem::Function(f)) = parser.next_item().unwrap() else { panic!("expected function") };
    let Stmt::Block(body) = f.body else { panic!("expected block") };
    let Stmt::Return(Some(result)) = &body[0] else { panic!("expected return") };
    let Expr::StatementExpression(stmts) = result.without_location() else { panic!("expected statement expression") };
    assert_eq!(&src[result.location().unwrap().lo..result.location().unwrap().hi], "({int x=1; x+=2; ({x;});})");
    assert_eq!(stmts.len(), 3);
    assert!(matches!(&stmts[0], Stmt::Decl(_)));
    assert!(matches!(&stmts[1], Stmt::Expr(Some(e)) if matches!(e.without_location(), Expr::CompoundAssign(..))));
    assert!(matches!(&stmts[2], Stmt::Expr(Some(e)) if matches!(e.without_location(), Expr::StatementExpression(_))));
}
