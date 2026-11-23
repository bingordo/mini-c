//! Driver tests for option parsing, diagnostics, and compilation behavior.
    use super::*;

    #[test]
    fn native_driver_defaults_to_internal_and_keeps_external_fallback() {
        let default = parse_options([
            "--emit-obj", "out.o", "main.c",
        ].into_iter().map(str::to_owned)).unwrap();
        assert_eq!(default.cpp.unwrap().executable, std::ffi::OsString::from(
            if cfg!(feature = "internal-cpp") { "internal" } else { "cc" }
        ));
        let fallback = parse_options([
            "--emit-obj", "out.o", "--cpp=cc", "main.c",
        ].into_iter().map(str::to_owned)).unwrap();
        assert_eq!(fallback.cpp.unwrap().executable, "cc");
    }

    #[cfg(all(feature = "internal-cpp", target_arch = "x86_64", target_os = "linux"))]
    #[test]
    fn internal_cpp_emits_native_object_without_external_cpp() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let dir = std::env::temp_dir().join(format!("mini-c-rcc-obj-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("program.c");
        std::fs::write(&source,
            "#define VALUE 19\nint main(void) { return VALUE - 19; }\n").unwrap();
        let output = dir.join("output.o");
        let args = vec![
            "--emit-obj".to_string(),
            output.to_string_lossy().into_owned(),
            source.to_string_lossy().into_owned(),
        ];
        let options = parse_options(args).unwrap();
        assert_eq!(options.cpp.as_ref().unwrap().executable, std::ffi::OsString::from("internal"));
        let preprocessed = preprocess::file(
            Path::new(&options.source), options.cpp.as_ref().unwrap()
        );
        let result = preprocessed.and_then(|text|
            compile_source_diagnostic(&text)
                .map_err(|e| preprocess::Error::Internal(e.to_string()))
        );
        let _ = std::fs::remove_dir_all(dir);
        let bytes = result.unwrap();
        assert!(bytes.starts_with(b"\x7fELF"));
    }

    #[test]
    fn native_diagnostics_restore_header_origins_and_raw_columns() {
        let source = "# 1 \"source.c\"\n# 8 \"headers with spaces.h\" 1\nint wrong = ;\n";
        let diagnostic = compile_source_diagnostic(source).unwrap_err();
        let rendered = format_compiler_error("input.c", source, true, diagnostic);
        assert!(rendered.contains("headers with spaces.h:8"), "{rendered}");
        assert!(rendered.contains("error[S"), "{rendered}");

        let source = "int wrong = ;\n";
        let diagnostic = compile_source_diagnostic(source).unwrap_err();
        let rendered = format_compiler_error("raw.c", source, false, diagnostic);
        assert!(rendered.contains("raw.c:1:"), "{rendered}");
    }

    #[test]
    fn common_cpp_flags_parse_for_object_emitter() {
        let opts = parse_options([
            "--emit-obj", "output with spaces.o",
            "-I", "path with spaces",
            "--cpp=clang", "-DNAME=17", "-UOLD", "source.c",
        ].into_iter().map(str::to_owned)).unwrap();
        assert_eq!(opts.output, "output with spaces.o");
        assert_eq!(opts.source, "source.c");
        let cpp = opts.cpp.unwrap();
        assert_eq!(cpp.executable, "clang");
        assert_eq!(
            cpp.args,
            vec!["-E", "-Ipath with spaces", "-DNAME=17", "-UOLD"]
                .into_iter().map(std::ffi::OsString::from).collect::<Vec<_>>(),
        );
        let raw = parse_options([
            "--cpp=none", "--emit-obj", "out.o", "--", "-source.c",
        ].into_iter().map(str::to_owned)).unwrap();
        assert!(raw.cpp.is_none());
        assert_eq!(raw.source, "-source.c");
    }

    #[test]
    fn native_warning_options_share_frontend_policy() {
        let opts = parse_options([
            "--emit-obj", "out.o", "-Wall", "-Werror",
            "-Wno-unused-variable", "-Wno-shadow", "main.c",
        ].into_iter().map(str::to_owned)).unwrap();
        assert!(opts.warnings.wall);
        assert!(opts.warnings.warnings_as_errors);
        assert!(!opts.warnings.enabled("shadow"));
        assert!(!opts.warnings.enabled("unused-variable"));
        let enabled = parse_options(["--emit-obj", "out.o", "-Wall", "main.c"]
            .into_iter().map(str::to_owned)).unwrap();
        assert!(enabled.warnings.enabled("unused-variable"));
        assert!(parse_options([
            "--emit-obj", "out.o", "-Wshadow", "main.c",
        ].into_iter().map(str::to_owned)).unwrap().warnings.enabled("shadow"));
        assert!(parse_options([
            "--emit-obj", "out.o", "-Wformat", "main.c",
        ].into_iter().map(str::to_owned)).is_err());
    }

    #[test]
    fn invalid_native_cpp_options_are_rejected() {
        for args in [
            vec!["source.c"],
            vec!["--emit-obj", "out.o"],
            vec!["--emit-obj", "out.o", "a.c", "b.c"],
            vec!["--emit-obj", "out.o", "--cpp=none", "-DVALUE=1", "a.c"],
            vec!["--emit-obj", "out.o", "-I"],
            vec!["--emit-obj", "out.o", "--bad", "a.c"],
        ] {
            assert!(parse_options(args.into_iter().map(str::to_owned)).is_err());
        }
    }

    #[test]
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    fn preprocessed_header_and_macro_emit_elf_object() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("mini-c-obj-preproc-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(dir.join("headers with spaces")).unwrap();
        let include = dir.join("headers with spaces");
        let src = dir.join("src.c");
        std::fs::write(include.join("value.h"), "#define HEADER_VALUE 23\n").unwrap();
        std::fs::write(&src, "#include <value.h>\nint main(void) { return HEADER_VALUE-FEATURE; }\n").unwrap();
        let flags = vec![
            "--emit-obj".to_owned(), dir.join("program.o").to_string_lossy().into_owned(),
            "-I".to_owned(), include.to_string_lossy().into_owned(),
            "-DFEATURE=17".to_owned(), src.to_string_lossy().into_owned(),
        ];
        let opts = parse_options(flags).unwrap();
        let bytes = preprocess::file(Path::new(&opts.source), opts.cpp.as_ref().unwrap());
        match bytes {
            Err(preprocess::Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
            other => {
                let compiled = compile_source_diagnostic(&other.unwrap()).unwrap();
                assert!(compiled.starts_with(b"\x7fELF"));
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }

#[test]
fn native_warning_collection_respects_scope_and_groups() {
    let source = "int f(int a, int unused) { int x = a; int dead = 1; return x; }";
    let mut policy = WarningPolicy::default();
    policy.apply_flag("-Wall").unwrap();
    policy.apply_flag("-Wextra").unwrap();
    let warnings = collect_semantic_warnings(source, &policy).unwrap();
    let groups: Vec<_> = warnings.iter().map(|d| d.warning_group).collect();
    assert_eq!(groups, vec![Some("unused-parameter"), Some("unused-variable")]);
    assert!(warnings.iter().any(|d| d.diagnostic.message.contains("dead")));
    assert!(warnings.iter().any(|d| d.diagnostic.message.contains("unused")));
}

#[test]
fn native_warning_promotion_blocks_object_write() {
    let source = "int f(void) { int dead = 1; return 0; }";
    let mut policy = WarningPolicy::default();
    assert!(collect_semantic_warnings(source, &policy).unwrap().is_empty());
    policy.apply_flag("-Wall").unwrap();
    assert!(!emit_semantic_warnings("source.c", source, false, &policy).unwrap());
    policy.apply_flag("-Werror").unwrap();
    assert!(emit_semantic_warnings("source.c", source, false, &policy).unwrap());
    policy.apply_flag("-Wno-unused-variable").unwrap();
    assert!(!emit_semantic_warnings("source.c", source, false, &policy).unwrap());
}

#[test]
fn native_warning_checks_preserve_preprocessed_origins() {
    let source = "# 20 \"source.h\"\nint f(void) { int dead = 1; return 0; }\n";
    let mut policy = WarningPolicy::default();
    policy.apply_flag("-Wunused-variable").unwrap();
    let (messages, failed) =
        format_semantic_warnings("main.c", source, true, &policy).unwrap();
    assert!(!failed);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].starts_with("source.h:20: warning:"), "{messages:?}");
    assert!(messages[0].contains("[-Wunused-variable]"), "{messages:?}");
}

#[test]
fn native_warning_locations_keep_raw_columns_and_werror_severity() {
    let source = "int f(void) { int dead = 1; return 0; }";
    let mut policy = WarningPolicy::default();
    policy.apply_flag("-Wunused-variable").unwrap();
    let (warnings, failed) =
        format_semantic_warnings("raw.c", source, false, &policy).unwrap();
    assert!(!failed);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].starts_with("raw.c:1:"), "{warnings:?}");
    assert!(warnings[0].contains(": warning:"), "{warnings:?}");
    policy.apply_flag("-Werror").unwrap();
    let (errors, failed) =
        format_semantic_warnings("raw.c", source, false, &policy).unwrap();
    assert!(failed);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains(": error:"), "{errors:?}");
    assert!(errors[0].contains("[-Wunused-variable]"), "{errors:?}");
    policy.apply_flag("-Wno-unused-variable").unwrap();
    let (suppressed, failed) =
        format_semantic_warnings("raw.c", source, false, &policy).unwrap();
    assert!(!failed);
    assert!(suppressed.is_empty(), "{suppressed:?}");
}

