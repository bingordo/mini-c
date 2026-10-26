//! M2.25: byte-accurate parser diagnostics and bounded malformed-input tests.
use mini_c_syntax::{
    ast::Span,
    parser::Parser,
    source::SourceFile,
    stream::ItemSource,
};

fn syntax_error(source: &str) -> mini_c_syntax::diag::Diagnostic {
    let mut parser = Parser::new(source);
    loop {
        match parser.next_item() {
            Err(e) => return e,
            Ok(Some(_)) => (),
            Ok(None) => panic!("invalid input was silently accepted: {source:?}"),
        }
    }
}

#[test]
fn truncated_top_level_specifiers_are_rejected_at_eof() {
    for source in ["typedef", "extern", "static", "typedef  ", "int a;\nextern"] {
        let error = syntax_error(source);
        assert!(
            error.message.contains("expected type specifier"),
            "{source:?}: {error:?}"
        );
        assert_eq!(
            error.span,
            Span { lo: source.len(), hi: source.len() },
            "{source:?}"
        );
    }
}

#[test]
fn eof_diagnostics_use_end_of_source_not_byte_zero() {
    for (source, expected) in [
        ("int x", "expected ';'"),
        ("int", "expected identifier"),
        ("int f(", "expected parameter type"),
        ("int f(void", "expected ')'"),
        ("int f(void) {", "unterminated function body"),
        ("int f(void) { return", "expected expression"),
        ("int f(void) { return 1", "expected ';'"),
        ("int f(void) { { int x;", "unterminated function body"),
        ("int f(void) { int *", "expected identifier"),
    ] {
        let error = syntax_error(source);
        assert!(
            error.message.contains(expected),
            "for {source:?} expected {expected:?}, found {error:?}"
        );
        assert_eq!(
            error.span,
            Span { lo: source.len(), hi: source.len() },
            "for {source:?}"
        );
    }
}

#[test]
fn unexpected_non_eof_token_retains_its_span() {
    for (source, location, expected) in [
        ("int x )", Span { lo: 6, hi: 7 }, "expected ';'"),
        ("int f(void) { return ) ; }", Span { lo: 21, hi: 22 }, "expected expression"),
        ("int f(,);", Span { lo: 6, hi: 7 }, "expected a type specifier"),
    ] {
        let error = syntax_error(source);
        assert_eq!(error.span, location, "{source:?}: {error:?}");
        assert!(error.message.contains(expected), "{error:?}");
    }
}

#[test]
fn diagnostic_positions_are_utf8_byte_offsets() {
    // This is a comment, so the scanner accepts the non-ASCII text.
    let source = "/* β */\nint x";
    let error = syntax_error(source);
    assert_eq!(error.span.lo, source.len());
    let file = SourceFile::new("input.c", source);
    assert_eq!(file.location(error.span.lo), (2, 6));
}

#[test]
fn bad_input_mutations_terminate_without_panicking() {
    let good = "int f(int x) { if (x) { return x + 1; } return 0; }";
    // Remove each byte once from an all-ASCII valid program. Accept a valid
    // mutation or report an error, but never panic or loop indefinitely.
    for i in 0..good.len() {
        let mut source = good.to_owned();
        source.remove(i);
        let completed = std::panic::catch_unwind(|| {
            let mut parser = Parser::new(&source);
            for _ in 0..8 {
                match parser.next_item() {
                    Ok(Some(_)) => (),
                    Ok(None) | Err(_) => return,
                }
            }
            panic!("parser produced too many items from {source:?}");
        });
        assert!(completed.is_ok(), "parser panic for mutation at {i}");
    }
}
