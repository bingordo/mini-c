use mini_c_syntax::{ast::Type, parser::Parser, stream::{ExternalItem, ItemSource}};

#[test]
fn printf_variadic_prototype_is_not_fixed_arity() {
    let source = "extern int printf(const char *restrict fmt, ...);";
    let Some(ExternalItem::Declaration(decl)) = Parser::new(source).next_item().unwrap()
    else { panic!("expected prototype"); };
    let Type::VariadicFunction { result, params } = decl.ty
    else { panic!("ellipsis must be retained in the function type"); };
    assert_eq!(*result, Type::Int);
    assert_eq!(params.len(), 1);
    assert_ne!(Type::VariadicFunction { result: Box::new(Type::Int), params: vec![Type::Int] },
               Type::Function { result: Box::new(Type::Int), params: vec![Type::Int] });
}

#[test]
fn function_pointer_variadic_type_retains_ellipsis() {
    let source = "int (*fp)(const char *, ...);";
    let Some(ExternalItem::Declaration(decl)) = Parser::new(source).next_item().unwrap()
    else { panic!("expected pointer declaration"); };
    assert!(matches!(decl.ty, Type::Pointer(inner)
        if matches!(*inner, Type::VariadicFunction { .. })));
}

#[test]
fn malformed_variadic_prototypes_are_rejected() {
    for source in [
        "int printf(...);",
        "int printf(int, ..., int);",
        "int printf(int, ... , ...);",
        "int printf(int, ... int);",
        "int printf(int,);",
    ] {
        assert!(Parser::new(source).next_item().is_err(), "{source}");
    }
}
