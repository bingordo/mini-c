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
