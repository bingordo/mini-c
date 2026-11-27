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
fn volatile_auto_store_still_warns_like_gcc() {
    let reports = warning_names(
        "int f(void) { volatile int x = 0; x = 1; return 0; }",
        &["-Wall"]);
    assert_eq!(reports, vec![
        ("variable 'x' set but not used".into(), "unused-but-set-variable"),
    ]);
}

#[test]
fn compound_assignment_counts_as_read_in_gcc_compatibility_mode() {
    let reports = warning_names(
        "int f(void) { int x = 0; x += 1; return 0; }", &["-Wall"]);
    assert!(reports.is_empty(), "{reports:?}");
}

#[test]
fn postfix_increment_counts_as_read_in_gcc_compatibility_mode() {
    let reports = warning_names(
        "int f(void) { int x = 0; x++; return 0; }", &["-Wall"]);
    assert!(reports.is_empty(), "{reports:?}");
}

#[test]
fn volatile_unused_declaration_reports_unused_variable() {
    let reports = warning_names(
        "int f(void) { volatile int x = 0; return 0; }", &["-Wall"]);
    assert_eq!(reports, vec![("unused variable 'x'".into(), "unused-variable")]);
}

fn narrowing_warnings_with_target(source: &str, target: Target) -> Vec<ReportedDiagnostic> {
    let mut parser = Parser::new_with_spans(source);
    let mut checker = Checker::new(target).unwrap();
    let mut policy = WarningPolicy::default();
    policy.apply_flag("-Wconversion").unwrap();
    checker.set_warning_policy(policy);
    while let Some(item) = parser.next_item().unwrap() {
        checker.check_item(item).unwrap();
    }
    checker.take_warnings()
}

#[test]
fn narrowing_variable_assignment_emits_conversion_warning() {
    let warnings = narrowing_warnings_with_target(
        "int f(int x) { short y = x; return y; }", Target::default());
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(warnings[0].warning_group, Some("conversion"));
}

#[test]
fn fitting_constant_does_not_trigger_conversion_warning() {
    let warnings = narrowing_warnings_with_target(
        "int f(void) { unsigned char a = 12; return a; }", Target::default());
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn out_of_range_constant_triggers_conversion_warning() {
    let warnings = narrowing_warnings_with_target(
        "int f(void) { unsigned char a = 300; return a; }", Target::default());
    assert_eq!(warnings.len(), 1, "{warnings:?}");
}

#[test]
fn same_width_signedness_conversion_is_not_in_narrowing_group() {
    let warnings = narrowing_warnings_with_target(
        "int f(unsigned int x) { int y = x; return y; }", Target::default());
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn target_long_width_controls_narrowing() {
    let source = "int f(long x) { int y = x; return y; }";
    assert_eq!(narrowing_warnings_with_target(source, Target::default()).len(), 1);
    assert!(narrowing_warnings_with_target(source, Target::wasm32()).is_empty());
}

#[test]
fn failed_item_discards_conversion_diagnostics() {
    let mut parser = Parser::new_with_spans(
        "int f(int x) { short a = x; int missing = never_declared; return a; }"
    );
    let mut checker = Checker::new(Target::default()).unwrap();
    let mut policy = WarningPolicy::default();
    policy.apply_flag("-Wconversion").unwrap();
    checker.set_warning_policy(policy);
    assert!(checker.check_item(parser.next_item().unwrap().unwrap()).is_err());
    assert!(checker.take_warnings().is_empty());
}

fn signedness_warnings(source: &str, target: Target) -> Vec<ReportedDiagnostic> {
    let mut parser = Parser::new_with_spans(source);
    let mut checker = Checker::new(target).unwrap();
    let mut policy = WarningPolicy::default();
    policy.apply_flag("-Wsign-conversion").unwrap();
    checker.set_warning_policy(policy);
    while let Some(item) = parser.next_item().unwrap() {
        checker.check_item(item).unwrap();
    }
    checker.take_warnings()
}

#[test]
fn signed_to_unsigned_variable_conversion_warns() {
    let reports = signedness_warnings(
        "unsigned int f(int x) { unsigned int y = x; return y; }",
        Target::default());
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].warning_group, Some("sign-conversion"));
}

