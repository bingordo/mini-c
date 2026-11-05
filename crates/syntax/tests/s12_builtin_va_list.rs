use mini_c_syntax::{ast::Type, parser::Parser, stream::{ExternalItem, ItemSource}};

#[test]
fn gcc_builtin_va_list_is_not_an_ordinary_typedef_name() {
    let mut parser = Parser::new(
        "typedef __builtin_va_list __gnuc_va_list; typedef __gnuc_va_list va_list; "
    );
    let Some(ExternalItem::Typedef(first)) = parser.next_item().unwrap() else {
        panic!("expected builtin va_list typedef");
    };
    assert_eq!(first.ty, Type::BuiltinVaList);
    let Some(ExternalItem::Typedef(second)) = parser.next_item().unwrap() else {
        panic!("expected va_list alias");
    };
    assert_eq!(second.ty, Type::Alias("__gnuc_va_list".into()));
    assert!(parser.next_item().unwrap().is_none());
}

#[test]
fn gcc_va_list_prototype_is_parsed_without_header_stripping() {
    let mut parser = Parser::new(
        "typedef __builtin_va_list __gnuc_va_list; "
    );
    assert!(parser.next_item().unwrap().is_some());
    let declaration = "extern int vprintf(const char *restrict fmt, __gnuc_va_list ap);";
    // A standalone parse of an alias cannot use the first parser's typedef
    // scope; parse the complete declaration stream instead.
    let source = format!("typedef __builtin_va_list __gnuc_va_list; {declaration}");
    let mut parser = Parser::new(&source);
    parser.next_item().unwrap();
    let Some(ExternalItem::Declaration(decl)) = parser.next_item().unwrap() else {
        panic!("expected vprintf declaration");
    };
    assert!(matches!(decl.ty, Type::Function { .. }));
}
