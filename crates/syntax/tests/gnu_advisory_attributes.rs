//! Whitelisted, semantically conservative GNU declarator annotations.
//! Unsupported ABI-affecting attributes must never be silently skipped.
use mini_c_syntax::{
    ast::Type,
    diag::DiagnosticCode,
    parser::Parser,
    stream::{ExternalItem, ItemSource},
};

#[test]
fn glibc_memcpy_and_bcmp_declaration_attributes_parse_without_rewrites() {
    let source = r#"
        typedef long unsigned int size_t;
        extern void *memcpy(void *__restrict __dest,
            const void *__restrict __src, size_t __n)
            __attribute__ ((__nothrow__ , __leaf__))
            __attribute__ ((__nonnull__ (1, 2)));
        extern int bcmp(const void *__s1, const void *__s2, size_t __n)
            __attribute__ ((__nothrow__ , __leaf__))
            __attribute__ ((__pure__))
            __attribute__ ((__nonnull__ (1, 2)));
    "#;
    let mut parser = Parser::new(source);
    assert!(matches!(parser.next_item().unwrap(), Some(ExternalItem::Typedef(_))));
    let Some(ExternalItem::Declaration(copy)) = parser.next_item().unwrap() else {
        panic!("memcpy declaration missing");
    };
    assert_eq!(copy.name, "memcpy");
    let Type::Function { result, params } = copy.ty else { panic!("expected function"); };
    assert_eq!(*result, Type::Pointer(Box::new(Type::Void)));
    assert_eq!(params.len(), 3);
    let Some(ExternalItem::Declaration(compare)) = parser.next_item().unwrap() else {
        panic!("bcmp declaration missing");
    };
    assert_eq!(compare.name, "bcmp");
    assert!(parser.next_item().unwrap().is_none());
}

#[test]
fn harmless_optimization_hints_do_not_change_c_function_signature() {
    let mut parser = Parser::new(
        "int f(int x) __attribute__((__pure__, __leaf__)) __attribute__((__nothrow__));"
    );
    let Some(ExternalItem::Declaration(decl)) = parser.next_item().unwrap() else {
        panic!("function declaration expected");
    };
    assert_eq!(decl.ty, Type::Function {
        result: Box::new(Type::Int),
        params: vec![Type::Int],
    });
    assert!(parser.next_item().unwrap().is_none());
}

#[test]
fn reject_attributes_that_can_change_abi_layout_or_control_flow() {
    for attribute in [
        "__noreturn__", "__returns_twice__", "__packed__", "__aligned__(16)",
        "ms_abi", "sysv_abi", "__vector_size__(16)", "mode(QI)",
        "__section__(\".fast\")", "unknown_extension",
        "__format__(printf, 1, 2)",
    ] {
        let source = format!("extern int f(int x) __attribute__(({attribute}));");
        let error = Parser::new(&source).next_item().unwrap_err();
        assert_eq!(error.code, DiagnosticCode::SyntaxInvalidDeclaration,
            "attribute {attribute}: {error:?}");
        assert!(error.message.contains("unsupported GNU attribute"),
            "attribute {attribute}: {error:?}");
    }
}

#[test]
fn reject_malformed_or_misplaced_advisory_annotations() {
    for source in [
        "extern int f(int) __attribute__((__nonnull__(0)));",
        "extern int f(int) __attribute__((__nonnull__(abc)));",
        "extern int f(int) __attribute__((__nonnull__(1,,2)));",
        "extern int f(int) __attribute__((__nothrow__(1)));",
        "extern int f(int) __attribute__((__nothrow__ __leaf__));",
        "extern int f(int) __attribute__((__nothrow__);",
        "int x __attribute__((__pure__));",
    ] {
        assert!(Parser::new(source).next_item().is_err(), "{source}");
    }
}

#[test]
fn glibc_access_annotations_parse_without_erasing_pointer_types() {
    let source = r#"
        typedef long unsigned int size_t;
        extern size_t strxfrm(char *__restrict __dest,
            const char *__restrict __src, size_t __n)
            __attribute__((__nothrow__, __leaf__))
            __attribute__((__nonnull__(2)))
            __attribute__((__access__(__write_only__, 1, 3)));
        extern int inspect(const char *p, size_t n)
            __attribute__((__access__(__read_only__, 1, 2)));
        extern int no_access(void *p)
            __attribute__((__access__(__none__, 1)));
    "#;
    let mut parser = Parser::new(source);
    assert!(matches!(parser.next_item().unwrap(), Some(ExternalItem::Typedef(_))));
    let Some(ExternalItem::Declaration(transform)) = parser.next_item().unwrap() else {
        panic!("strxfrm declaration missing");
    };
    assert_eq!(transform.name, "strxfrm");
    let Type::Function { params, .. } = transform.ty else { panic!("function expected"); };
    assert_eq!(params.len(), 3);
    let Some(ExternalItem::Declaration(inspect)) = parser.next_item().unwrap() else {
        panic!("inspect declaration missing");
    };
    assert_eq!(inspect.name, "inspect");
    let Some(ExternalItem::Declaration(no_access)) = parser.next_item().unwrap() else {
        panic!("no_access declaration missing");
    };
    assert_eq!(no_access.name, "no_access");
    assert!(parser.next_item().unwrap().is_none());
}

#[test]
fn malformed_access_attributes_remain_fail_closed() {
    for attribute in [
        "__access__", "__access__()", "__access__(bad, 1)",
        "__access__(__read_only__, 0)", "__access__(__write_only__, abc)",
        "__access__(__write_only__, 1, 0)",
        "__access__(__write_only__, 1, 2, 3)",
        "__access__(__read_only__, 1,)",
    ] {
        let source = format!(
            "extern int f(char *p, int n) __attribute__(({attribute}));"
        );
        let error = Parser::new(&source).next_item().unwrap_err();
        assert_eq!(error.code, DiagnosticCode::SyntaxInvalidDeclaration,
            "{attribute}: {error:?}");
    }
}
