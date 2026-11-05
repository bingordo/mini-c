use mini_c_syntax::{ast::Type, parser::Parser, stream::{ExternalItem, ItemSource}};

fn accept(source: &str) {
    let mut p = Parser::new(source);
    assert!(matches!(p.next_item(), Ok(Some(ExternalItem::Declaration(_)))), "{source}");
    assert!(p.next_item().unwrap().is_none());
}
fn reject(source: &str) {
    assert!(Parser::new(source).next_item().is_err(), "must reject: {source}");
}

#[test]
fn real_glibc_stdio_format_annotations() {
    accept("extern int snprintf(char *s, unsigned long len, const char *fmt, ...) __attribute__((__nothrow__)) __attribute__((__format__(__printf__, 3, 4)));");
    accept("extern int vsnprintf(char *s, unsigned long len, const char *fmt, void *ap) __attribute__((__format__(__printf__, 3, 0)));");
    accept("extern int printf(const char *fmt, ...) __attribute__((format(gnu_printf, 1, 2)));");
    accept("extern int scanf(const char *fmt, ...) __attribute__((__format__(__scanf__, 1, 2)));");
}

#[test]
fn malformed_and_unsupported_annotations_fail_closed() {
    for source in [
        "extern int f(const char *fmt, ...) __attribute__((format(ms_printf,1,2)));",
        "extern int f(const char *fmt, ...) __attribute__((format(printf,0,2)));",
        "extern int f(const char *fmt, ...) __attribute__((format(printf,2,2)));",
        "extern int f(const char *fmt, ...) __attribute__((format(printf,1,1)));",
        "extern int f(const char *fmt, ...) __attribute__((format(printf,1,3)));",
        "extern int f(const char *fmt, ...) __attribute__((format(printf,1,-2)));",
        "extern int f(const char *fmt, ...) __attribute__((format(printf,x,2)));",
        "extern int f(const char *fmt, ...) __attribute__((format(printf,1,2,3)));",
        "extern int f(const char *fmt) __attribute__((format(printf,1,2)));",
        "extern int f(const char *fmt, ...) __attribute__((format(printf,1)));",
        "extern int x __attribute__((format(printf,1,2)));",
        "extern int f(const char *fmt, ...) __attribute__((ms_abi));",
    ] {
        reject(source);
    }
}

#[test]
fn attribute_does_not_change_variadic_function_type() {
    let mut p = Parser::new("extern int printf(const char *fmt, ...) __attribute__((format(printf,1,2)));");
    let Some(ExternalItem::Declaration(decl)) = p.next_item().unwrap() else { panic!("expected declaration"); };
    assert!(matches!(decl.ty, Type::VariadicFunction { params, .. } if params.len() == 1));
}
