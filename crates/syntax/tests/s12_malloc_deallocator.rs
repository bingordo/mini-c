use mini_c_syntax::{parser::Parser, stream::{ExternalItem, ItemSource}};

fn valid(source: &str) {
    let mut parser = Parser::new(source);
    while let Some(_) = parser.next_item().unwrap() {}
}

fn invalid(source: &str) {
    let mut parser = Parser::new(source);
    assert!(parser.next_item().is_err(), "should reject: {source}");
}

#[test]
fn parse_real_glibc_tmpfile_malloc_deallocator_attribute() {
    valid("extern void *tmpfile(void) __attribute__((__malloc__)) __attribute__((__malloc__(fclose, 1)));");
    valid("extern void *allocate(void) __attribute__((malloc(free,1)));");
    valid("extern void *alloc(void) __attribute__((__malloc__ (free, 1), __nothrow__));");
}

#[test]
fn reject_unbounded_or_malformed_deallocator_attributes() {
    for src in [
        "extern void *f(void) __attribute__((malloc(free)));",
        "extern void *f(void) __attribute__((malloc(free, 0)));",
        "extern void *f(void) __attribute__((malloc(free, -1)));",
        "extern void *f(void) __attribute__((malloc(free, 1, 2)));",
        "extern void *f(void) __attribute__((malloc(3, 1)));",
        "extern void *f(void) __attribute__((malloc(free, x)));",
        "extern void *f(void) __attribute__((malloc(free, 1u)));",
        "extern void *f(void) __attribute__((malloc(free, 0x1)));",
        "extern void *f(void) __attribute__((malloc(free, )));",
        "extern void *f(void) __attribute__((malloc(free, 1));",
        "extern void *f(void) __attribute__((malloc(free, 1))) __attribute__((__ms_abi__));",
        "extern int x __attribute__((malloc(free, 1)));",
    ] {
        invalid(src);
    }
}

#[test]
fn does_not_change_function_signature() {
    let mut parser = Parser::new(
        "extern void *tmpfile(void) __attribute__((__malloc__(fclose, 1)));"
    );
    let Some(ExternalItem::Declaration(decl)) = parser.next_item().unwrap()
    else { panic!("function declaration expected"); };
    assert_eq!(decl.name, "tmpfile");
    assert!(matches!(decl.ty, mini_c_syntax::ast::Type::Function { .. }));
}
