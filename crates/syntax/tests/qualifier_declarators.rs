use mini_c_syntax::{
    ast::{Qualifiers, Type},
    parser::Parser,
    stream::{ExternalItem, ItemSource},
};

fn declaration_type(source: &str) -> Type {
    let item = Parser::new(source).next_item().unwrap().unwrap();
    let ExternalItem::Declaration(decl) = item else { panic!("expected declaration"); };
    decl.ty
}

#[test]
fn const_preserves_pointee_vs_pointer_level() {
    let cv = Qualifiers { is_const: true, is_restrict: false };
    assert_eq!(declaration_type("const int *p;"),
        Type::Pointer(Box::new(Type::Qualified(Box::new(Type::Int), cv))));
    assert_eq!(declaration_type("int const *p;"),
        Type::Pointer(Box::new(Type::Qualified(Box::new(Type::Int), cv))));
    assert_eq!(declaration_type("int * const p;"),
        Type::Qualified(Box::new(Type::Pointer(Box::new(Type::Int))), cv));
    assert_eq!(declaration_type("int *p;"),
        Type::Pointer(Box::new(Type::Int)));
}

#[test]
fn restrict_is_preserved_per_pointer_and_gnu_spellings_work() {
    let restrict = Qualifiers { is_const: false, is_restrict: true };
    for text in ["int *restrict p;", "int *__restrict p;", "int *__restrict__ p;"] {
        assert_eq!(declaration_type(text),
            Type::Qualified(Box::new(Type::Pointer(Box::new(Type::Int))), restrict),
            "{text}");
    }
    let both = Qualifiers { is_const: true, is_restrict: true };
    assert_eq!(declaration_type("int *const restrict p;"),
        Type::Qualified(Box::new(Type::Pointer(Box::new(Type::Int))), both));
}

#[test]
fn parameter_qualifiers_and_typedefs_parse_without_string_rewriting() {
    let source = "typedef const char *CStr; int f(const void *__restrict p, CStr q);";
    let mut parser = Parser::new(source);
    assert!(matches!(parser.next_item().unwrap().unwrap(), ExternalItem::Typedef(_)));
    assert!(matches!(parser.next_item().unwrap().unwrap(), ExternalItem::Declaration(_)));
    assert!(parser.next_item().unwrap().is_none());
}
