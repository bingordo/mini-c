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

#[test]
fn broad_diagnostic_codes_remain_stable() {
    use mini_c_syntax::diag::DiagnosticCode;
    let syntax = syntax_error("int = 4;");
    assert_eq!(syntax.code, DiagnosticCode::Syntax);
    assert_eq!(syntax.code.as_str(), "S0001");

    let lexical = syntax_error("int x; @");
    assert_eq!(lexical.code, DiagnosticCode::Lexical);
    assert_eq!(lexical.code.as_str(), "L0001");

    let semantic = mini_c_syntax::diag::Diagnostic::semantic(
        Span { lo: 3, hi: 4 }, "bad type",
    );
    assert_eq!(semantic.code.as_str(), "T0001");
}

#[test]
fn recovers_multiple_bad_declarations_and_keeps_valid_following_items() {
    use mini_c_syntax::stream::ExternalItem;
    let source = "int first = ; int ok; int second = ; int tail;";
    let mut parser = Parser::new(source);
    let mut names = Vec::new();
    let errors = parser.for_each_item_recovering(4, |item| {
        if let ExternalItem::Declaration(decl) = item { names.push(decl.name); }
    });
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert_eq!(names, ["ok", "tail"]);
    assert!(errors[0].span.lo < errors[1].span.lo);
}

#[test]
fn recovery_skips_failed_function_body_and_resumes_at_next_item() {
    use mini_c_syntax::stream::ExternalItem;
    let source = "int bad(void) { int x = ; return 0; } int good;";
    let mut parser = Parser::new(source);
    let mut names = Vec::new();
    let errors = parser.for_each_item_recovering(4, |item| {
        if let ExternalItem::Declaration(decl) = item { names.push(decl.name); }
    });
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(names, ["good"]);
}

#[test]
fn recovery_clears_failed_typedef_without_losing_prior_typedefs() {
    use mini_c_syntax::stream::ExternalItem;
    let source = "typedef int Good; typedef long Bad = ; Good ok; Bad broken;";
    let mut parser = Parser::new(source);
    let mut names = Vec::new();
    let errors = parser.for_each_item_recovering(8, |item| {
        if let ExternalItem::Declaration(decl) = item { names.push(decl.name); }
    });
    assert_eq!(names, ["ok"]);
    assert_eq!(errors.len(), 2, "{errors:?}");
}

#[test]
fn recovery_does_not_loop_on_invalid_utf8_or_missing_semicolon() {
    use mini_c_syntax::stream::ExternalItem;
    for source in [
        "int x = ; @; int y;",
        "int x int y; int z;",
        "int f(void) { /* unfinished",
    ] {
        let mut parser = Parser::new(source);
        let mut count = 0;
        let errors = parser.for_each_item_recovering(4, |item| {
            if matches!(item, ExternalItem::Declaration(_)) { count += 1; }
        });
        assert!(!errors.is_empty(), "{source:?}");
        assert!(errors.len() <= 4);
        if source.contains("int y;") { assert_eq!(count, 1); }
    }
}

#[test]
fn recovery_error_limit_bounds_work_and_zero_disables_parsing() {
    let source = "int a = ; int b = ; int c = ; int good;";
    let mut parser = Parser::new(source);
    assert!(parser.for_each_item_recovering(0, |_| panic!("should not parse")).is_empty());
    let errors = parser.for_each_item_recovering(2, |_| ());
    assert_eq!(errors.len(), 2);
}
