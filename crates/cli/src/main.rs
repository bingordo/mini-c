use mini_c_syntax::parser::Parser;
use mini_c_syntax::source::SourceFile;
use mini_c_syntax::stream::ItemSource;
use std::env;
use std::fs;
use std::process::ExitCode;

fn check(path: &str) -> Result<(), String> {
    let source = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let file = SourceFile::new(path, &source);
    let mut parser = Parser::new(&source);
    loop {
        match parser.next_item() {
            Ok(Some(_)) => {},
            Ok(None) => return Ok(()),
            Err(error) => {
                let (line, col) = file.location(error.span.lo);
                return Err(format!("{path}:{line}:{col}: error: {}", error.message));
            }
        }
    }
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    if args.next().as_deref() != Some("check") {
        eprintln!("usage: mini-c check <file.c> [file.c ...]");
        return ExitCode::FAILURE;
    }
    let paths: Vec<_> = args.collect();
    if paths.is_empty() {
        eprintln!("usage: mini-c check <file.c> [file.c ...]");
        return ExitCode::FAILURE;
    }
    let mut failed = false;
    for path in paths {
        if let Err(error) = check(&path) {
            eprintln!("{error}");
            failed = true;
        }
    }
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}
