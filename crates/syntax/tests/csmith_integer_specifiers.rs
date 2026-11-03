//! C99 integer declaration specifier order: source-level Csmith/glibc inputs.
use mini_c_syntax::{
    ast::Type, diag::DiagnosticCode, parser::Parser,
    stream::{ExternalItem, ItemSource},
};

#[test]
fn system_size_t_typedef_is_equivalent_to_canonical_unsigned_long() {
    for spelling in [
        "long unsigned int", "unsigned long int", "unsigned long",
        "int unsigned long", "long int unsigned", "int long unsigned",
    ] {
        let source = format!("typedef {spelling} size_t; size_t count;");
        let mut parser = Parser::new(&source);
        let Some(ExternalItem::Typedef(definition)) = parser.next_item().unwrap()
        else { panic!("expected system typedef: {spelling}"); };
        assert_eq!(definition.name, "size_t");
        assert_eq!(definition.ty, Type::Unsigned(Box::new(Type::Long)));
        let Some(ExternalItem::Declaration(usage)) = parser.next_item().unwrap()
        else { panic!("expected typedef usage: {spelling}"); };
        assert_eq!(usage.ty, Type::Alias("size_t".into()));
        assert!(parser.next_item().unwrap().is_none());
    }
}

#[test]
fn signed_unsigned_and_char_specifier_orders_retain_type_meaning() {
    let pairs = [
        ("signed char", Type::Signed(Box::new(Type::Char))),
        ("char signed", Type::Signed(Box::new(Type::Char))),
        ("unsigned char", Type::Unsigned(Box::new(Type::Char))),
        ("char unsigned", Type::Unsigned(Box::new(Type::Char))),
        ("int unsigned", Type::Unsigned(Box::new(Type::Int))),
        ("unsigned int", Type::Unsigned(Box::new(Type::Int))),
        ("int signed", Type::Signed(Box::new(Type::Int))),
        ("signed int", Type::Signed(Box::new(Type::Int))),
        ("long signed int", Type::Signed(Box::new(Type::Long))),
        ("int long signed", Type::Signed(Box::new(Type::Long))),
        ("int", Type::Int),
        ("char", Type::Char),
        ("long int", Type::Long),
    ];
    for (spelling, expected) in pairs {
        let source = format!("{spelling} value;");
        let Some(ExternalItem::Declaration(decl)) =
            Parser::new(&source).next_item().unwrap()
        else { panic!("expected object declaration: {spelling}"); };
        assert_eq!(decl.name, "value");
        assert_eq!(decl.ty, expected, "{spelling}");
    }
}

#[test]
fn rejected_duplicate_and_conflicting_specifiers_have_precise_diagnostics() {
    for source in [
        "unsigned signed int bad;", "signed unsigned int bad;",
        "long long bad;", "long char bad;", "char int bad;",
        "int int bad;", "unsigned unsigned bad;",
        "long unsigned long bad;", "char char bad;",
    ] {
        let error = Parser::new(source).next_item().unwrap_err();
        assert_eq!(error.code, DiagnosticCode::SyntaxInvalidDeclaration,
            "{source}: {error:?}");
        assert!(error.span.hi > error.span.lo, "{source}: {error:?}");
    }
}

#[test]
fn csmith_style_parameter_declarations_accept_any_integer_specifier_order() {
    let source = "extern long unsigned int safe_foo(unsigned long v, int unsigned x);";
    let Some(ExternalItem::Declaration(decl)) =
        Parser::new(source).next_item().unwrap()
    else { panic!("function declaration expected"); };
    let Type::Function { result, params } = decl.ty
    else { panic!("function type expected") };
    assert_eq!(*result, Type::Unsigned(Box::new(Type::Long)));
    assert_eq!(params, vec![
        Type::Unsigned(Box::new(Type::Long)),
        Type::Unsigned(Box::new(Type::Int)),
    ]);
}
