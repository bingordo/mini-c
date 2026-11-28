//! Driver tests for option parsing, diagnostics, and compilation behavior.
    use super::*;
    use std::ffi::OsString;


    #[cfg(feature = "internal-cpp")]
    #[test]
    fn opt_in_bounded_cpp_pipe_checks_macros_headers_and_lexical_origins() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let dir = std::env::temp_dir().join(format!(
            "mini-c-direct-cpp-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.join("main.c");
        std::fs::write(dir.join("defs.h"), "#define PLUS1(x) ((x)+1)\n").unwrap();
        std::fs::write(&root, "#include \"defs.h\"\ntypedef int T;\nT value = PLUS1\n(41);\nint f(void) { return value; }\n").unwrap();
        let config = preprocess::Config {
            executable: "internal-stream".into(),
            args: vec!["-E", "-nostdinc"].into_iter().map(Into::into).collect(),
        };
        assert!(check(root.to_str().unwrap(), Some(&config)).is_ok());
        assert!(check_recovering(root.to_str().unwrap(), Some(&config), 4).is_ok());
        let old = preprocess::Config {
            executable: "internal".into(), args: config.args.clone(),
        };
        assert!(check(root.to_str().unwrap(), Some(&old)).is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(feature = "internal-cpp")]
    #[test]
    fn opt_in_bounded_cpp_pipe_reports_late_error_and_logical_file() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let dir = std::env::temp_dir().join(format!(
            "mini-c-direct-late-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.join("main.c");
        let config = preprocess::Config {
            executable: "internal-stream".into(),
            args: vec!["-E", "-nostdinc"].into_iter().map(Into::into).collect(),
        };
        std::fs::write(&root,
            "#line 77 \"generated.c\"\nint first;\n#error late_failure\n"
        ).unwrap();
        let failure = check(root.to_str().unwrap(), Some(&config)).unwrap_err();
        assert!(failure.contains("late_failure"), "{failure}");
        std::fs::write(&root,
            "#line 123 \"virtual.c\"\nint f(void) { return missing; }\n"
        ).unwrap();
        let failure = check(root.to_str().unwrap(), Some(&config)).unwrap_err();
        assert!(failure.contains("virtual.c:123"), "{failure}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(feature = "internal-cpp")]
    #[test]
    fn opt_in_bounded_cpp_pipe_accepts_atomic_integers_and_rejects_unsupported_types() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let dir = std::env::temp_dir().join(format!(
            "mini-c-direct-atomic-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.join("atomic.c");
        let config = preprocess::Config {
            executable: "internal-stream".into(),
            args: vec!["-E", "-nostdinc"].into_iter().map(Into::into).collect(),
        };

        // C11 atomic integer declarations are now part of the supported
        // semantic type system, including in the streaming checker.
        std::fs::write(&root, "int first;\n_Atomic int counter;\n").unwrap();
        assert!(check(root.to_str().unwrap(), Some(&config)).is_ok());
        assert!(check_recovering(root.to_str().unwrap(), Some(&config), 4).is_ok());

        // Unsupported atomic storage must still fail rather than allowing
        // the preceding valid declaration to hide a late semantic error.
        std::fs::write(&root, "int first;\n_Atomic float counter;\n").unwrap();
        let failure = check(root.to_str().unwrap(), Some(&config)).unwrap_err();
        assert!(failure.contains("atomic.c:2:"), "{failure}");
        let failure = check_recovering(root.to_str().unwrap(), Some(&config), 4).unwrap_err();
        assert!(failure.contains("error[T0001]"), "{failure}");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn preprocessor_flags_parse_without_shell_splitting() {
        let options = parse_options([
            "check", "-I", "include dir", "-DNAME=one two;$(touch never)",
            "-D", "OTHER=42", "-UOLD", "-U", "OBSOLETE",
            "--cpp=clang", "--cpp-arg=-undef", "source.c",
        ].into_iter().map(str::to_owned)).unwrap();
        let cpp = options.cpp.unwrap();
        assert_eq!(cpp.executable, OsString::from("clang"));
        assert_eq!(cpp.args, vec![
            "-E", "-Iinclude dir", "-DNAME=one two;$(touch never)",
            "-DOTHER=42", "-UOLD", "-UOBSOLETE", "-undef", "-std=c11",
        ].into_iter().map(OsString::from).collect::<Vec<_>>());
        assert_eq!(options.paths, ["source.c"]);
        assert_eq!(options.max_errors, None);

        let default = parse_options(["check", "a.c"].into_iter().map(str::to_owned)).unwrap();
        assert_eq!(default.cpp.unwrap().executable, OsString::from(
            if cfg!(feature = "internal-cpp") { "internal" } else { "cc" }
        ));
        let fallback = parse_options(["check", "--cpp=cc", "a.c"]
            .into_iter().map(str::to_owned)).unwrap();
        assert_eq!(fallback.cpp.unwrap().executable, OsString::from("cc"));
        let raw = parse_options(["check", "--cpp=none", "a.c"]
            .into_iter().map(str::to_owned)).unwrap();
        assert!(raw.cpp.is_none());
        let after_end = parse_options(["check", "--cpp=none", "--", "-file.c"]
            .into_iter().map(str::to_owned)).unwrap();
        assert_eq!(after_end.paths, ["-file.c"]);
    }

    #[test]
    fn shadow_warning_is_promoted_to_error_only_when_requested() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let path = std::env::temp_dir().join(format!("mini-c-shadow-{}-{}.c",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::write(&path,
            "int global; int f(int arg) { int global = arg; return global; }\n").unwrap();
        let mut policy = WarningPolicy::default();
        assert!(check_with_policy(path.to_str().unwrap(), None, &policy).is_ok());
        policy.apply_flag("-Wshadow").unwrap();
        assert!(check_with_policy(path.to_str().unwrap(), None, &policy).is_ok());
        policy.apply_flag("-Werror").unwrap();
        assert!(check_with_policy(path.to_str().unwrap(), None, &policy)
            .unwrap_err().contains("warnings treated as errors"));
        policy.apply_flag("-Wno-shadow").unwrap();
        assert!(check_with_policy(path.to_str().unwrap(), None, &policy).is_ok());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn targeted_werror_promotes_only_one_warning_class() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let path = std::env::temp_dir().join(format!("mini-c-werror-group-{}-{}.c",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::write(&path,
            "int f(int unused) { int local = 1; return 0; }\n").unwrap();
        let filename = path.to_str().unwrap();
        let mut policy = WarningPolicy::default();
        policy.apply_flag("-Wextra").unwrap();
        policy.apply_flag("-Wall").unwrap();
        policy.apply_flag("-Werror=unused-parameter").unwrap();
        assert!(check_with_policy(filename, None, &policy).is_err());
        policy.apply_flag("-Wno-error=unused-parameter").unwrap();
        assert!(check_with_policy(filename, None, &policy).is_ok());
        policy.apply_flag("-Werror=unused-variable").unwrap();
        assert!(check_with_policy(filename, None, &policy).is_err());
        policy.apply_flag("-Wno-unused-variable").unwrap();
        assert!(check_with_policy(filename, None, &policy).is_ok());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn warning_options_are_parsed_without_leaking_to_cpp() {
        let opts = parse_options([
            "check", "-Wall", "-Wextra", "-Werror",
            "-Wno-shadow", "--cpp=none", "sample.c",
        ].into_iter().map(str::to_owned)).unwrap();
        assert!(opts.warnings.wall);
        assert!(opts.warnings.wextra);
        assert!(opts.warnings.warnings_as_errors);
        assert!(!opts.warnings.enabled("shadow"));
        assert!(opts.warnings.enabled("unused-variable"));
        assert!(opts.warnings.enabled("unused-parameter"));
        assert!(opts.cpp.is_none());
        assert!(parse_options([
            "check", "-Wformat", "sample.c",
        ].into_iter().map(str::to_owned)).is_err());
    }

    #[test]
    fn invalid_preprocessor_options_fail_before_execution() {
        for values in [
            vec!["check", "--cpp=none", "-DNAME=1", "file.c"],
            vec!["check", "-I", "" , "file.c"],
            vec!["check", "-D", "file.c"],
            vec!["check", "--cpp=", "file.c"],
            vec!["check", "--cpp-arg=", "file.c"],
            vec!["check", "--max-errors=0", "file.c"],
            vec!["check", "--unknown", "file.c"],
            vec!["check", "--cpp=none"],
        ] {
            assert!(parse_options(values.into_iter().map(str::to_owned)).is_err());
        }
    }

    #[test]
    fn external_cc_preprocessing_includes_macros_and_maps_errors() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("mini-c cpp with spaces {} {nonce}", std::process::id()));
        std::fs::create_dir_all(dir.join("include folder")).unwrap();
        let header = dir.join("include folder").join("config.h");
        let nested = dir.join("include folder").join("nested.h");
        let source = dir.join("source file.c");
        std::fs::write(&nested, "#define HEADER_VALUE 17\n").unwrap();
        std::fs::write(&header, "#include \"nested.h\"\n").unwrap();
        std::fs::write(&source,
            "#include <config.h>\n#if FEATURE != 19\n#error wrong_feature\n#endif\n#ifdef OLD\n#error old_macro\n#endif\nint result = HEADER_VALUE+FEATURE;\n"
        ).unwrap();
        let arguments = vec![
            "check".to_owned(), "--cpp=cc".to_owned(), "-I".to_owned(),
            dir.join("include folder").to_string_lossy().into_owned(),
            "-DFEATURE=19".to_owned(), "-DOLD=1".to_owned(),
            "-U".to_owned(), "OLD".to_owned(),
            source.to_string_lossy().into_owned(),
        ];
        let options = parse_options(arguments).unwrap();
        assert_eq!(options.cpp.as_ref().unwrap().executable, OsString::from("cc"));
        let outcome = check(source.to_str().unwrap(), options.cpp.as_ref());
        assert!(outcome.is_ok(), "{outcome:?}");

        std::fs::write(&nested, "int invalid = ;\n").unwrap();
        let failure = check(source.to_str().unwrap(), options.cpp.as_ref()).unwrap_err();
        assert!(failure.contains("nested.h:1"), "{failure}");
        assert!(failure.contains("error"), "{failure}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(feature = "internal-cpp")]
    #[test]
    fn default_cpp_driver_checks_hosted_stdio_without_external_preprocessor() {
        let input = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/glibc-s12/hello_stdio.c");
        if !cfg!(target_os = "linux") || !Path::new("/usr/include/stdio.h").is_file() {
            return;
        }
        let args = vec![
            "check".to_string(), "-D_GNU_SOURCE".to_string(),
            input.display().to_string(),
        ];
        let options = parse_options(args).unwrap();
        assert_eq!(options.cpp.as_ref().unwrap().executable, OsString::from("internal"));
        check(&options.paths[0], options.cpp.as_ref()).unwrap();
    }

    #[cfg(all(feature = "internal-cpp", target_os = "linux"))]
    #[test]
    fn unchanged_musl_memmem_passes_checked_frontend() {
        // The original musl source exercises multiple helpers, complex
        // pointer arithmetic, shifts, statement expressions, and system
        // declarations. Do not modify the fixture or preprocess it by hand.
        let input = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/musl-s10/string/memmem.c");
        if !Path::new("/usr/include/string.h").is_file() {
            return;
        }
        let opts = parse_options([
            "check".to_owned(), "--cpp=internal".to_owned(),
            input.to_string_lossy().into_owned(),
        ]).unwrap();
        let result = check(&opts.paths[0], opts.cpp.as_ref());
        assert!(result.is_ok(), "unchanged musl memmem.c: {result:?}");
    }

    #[cfg(feature = "internal-cpp")]
    #[test]
    fn internal_cpp_driver_checks_macro_expanded_translation_unit() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let dir = std::env::temp_dir().join(format!("mini-c-driver-internal-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("program.c");
        std::fs::write(&source, "#define INC(x) ((x)+1)\nint f(void) { return INC(16); }\n").unwrap();
        let opts = parse_options(["check", "--cpp=internal",
            source.to_str().unwrap()].into_iter().map(str::to_owned)).unwrap();
        let result = check(&opts.paths[0], opts.cpp.as_ref());
        let _ = std::fs::remove_dir_all(dir);
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn cli_checks_types_and_maps_semantic_locations() {
        let unique = format!("mini-c-sema-check-{}.c", std::process::id());
        let path = std::env::temp_dir().join(unique);
        std::fs::write(&path, "int f(void) {\n return unknown;\n}\n").unwrap();
        let error = check(path.to_str().unwrap(), None).unwrap_err();
        std::fs::remove_file(&path).unwrap();
        assert!(error.contains("undeclared identifier"), "{error}");
        assert!(error.contains(".c:"), "{error}");
    }

    #[test]
    fn cli_accepts_typed_valid_functions() {
        let unique = format!("mini-c-sema-valid-{}.c", std::process::id());
        let path = std::env::temp_dir().join(unique);
        std::fs::write(&path, "typedef unsigned int Value;\nValue g = 2;\nint twice(int x) { return x + x; }\n").unwrap();
        let result = check(path.to_str().unwrap(), None);
        std::fs::remove_file(&path).unwrap();
        assert!(result.is_ok(), "{result:?}");
    }
    #[test]
    fn cli_uses_exact_declaration_name_columns() {
        let path = std::env::temp_dir().join(format!(
            "mini-c-named-diagnostic-{}.c", std::process::id(),
        ));
        std::fs::write(&path, "int x;\nlong x;\n").unwrap();
        let filename = path.to_str().unwrap();
        let result = check_recovering(filename, None, 3).unwrap_err();
        assert!(result.contains(":2:6: error[T0103]"), "{result}");
        let result = check(filename, None).unwrap_err();
        assert!(result.contains(":2:6:"), "{result}");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn opt_in_multi_error_cli_reports_bounded_codes_and_locations() {
        let path = std::env::temp_dir().join(format!(
            "mini-c-multi-diagnostic-{}.c", std::process::id(),
        ));
        std::fs::write(
            &path,
            "int broken = ;\nint okay(void) { return 1; }\nint wrong(void) { return missing; }\nint next(void) { return unknown; }\n",
        ).unwrap();
        let filename = path.to_str().unwrap();
        let first_two = check_recovering(filename, None, 2).unwrap_err();
        assert!(first_two.contains("error[S0102]"), "{first_two}");
        assert!(first_two.contains("error[T0105]"), "{first_two}");
        assert!(!first_two.contains("unknown"), "{first_two}");
        assert_eq!(first_two.lines().filter(|line| line.contains(": error[")).count(), 2, "{first_two}");

        let all = check_recovering(filename, None, 4).unwrap_err();
        assert_eq!(all.lines().filter(|line| line.contains(": error[")).count(), 3, "{all}");
        assert!(all.contains("unknown"), "{all}");
        std::fs::remove_file(path).unwrap();
    }


    #[cfg(feature = "internal-cpp")]
    #[test]
    fn internal_cpp_reports_original_header_spelling_with_logical_line() {
        let dir = std::env::temp_dir().join(format!("mini-c-origin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.join("main.c");
        std::fs::write(dir.join("bad.h"), "#line 42 \"virtual.h\"\nint bad = ;\n").unwrap();
        std::fs::write(&root, "#include \"bad.h\"\n").unwrap();
        let config = preprocess::Config {
            executable: "internal".into(), args: vec!["-E".into(), "-nostdinc".into()],
        };
        let error = check(root.to_str().unwrap(), Some(&config)).unwrap_err();
        std::fs::remove_dir_all(dir).unwrap();
        assert!(error.contains("virtual.h:42:11:"), "{error}");
        assert!(error.contains("int bad = ;"), "{error}");
        assert!(error.contains('^'), "{error}");
    }

    #[cfg(feature = "internal-cpp")]
    #[test]
    fn nested_macro_diagnostic_shows_both_invocations() {
        let path = std::env::temp_dir().join(format!("mini-c-nested-origin-{}.c", std::process::id()));
        std::fs::write(&path, "#define INNER ;\n#define OUTER INNER\nint x = OUTER;\n").unwrap();
        let config = preprocess::Config {
            executable: "internal".into(), args: vec!["-E".into(), "-nostdinc".into()],
        };
        let error = check(path.to_str().unwrap(), Some(&config)).unwrap_err();
        let streamed = check(path.to_str().unwrap(), Some(&preprocess::Config {
            executable: "internal-stream".into(), args: config.args.clone(),
        })).unwrap_err();
        std::fs::remove_file(path).unwrap();
        assert_eq!(error, streamed);
        let inner = error.find("in expansion of macro 'INNER'").expect(&error);
        let outer = error.find("in expansion of macro 'OUTER'").expect(&error);
        assert!(inner < outer, "{error}");
        assert!(error.contains("#define OUTER INNER"), "{error}");
        assert!(error.contains("int x = OUTER;"), "{error}");
    }
