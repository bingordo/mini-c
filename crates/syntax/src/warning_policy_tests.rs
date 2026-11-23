//! Focused tests for GCC-style warning policy and diagnostic severity.
use super::*;

    #[test]
    fn warning_groups_are_off_by_default_and_promotable() {
        let mut options = WarningPolicy::default();
        let sample = ReportedDiagnostic::warning(
            Diagnostic::semantic(Span { lo: 0, hi: 0 }, "unused variable"),
            "unused-variable",
        );
        assert_eq!(options.resolve(&sample), None);
        options.apply_flag("-Wall").unwrap();
        assert_eq!(options.resolve(&sample), Some(DiagnosticSeverity::Warning));
        options.apply_flag("-Werror").unwrap();
        assert_eq!(options.resolve(&sample), Some(DiagnosticSeverity::Error));
        options.apply_flag("-Wno-unused-variable").unwrap();
        assert_eq!(options.resolve(&sample), None);
        options.apply_flag("-Wunused-variable").unwrap();
        assert_eq!(options.resolve(&sample), Some(DiagnosticSeverity::Error));
    }

    #[test]
    fn unknown_group_is_not_accepted_as_implemented() {
        let mut options = WarningPolicy::default();
        assert!(options.apply_flag("-Wformat").is_err());
        assert!(options.apply_flag("-Wno-format").is_err());
        assert!(options.apply_flag("-Wextra").is_ok());
        assert!(!options.enabled("conversion"));
    }

    #[test]
    fn errors_cannot_be_suppressed_by_warning_options() {
        let report = ReportedDiagnostic::error(
            Diagnostic::semantic(Span { lo: 0, hi: 0 }, "bad type"),
        );
        assert_eq!(WarningPolicy::default().resolve(&report), Some(DiagnosticSeverity::Error));
    }
