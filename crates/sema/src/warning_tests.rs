//! Semantic warning tests for lexical scope behavior and rollback after errors.
use super::*;
use mini_c_syntax::parser::Parser;

/// Collect warnings from successfully checked items without retaining the AST.
fn warnings_from(source: &str) -> Result<Vec<ReportedDiagnostic>, Diagnostic> {
    let mut parser = Parser::new_with_spans(source);
    let mut checker = Checker::new(Target::default()).unwrap();
    let mut policy = WarningPolicy::default();
    policy.apply_flag("-Wshadow").unwrap();
    checker.set_warning_policy(policy);
    while let Some(item) = parser.next_item()? {
        checker.check_item(item)?;
    }
    Ok(checker.take_warnings())
}

#[test]
fn shadow_of_global_by_parameter_or_local_is_reported() {
    let reports = warnings_from(
        "int value; int f(int value) { int local = value; { int local = 7; } return local; }"
    ).unwrap();
    assert_eq!(reports.len(), 2);
    assert!(reports.iter().all(|r| r.warning_group == Some("shadow")));
}

#[test]
fn sibling_blocks_do_not_shadow_each_other() {
    let reports = warnings_from(
        "int f(void) { { int x = 1; } { int x = 2; } return 0; }"
    ).unwrap();
    assert!(reports.is_empty(), "{reports:?}");
}

#[test]
fn failing_item_does_not_leak_emitted_warning() {
    let mut parser = Parser::new_with_spans(
        "int value; int f(void) { int value = 1; int value = 2; return value; }"
    );
    let mut checker = Checker::new(Target::default()).unwrap();
    let mut policy = WarningPolicy::default();
    policy.apply_flag("-Wshadow").unwrap();
    checker.set_warning_policy(policy);
    checker.check_item(parser.next_item().unwrap().unwrap()).unwrap();
    assert!(checker.check_item(parser.next_item().unwrap().unwrap()).is_err());
    assert!(checker.take_warnings().is_empty());
}
