//! C99 aggregate member declarations with comma-separated declarators.
//! A real musl <inttypes.h> uses: typedef struct { intmax_t quot, rem; } imaxdiv_t.
//! This is ordinary C grammar, independent of GNU symbol/ABI attributes.
use mini_c_syntax::{
    ast::{Expr, Type},
    parser::Parser,
    stream::{ExternalItem, ItemSource},
};

#[test]
fn original_musl_imaxdiv_member_group_preserves_both_layout_fields() {
    let source = concat!(
        "typedef long long intmax_t;\n",
        "typedef struct { intmax_t quot, rem; } imaxdiv_t;\n",
        "imaxdiv_t value;\n",
    );
    let mut parser = Parser::new(source);
    assert!(matches!(parser.next_item().unwrap(), Some(ExternalItem::Typedef(_))));
    let Some(ExternalItem::Typedef(group)) = parser.next_item().unwrap() else {
        panic!("musl imaxdiv_t typedef missing")
    };
    let Type::Struct { members, .. } = group.ty else {
        panic!("musl anonymous struct not retained")
    };
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].name, "quot");
    assert_eq!(members[1].name, "rem");
    assert_eq!(members[0].ty, Type::LongLong);
    assert_eq!(members[1].ty, Type::LongLong);
    assert_eq!(members[0].span, members[1].span,
        "all members from the same declaration share its original span");
    let Some(ExternalItem::Declaration(value)) = parser.next_item().unwrap() else {
        panic!("declaration after musl struct missing")
    };
    assert_eq!(value.name, "value");
    assert_eq!(value.ty, Type::Alias("imaxdiv_t".into()));
    assert!(parser.next_item().unwrap().is_none());
}

#[test]
fn aggregate_member_groups_preserve_independent_pointer_array_shapes() {
    let source = "struct Pair { int *first, second[3], third; long count; }; \
                  union Mix { long x, *next; int alt; };";
    let mut parser = Parser::new(source);
    let Some(ExternalItem::Tag(Type::Struct { members, .. })) =
        parser.next_item().unwrap() else {
        panic!("struct tag expected")
    };
    assert_eq!(members.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
        ["first", "second", "third", "count"]);
    assert_eq!(members[0].ty, Type::Pointer(Box::new(Type::Int)));
    assert_eq!(members[1].ty,
        Type::Array(Box::new(Type::Int), Some(Box::new(Expr::Int(3)))));
    assert_eq!(members[2].ty, Type::Int);
    assert_eq!(members[3].ty, Type::Long);

    let Some(ExternalItem::Tag(Type::Union { members, .. })) =
        parser.next_item().unwrap() else {
        panic!("union tag expected")
    };
    assert_eq!(members.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
        ["x", "next", "alt"]);
    assert_eq!(members[0].ty, Type::Long);
    assert_eq!(members[1].ty, Type::Pointer(Box::new(Type::Long)));
    assert_eq!(members[2].ty, Type::Int);
    assert!(parser.next_item().unwrap().is_none());
}

#[test]
fn invalid_aggregate_member_groups_remain_rejected() {
    for source in [
        "struct S { int a, ; };",
        "struct S { int a, b };",
        "union U { long x,, y; };",
        "struct S { int a = 1, b; };",
        "union U { int (*fn)(void), g(void); };",
    ] {
        assert!(Parser::new(source).next_item().is_err(), "{source}");
    }
}
