//! Stage 1: retain the C11 atomic type specifier without lowering it.
use mini_c_syntax::{
    ast::{Qualifiers, Type},
    parser::Parser,
    stream::{ExternalItem, ItemSource},
};

fn parsed_type(source: &str) -> Type {
    let mut parser = Parser::new(source);
    let item = parser.next_item().expect("valid atomic type syntax")
        .expect("expected declaration");
    let ExternalItem::Declaration(decl) = item else {
        panic!("expected declaration for {source}");
    };
    decl.ty
}

#[test]
fn parenthesized_atomic_types_remain_distinct_from_ordinary_types() {
    let atomic = Type::Atomic(Box::new(Type::Int));
    assert_eq!(parsed_type("_Atomic(int) counter;"), atomic);
    assert_eq!(parsed_type("_Atomic(int) *p;"),
        Type::Pointer(Box::new(atomic.clone())));
    assert_eq!(parsed_type("_Atomic(int *) p;"),
        Type::Atomic(Box::new(Type::Pointer(Box::new(Type::Int)))));
    assert_eq!(parsed_type("const _Atomic(int) counter;"),
        Type::Qualified(Box::new(atomic.clone()),
            Qualifiers { is_const: true, is_restrict: false, is_atomic: false }));
    assert_eq!(parsed_type("_Atomic(int) const counter;"),
        Type::Qualified(Box::new(atomic),
            Qualifiers { is_const: true, is_restrict: false, is_atomic: false }));
}

#[test]
fn parenthesized_atomic_type_is_valid_syntax_but_bad_forms_reject() {
    let mut parser = Parser::new("typedef _Atomic(int) Counter; Counter x;");
    assert!(matches!(parser.next_item().unwrap(), Some(ExternalItem::Typedef(_))));
    assert!(matches!(parser.next_item().unwrap(), Some(ExternalItem::Declaration(_))));
    assert!(parser.next_item().unwrap().is_none());
    for source in ["_Atomic() x;", "_Atomic(42) x;", "_Atomic(int x) y;"] {
        assert!(Parser::new(source).next_item().is_err(), "{source}");
    }
}