#[test]
fn unsigned_to_signed_variable_conversion_warns_at_same_width() {
    let reports = signedness_warnings(
        "int f(unsigned int x) { int y = x; return y; }",
        Target::default());
    assert_eq!(reports.len(), 1, "{reports:?}");
}

#[test]
fn unsigned_to_wider_signed_destination_does_not_warn() {
    let reports = signedness_warnings(
        "long f(unsigned int x) { long y = x; return y; }",
        Target::default());
    assert!(reports.is_empty(), "{reports:?}");
}

#[test]
fn positive_fitting_constant_does_not_warn() {
    let reports = signedness_warnings(
        "unsigned int f(void) { unsigned int x = 42; return x; }",
        Target::default());
    assert!(reports.is_empty(), "{reports:?}");
}

#[test]
fn negative_constant_to_unsigned_warns() {
    let reports = signedness_warnings(
        "unsigned int f(void) { unsigned int x = -1; return x; }",
        Target::default());
    assert_eq!(reports.len(), 1, "{reports:?}");
}

#[test]
fn target_long_width_changes_signedness_safety() {
    let source = "long f(unsigned int x) { long y = x; return y; }";
    assert!(signedness_warnings(source, Target::default()).is_empty());
    assert_eq!(signedness_warnings(source, Target::wasm32()).len(), 1);
}

#[test]
fn return_statement_uses_shared_sign_conversion_logic() {
    let reports = signedness_warnings("unsigned int f(int x) { return x; }",
        Target::default());
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].warning_group, Some("sign-conversion"));
}

#[test]
fn explicit_cast_suppresses_implicit_sign_warning() {
    let reports = signedness_warnings(
        "unsigned int f(int x) { return (unsigned int)x; }",
        Target::default());
    assert!(reports.is_empty(), "{reports:?}");
}

#[test]
fn return_statement_uses_shared_narrowing_conversion_logic() {
    let reports = narrowing_warnings_with_target(
        "short f(int x) { return x; }", Target::default());
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].warning_group, Some("conversion"));
}

#[test]
fn mixed_signed_unsigned_arithmetic_warns_for_signed_operand() {
    let reports = signedness_warnings(
        "unsigned int f(int a, unsigned int b) { return a + b; }",
        Target::default());
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].warning_group, Some("sign-conversion"));
}

#[test]
fn explicit_cast_does_not_warn_in_mixed_arithmetic() {
    let reports = signedness_warnings(
        "unsigned int f(int a, unsigned int b) { return (unsigned int)a + b; }",
        Target::default());
    assert!(reports.is_empty(), "{reports:?}");
}

#[test]
fn negative_constant_arithmetic_warns_but_positive_constant_does_not() {
    let negative = signedness_warnings(
        "unsigned int f(unsigned int x) { return x + -1; }",
        Target::default());
    assert_eq!(negative.len(), 1, "{negative:?}");
    let positive = signedness_warnings(
        "unsigned int f(unsigned int x) { return x + 1; }",
        Target::default());
    assert!(positive.is_empty(), "{positive:?}");
}

#[test]
fn unsigned_operand_promoted_to_wider_signed_is_safe() {
    let reports = signedness_warnings(
        "long f(unsigned int a, long b) { return a + b; }",
        Target::default());
    assert!(reports.is_empty(), "{reports:?}");
}

#[test]
fn mixed_relational_operators_do_not_emit_sign_conversion() {
    // GCC reserves this case for -Wsign-compare.
    let reports = signedness_warnings(
        "int f(int a, unsigned int b) { return a < b; }",
        Target::default());
    assert!(reports.is_empty(), "{reports:?}");
}

