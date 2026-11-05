use mini_c_syntax::{ast::StorageClass, parser::Parser, stream::{ExternalItem, ItemSource}};

#[test]
fn register_on_local_and_parameter_is_preserved() {
    let source = "int f(register int x) { register int y=x; for (register int i=0; i<3; i++) y+=i; return y; }";
    let Some(ExternalItem::Function(func)) = Parser::new(source).next_item().unwrap()
    else { panic!("expected function"); };
    assert_eq!(func.params[0].storage, StorageClass::Register);
    let mini_c_syntax::ast::Stmt::Block(stmts) = func.body else {
        panic!("expected function body");
    };
    let mini_c_syntax::ast::Stmt::Decl(local) = &stmts[0] else {
        panic!("expected register local");
    };
    assert_eq!(local.storage, StorageClass::Register);
    let mini_c_syntax::ast::Stmt::For { init: Some(init), .. } = &stmts[1] else {
        panic!("expected for with register initializer");
    };
    let mini_c_syntax::ast::Stmt::Decl(index) = init.as_ref() else {
        panic!("expected register loop local");
    };
    assert_eq!(index.storage, StorageClass::Register);
}

#[test]
fn file_scope_register_and_register_typedef_are_rejected() {
    for source in [
        "register int global;",
        "register int f(void);",
        "register int f(void) { return 0; }",
        "typedef register int I;",
        "int f(void) { typedef register int I; return 0; }",
    ] {
        assert!(Parser::new(source).next_item().is_err(), "{source}");
    }
}
