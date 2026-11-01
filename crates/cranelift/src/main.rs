//! Standalone native ELF object emitter. The external C preprocessor is
//! explicitly configured by the driver, not invoked from the parser.
use mini_c_cranelift::{compile_source_diagnostic, CompileError};
use mini_c_preprocess::source_map::SourceMap;
use mini_c_syntax::source::SourceFile;
use mini_c_preprocess::{self as preprocess, driver_options::CppOptions};
use std::{env, fs, path::Path, process::ExitCode};

struct Options {
    output: String,
    source: String,
    cpp: Option<preprocess::Config>,
}

fn parse_options(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut args = args.into_iter();
    let mut cpp = CppOptions::default();
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

fn main() -> ExitCode {
    let opts = match parse_options(env::args().skip(1)) {
        Ok(opts) => opts,
        Err(e) => {
            eprintln!("{e}");
            eprintln!(
                "usage: mini-c-cranelift --emit-obj OUTPUT.o \
                 [--cpp=none|--cpp=cc|--cpp=clang|--cpp=<path>] \
                 [-I dir] [-DNAME=value] [-UNAME] [--cpp-arg=<arg>] INPUT.c"
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
        Ok(bytes) => match fs::write(&opts.output, bytes) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => { eprintln!("{}: {e}", opts.output); ExitCode::FAILURE }
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
mod tests {
    use super::*;

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
}