fn sign_compare_warnings(source: &str, target: Target, flags: &[&str]) -> Vec<ReportedDiagnostic> {
    let mut parser = Parser::new_with_spans(source);
    let mut checker = Checker::new(target).unwrap();
    let mut policy = WarningPolicy::default();
    for flag in flags { policy.apply_flag(flag).unwrap(); }
    checker.set_warning_policy(policy);
    while let Some(item) = parser.next_item().unwrap() {
        checker.check_item(item).unwrap();
    }
    checker.take_warnings()
}

#[test]
fn mixed_sign_comparison_warns_under_wall() {
    let reports = sign_compare_warnings(
        "int f(int a, unsigned int b) { return a < b; }",
        Target::default(), &["-Wall"]);
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].warning_group, Some("sign-compare"));
}

#[test]
fn nonnegative_constant_comparison_does_not_warn() {
    let reports = sign_compare_warnings(
        "int f(unsigned int b) { return 1 < b; }",
        Target::default(), &["-Wsign-compare"]);
    assert!(reports.is_empty(), "{reports:?}");
}

#[test]
fn negative_constant_comparison_warns() {
    let reports = sign_compare_warnings(
        "int f(unsigned int b) { return -1 < b; }",
        Target::default(), &["-Wsign-compare"]);
    assert_eq!(reports.len(), 1, "{reports:?}");
}

#[test]
fn widening_to_signed_common_type_needs_no_sign_compare() {
    let source = "int f(unsigned int x, long y) { return x < y; }";
    assert!(sign_compare_warnings(source, Target::default(), &["-Wall"]).is_empty());
    assert_eq!(sign_compare_warnings(source, Target::wasm32(), &["-Wall"]).len(), 1);
}

#[test]
fn sign_compare_can_be_disabled_independently_of_wall() {
    let reports = sign_compare_warnings(
        "int f(int x, unsigned int y) { return x == y; }",
        Target::default(), &["-Wall", "-Wno-sign-compare"]);
    assert!(reports.is_empty(), "{reports:?}");
}

#[test]
fn sign_compare_is_not_implicitly_a_sign_conversion_warning() {
    let reports = sign_compare_warnings(
        "int f(int x, unsigned int y) { return x < y; }",
        Target::default(), &["-Wsign-conversion"]);
    assert!(reports.is_empty(), "{reports:?}");
}

#[test]
fn compound_addition_warns_when_signed_rhs_becomes_unsigned() {
    let reports = signedness_warnings(
        "unsigned int f(unsigned int x, int y) { x += y; return x; }",
        Target::default());
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].warning_group, Some("sign-conversion"));
}

#[test]
fn compound_addition_warns_for_both_computation_and_store_signedness() {
    let reports = signedness_warnings(
        "int f(int x, unsigned int y) { x += y; return x; }",
        Target::default());
    assert_eq!(reports.len(), 2, "{reports:?}");
    assert!(reports.iter().all(|w| w.warning_group == Some("sign-conversion")));
}

#[test]
fn compound_unsigned_narrow_store_is_a_conversion_not_sign_conversion() {
    let source = "unsigned short f(unsigned short x, int y) { x += y; return x; }";
    assert!(signedness_warnings(source, Target::default()).is_empty());
    let reports = warnings(source, &["-Wconversion"]);
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].warning_group, Some("conversion"));
}

#[test]
fn compound_signed_narrow_store_is_a_conversion() {
    let source = "short f(short x, unsigned int y) { x += y; return x; }";
    let reports = warnings(source, &["-Wsign-conversion", "-Wconversion"]);
    assert_eq!(reports.len(), 2, "{reports:?}");
    assert!(reports.iter().any(|w| w.warning_group == Some("conversion")));
    assert!(reports.iter().any(|w| w.warning_group == Some("sign-conversion")));
}

#[test]
fn compound_fitting_positive_constant_does_not_warn() {
    let reports = signedness_warnings(
        "unsigned int f(unsigned int x) { x += 1; return x; }",
        Target::default());
    assert!(reports.is_empty(), "{reports:?}");
}

