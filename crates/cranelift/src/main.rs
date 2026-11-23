//! Standalone native ELF object emitter. Preprocessing is a driver operation,
//! never a parser side effect. The internal CPP is the default; --cpp=cc
//! selects the external compatibility fallback.
use mini_c_cranelift::{compile_source_diagnostic, CompileError};
use mini_c_preprocess::source_map::SourceMap;
use mini_c_syntax::source::SourceFile;
use mini_c_syntax::diag::{Diagnostic, DiagnosticSeverity, ReportedDiagnostic, WarningPolicy};
use mini_c_syntax::parser::Parser;
use mini_c_syntax::stream::ItemSource;
use mini_c_sema::{Checker, Target};
use mini_c_preprocess::{self as preprocess, driver_options::CppOptions};
use std::{env, fs, path::Path, process::ExitCode};

struct Options {
    output: String,
    source: String,
    cpp: Option<preprocess::Config>,
    warnings: WarningPolicy,
}

fn parse_options(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut args = args.into_iter();
    let mut cpp = CppOptions::default();
    let mut warnings = WarningPolicy::default();
    let mut output = None;
    let mut inputs = Vec::new();
    let mut end_options = false;
    while let Some(arg) = args.next() {
        if end_options {
            inputs.push(arg);
        } else if arg == "--" {
            end_options = true;
        } else if arg == "--emit-obj" {
            let name = args.next().ok_or("--emit-obj requires an output filename")?;
            if name.is_empty() || output.replace(name).is_some() {
                return Err("--emit-obj requires exactly one output filename".into());
            }
        } else if arg.starts_with("-W") {
            warnings.apply_flag(&arg)?;
        } else if cpp.consume(&arg, &mut args)? {
            // Shared -I, -D, -U and --cpp options with the semantic checker.
        } else if arg.starts_with('-') {
            return Err(format!("unknown option: {arg}"));
        } else {
            inputs.push(arg);
        }
    }
    if inputs.len() != 1 {
        return Err("expected exactly one input C source file".into());
    }
    let output = output.ok_or("missing --emit-obj OUTPUT.o")?;
    Ok(Options {
        output,
        source: inputs.remove(0),
        cpp: cpp.finish()?,
        warnings,
    })
}

fn format_compiler_error(
    filename: &str, source: &str, preprocessed: bool, error: CompileError,
) -> String {
    match error {
        CompileError::Diagnostic(d) => {
            let location = if preprocessed {
                let map = SourceMap::from_preprocessed(source, filename);
                let origin = map.origin(d.span.lo);
                format!("{}:{}", origin.file, origin.line)
            } else {
                let file = SourceFile::new(filename, source);
                let (line, col) = file.location(d.span.lo);
                format!("{filename}:{line}:{col}")
            };
            format!("{location}: error[{}]: {}", d.code.as_str(), d.message)
        }
        CompileError::Backend(message) => format!("{filename}: error: {message}"),
    }
}

/// Run optional semantic warning checks without changing Cranelift's fast
/// no-warning compilation path. Rechecking source is intentional for this
/// transition; merging the pass into object compilation can remove the
/// additional parse later.
fn collect_semantic_warnings(
    source: &str, policy: &WarningPolicy
) -> Result<Vec<ReportedDiagnostic>, Diagnostic> {
    if !policy.enabled("shadow")
        && !policy.enabled("unused-variable")
        && !policy.enabled("unused-parameter")
    {
        return Ok(Vec::new());
    }
    let mut parser = Parser::new_with_spans(source);
    let mut checker = Checker::new(Target::default())?;
    checker.set_warning_policy(policy.clone());
    while let Some(item) = parser.next_item()? {
        checker.check_item(item)?;
    }
    Ok(checker.take_warnings())
}

/// Render optional source-located warnings and report whether -Werror
/// disallows writing the object file.
fn emit_semantic_warnings(
    filename: &str, source: &str, preprocessed: bool, policy: &WarningPolicy
) -> Result<bool, Diagnostic> {
    let reports = collect_semantic_warnings(source, policy)?;
    let source_file = SourceFile::new(filename, source);
    let map = preprocessed.then(|| SourceMap::from_preprocessed(source, filename));
    let mut failed = false;
    for report in reports {
        let Some(severity) = policy.resolve(&report) else { continue; };
        let position = if let Some(ref map) = map {
            let origin = map.origin(report.diagnostic.span.lo);
            format!("{}:{}", origin.file, origin.line)
        } else {
            let (line, col) = source_file.location(report.diagnostic.span.lo);
            format!("{filename}:{line}:{col}")
        };
        let label = match severity {
            DiagnosticSeverity::Warning => "warning",
            DiagnosticSeverity::Error => { failed = true; "error" },
            DiagnosticSeverity::Note => "note",
        };
        eprintln!("{position}: {label}: {} [-W{}]",
            report.diagnostic.message, report.warning_group.unwrap_or("unknown"));
    }
    Ok(failed)
}

fn main() -> ExitCode {
    let opts = match parse_options(env::args().skip(1)) {
        Ok(opts) => opts,
        Err(e) => {
            eprintln!("{e}");
            eprintln!(
                "usage: mini-c-cranelift --emit-obj OUTPUT.o \
                 [--cpp=internal|--cpp=cc|--cpp=none|--cpp=clang|--cpp=<path>] \
                 [-I dir] [-DNAME=value] [-UNAME] [--cpp-arg=<arg>] [-Wall|-Wextra|-Werror|-Wno-*] INPUT.c"
            );
            return ExitCode::FAILURE;
        }
    };
    let source = match &opts.cpp {
        Some(config) => preprocess::file(Path::new(&opts.source), config)
            .map_err(|e| e.to_string()),
        None => fs::read_to_string(&opts.source).map_err(|e| e.to_string()),
    };
    let source = match source {
        Ok(source) => source,
        Err(e) => {
            eprintln!("{}: {e}", opts.source);
            return ExitCode::FAILURE;
        }
    };
    match compile_source_diagnostic(&source) {
        Ok(bytes) => {
            match emit_semantic_warnings(
                &opts.source, &source, opts.cpp.is_some(), &opts.warnings,
            ) {
                Ok(true) => ExitCode::FAILURE,
                Ok(false) => match fs::write(&opts.output, bytes) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => { eprintln!("{}: {e}", opts.output); ExitCode::FAILURE }
                },
                Err(error) => {
                    eprintln!("{}: error[{}]: {}", opts.source,
                        error.code.as_str(), error.message);
                    ExitCode::FAILURE
                }
            }
        },
        Err(error) => {
            eprintln!("{}", format_compiler_error(
                &opts.source, &source, opts.cpp.is_some(), error,
            ));
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
