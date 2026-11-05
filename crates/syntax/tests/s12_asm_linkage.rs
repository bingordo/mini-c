use mini_c_syntax::{ast::Type, parser::Parser, stream::{ExternalItem, ItemSource}};

fn parse_decl(src: &str) -> mini_c_syntax::ast::Decl {
    let mut p = Parser::new(src);
    let Some(ExternalItem::Declaration(decl)) = p.next_item().unwrap()
    else { panic!("expected a declaration"); };
    assert!(p.next_item().unwrap().is_none());
    decl
}

#[test]
fn glibc_fscanf_alias_uses_concatenated_string_fragments() {
    let plain = parse_decl("extern int fscanf(void *stream, const char *fmt, ...);");
    assert!(matches!(plain.ty, Type::VariadicFunction { .. }));
    assert!(plain.asm_label.is_none());
    let d = parse_decl(concat!(
        "extern int fscanf(void *stream, const char *fmt, ...) ",
        "__asm__(\"\" \"__isoc23_fscanf\") __attribute__((__nonnull__(1)));"
    ));
    assert_eq!(d.name, "fscanf");
    assert_eq!(d.asm_label.as_deref(), Some("__isoc23_fscanf"));
    assert!(matches!(d.ty, Type::VariadicFunction { .. }));
    let d = parse_decl("extern int putsvia(const char *) __asm__(\"puts\");");
    assert_eq!(d.asm_label.as_deref(), Some("puts"));
}

#[test]
fn malformed_asm_labels_rejected_instead_of_ignored() {
    for source in [
        "extern int f(void) __asm__();",
        "extern int f(void) __asm__(\"\");",
        "extern int f(void) __asm__(42);",
        "extern int f(void) __asm__(\"puts\", \"extra\");",
        "extern int f(void) __asm__(\"puts\"",
        "extern int f(void) __asm__(\"bad\\0name\");",
        "extern int f(void) __asm__(puts);",
        "typedef int t __asm__(\"other\");",
        "extern int x __asm__(\"other\");",
    ] {
        assert!(Parser::new(source).next_item().is_err(), "{source}");
    }
}
