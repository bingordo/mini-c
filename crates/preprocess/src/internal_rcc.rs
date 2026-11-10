//! Experimental, independently hosted C99 preprocessor adapter.
//!
//! This is NOT a dependency of Mini-C's lexer/parser/sema. It consumes the
//! expanded preprocessing tokens of rcc_preprocess and converts them back to
//! token-separated C text for Mini-C's unchanged, standalone parser.
//! The adapter is feature gated pending GCC/Clang differential validation.
use std::{collections::BTreeMap, io::Read, path::{Path, PathBuf}, sync::Arc};

use rcc_errors::Level;
use rcc_lexer::PpTokenKind;
use rcc_session::Session;

/// Maximum generated text in an experimental translation unit.
const MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
/// Bound the root source before it is added to the session source map.
/// Included headers use the same total-size budget inside rcc_preprocess.
const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;

/// C99 preprocessing with a source-location-preserving text adapter.
///
/// The caller passes the same OS arguments used by the external frontend:
/// -E, -I<path>, -D<name>[=<value>] and -U<name>. Unknown flags fail closed.
/// This function never launches an external C preprocessor or shell.
pub fn file(path: &Path, args: &[std::ffi::OsString]) -> Result<String, String> {
    let mut opts = rcc_session::Options::default();
    // GCC applies -D and -U in command-line order. The pinned rcc Options
    // instead processes every -D before every -U, so normalize each name to
    // its final operation before passing it into the independent engine.
    // The outer Option distinguishes undef from a valueless -DNAME.
    let mut macro_actions: BTreeMap<String, Option<Option<String>>> = BTreeMap::new();
    for arg in args {
        let arg = arg.to_str().ok_or("internal preprocessor needs UTF-8 option values")?;
        if arg == "-E" {
            continue;
        }
        if let Some(dir) = arg.strip_prefix("-I") {
            if dir.is_empty() { return Err("internal -I requires a directory".into()); }
            opts.include_paths.push(PathBuf::from(dir));
        } else if let Some(definition) = arg.strip_prefix("-D") {
            if definition.is_empty() { return Err("internal -D requires a macro name".into()); }
            let (name, value) = match definition.split_once('=') {
                Some((name, value)) => (name, Some(value.to_owned())),
                None => (definition, None),
            };
            if !c_identifier(name) {
                return Err(format!("invalid internal macro name: {name}"));
            }
            macro_actions.insert(name.to_owned(), Some(value));
        } else if let Some(name) = arg.strip_prefix("-U") {
            if !c_identifier(name) {
                return Err(format!("invalid internal undef name: {name}"));
            }
            macro_actions.insert(name.to_owned(), None);
        } else {
            return Err(format!(
                "unsupported internal preprocessor option {arg:?}; use --cpp=cc for this switch"
            ));
        }
    }
    for (name, action) in macro_actions {
        match action {
            Some(value) => opts.cli_defines.push((name, value)),
            None => opts.cli_undefines.push(name),
        }
    }

    let (mut session, captured) = Session::for_test();
    session.opts = opts;
    // Do not call Session::load_source_file here: its unbounded disk read
    // happens before the internal preprocessor's budget can be checked.
    let mut root_bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| format!("cannot read C source {}: {e}", path.display()))?
        .take((MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut root_bytes)
        .map_err(|e| format!("cannot read C source {}: {e}", path.display()))?;
    if root_bytes.len() > MAX_SOURCE_BYTES {
        return Err(format!(
            "preprocessing source byte limit exceeded ({} bytes) for {}",
            MAX_SOURCE_BYTES, path.display()
        ));
    }
    let source = String::from_utf8(root_bytes)
        .map_err(|e| format!("cannot decode C source {}: {e}", path.display()))?;
    let root = session.source_map.write()
        .map_err(|_| "internal source map lock poisoned")?
        .add_file(path.to_path_buf(), Arc::from(source));
    let mut preprocessor = rcc_preprocess::Preprocessor::new(&mut session);
    let tokens = preprocessor.run(root);

    // The narrow rcc_preprocess patch preserves the invocation site of
    // replacement-list tokens separately from their spelling spans, so
    // __LINE__ and __FILE__ inside nested macro calls use the caller.
    // Keep rcc's logical #line map alive while reconstructing the Mini-C
    // source stream. Physical token spans are not sufficient here: a source
    // file may have been renamed or renumbered by an active directive.
    let map = preprocessor.session.source_map.read()
        .map_err(|e| format!("internal source map lock poisoned: {e}"))?;
    let diagnostics = captured.diagnostics();
    let errors: Vec<_> = diagnostics.into_iter().filter(|d|
        matches!(d.level, Level::Error | Level::Bug)
    ).map(|d| {
        let location = d.labels.iter().find(|label| label.primary)
            .or_else(|| d.labels.first())
            .and_then(|label| {
                if label.span.file.0 == u32::MAX { return None; }
                let position = map.lookup_line_col(label.span.file, label.span.lo);
                let logical_file = preprocessor.line_overrides.effective_file(
                    &map, label.span.file, label.span.lo);
                let line = preprocessor.line_overrides.effective_line(
                    &map, label.span.file, label.span.lo);
                Some(format!("{}:{}:{}: ",
                    map.file(logical_file).name.display(), line, position.col))
            }).unwrap_or_default();
        match d.code {
            Some(code) => format!("{location}error[{code}]: {}", d.message),
            None => format!("{location}error: {}", d.message),
        }
    }).collect();
    if !errors.is_empty() { return Err(errors.join("\n")); }

    let mut output = String::new();
    // A directive marks the NEXT physical source line. Print one only when
    // a non-synthetic origin changes, then emit token-separated spelling.
    // Synthetic macro replacement buffers do not correspond to on-disk
    // user locations; retain the previous real-file origin for them.
    let mut last_origin: Option<(u32, u32)> = None;
    for token in tokens {
        match token.kind {
            PpTokenKind::Whitespace | PpTokenKind::Eof => continue,
            PpTokenKind::Newline => {
                if !output.ends_with('\n') { output.push('\n'); }
                continue;
            }
            _ => {}
        }
        let source = map.file(token.span.file);
        let lo = token.span.lo.0 as usize;
        let hi = token.span.hi.0 as usize;
        let spelling = source.src.get(lo..hi)
            .ok_or("preprocessor returned an invalid token span")?;
        let original_file = !source.name.to_string_lossy().starts_with('<');
        if original_file {
            let logical_file = preprocessor.line_overrides.effective_file(
                &map, token.span.file, token.span.lo);
            let logical_line = preprocessor.line_overrides.effective_line(
                &map, token.span.file, token.span.lo);
            let location = (logical_file.0, logical_line);
            if last_origin != Some(location) {
                if !output.is_empty() && !output.ends_with('\n') { output.push('\n'); }
                let filename = map.file(logical_file).name.to_string_lossy()
                    .replace('\\', "\\\\").replace('"', "\\\"");
                output.push_str(&format!("# {} \"{}\"\n", logical_line, filename));
                last_origin = Some(location);
            }
        }
        // A space between every pair of tokens is conservative and avoids
        // re-lexing "a + + b" as "a ++ b" or merging pasted pp-numbers.
        if !output.is_empty() && !output.ends_with('\n') {
            output.push(' ');
        }
        output.push_str(spelling);
        if output.len() > MAX_OUTPUT_BYTES {
            return Err("internal preprocessor output exceeded 16 MiB limit".into());
        }
    }
    output.push('\n');
    Ok(output)
}

