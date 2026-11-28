//! Experimental, independently hosted C99 preprocessor adapter.
//!
//! This is NOT a dependency of Mini-C's lexer/parser/sema. It consumes the
//! expanded preprocessing tokens of rcc_preprocess and converts them back to
//! token-separated C text for Mini-C's unchanged, standalone parser.
//! The adapter is feature gated pending GCC/Clang differential validation.
use std::{collections::BTreeMap, io::Read, path::{Path, PathBuf}, sync::Arc};

use rcc_errors::Level;
use rcc_lexer::PpTokenKind;
use rcc_session::{Arch, DataModel, Endianness, Environment, Os, Session, TargetTriple};

// Both adapters snapshot traces while the source map is still live.
macro_rules! capture_expansion {
    ($context:expr, $map:expr, $index:expr, $cache:expr) => {{
        let context = $context;
        let map = $map;
        let index = $index;
        let frame_cache = &mut $cache;
    let mut frame = context.output_traces().get(index).and_then(Option::as_ref);
    let mut pending = Vec::new();
    let mut parent = None;
    while let Some(current) = frame {
        let key = Arc::as_ptr(current);
        if let Some(cached) = frame_cache.get(&key) {
            parent = Some(Arc::clone(cached));
            break;
        }
        pending.push((key, current));
        frame = current.parent.as_ref();
    }
    let site = |frame: &rcc_preprocess::expand::ExpansionFrame, definition: bool| {
        let span = if definition { frame.definition } else { frame.invocation };
        if span.file.0 == u32::MAX { return None; }
        let physical = map.file(span.file);
        let logical = context.line_overrides.effective_file(&map, span.file, span.lo);
        Some(SourceSite {
            file: Arc::from(map.file(logical).name.to_string_lossy().as_ref()),
            line: context.line_overrides.effective_line(&map, span.file, span.lo) as usize,
            source: physical.src.clone(),
            start: span.lo.0 as usize,
            end: span.hi.0 as usize,
        })
    };
    for (key, current) in pending.into_iter().rev() {
        if let Some(invocation) = site(current, false) {
            let converted = Arc::new(MacroOrigin {
    name: Arc::from(context.session.interner.get(current.name)),
    invocation,
    definition: site(current, true),
    parent,
            });
            frame_cache.insert(key, Arc::clone(&converted));
            parent = Some(converted);
        }
    }
    parent

    }};
}

