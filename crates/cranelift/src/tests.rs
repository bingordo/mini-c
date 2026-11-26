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
    fn output_filename_option_does_not_consume_the_next_flag() {
        for next in ["-Werror", "-Wall", "-I", "--cpp=none", "--emit-obj", "--"] {
            let args = ["--emit-obj", next, "main.c"];
            let err = parse_options(args.into_iter().map(str::to_owned)).err()
                .expect("option-looking output name must be rejected");
            assert!(err.contains("output filename"), "{next}: {err}");
        }
        // Explicitly supplied filenames, including spaces, remain valid.
        let opts = parse_options([
            "--emit-obj", "obj with spaces.o", "-Wall", "main.c",
        ].into_iter().map(str::to_owned)).unwrap();
        assert_eq!(opts.output, "obj with spaces.o");
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

/// Exercise all implemented semantic warning groups through native ELF
/// emission, including default enablement, promotion, and explicit suppression.
#[test]
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn semantic_warning_groups_gate_elf_object_output() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "mini-c-warning-matrix-{}-{nonce}", std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();

    // GCC-compatible policy: unused-variable follows -Wall,
    // unused-parameter follows -Wextra, and shadow is explicitly opt-in.
    /// One C fixture with the expected warning-policy outcomes.
    struct WarningCase {
        name: &'static str,
        contents: &'static str,
        allowed: &'static [&'static str],
        promoted: &'static [&'static str],
        suppressed: &'static [&'static str],
    }
    let cases = [
        WarningCase {
            name: "unused-variable",
            contents: "int main(void) { int dead = 1; return 0; }",
            allowed: &["-Wunused-variable"],
            promoted: &["-Wunused-variable", "-Werror"],
            suppressed: &["-Wunused-variable", "-Werror", "-Wno-unused-variable"],
        },
        WarningCase {
            name: "shadow",
            contents: "int value; int main(void) { int value = 3; return value - 3; }",
            allowed: &["-Wall", "-Werror"],
            promoted: &["-Wshadow", "-Werror"],
            suppressed: &["-Wshadow", "-Werror", "-Wno-shadow"],
        },
        WarningCase {
            name: "unused-parameter",
            contents: "int f(int unused) { return 17; } int main(void) { return f(4)-17; }",
            allowed: &["-Wall", "-Werror"],
            promoted: &["-Wextra", "-Werror"],
            suppressed: &["-Wextra", "-Werror", "-Wno-unused-parameter"],
        },
    ];

    for WarningCase { name, contents, allowed, promoted, suppressed } in cases {
        let source = dir.join(format!("{name}.c"));
        let output = dir.join(format!("{name}.o"));
        std::fs::write(&source, contents).unwrap();
        let options = |flags: &[&str]| {
            let mut args = vec![
                "--cpp=none".to_owned(), "--emit-obj".to_owned(),
                output.to_string_lossy().into_owned(),
            ];
            args.extend(flags.iter().map(|flag| (*flag).to_owned()));
            args.push(source.to_string_lossy().into_owned());
            parse_options(args).unwrap()
        };

        assert_eq!(compile_to_object(&options(allowed)), ExitCode::SUCCESS, "{name}");
        assert!(std::fs::read(&output).unwrap().starts_with(b"\x7fELF"), "{name}");
        std::fs::remove_file(&output).unwrap();

        assert_eq!(compile_to_object(&options(promoted)), ExitCode::FAILURE, "{name}");
        assert!(!output.exists(), "{name}: promoted warning created an object");

        assert_eq!(compile_to_object(&options(suppressed)), ExitCode::SUCCESS, "{name}");
        assert!(std::fs::read(&output).unwrap().starts_with(b"\x7fELF"), "{name}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn object_output_must_not_overwrite_source_or_hard_link() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "mini-c-object-alias-{}-{nonce}", std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("program.c");
    let alias = dir.join("program-alias.o");
    let symlink = dir.join("program-symlink.o");
    let original = b"int main(void) { return 0; }\n";
    std::fs::write(&source, original).unwrap();
    std::fs::hard_link(&source, &alias).unwrap();
    std::os::unix::fs::symlink(&source, &symlink).unwrap();

    for output in [&source, &alias, &symlink] {
        let options = parse_options([
            "--cpp=none".to_owned(), "--emit-obj".to_owned(),
            output.to_string_lossy().into_owned(),
            source.to_string_lossy().into_owned(),
        ]).unwrap();
        assert_eq!(compile_to_object(&options), ExitCode::FAILURE);
        assert_eq!(std::fs::read(&source).unwrap(), original);
        assert_eq!(std::fs::read(&alias).unwrap(), original);
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn cmake_abi_adjacent_literals_parse_emit_link_and_execute() {
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "mini-c-cmake-abi-{}-{nonce}", std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("cmake_abi.c");
    let object = dir.join("cmake_abi.o");
    let executable = dir.join("cmake_abi");
    // This exact initializer shape previously blocked CMakeCCompilerABI.c.
    // Each fragment has its own escapes, and exactly one implicit NUL
    // remains after translation phase 6 concatenation.
    std::fs::write(&source, concat!(
        "static char const info_abi[] = \"INFO:abi[\" \"ELF\" \"]\";\n",
        "static char const embedded[] = \"a\\0\" /* join */ \"b\";\n",
        "int main(void) {\n",
        "  return !(sizeof(info_abi) == 14 && info_abi[9] == 'E'\n",
        "    && info_abi[11] == 'F' && sizeof(embedded) == 4\n",
        "    && embedded[0] == 'a' && embedded[1] == 0\n",
        "    && embedded[2] == 'b' && embedded[3] == 0);\n",
        "}\n",
    )).unwrap();
    let opts = parse_options([
        "--cpp=none".to_owned(),
        "--emit-obj".to_owned(),
        object.to_string_lossy().into_owned(),
        source.to_string_lossy().into_owned(),
    ]).unwrap();
    assert_eq!(compile_to_object(&opts), ExitCode::SUCCESS);
    assert!(std::fs::read(&object).unwrap().starts_with(b"\x7fELF"));
    let link = Command::new("cc")
        .arg(&object).arg("-o").arg(&executable)
        .output().unwrap();
    assert!(link.status.success(), "link: {}", String::from_utf8_lossy(&link.stderr));
    assert!(Command::new(&executable).status().unwrap().success(),
        "adjacent string fragment content or NUL termination is wrong");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn c99_nonprototype_call_links_to_typed_external_definition() {
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("mini-c-no-prototype-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("caller.c");
    let helper = dir.join("helper.c");
    let object = dir.join("caller.o");
    let executable = dir.join("program");
    std::fs::write(&source, concat!(
        "int mix();\n",
        "int main(void) { char a=7; short b=9; return mix(a,b,2.0)!=18; }\n",
    )).unwrap();
    std::fs::write(&helper,
        "int mix(int a, int b, double c) { return a+b+(int)c; }\n").unwrap();
    let opts = parse_options([
        "--cpp=none".to_owned(), "--emit-obj".to_owned(),
        object.to_string_lossy().into_owned(), source.to_string_lossy().into_owned(),
    ]).unwrap();
    assert_eq!(compile_to_object(&opts), ExitCode::SUCCESS);
    let link = Command::new("cc").arg(&object).arg(&helper)
        .arg("-o").arg(&executable).output().unwrap();
    assert!(link.status.success(), "link: {}", String::from_utf8_lossy(&link.stderr));
    assert!(Command::new(&executable).status().unwrap().success(),
        "non-prototype call did not pass promoted arguments correctly");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn c99_identifier_list_definition_receives_promoted_arguments() {
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("mini-c-kr-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("definition.c");
    let helper = dir.join("caller.c");
    let object = dir.join("definition.o");
    let executable = dir.join("program");
    std::fs::write(&source, concat!(
        "int mix();\n",
        "int mix(a,b,c) float c; signed char a; short b; {\n",
        "  return sizeof(a)==1 && sizeof(b)==2 && sizeof(c)==4 && a==-7 && b==9 && c==2.5;\n",
        "}\n",
    )).unwrap();
    std::fs::write(&helper,
        "int mix(int, int, double); int main(void) { return !mix(-7,9,2.5); }\n").unwrap();
    let opts = parse_options([
        "--cpp=none".to_owned(), "--emit-obj".to_owned(),
        object.to_string_lossy().into_owned(), source.to_string_lossy().into_owned(),
    ]).unwrap();
    assert_eq!(compile_to_object(&opts), ExitCode::SUCCESS);
    let link = Command::new("cc").arg(&object).arg(&helper)
        .arg("-o").arg(&executable).output().unwrap();
    assert!(link.status.success(), "link: {}", String::from_utf8_lossy(&link.stderr));
    assert!(Command::new(&executable).status().unwrap().success(),
        "old-style definition did not receive promoted arguments correctly");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn c99_indirect_nonprototype_and_variadic_calls_preserve_registers_and_stack() {
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("mini-c-indirect-varargs-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("caller.c");
    let helper = dir.join("helper.c");
    let object = dir.join("caller.o");
    let executable = dir.join("program");
    std::fs::write(&source, concat!(
        "int small(); int large(); int vari(int,...);\n",
        "int main(void) {\n",
        "  int (*p)()=small; int (*q)()=large; int (*v)(int,...)=vari;\n",
        "  char a=3; float b=2.5f;\n",
        "  if(p(a,b)!=1 || (*p)(a,b)!=1) return 1;\n",
        "  if(q(1,2,3,4,5,6,7,1.0,2.0,3.0,4.0,5.0,6.0,7.0,8.0,9.0)!=1) return 2;\n",
        "  if(v(1,2,3,4,5,6,7,1.0,2.0,3.0,4.0,5.0,6.0,7.0,8.0,9.0)!=1) return 3;\n",
        "  return 0;\n",
        "}\n",
    )).unwrap();
    std::fs::write(&helper, concat!(
        "#include <stdarg.h>\n",
        "int small(int a,double b) {return a==3 && b==2.5;}\n",
        "int large(int a,int b,int c,int d,int e,int f,int g,",
        "double h,double i,double j,double k,double l,double m,double n,double o,double p) {",
        "return a==1&&b==2&&c==3&&d==4&&e==5&&f==6&&g==7&&h==1&&i==2&&j==3&&k==4&&l==5&&m==6&&n==7&&o==8&&p==9;}\n",
        "int vari(int a,...) { va_list ap; va_start(ap,a); int ok=a==1; ",
        "for(int i=2;i<=7;i++) if(va_arg(ap,int)!=i) ok=0; ",
        "for(int i=1;i<=9;i++) if(va_arg(ap,double)!=i) ok=0; ",
        "va_end(ap);return ok;}\n",
    )).unwrap();
    let opts = parse_options([
        "--cpp=none".to_owned(), "--emit-obj".to_owned(),
        object.to_string_lossy().into_owned(), source.to_string_lossy().into_owned(),
    ]).unwrap();
    assert_eq!(compile_to_object(&opts), ExitCode::SUCCESS);
    let link = Command::new("cc").arg(&object).arg(&helper)
        .arg("-o").arg(&executable).output().unwrap();
    assert!(link.status.success(), "link: {}", String::from_utf8_lossy(&link.stderr));
    let status = Command::new(&executable).status().unwrap();
    assert!(status.success(), "indirect argument forwarding failed: {status}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn c99_inline_only_body_uses_external_definition_across_translation_units() {
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("mini-c-inline-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("caller.c");
    let helper = dir.join("definition.c");
    let object = dir.join("caller.o");
    let executable = dir.join("program");
    std::fs::write(&source, concat!(
        "inline int f(void){return 1;}\n",
        "inline int g(void){return 3;}\n",
        "int main(void){int (*p)(void)=f; return p()!=2 || g()!=3;}\n",
        "extern inline int g(void);\n",
    )).unwrap();
    std::fs::write(&helper, "int f(void){return 2;}\n").unwrap();
    let opts = parse_options([
        "--cpp=none".to_owned(), "--emit-obj".to_owned(),
        object.to_string_lossy().into_owned(), source.to_string_lossy().into_owned(),
    ]).unwrap();
    assert_eq!(compile_to_object(&opts), ExitCode::SUCCESS);
    let link = Command::new("cc").arg(&object).arg(&helper)
        .arg("-o").arg(&executable).output().unwrap();
    assert!(link.status.success(), "link: {}", String::from_utf8_lossy(&link.stderr));
    assert!(Command::new(&executable).status().unwrap().success());
    let _ = std::fs::remove_dir_all(dir);
}
