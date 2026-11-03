//! C99 restrict and glibc __restrict qualifiers on pointer declarators.
//! Restrict is deliberately not exploited for alias assumptions by Mini-C.
use mini_c_syntax::{
    ast::Type,
    parser::Parser,
    stream::{ExternalItem, ItemSource},
};

#[test]
fn csmith_libc_restrict_parameter_spellings_parse_as_pointer_types() {
    for qualifier in ["restrict", "__restrict", "__restrict__"] {
        let src = format!(
            "extern void *memcpy(void *{qualifier} dst, const_unused, void *{qualifier} src);"
        );
        // This fixture uses only supported integer/pointer syntax; no
        // implicit-int fallback or unmodeled C qualifier is introduced.
        let src = src.replace("const_unused", "int n");
        let mut parser = Parser::new(&src);
        let Some(ExternalItem::Declaration(decl)) = parser.next_item().unwrap()
        else { panic!("expected function prototype for {qualifier}") };
        let Type::Function { result, params } = decl.ty
        else { panic!("expected function type") };
        assert_eq!(*result, Type::Pointer(Box::new(Type::Void)));
        assert_eq!(params, vec![
            Type::Pointer(Box::new(Type::Void)),
            Type::Int,
            Type::Pointer(Box::new(Type::Void)),
        ]);
        assert!(parser.next_item().unwrap().is_none());
    }
}

#[test]
fn restrict_pointer_declaration_does_not_change_unqualified_type() {
    let source = "int *restrict one; int *__restrict two; int **__restrict__ cursor;";
    let mut parser = Parser::new(source);
    for name in ["one", "two", "cursor"] {
        let Some(ExternalItem::Declaration(decl)) = parser.next_item().unwrap()
        else { panic!("declaration expected") };
        assert_eq!(decl.name, name);
        if name != "cursor" {
            assert_eq!(decl.ty, Type::Pointer(Box::new(Type::Int)));
        } else {
            assert_eq!(decl.ty,
                Type::Pointer(Box::new(Type::Pointer(Box::new(Type::Int)))));
        }
    }
    assert!(parser.next_item().unwrap().is_none());
}

#[test]
fn restrict_does_not_silently_accept_invalid_or_volatile_declarations() {
    for source in [
        "int restrict *bad;",
        "int *volatile bad;",
        "int *__restrict restrict bad;",
        "int *__restrict",
    ] {
        assert!(Parser::new(source).next_item().is_err(),
            "unsupported/invalid qualifier was silently accepted: {source}");
    }
}
