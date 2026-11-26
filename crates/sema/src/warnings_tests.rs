//! Focused semantic warning regressions with lexical binding identities.
use super::*;
use mini_c_syntax::parser::Parser;
use mini_c_syntax::stream::ItemSource;
use mini_c_syntax::diag::WarningPolicy;
use crate::Target;

fn warnings(source: &str, flags: &[&str]) -> Vec<ReportedDiagnostic> {
    let mut parser = Parser::new_with_spans(source);
    let mut checker = Checker::new(Target::default()).unwrap();
    let mut options = WarningPolicy::default();
    for flag in flags { options.apply_flag(flag).unwrap(); }
    checker.set_warning_policy(options);
    while let Some(item) = parser.next_item().unwrap() {
        checker.check_item(item).unwrap();
    }
    checker.take_warnings()
}

fn warning_names(source: &str, flags: &[&str]) -> Vec<(String, &'static str)> {
    warnings(source, flags).into_iter()
        .map(|w| (w.diagnostic.message, w.warning_group.unwrap()))
        .collect()
}

#[test]
fn unused_variable_respects_local_references() {
    let warnings = warning_names(
        "int f(void) { int used = 1; int unused = 2; return used; }",
        &["-Wall"]);
    assert_eq!(warnings, vec![("unused variable 'unused'".into(), "unused-variable")]);
}

#[test]
fn unused_parameter_requires_wextra() {
    let source = "int f(int used, int unused) { return used; }";
    assert!(warning_names(source, &["-Wall"]).is_empty());
    let warnings = warning_names(source, &["-Wextra"]);
    assert_eq!(warnings, vec![("unused parameter 'unused'".into(), "unused-parameter")]);
}

#[test]
fn nested_shadow_names_track_distinct_bindings() {
    let source = "int f(int value) { { int value = 1; return value; } }";
    let warnings = warning_names(source, &["-Wunused-parameter", "-Wunused-variable", "-Wshadow"]);
    assert!(warnings.iter().any(|(msg,group)| msg == "unused parameter 'value'" && *group == "unused-parameter"));
    assert!(!warnings.iter().any(|(msg,group)| msg == "unused variable 'value'" && *group == "unused-variable"));
    assert!(warnings.iter().any(|(_,group)| *group == "shadow"));
}

#[test]
fn sibling_scope_references_do_not_leak_to_other_binding() {
    let source = "int f(void) { { int local = 1; } { int local = 2; return local; } }";
    let warnings = warning_names(source, &["-Wunused-variable"]);
    assert_eq!(warnings, vec![("unused variable 'local'".into(), "unused-variable")]);
}

#[test]
fn address_taken_counts_as_referenced() {
    let source = "int f(void) { int x = 1; int *ptr = &x; return *ptr; }";
    assert!(warning_names(source, &["-Wall"]).is_empty());
}

#[test]
fn warning_groups_can_be_disabled_selectively() {
    let source = "int f(int param) { int unused = 1; return 0; }";
    assert!(warning_names(source, &["-Wall", "-Wextra", "-Wno-unused-variable",
        "-Wno-unused-parameter"]).is_empty());
}

#[test]
fn multiple_unused_bindings_are_ordered_by_source_location() {
    let warnings = warning_names(
        "int f(void) { int first = 1; int second = 2; return 0; }",
        &["-Wunused-variable"]);
    assert_eq!(warnings, vec![
        ("unused variable 'first'".into(), "unused-variable"),
        ("unused variable 'second'".into(), "unused-variable"),
    ]);
}

#[test]
fn extern_alias_does_not_mark_shadowed_auto_as_used() {
    let warnings = warning_names(
        "int f(void) { int local = 1; { extern int local; return local; } }",
        &["-Wunused-variable"]);
    assert_eq!(warnings,
        vec![("unused variable 'local'".into(), "unused-variable")]);
}

#[test]
fn assigned_but_never_read_reports_distinct_group() {
    let reports = warning_names(
        "int f(void) { int x = 0; x = 4; return 0; }", &["-Wall"]);
    assert_eq!(reports, vec![(
        "variable 'x' set but not used".into(), "unused-but-set-variable"
    )]);
}

#[test]
fn plain_initializer_without_uses_stays_unused_variable() {
    let reports = warning_names("int f(void) { int x = 3; return 0; }", &["-Wall"]);
    assert_eq!(reports, vec![("unused variable 'x'".into(), "unused-variable")]);
}

#[test]
fn read_after_write_is_not_write_only() {
    let reports = warning_names("int f(void) { int x; x = 1; return x; }", &["-Wall"]);
    assert!(reports.is_empty(), "{reports:?}");
}

#[test]
fn assignment_rhs_read_keeps_binding_used() {
    let reports = warning_names("int f(void) { int x = 1; x = x + 1; return 0; }", &["-Wall"]);
    assert!(reports.is_empty(), "{reports:?}");
}

#[test]
fn shadowed_names_have_independent_write_records() {
    let reports = warning_names(
        "int f(void) { int x = 1; { int x = 2; x = 3; } return x; }",
        &["-Wall"]);
    assert_eq!(reports, vec![("variable 'x' set but not used".into(),
        "unused-but-set-variable")]);
}

#[test]
fn write_only_warnings_can_be_disabled_without_disabling_unused_variables() {
    let reports = warning_names(
        "int f(void) { int x = 1; int y = 2; x = 3; return 0; }",
        &["-Wall", "-Wno-unused-but-set-variable"]);
    assert_eq!(reports, vec![("unused variable 'y'".into(), "unused-variable")]);
}

#[test]
fn volatile_stores_do_not_produce_unused_warnings() {
    let reports = warning_names(
        "int f(void) { volatile int x = 0; x = 1; return 0; }",
        &["-Wall"]);
    assert!(reports.is_empty(), "{reports:?}");
}