#[test]
fn compound_negative_constant_warns() {
    let reports = signedness_warnings(
        "unsigned int f(unsigned int x) { x += -1; return x; }",
        Target::default());
    assert_eq!(reports.len(), 1, "{reports:?}");
}

#[test]
fn compound_explicit_cast_suppresses_rhs_sign_conversion() {
    let reports = signedness_warnings(
        "unsigned int f(unsigned int x, int y) { x += (unsigned int)y; return x; }",
        Target::default());
    assert!(reports.is_empty(), "{reports:?}");
}

#[test]
fn compound_safe_widening_remains_warning_free() {
    let reports = signedness_warnings(
        "long f(long x, unsigned int y) { x += y; return x; }",
        Target::default());
    assert!(reports.is_empty(), "{reports:?}");
}

#[test]
fn compound_shift_does_not_invent_a_usual_arithmetic_sign_change() {
    let reports = signedness_warnings(
        "unsigned int f(unsigned int x, int y) { x <<= y; return x; }",
        Target::default());
    assert!(reports.is_empty(), "{reports:?}");
}

#[test]
fn compound_warning_can_be_disabled_without_disabling_other_warnings() {
    let reports = warnings(
        "unsigned int f(unsigned int x, int y) { x += y; return x; }",
        &["-Wsign-conversion", "-Wno-sign-conversion"]);
    assert!(reports.is_empty(), "{reports:?}");
}

#[test]
fn floating_variable_to_integer_uses_float_conversion() {
    let reports = warnings("int f(double x) { int y = x; return y; }",
        &["-Wfloat-conversion"]);
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].warning_group, Some("float-conversion"));
}

#[test]
fn double_to_float_variable_warns_but_float_to_double_is_safe() {
    let narrowing = warnings("float f(double x) { return x; }",
        &["-Wfloat-conversion"]);
    assert_eq!(narrowing.len(), 1, "{narrowing:?}");
    assert!(warnings("double f(float x) { return x; }",
        &["-Wfloat-conversion"]).is_empty());
}

#[test]
fn exactly_representable_float_constants_are_suppressed() {
    assert!(warnings("float f(void) { return 1.0; }",
        &["-Wfloat-conversion"]).is_empty());
    assert!(warnings("int f(void) { return 1.0; }",
        &["-Wfloat-conversion"]).is_empty());
}

#[test]
fn inexact_float_constants_warn() {
    assert_eq!(warnings("float f(void) { return 1.1; }",
        &["-Wfloat-conversion"]).len(), 1);
    assert_eq!(warnings("int f(void) { return 1.5; }",
        &["-Wfloat-conversion"]).len(), 1);
}

#[test]
fn integer_to_float_dynamic_precision_loss_uses_conversion_group() {
    let reports = warnings("float f(int x) { return x; }", &["-Wconversion"]);
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].warning_group, Some("conversion"));
    assert!(warnings("double f(int x) { return x; }",
        &["-Wconversion"]).is_empty());
}

#[test]
fn integer_to_float_constant_exactness_uses_float_conversion_group() {
    assert!(warnings("float f(void) { return 16777216; }",
        &["-Wfloat-conversion"]).is_empty());
    let reports = warnings("float f(void) { return 16777217; }",
        &["-Wfloat-conversion"]);
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].warning_group, Some("float-conversion"));
}

#[test]
fn floating_warnings_are_not_enabled_by_wall() {
    assert!(warnings("int f(double x) { return x; }",
        &["-Wall"]).is_empty());
}

#[test]
fn explicit_float_cast_suppresses_implicit_warning() {
    assert!(warnings("int f(double x) { return (int)x; }",
        &["-Wconversion"]).is_empty());
}

#[test]
fn float_conversion_can_be_suppressed_independently() {
    assert!(warnings("float f(double x) { return x; }",
        &["-Wconversion", "-Wno-float-conversion"]).is_empty());
}
