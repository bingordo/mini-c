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

#[cfg(test)]
fn check(path: &str, cpp: Option<&preprocess::Config>) -> Result<(), String> {
    check_with_policy(path, cpp, &WarningPolicy::default())
}

/// Render original source only. Preprocessor line markers do not establish
/// original columns, so never use expanded-text offsets for their carets.
fn render_diagnostic(
    file: &SourceFile<'_>, map: Option<&SourceMap>, origins: &preprocess::token_origins::TokenOrigins,
    diagnostic: &mini_c_syntax::diag::Diagnostic, level: &str, suffix: &str,
) -> String {
    use mini_c_syntax::diagnostic_excerpt::write_excerpt;
    let (name, line, column, end_column, excerpt) = if let Some((origin, (excerpt, column, end))) = origins.lookup(diagnostic.span.lo)
        .and_then(|origin| origin.excerpt(diagnostic.span.lo, diagnostic.span.hi).map(|excerpt| (origin, excerpt))) {
        (origin.file.as_ref(), origin.line, Some(column), Some(end), Some(excerpt))
    } else if let Some(map) = map {
        let origin = map.origin(diagnostic.span.lo);
        // A logical #line filename may not correspond to a physical file.
        // Do not read arbitrary paths from preprocessor markers.
        (origin.file.as_str(), origin.line, None, None, None)
    } else {
        let (line, column) = file.location(diagnostic.span.lo);
        let (end_line, end_column) = file.location(diagnostic.span.hi);
        (file.name, line, Some(column),
            (line == end_line).then_some(end_column),
            file.line_text(line))
    };
    let location = match column {
        Some(column) => format!("{name}:{line}:{column}"),
        None => format!("{name}:{line}"),
    };
    let mut output = format!("{location}: {level}: {}{suffix}", diagnostic.message);
    if let Some(excerpt) = excerpt {
        output.push('\n');
        write_excerpt(&mut output, excerpt, line, column, end_column)
            .expect("writing to String cannot fail");
        output.pop(); // The caller supplies the final newline.
    }
    let mut frame = origins.lookup(diagnostic.span.lo).and_then(|origin| origin.expansion.as_deref());
    while let Some(expansion) = frame {
        use std::fmt::Write;
        let site = &expansion.invocation;
        if let Some((excerpt, column, end)) = site.excerpt() {
            write!(output, "\n{}:{}:{}: note: in expansion of macro '{}'", site.file, site.line, column, expansion.name).unwrap();
            output.push('\n');
            write_excerpt(&mut output, excerpt, site.line, Some(column), Some(end)).unwrap();
            output.pop();
        } else {
            write!(output, "\n{}:{}: note: in expansion of macro '{}'", site.file, site.line, expansion.name).unwrap();
        }
        frame = expansion.parent.as_deref();
    }
    output
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
    let (source, origins) = match cpp {
        Some(config) => preprocess::file_with_origins(Path::new(path), config)
            .map_err(|e| format!("{path}: {e}"))?,
        None => (fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?, Default::default()),
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
                        let level = match severity {
                            DiagnosticSeverity::Error => {
                                warning_failed = true;
                                "error"
                            }
                            DiagnosticSeverity::Warning => "warning",
                            DiagnosticSeverity::Note => "note",
                        };
                        eprintln!("{}", render_diagnostic(&file, map.as_ref(), &origins, &warning.diagnostic, level,
                            &format!(" [-W{}]", warning.warning_group.unwrap_or("unknown"))));
                    }
                }
            },
            Ok(None) => return if let Err(error) = checker.finish_translation_unit() {
                Err(render_diagnostic(&file, map.as_ref(), &origins, &error, "error", ""))
            } else if warning_failed {
                Err(format!("{path}: warnings treated as errors"))
            } else { Ok(()) },
            Err(error) => {
                return Err(render_diagnostic(&file, map.as_ref(), &origins, &error, "error", ""));
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
    let (source, origins) = match cpp {
        Some(config) => preprocess::file_with_origins(Path::new(path), config)
            .map_err(|e| format!("{path}: {e}"))?,
        None => (fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?, Default::default()),
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
        render_diagnostic(&file, map.as_ref(), &origins, &error, &format!("error[{}]", error.code.as_str()), "")
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
#[path = "tests.rs"]
mod tests;
