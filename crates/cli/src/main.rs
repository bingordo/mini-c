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

fn usage() {
    eprintln!("usage: mini-c check [--cpp=none|--cpp=cc|--cpp=clang|--cpp=<path>] [--cpp-arg=<arg>] <file.c> [file.c ...]");
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    if args.next().as_deref() != Some("check") {
        usage();
        return ExitCode::FAILURE;
    }
    let mut cpp: Option<preprocess::Config> = None;
    let mut paths = Vec::new();
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
        if let Err(error) = check(&path, cpp.as_ref()) {
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
}