#[test]
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn warning_promotion_prevents_object_creation_but_normal_warning_emits_elf() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "mini-c-cranelift-warning-gate-{}-{nonce}", std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("source.c");
    let output = dir.join("output.o");
    std::fs::write(&source, "int main(void) { int unused = 1; return 0; }\n").unwrap();

    let options = |warnings: &[&str]| {
        let mut args = vec![
            "--cpp=none".to_owned(),
            "--emit-obj".to_owned(),
            output.to_string_lossy().into_owned(),
        ];
        args.extend(warnings.iter().map(|flag| (*flag).to_owned()));
        args.push(source.to_string_lossy().into_owned());
        parse_options(args).unwrap()
    };

    assert_eq!(
        compile_to_object(&options(&["-Wunused-variable", "-Werror"])),
        ExitCode::FAILURE
    );
    assert!(!output.exists(), "-Werror must not create an object");
    assert_eq!(
        compile_to_object(&options(&["-Wunused-variable"])),
        ExitCode::SUCCESS
    );
    assert!(std::fs::read(&output).unwrap().starts_with(b"\x7fELF"));
    std::fs::remove_file(&output).unwrap();
    assert_eq!(
        compile_to_object(&options(&["-Wunused-variable", "-Werror",
                                     "-Wno-unused-variable"])),
        ExitCode::SUCCESS
    );
    assert!(std::fs::read(&output).unwrap().starts_with(b"\x7fELF"));
    let _ = std::fs::remove_dir_all(dir);
}
