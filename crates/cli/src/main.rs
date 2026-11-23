use mini_c_preprocess::{self as preprocess, source_map::SourceMap};
use mini_c_sema::{Checker, Target};
use mini_c_syntax::parser::Parser;
use mini_c_syntax::diag::{DiagnosticSeverity, WarningPolicy};
use mini_c_syntax::source::SourceFile;
use mini_c_syntax::stream::ItemSource;
use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

#[cfg(feature = "internal-cpp")]
mod stream_check;

fn check(path: &str, cpp: Option<&preprocess::Config>) -> Result<(), String> {
    check_with_policy(path, cpp, &WarningPolicy::default())
}

fn check_with_policy(
    path: &str, cpp: Option<&preprocess::Config>, warnings: &WarningPolicy
) -> Result<(), String> {
    if let Some(config) = cpp {
        if config.executable == std::ffi::OsStr::new("internal-stream") {
            if warnings.wall || warnings.wextra || warnings.overrides.iter().any(|(_, enabled)| *enabled) {
                return Err("warnings are not yet supported with --cpp=internal-stream".into());
            }
            #[cfg(feature = "internal-cpp")]
            return stream_check::check(path, config, None);
            #[cfg(not(feature = "internal-cpp"))]
            return Err("internal-stream requires --features internal-cpp".into());
        }
    }
    let source = match cpp {
        Some(config) => preprocess::file(Path::new(path), config)
            .map_err(|e| format!("{path}: {e}"))?,
        None => fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?,
    };
    let file = SourceFile::new(path, &source);
    let map = cpp.map(|_| SourceMap::from_preprocessed(&source, path));
    let mut parser = Parser::new_with_spans(&source);
    let mut checker = Checker::new(Target::default()).expect("valid default target");
    checker.set_warning_policy(warnings.clone());
    let mut warning_failed = false;
    loop {
        // The checker only retains symbol/type state, not completed function ASTs.
        let checked = parser.next_item().and_then(|item| {
            item.map(|item| checker.check_item(item)).transpose()
        });
        match checked {
            Ok(Some(_)) => {
                for warning in checker.take_warnings() {
                    if let Some(severity) = warnings.resolve(&warning) {
                        let location = if let Some(ref map) = map {
                            let origin = map.origin(warning.diagnostic.span.lo);
                            format!("{}:{}", origin.file, origin.line)
                        } else {
                            let (line, col) = file.location(warning.diagnostic.span.lo);
                            format!("{path}:{line}:{col}")
                        };
                        let level = match severity {
                            DiagnosticSeverity::Error => {
                                warning_failed = true;
                                "error"
                            }
                            DiagnosticSeverity::Warning => "warning",
                            DiagnosticSeverity::Note => "note",
                        };
                        eprintln!("{location}: {level}: {} [-W{}]",
                            warning.diagnostic.message,
                            warning.warning_group.unwrap_or("unknown"));
                    }
                }
            },
            Ok(None) => return if warning_failed {
                Err(format!("{path}: warnings treated as errors"))
            } else { Ok(()) },
            Err(error) => {
                let location = if let Some(ref map) = map {
                    let origin = map.origin(error.span.lo);
                    format!("{}:{}", origin.file, origin.line)
                } else {
                    let (line, col) = file.location(error.span.lo);
                    format!("{path}:{line}:{col}")
                };
                return Err(format!("{location}: error: {}", error.message));
            }
        }
    }
}

/// Opt-in multi-error checking: each item is checked transactionally and
/// unsuccessful items do not poison later typedef or semantic symbol state.
fn check_recovering(
    path: &str, cpp: Option<&preprocess::Config>, max_errors: usize
) -> Result<(), String> {
    if let Some(config) = cpp {
        if config.executable == std::ffi::OsStr::new("internal-stream") {
            #[cfg(feature = "internal-cpp")]
            return stream_check::check(path, config, Some(max_errors));
            #[cfg(not(feature = "internal-cpp"))]
            return Err("internal-stream requires --features internal-cpp".into());
        }
    }
    let source = match cpp {
        Some(config) => preprocess::file(Path::new(path), config)
            .map_err(|e| format!("{path}: {e}"))?,
        None => fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?,
    };
    let file = SourceFile::new(path, &source);
    let map = cpp.map(|_| SourceMap::from_preprocessed(&source, path));
    let mut parser = Parser::new_with_spans(&source);
    let mut checker = Checker::new(Target::default()).expect("valid default target");
    let errors = mini_c_sema::diagnose_stream_recovering(
        &mut parser, &mut checker, max_errors,
    );
    if errors.is_empty() { return Ok(()); }
    Err(errors.into_iter().map(|error| {
        let location = if let Some(ref map) = map {
            let origin = map.origin(error.span.lo);
            format!("{}:{}", origin.file, origin.line)
        } else {
            let (line, column) = file.location(error.span.lo);
            format!("{path}:{line}:{column}")
        };
        format!("{location}: error[{}]: {}", error.code.as_str(), error.message)
    }).collect::<Vec<_>>().join("\n"))
}

