//! Focused tests for GCC-style warning policy and diagnostic severity.
use super::*;

    #[test]
    fn shadow_warning_is_opt_in_and_promotable() {
        let mut options = WarningPolicy::default();
        let sample = ReportedDiagnostic::warning(
            Diagnostic::semantic(Span { lo: 0, hi: 0 }, "shadowed binding"),
            "shadow",
        );
        assert_eq!(options.resolve(&sample), None);
        options.apply_flag("-Wall").unwrap();
        options.apply_flag("-Wextra").unwrap();
        assert_eq!(options.resolve(&sample), None);
        assert!(options.enabled("unused-variable"));
        assert!(options.enabled("unused-parameter"));
        options.apply_flag("-Wshadow").unwrap();
        assert_eq!(options.resolve(&sample), Some(DiagnosticSeverity::Warning));
        options.apply_flag("-Werror").unwrap();
        assert_eq!(options.resolve(&sample), Some(DiagnosticSeverity::Error));
        options.apply_flag("-Wno-shadow").unwrap();
        assert_eq!(options.resolve(&sample), None);
        options.apply_flag("-Wshadow").unwrap();
        assert_eq!(options.resolve(&sample), Some(DiagnosticSeverity::Error));
    }

    #[test]
    fn unknown_group_is_not_accepted_as_implemented() {
        let mut options = WarningPolicy::default();
        assert!(options.apply_flag("-Wformat").is_err());
        assert!(options.apply_flag("-Wunused-variable").is_ok());
        assert!(options.apply_flag("-Wno-format").is_err());
        assert!(options.apply_flag("-Wextra").is_ok());
        assert!(!options.enabled("conversion"));
        assert!(options.apply_flag("-Wno-unused-parameter").is_ok());
    }

    #[test]
    fn errors_cannot_be_suppressed_by_warning_options() {
        let report = ReportedDiagnostic::error(
            Diagnostic::semantic(Span { lo: 0, hi: 0 }, "bad type"),
        );
        assert_eq!(WarningPolicy::default().resolve(&report), Some(DiagnosticSeverity::Error));
    }

#[test]
fn per_group_werror_enables_and_promotes_only_selected_warning() {
    let mut policy = WarningPolicy::default();
    let shadow = ReportedDiagnostic::warning(
        Diagnostic::semantic(Span { lo: 1, hi: 2 }, "shadow"), "shadow");
    let unused = ReportedDiagnostic::warning(
        Diagnostic::semantic(Span { lo: 3, hi: 4 }, "unused"), "unused-variable");

    policy.apply_flag("-Werror=shadow").unwrap();
    assert_eq!(policy.resolve(&shadow), Some(DiagnosticSeverity::Error));
    assert_eq!(policy.resolve(&unused), None);

    policy.apply_flag("-Wall").unwrap();
    assert_eq!(policy.resolve(&unused), Some(DiagnosticSeverity::Warning));

    policy.apply_flag("-Werror").unwrap();
    assert_eq!(policy.resolve(&unused), Some(DiagnosticSeverity::Error));
    policy.apply_flag("-Wno-error=unused-variable").unwrap();
    assert_eq!(policy.resolve(&unused), Some(DiagnosticSeverity::Warning));
    assert_eq!(policy.resolve(&shadow), Some(DiagnosticSeverity::Error));
}

#[test]
fn per_group_warning_error_controls_do_not_claim_unsupported_groups() {
    let mut policy = WarningPolicy::default();
    for flag in ["-Werror=float-conversion", "-Wno-error=float-conversion",
                 "-Werror=", "-Wno-error="] {
        assert!(policy.apply_flag(flag).is_err(), "{flag}");
    }
    policy.apply_flag("-Werror=unused-parameter").unwrap();
    assert!(policy.enabled("unused-parameter"));
    policy.apply_flag("-Wno-error=unused-parameter").unwrap();
    let param = ReportedDiagnostic::warning(
        Diagnostic::semantic(Span { lo: 0, hi: 0 }, "unused parameter"),
        "unused-parameter");
    assert_eq!(policy.resolve(&param), Some(DiagnosticSeverity::Warning));
}

#[test]
fn sign_conversion_can_be_promoted_without_enabling_conversion() {
    let mut policy = WarningPolicy::default();
    policy.apply_flag("-Werror=sign-conversion").unwrap();
    assert!(policy.enabled("sign-conversion"));
    assert!(!policy.enabled("conversion"));
    let report = ReportedDiagnostic::warning(
        Diagnostic::semantic(Span { lo: 1, hi: 2 }, "signedness"),
        "sign-conversion");
    assert_eq!(policy.resolve(&report), Some(DiagnosticSeverity::Error));
    policy.apply_flag("-Wno-error=sign-conversion").unwrap();
    assert_eq!(policy.resolve(&report), Some(DiagnosticSeverity::Warning));
}
