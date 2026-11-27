//! Regression coverage for C99 nested function declarator binding order.
//! Definitions returning a function pointer use the params of the declared
//! function, not the nested function pointer's abstract prototype.
use mini_c_syntax::{
    ast::{Type, Stmt},
    parser::Parser,
    stream::{ExternalItem, ItemSource},
};

fn assert_returned_callback_type(ty:&Type) {
    let Type::Pointer(inner)=ty else {panic!("callback pointer");};
    let Type::Function{result,params}=inner.as_ref() else {panic!("callback function");};
    assert_eq!(**result,Type::Int);
    assert_eq!(params.len(),1);
    assert_eq!(params[0].ty,Type::Int);
    assert!(params[0].name.is_empty());
}

#[test]
fn function_returning_callback_keeps_defining_parameter_names() {
    let source = concat!(
        "int (*select(int flag, int (*callback)(int)))(int) {",
        " return callback; }"
    );
    let ExternalItem::Function(f) = Parser::new(source)
        .next_item().unwrap().expect("function") else { panic!("function"); };
    assert_eq!(f.name, "select");
    assert_eq!(f.params.len(), 2);
    assert_eq!(f.params[0].name, "flag");
    assert_eq!(f.params[0].ty, Type::Int);
    assert_eq!(f.params[1].name, "callback");
    assert_returned_callback_type(&f.params[1].ty);
    assert_returned_callback_type(&f.result);
    let Stmt::Block(stmts) = f.body else { panic!("body"); };
    assert_eq!(stmts.len(), 1);
}

#[test]
fn nested_callbacks_do_not_steal_definition_parameters() {
    let cases = [
        ("int (*get(void))(int) { return 0; }", vec![]),
        ("int (*choose(int flag, int (*left)(int), int (*right)(int)))(int) { return left; }",
            vec!["flag", "left", "right"]),
    ];
    for (source, names) in cases {
        let mut parser = Parser::new(source);
        let ExternalItem::Function(f) = parser.next_item().unwrap().expect("function")
            else { panic!("expected definition"); };
        assert_eq!(f.params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), names, "{source}");
        assert_returned_callback_type(&f.result);
    }
}

#[test]
fn abstract_prototype_and_typedef_callback_declarators_remain_legal() {
    let mut parser = Parser::new(concat!(
        "int (*choose(int, int (*)(int)))(int);",
        "typedef int (*Callback)(int);",
        "Callback identity(int x, Callback cb) { return cb; }"
    ));
    let ExternalItem::Declaration(proto) = parser.next_item().unwrap().expect("prototype")
        else { panic!("prototype"); };
    assert_eq!(proto.name, "choose");
    assert!(matches!(proto.ty, Type::Function { .. }));
    assert!(matches!(parser.next_item().unwrap(), Some(ExternalItem::Typedef(_))));
    let ExternalItem::Function(def) = parser.next_item().unwrap().expect("definition")
        else { panic!("definition"); };
    assert_eq!(def.params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["x", "cb"]);
    assert_eq!(def.result, Type::Alias("Callback".into()));
    assert!(parser.next_item().unwrap().is_none());
}

#[test]
fn missing_real_definition_parameter_names_still_fail() {
    // The named return function's own abstract int parameter is allowed,
    // but the outer definition must name its int parameter.
    for source in [
        "int (*bad(int))(int) { return 0; }",
        "int (*bad(int (*callback)(int), int))(int) { return callback; }",
    ] {
        let err = Parser::new(source).next_item()
            .expect_err("must reject unnamed defining parameter");
        assert!(err.message.contains("function definition requires parameter names"),
                "{source}: {err:?}");
    }
}
