use mini_c_preprocess::{self as preprocess, source_map::SourceMap};
use mini_c_sema::{Checker, Target};
use mini_c_syntax::parser::Parser;
use mini_c_syntax::source::SourceFile;
use mini_c_syntax::stream::ItemSource;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

fn check(path: &str, cpp: Option<&preprocess::Config>) -> Result<(), String> {
    let source = match cpp {
        Some(config) => preprocess::file(Path::new(path), config)
            .map_err(|e| format!("{path}: {e}"))?,
        None => fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?,
    };
    let file = SourceFile::new(path, &source);
    let map = cpp.map(|_| SourceMap::from_preprocessed(&source, path));
    let mut parser = Parser::new(&source);
    let mut checker = Checker::new(Target::default()).expect("valid default target");
    loop {
        // The checker only retains symbol/type state, not completed function ASTs.
        let checked = parser.next_item().and_then(|item| {
            item.map(|item| checker.check_item(item)).transpose()
        });
        match checked {
            Ok(Some(_)) => {},
            Ok(None) => return Ok(()),
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
    let source = match cpp {
        Some(config) => preprocess::file(Path::new(path), config)
            .map_err(|e| format!("{path}: {e}"))?,
        None => fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?,
    };
    let file = SourceFile::new(path, &source);
    let map = cpp.map(|_| SourceMap::from_preprocessed(&source, path));
    let mut parser = Parser::new(&source);
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
    eprintln!("usage: mini-c check [--cpp=none|--cpp=cc|--cpp=clang|--cpp=<path>] [--cpp-arg=<arg>] [--max-errors=<positive integer>] <file.c> [file.c ...]");
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    if args.next().as_deref() != Some("check") {
        usage();
        return ExitCode::FAILURE;
    }
    let mut cpp: Option<preprocess::Config> = None;
    let mut paths = Vec::new();
    let mut max_errors = None;
    for arg in args {
        if let Some(executable) = arg.strip_prefix("--cpp=") {
            cpp = if executable == "none" {
                None
            } else {
                Some(preprocess::Config {
                    executable: OsString::from(executable),
                    args: vec![OsString::from("-E")],
                })
            };
        } else if let Some(value) = arg.strip_prefix("--cpp-arg=") {
            match cpp.as_mut() {
                Some(config) => config.args.push(OsString::from(value)),
                None => {
                    eprintln!("--cpp-arg requires --cpp=<executable> first");
                    return ExitCode::FAILURE;
                }
            }
        } else if let Some(value) = arg.strip_prefix("--max-errors=") {
            match value.parse::<usize>() {
                Ok(n) if n > 0 => max_errors = Some(n),
                _ => {
                    eprintln!("--max-errors requires a positive integer");
                    return ExitCode::FAILURE;
                }
            }
        } else if arg.starts_with('-') {
            eprintln!("unknown option: {arg}");
            usage();
            return ExitCode::FAILURE;
        } else {
            paths.push(arg);
        }
    }
    if paths.is_empty() {
        usage();
        return ExitCode::FAILURE;
    }
    let mut failed = false;
    for path in paths {
        let result = match max_errors {
            Some(limit) => check_recovering(&path, cpp.as_ref(), limit),
            None => check(&path, cpp.as_ref()),
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
