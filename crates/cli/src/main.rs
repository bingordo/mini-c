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
    eprintln!(
        "usage: mini-c check [--cpp=none|--cpp=cc|--cpp=clang|--cpp=<path>] \
         [-I<dir>|-I <dir>] [-D<name>[=<value>]|-D <name>[=<value>]] \
         [-U<name>|-U <name>] [--cpp-arg=<arg>] \
         [--max-errors=<positive integer>] [--] <file.c> [file.c ...]"
    );
}

/// Parsed driver options. Argument collection is deliberately separate from
/// spawning a preprocessor so unit tests can check the exact argument vector.
#[derive(Debug)]
struct Options {
    cpp: Option<preprocess::Config>,
    max_errors: Option<usize>,
    paths: Vec<String>,
}

fn parse_options(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut args = args.into_iter();
    if args.next().as_deref() != Some("check") {
        return Err("expected 'check' subcommand".into());
    }
    let mut executable = Some(OsString::from("cc"));
    let mut cpp_args = vec![OsString::from("-E")];
    let mut paths = Vec::new();
    let mut max_errors = None;
    let mut end_options = false;

    while let Some(arg) = args.next() {
        if end_options {
            paths.push(arg);
            continue;
        }
        if arg == "--" {
            end_options = true;
        } else if let Some(value) = arg.strip_prefix("--cpp=") {
            if value.is_empty() {
                return Err("--cpp requires an executable name or 'none'".into());
            }
            executable = if value == "none" { None } else { Some(value.into()) };
        } else if let Some(value) = arg.strip_prefix("--cpp-arg=") {
            if value.is_empty() { return Err("--cpp-arg requires a value".into()); }
            cpp_args.push(value.into());
        } else if let Some(value) = arg.strip_prefix("--max-errors=") {
            match value.parse::<usize>() {
                Ok(n) if n > 0 => max_errors = Some(n),
                _ => return Err("--max-errors requires a positive integer".into()),
            }
        } else if matches!(arg.as_str(), "-I" | "-D" | "-U") {
            let value = args.next().ok_or_else(|| format!("{arg} requires a value"))?;
            if value.is_empty() { return Err(format!("{arg} requires a nonempty value")); }
            cpp_args.push(format!("{arg}{value}").into());
        } else if arg.starts_with("-I") || arg.starts_with("-D") || arg.starts_with("-U") {
            // Preserve values verbatim, including spaces, equals signs and
            // punctuation: Command::args does not invoke a shell.
            if arg.len() == 2 { return Err(format!("{arg} requires a value")); }
            cpp_args.push(arg.into());
        } else if arg.starts_with('-') {
            return Err(format!("unknown option: {arg}"));
        } else {
            paths.push(arg);
        }
    }
    if paths.is_empty() { return Err("no input files provided".into()); }
    if executable.is_none() && cpp_args.len() != 1 {
        return Err("preprocessor options -I/-D/-U/--cpp-arg cannot be used with --cpp=none".into());
    }
    let cpp = executable.map(|executable| preprocess::Config { executable, args: cpp_args });
    Ok(Options { cpp, max_errors, paths })
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
            Some(limit) => check_recovering(&path, options.cpp.as_ref(), limit),
            None => check(&path, options.cpp.as_ref()),
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