#[path = "internal_rcc_stream.rs"]
pub mod stream;

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
fn options_from_args(args: &[std::ffi::OsString]) -> Result<rcc_session::Options, String> {
    let mut opts = rcc_session::Options::default();
    // Hosted-mode include discovery is done by the adapter, not by
    // rcc_session::Options::default(): the latter intentionally starts
    // with no system include roots. Keep user -isystem directories ahead
    // of the discovered target/host defaults.
    let mut no_stdinc = false;
    let mut wasm32_target = false;
    // GCC applies -D and -U in command-line order. The pinned rcc Options
    // instead processes every -D before every -U, so normalize each name to
    // its final operation before passing it into the independent engine.
    // The outer Option distinguishes undef from a valueless -DNAME.
    let mut macro_actions: BTreeMap<String, Option<Option<String>>> = BTreeMap::new();
    let mut args = args.iter();
    while let Some(raw) = args.next() {
        let raw = raw.to_str().ok_or("internal preprocessor needs UTF-8 option values")?;
        // GCC accepts both attached and separate operands for these options.
        // Normalize once here: batch and streaming CPP share this parser.
        // Never consume a subsequent flag as a missing operand.
        let normalized;
        let arg = if matches!(raw, "-I" | "-D" | "-U" | "-isystem" | "--sysroot") {
            let value = args.next()
                .ok_or_else(|| format!("internal {raw} requires an operand"))?
                .to_str().ok_or("internal preprocessor needs UTF-8 option values")?;
            if value.is_empty() || value.starts_with('-') {
                return Err(format!("internal {raw} requires a nonempty operand"));
            }
            normalized = if raw == "--sysroot" {
                format!("--sysroot={value}")
            } else {
                format!("{raw}{value}")
            };
            normalized.as_str()
        } else {
            raw
        };
        if arg == "-E" {
            continue;
        }
        if arg == "--cpp-target=wasm32" {
            // The reused rcc frontend currently has no Wasm architecture
            // variant. Its target layout and OS are public data: configure
            // the exact ILP32 C sizes here and suppress the temporary i386
            // predefine after installation. This target is preprocessing-only;
            // it never authorizes rcc native codegen for wasm32.
            let target=&mut opts.target;
            target.triple=TargetTriple::new("wasm32-unknown-unknown");
            target.arch=Arch::I386;
            target.os=Os::None;
            target.env=Environment::Unknown;
            target.data_model=DataModel::Ilp32;
            target.pointer_width=32;
            target.endianness=Endianness::Little;
            target.layouts.pointer.size=4;
            target.layouts.pointer.align=4;
            target.layouts.long.size=4;
            target.layouts.long.align=4;
            target.layouts.builtin_va_list.size=4;
            target.layouts.builtin_va_list.align=4;
            target.llvm_data_layout="e-m:e-p:32:32-i64:64-n32:64-S128";
            wasm32_target=true;
            continue;
        }
        if arg == "-nostdinc" {
            no_stdinc = true;
            continue;
        }
        if let Some(dir) = arg.strip_prefix("--sysroot=") {
            if dir.is_empty() {
                return Err("internal --sysroot requires a directory".into());
            }
            opts.sysroot = Some(PathBuf::from(dir));
            continue;
        }
        if let Some(dir) = arg.strip_prefix("-isystem") {
            if dir.is_empty() {
                return Err("internal -isystem requires a directory".into());
            }
            opts.system_include_paths.push(PathBuf::from(dir));
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
    if !no_stdinc {
        opts.system_include_paths.extend(
            rcc_preprocess::include::discover_system_include_paths(
                &opts.target, opts.sysroot.as_deref(),
            ),
        );
    }
    for (name, action) in macro_actions {
        match action {
            Some(value) => opts.cli_defines.push((name, value)),
            None => opts.cli_undefines.push(name),
        }
    }

    if wasm32_target {
        // rcc has not added Arch::Wasm32 yet: remove the temporary i386
        // target spelling so source sees no contradictory host predefines.
        opts.cli_undefines.push("__i386__".into());
    }
    Ok(opts)
}

use crate::token_origins::{MacroOrigin, SourceSite, TokenOrigin, TokenOrigins};

pub fn file(path: &Path, args: &[std::ffi::OsString]) -> Result<String, String> {
    run_file(path, args, false).map(|(text, _)| text)
}

pub fn file_with_origins(path: &Path, args: &[std::ffi::OsString]) -> Result<(String, TokenOrigins), String> {
    run_file(path, args, true)
}

fn run_file(path: &Path, args: &[std::ffi::OsString], track: bool) -> Result<(String, TokenOrigins), String> {
    let opts = options_from_args(args)?;
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
    if track { preprocessor.enable_expansion_traces(); }
    // Consume each expanded token chunk immediately, instead of retaining
    // a complete pp-token Vec while building a second full output String.
    // The callback receives current #line metadata from the preprocessor.
    // Nothing is committed: a late error discards all buffered text.
    let mut output = String::new();
    let mut origins = TokenOrigins::default();
    let mut names = std::collections::HashMap::<u32, Arc<str>>::new();
    let mut last_origin: Option<(u32, u32)> = None;
    let mut adapter_error: Option<String> = None;
    preprocessor.run_with_context_sink(root, &mut |context, chunk| {
        if adapter_error.is_some() {
            return;
        }
        let map = match context.session.source_map.read() {
            Ok(map) => map,
            Err(e) => {
                adapter_error = Some(format!("internal source map lock poisoned: {e}"));
                return;
            }
        };
        // Cache conversions only for this chunk, so raw Arc addresses cannot
        // be reused after the preprocessor releases its frames.
        let mut frame_cache = std::collections::HashMap::new();
        for (index, token) in chunk.into_iter().enumerate() {
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
            let Some(spelling) = source.src.get(lo..hi) else {
                adapter_error = Some("preprocessor returned an invalid token span".into());
                return;
            };
            let original_file = !source.name.to_string_lossy().starts_with('<');
            if original_file {
                let logical_file = context.line_overrides.effective_file(
                    &map, token.span.file, token.span.lo);
                let logical_line = context.line_overrides.effective_line(
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
            // Token separation is conservative: 'a + + b' must not
            // become 'a ++ b', nor can adjacent pp-numbers merge.
            if !output.is_empty() && !output.ends_with('\n') {
                output.push(' ');
            }
            let expansion = if track {
                capture_expansion!(context, &map, index, frame_cache)
            } else { None };
            if track && (original_file || expansion.is_some()) {
                let logical_file = context.line_overrides.effective_file(
                    &map, token.span.file, token.span.lo);
                let line = context.line_overrides.effective_line(
                    &map, token.span.file, token.span.lo) as usize;
                let file = names.entry(logical_file.0).or_insert_with(||
                    Arc::from(map.file(logical_file).name.to_string_lossy().as_ref())).clone();
                origins.push(TokenOrigin {
                    output_start: output.len(), output_end: output.len() + spelling.len(),
                    source: source.src.clone(), source_start: lo, source_end: hi, file, line, expansion,
                });
            }
            output.push_str(spelling);
            if output.len() > MAX_OUTPUT_BYTES {
                adapter_error = Some("internal preprocessor output exceeded 16 MiB limit".into());
                return;
            }
        }
    });

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

    if let Some(error) = adapter_error {
        return Err(error);
    }
    output.push('\n');
    Ok((output, origins))
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
    fn wasm32_cpp_target_uses_real_ilp32_builtin_layout_and_no_host_arch() {
        let opts=options_from_args(&[
            OsString::from("-E"),OsString::from("-nostdinc"),
            OsString::from("--cpp-target=wasm32"),
            OsString::from("-D__wasm32__=1"),
        ]).unwrap();
        assert_eq!(opts.target.pointer_width,32);
        assert_eq!(opts.target.layouts.long.size,4);
        assert_eq!(opts.target.layouts.pointer.size,4);
        assert_eq!(opts.target.os,rcc_session::Os::None);

        let dir=std::env::temp_dir().join(format!(
            "mini-c-wasm32-internal-cpp-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let root=dir.join("main.c");
        std::fs::write(&root,r#"
            #if !defined(__wasm32__) || !defined(__ILP32__)
            #error wasm_target_missing
            #endif
            #if __SIZEOF_POINTER__ != 4 || __SIZEOF_LONG__ != 4
            #error wrong_wasm32_ilp32_builtin
            #endif
            #if __STDC_HOSTED__ != 0 || defined(__LP64__)
            #error host_data_model_leaked
            #endif
            #if defined(__i386__) || defined(__x86_64__) || defined(__linux__)
            #error host_architecture_leaked
            #endif
            int result = __SIZEOF_POINTER__+__SIZEOF_LONG__;
        "#).unwrap();
        let result=file(&root,&[
            OsString::from("-E"),OsString::from("-nostdinc"),
            OsString::from("--cpp-target=wasm32"),
            OsString::from("-D__wasm32__=1"),
            OsString::from("-D__ILP32__=1"),
        ]);
        let _=std::fs::remove_dir_all(dir);
        let output=result.expect("wasm32 internal C preprocessing");
        assert!(output.contains("result = 4 + 4") ||
            output.contains("result = 4+4"),"{output}");
    }

    #[test]
    fn internal_adapter_consumes_many_macro_free_chunks_in_order() {
        let dir = std::env::temp_dir().join(format!(
            "mini-c-stream-adapter-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.join("many.c");
        let mut source = String::new();
        for i in 0..2_000 {
            source.push_str(&format!("int item_{i} = {i};\\n"));
        }
        std::fs::write(&root, source.replace("\\n", "\n")).unwrap();
        let result = file(&root, &[OsString::from("-E"), OsString::from("-nostdinc")]);
        std::fs::remove_dir_all(dir).unwrap();
        let output = result.expect("internal streaming output must be accepted");
        assert!(output.contains("item_0 = 0"), "first declaration missing");
        assert!(output.contains("item_1999 = 1999"), "last declaration missing");
        assert_eq!(output.matches("int item_").count(), 2_000);
    }

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
    fn separated_gcc_cpp_options_match_attached_forms_in_both_adapters() {
        let split = [
            OsString::from("-E"), OsString::from("-nostdinc"),
            OsString::from("-I"), OsString::from("/project/includes"),
            OsString::from("-isystem"), OsString::from("/sdk/includes"),
            OsString::from("--sysroot"), OsString::from("/sdk"),
            OsString::from("-D"), OsString::from("ANSWER=41"),
            OsString::from("-U"), OsString::from("ANSWER"),
            OsString::from("-D"), OsString::from("ANSWER=42"),
        ];
        let attached = [
            OsString::from("-E"), OsString::from("-nostdinc"),
            OsString::from("-I/project/includes"),
            OsString::from("-isystem/sdk/includes"),
            OsString::from("--sysroot=/sdk"),
            OsString::from("-DANSWER=41"),
            OsString::from("-UANSWER"),
            OsString::from("-DANSWER=42"),
        ];
        let split = options_from_args(&split).unwrap();
        let attached = options_from_args(&attached).unwrap();
        assert_eq!(split.include_paths, attached.include_paths);
        assert_eq!(split.system_include_paths, attached.system_include_paths);
        assert_eq!(split.sysroot, attached.sysroot);
        assert_eq!(split.cli_defines, attached.cli_defines);
        assert_eq!(split.cli_undefines, attached.cli_undefines);
        assert_eq!(split.cli_defines, vec![("ANSWER".into(), Some("42".into()))]);
    }


    #[test]
    fn separated_flags_work_in_batch_and_streaming_cpp() {
        let dir = std::env::temp_dir().join(format!(
            "mini-c-cpp-split-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.join("source.c");
        std::fs::write(dir.join("constants.h"), "#define BASE 9\n").unwrap();
        std::fs::write(&root, "#include \"constants.h\"\nint answer = BASE + OFFSET;\n").unwrap();
        let args = vec![
            OsString::from("-E"), OsString::from("-nostdinc"),
            OsString::from("-I"), dir.clone().into_os_string(),
            OsString::from("-D"), OsString::from("OFFSET=33"),
        ];
        let batch = file(&root, &args).expect("split flags in batch preprocessor");
        let stream = stream::start(&root, &args)
            .expect("split flags in streaming preprocessor")
            .map(|token| token.expect("stream token").spelling.to_string())
            .collect::<Vec<_>>().join(" ");
        std::fs::remove_dir_all(dir).unwrap();
        for output in [batch.as_str(), stream.as_str()] {
            assert!(output.contains("answer"), "{output}");
            assert!(output.contains("9"), "{output}");
            assert!(output.contains("33"), "{output}");
        }
    }

    #[test]
    fn separated_gcc_cpp_options_reject_missing_and_flag_operands() {
        for args in [
            vec!["-I"], vec!["-D"], vec!["-U"],
            vec!["-isystem"], vec!["--sysroot"],
            vec!["-I", "-DNAME=1"], vec!["-D", "-UFOO"],
            vec!["-isystem", "-nostdinc"],
            vec!["--sysroot", "-E"],
        ] {
            let options = args.iter().copied().map(OsString::from).collect::<Vec<_>>();
            let error = options_from_args(&options).unwrap_err();
            assert!(error.contains("requires"), "{args:?}: {error}");
        }
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
    fn isystem_include_directory_is_used_after_project_include_paths() {
        let dir = std::env::temp_dir().join(format!(
            "mini-c-rcc-isystem-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let user = dir.join("user");
        let system = dir.join("system");
        std::fs::create_dir_all(&user).unwrap();
        std::fs::create_dir_all(&system).unwrap();
        std::fs::write(user.join("values.h"), "int user_header;\n").unwrap();
        std::fs::write(system.join("values.h"), "int system_header;\n").unwrap();
        std::fs::write(system.join("system_only.h"), "#define FROM_SYSTEM 42\n").unwrap();
        let root = dir.join("root.c");
        std::fs::write(&root,
            "#include <values.h>\n#include <system_only.h>\nint value = FROM_SYSTEM;\n"
        ).unwrap();
        let result = file(&root, &[
            OsString::from("-E"),
            OsString::from(format!("-I{}", user.display())),
            OsString::from(format!("-isystem{}", system.display())),
            OsString::from("-nostdinc"),
        ]);
        let _ = std::fs::remove_dir_all(dir);
        let output = result.unwrap();
        assert!(output.contains("user_header"), "{output}");
        assert!(!output.contains("system_header"), "{output}");
        assert!(output.contains("42"), "{output}");
    }

    #[test]
    fn command_line_gnu_source_can_be_redefined_by_real_sources() {
        let dir = std::env::temp_dir().join(format!(
            "mini-c-gnu-feature-redefine-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.join("unit.c");
        std::fs::write(&root,
            "#define _GNU_SOURCE\n#ifdef _GNU_SOURCE\nint selected = 1;\n#endif\n"
        ).unwrap();
        let output = file(&root, &[
            OsString::from("-E"), OsString::from("-D_GNU_SOURCE")
        ]);
        std::fs::remove_dir_all(dir).unwrap();
        let output = output.unwrap();
        assert!(output.contains("selected"), "{output}");
    }

    #[test]
    fn arbitrary_cli_macro_redefinition_still_fails_under_strict_c99() {
        let dir = std::env::temp_dir().join(format!(
            "mini-c-strict-redefine-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.join("unit.c");
        std::fs::write(&root, "#define USER_VALUE 2\nint selected = USER_VALUE;\n").unwrap();
        let result = file(&root, &[
            OsString::from("-E"), OsString::from("-DUSER_VALUE=1")
        ]);
        std::fs::remove_dir_all(dir).unwrap();
        let error = result.expect_err("unrelated redefinition must remain diagnosed");
        assert!(error.contains("USER_VALUE"), "{error}");
        assert!(error.contains("redefined"), "{error}");
    }

    #[test]
    fn unmodified_musl_strcasestr_preprocesses_with_cli_gnu_source() {
        if !cfg!(target_os = "linux") || !Path::new("/usr/include/string.h").is_file() {
            return;
        }
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/musl-s10/string/strcasestr.c");
        let output = file(&source, &[
            OsString::from("-E"), OsString::from("-D_GNU_SOURCE")
        ]).expect("unmodified musl strcasestr must preprocess");
        assert!(output.contains("strcasestr"), "{output}");
    }

    #[test]
    fn hosted_linux_system_stdio_header_is_discovered() {
        // This is a preprocessor-only smoke test; Mini-C's C frontend
        // may still reject language constructs in the unmodified header.
        if !cfg!(target_os = "linux") || !Path::new("/usr/include/stdio.h").is_file() {
            return;
        }
        let dir = std::env::temp_dir().join(format!(
            "mini-c-rcc-host-stdio-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.join("main.c");
        std::fs::write(&root, "#include <stdio.h>\nint main(void) { return 0; }\n").unwrap();
        let result = file(&root, &[OsString::from("-E")]);
        let _ = std::fs::remove_dir_all(dir);
        let output = result.unwrap();
        assert!(output.contains("printf"), "stdio declarations missing");
        assert!(output.contains("main"), "user source lost");
    }

    #[test]
    fn nostdinc_skips_implicit_system_header_search() {
        if !cfg!(target_os = "linux") || !Path::new("/usr/include/stdio.h").is_file() {
            return;
        }
        let dir = std::env::temp_dir().join(format!(
            "mini-c-rcc-no-stdinc-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.join("main.c");
        std::fs::write(&root, "#include <stdio.h>\n").unwrap();
        let result = file(&root, &[
            OsString::from("-E"), OsString::from("-nostdinc"),
        ]);
        let _ = std::fs::remove_dir_all(dir);
        let failure = result.unwrap_err();
        assert!(failure.contains("stdio.h"), "{failure}");
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
