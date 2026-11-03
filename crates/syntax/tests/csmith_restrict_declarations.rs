//! C99 pointer restrict (and GNU spellings) from ordinary libc declarations.
//! Since Mini-C does not make no-alias assumptions, dropping restrict after
//! syntax checking is conservative: it does not change load/store semantics.
use mini_c_syntax::{
    ast::Type,
    parser::Parser,
    stream::{ExternalItem, ItemSource},
};

#[test]
fn pointer_restrict_specifiers_preserve_parameter_types() {
    for qualifier in ["restrict", "__restrict", "__restrict__"] {
        let source = format!(
            "extern void *copy(void *{qualifier} dest, const_void_dummy *{qualifier} src);"
        );
        // Use a typedef here; the parser is not being asked to accept
        // independently unsupported C const/pointee qualifiers.
        let source = format!("typedef void *const_void_dummy; {source}");
        let mut p = Parser::new(&source);
        assert!(matches!(p.next_item().unwrap(),Some(ExternalItem::Typedef(_))));
        let Some(ExternalItem::Declaration(decl)) = p.next_item().unwrap() else {
            panic!("expected normal C function declaration for {qualifier}");
        };
        assert_eq!(decl.name, "copy");
        let Type::Function { result, params } = decl.ty else {
            panic!("expected function type");
        };
        assert_eq!(*result, Type::Pointer(Box::new(Type::Void)));
        assert_eq!(params, vec![
            Type::Pointer(Box::new(Type::Void)),
            Type::Pointer(Box::new(Type::Alias("const_void_dummy".into()))),
        ]);
        assert!(p.next_item().unwrap().is_none());
    }
}

#[test]
fn restrict_applies_to_each_pointer_level_not_the_pointee() {
    let source = "int f(int *restrict * __restrict p);";
    let Some(ExternalItem::Declaration(decl))=Parser::new(source).next_item().unwrap()
        else { panic!("expected function declaration"); };
    let Type::Function { params, .. }=decl.ty else { panic!("function type"); };
    assert_eq!(params, vec![Type::Pointer(Box::new(Type::Pointer(Box::new(Type::Int))))]);
}

#[test]
fn reject_unattached_restrict_and_volatile_instead_of_silently_ignoring() {
    for source in ["restrict int x;", "__restrict int x;", "int volatile x;", "int *volatile p;"] {
        assert!(Parser::new(source).next_item().is_err(), "{source}");
    }
}