fn c_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic()) &&
        chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::OsString, time::{SystemTime, UNIX_EPOCH}};

    #[test]
    fn rejects_oversize_disk_root_without_unbounded_read() {
        let dir = std::env::temp_dir().join(format!(
            "mini-c-rcc-root-budget-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("too-large.c");
        // Sparse file: no large allocation required in the test runner.
        let file = std::fs::File::create(&source).unwrap();
        file.set_len((MAX_SOURCE_BYTES + 1) as u64).unwrap();
        let error = super::file(&source, &[OsString::from("-E")]).unwrap_err();
        assert!(error.contains("source byte limit exceeded"), "{error}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cli_macro_names_and_unsupported_options_are_checked() {
        assert!(c_identifier("HELLO_123"));
        assert!(!c_identifier("123HELLO"));
        assert!(!c_identifier("bad-name"));
        assert!(!c_identifier(""));
        let args = [OsString::from("-E"), OsString::from("-include"),
            OsString::from("header.h")];
        assert!(file(Path::new("/unused/source.c"), &args).unwrap_err()
            .contains("unsupported internal preprocessor option"));
    }

    #[test]
    fn object_function_macro_expansion_and_conditions() {
        let dir = std::env::temp_dir().join(format!(
            "mini-c-rcc-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("main.c");
        std::fs::write(&src, "#define INC(x) ((x)+1)\n\
            #define VAL 17\n\
            #if VAL==17\n\
            int result=INC(VAL);\n\
            #else\n\
            #error bad conditional\n\
            #endif\n").unwrap();
        let result = file(&src, &[OsString::from("-E")]);
        let _ = std::fs::remove_dir_all(&dir);
        let output = result.unwrap();
        let tokens = mini_c_syntax::lexer::scan(&output).unwrap();
        let values: Vec<_> = tokens.iter().map(|t| t.text).collect();
        assert_eq!(values,
            ["int","result","=","(","(","17",")","+","1",")",";"]);
    }

    #[test]
    fn rejects_invalid_directives_without_returning_partial_source() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let dir = std::env::temp_dir().join(format!(
            "mini-c-rcc-negative-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("invalid.c");
        std::fs::write(&src, "#if 1\n#error deliberately-invalid\n#endif\nint x;\n").unwrap();
        let result = file(&src, &[OsString::from("-E")]);
        let _ = std::fs::remove_dir_all(&dir);
        let error = result.unwrap_err();
        assert!(error.contains("deliberately-invalid"), "{error}");
    }

    #[test]
    fn line_directives_preserve_virtual_filenames_and_logical_lines() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let dir = std::env::temp_dir().join(format!(
            "mini-c-rcc-line-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("line.c");
        std::fs::write(&src,
            "#line 45 \"generated name.c\"\nint first;\n#line 100\nint second;\n",
        ).unwrap();
        let result = file(&src, &[OsString::from("-E")]);
        let _ = std::fs::remove_dir_all(&dir);
        let output = result.unwrap();
        let mapping = crate::source_map::SourceMap::from_preprocessed(&output,
            src.to_str().unwrap());
        assert_eq!(mapping.origin(output.find("first").unwrap()),
            &crate::source_map::Origin {
                file: "generated name.c".into(), line: 45,
            });
        assert_eq!(mapping.origin(output.find("second").unwrap()),
            &crate::source_map::Origin {
                file: "generated name.c".into(), line: 100,
            });
    }

    #[test]
    fn preprocessor_errors_respect_line_directive_overrides() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let dir = std::env::temp_dir().join(format!(
            "mini-c-rcc-error-line-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("unit.c");
        std::fs::write(&src,
            "#line 123 \"generated.c\"\n#error expected_diagnostic\n",
        ).unwrap();
        let result = file(&src, &[OsString::from("-E")]);
        let _ = std::fs::remove_dir_all(dir);
        let error = result.unwrap_err();
        assert!(error.contains("generated.c:123:"), "{error}");
        assert!(error.contains("expected_diagnostic"), "{error}");
    }

    #[test]
    fn invalid_directive_in_nested_header_reports_original_file_and_line() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let dir = std::env::temp_dir().join(format!(
            "mini-c-rcc-location-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("unit.c");
        let header = dir.join("broken.h");
        std::fs::write(&src, "#include \"broken.h\"\n").unwrap();
        std::fs::write(&header, "int fine;\n#error include_failure\n").unwrap();
        let result = file(&src, &[OsString::from("-E")]);
        let _ = std::fs::remove_dir_all(dir);
        let failure = result.unwrap_err();
        assert!(failure.contains("broken.h:2:"), "{failure}");
        assert!(failure.contains("include_failure"), "{failure}");
    }

    #[test]
    fn header_source_locations_and_command_line_flags() {
        let dir = std::env::temp_dir().join(format!(
            "mini-c-rcc-header-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let inc = dir.join("include with spaces");
        std::fs::create_dir_all(&inc).unwrap();
        std::fs::write(inc.join("values.h"),
            "#ifndef VALUES_H\n#define VALUES_H\n#define VALUE 17\nint from_header;\n#endif\n").unwrap();
        let src = dir.join("root.c");
        std::fs::write(&src, "#include <values.h>\nint val=VALUE;\n").unwrap();
        let args = [
            OsString::from("-E"),
            OsString::from(format!("-I{}", inc.display())),
            OsString::from("-DOTHER=23"),
            OsString::from("-UOLD"),
        ];
        let result = file(&src, &args);
        let _ = std::fs::remove_dir_all(&dir);
        let output = result.unwrap();
        assert!(output.contains("from_header"), "{output}");
        assert!(output.contains("\""), "{output}");
        let map = crate::source_map::SourceMap::from_preprocessed(
            &output, src.to_str().unwrap(),
        );
        let origin = map.origin(output.find("from_header").unwrap());
        assert!(origin.file.ends_with("values.h"), "{origin:?}");
        assert_eq!(origin.line, 4);
        let tokens = mini_c_syntax::lexer::scan(&output).unwrap();
        assert!(tokens.iter().any(|tok| tok.text == "17"));
    }
}
