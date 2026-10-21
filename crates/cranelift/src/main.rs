//! Standalone native-object emission, deliberately independent of the normal check CLI.
use mini_c_cranelift::compile_source;
use std::{env, fs, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.len() != 3 || args[0] != "--emit-obj" {
        eprintln!("usage: mini-c-cranelift --emit-obj OUTPUT.o INPUT.c");
        return ExitCode::FAILURE;
    }
    let source = match fs::read_to_string(&args[2]) {
        Ok(s) => s,
        Err(e) => { eprintln!("{}: {e}", args[2]); return ExitCode::FAILURE; }
    };
    match compile_source(&source).and_then(|bytes| {
        fs::write(&args[1], bytes).map_err(|e| e.to_string())
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => { eprintln!("{}: {e}", args[2]); ExitCode::FAILURE }
    }
}