fn usage() {
    eprintln!(
        "usage: mini-c check [--cpp=internal|--cpp=internal-stream|--cpp=cc|--cpp=none|--cpp=clang|--cpp=<path>] \
         [-I<dir>|-I <dir>] [-D<name>[=<value>]|-D <name>[=<value>]] \
         [-U<name>|-U <name>] [--cpp-arg=<arg>] \
         [--max-errors=<positive integer>] [-Wall|-Wextra|-Werror|-Wno-*] [--] <file.c> [file.c ...]"
    );
}

/// Parsed driver options. Argument collection is deliberately separate from
/// spawning a preprocessor so unit tests can check the exact argument vector.
#[derive(Debug)]
struct Options {
    cpp: Option<preprocess::Config>,
    max_errors: Option<usize>,
    warnings: WarningPolicy,
    paths: Vec<String>,
}

fn parse_options(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut args = args.into_iter();
    if args.next().as_deref() != Some("check") {
        return Err("expected 'check' subcommand".into());
    }
    let mut cpp = preprocess::driver_options::CppOptions::default();
    let mut paths = Vec::new();
    let mut max_errors = None;
    let mut warnings = WarningPolicy::default();
    let mut end_options = false;
    while let Some(arg) = args.next() {
        if end_options {
            paths.push(arg);
            continue;
        }
        if arg == "--" {
            end_options = true;
        } else if let Some(value) = arg.strip_prefix("--max-errors=") {
            match value.parse::<usize>() {
                Ok(n) if n > 0 => max_errors = Some(n),
                _ => return Err("--max-errors requires a positive integer".into()),
            }
        } else if arg.starts_with("-W") {
            warnings.apply_flag(&arg)?;
        } else if cpp.consume(&arg, &mut args)? {
            // Recognized preprocessor option, with any split argument consumed.
        } else if arg.starts_with('-') {
            return Err(format!("unknown option: {arg}"));
        } else {
            paths.push(arg);
        }
    }
    if paths.is_empty() { return Err("no input files provided".into()); }
    let cpp = cpp.finish()?;
    Ok(Options { cpp, max_errors, warnings, paths })
}

fn main() -> ExitCode {
    let options = match parse_options(env::args().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}");
            usage();
            return ExitCode::FAILURE;
        }
    };
    let mut failed = false;
    for path in options.paths {
        let result = match options.max_errors {
            Some(limit) => {
                if options.warnings.wall || options.warnings.wextra
                    || options.warnings.overrides.iter().any(|(_, enabled)| *enabled)
                {
                    Err("warning analyses are not supported with --max-errors yet".into())
                } else {
                    check_recovering(&path, options.cpp.as_ref(), limit)
                }
            },
            None => check_with_policy(&path, options.cpp.as_ref(), &options.warnings),
        };
        if let Err(error) = result {
            eprintln!("{error}");
            failed = true;
        }
    }
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}


#[cfg(test)]
mod tests {
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
    fn opt_in_bounded_cpp_pipe_rejects_atomic_types_explicitly() {
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
        // A preceding valid declaration must not make an unsupported
        // atomic type appear successful in the pull-driven checker.
        std::fs::write(&root, "int first;\n_Atomic int counter;\n").unwrap();
        let failure = check(root.to_str().unwrap(), Some(&config)).unwrap_err();
        assert!(failure.contains("C11 _Atomic type is unsupported"), "{failure}");
        assert!(failure.contains("atomic.c:2:"), "{failure}");
        let failure = check_recovering(root.to_str().unwrap(), Some(&config), 4).unwrap_err();
        // Syntax is now retained; the checked semantic type is still rejected.
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
            "-DOTHER=42", "-UOLD", "-UOBSOLETE", "-undef",
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
    fn warning_options_are_parsed_without_leaking_to_cpp() {
        let opts = parse_options([
            "check", "-Wall", "-Wextra", "-Werror",
            "-Wno-unused-variable", "--cpp=none", "sample.c",
        ].into_iter().map(str::to_owned)).unwrap();
        assert!(opts.warnings.wall);
        assert!(opts.warnings.wextra);
        assert!(opts.warnings.warnings_as_errors);
        assert!(!opts.warnings.enabled("unused-variable"));
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
        assert_eq!(first_two.lines().count(), 2, "{first_two}");

        let all = check_recovering(filename, None, 4).unwrap_err();
        assert_eq!(all.lines().count(), 3, "{all}");
        assert!(all.contains("unknown"), "{all}");
        std::fs::remove_file(path).unwrap();
    }

}
