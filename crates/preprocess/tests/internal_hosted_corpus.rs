//! Check *preprocessing* independently of the Mini-C syntax/sema/frontend.
//! The pinned musl S10 files are unchanged real-world clients of glibc headers.
//! An unrelated frontend type/attribute failure must not be classified as CPP.
#![cfg(feature = "internal-rcc")]

use mini_c_preprocess::{file, Config, Error};
use std::{ffi::OsString, path::Path};

const MUSL_S10_STRING: &[&str] = &[
    "bcmp", "bcopy", "bzero", "explicit_bzero", "index", "memccpy",
    "memchr", "memcmp", "memcpy", "memmem", "memmove", "mempcpy",
    "memrchr", "memset", "rindex", "stpcpy", "stpncpy", "strcasecmp",
    "strcasestr", "strcat", "strchr", "strchrnul", "strcmp", "strcpy",
];

#[test]
fn all_reference_accepted_original_musl_sources_preprocess_internally() {
    if !cfg!(target_os = "linux") || !Path::new("/usr/include/string.h").is_file() {
        // These are host-glibc integration tests, not cross-platform unit tests.
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/musl-s10/string");
    let internal = Config {
        executable: OsString::from("internal"),
        args: vec![OsString::from("-E"), OsString::from("-D_GNU_SOURCE")],
    };
    let reference = Config {
        executable: OsString::from("cc"),
        args: vec![
            OsString::from("-E"), OsString::from("-std=gnu11"),
            OsString::from("-D_GNU_SOURCE"),
        ],
    };
    let mut reference_pass = 0;
    let mut internal_pass = 0;
    let mut mismatches = Vec::new();
    let mut reference_failures = Vec::new();
    for name in MUSL_S10_STRING {
        let source = root.join(format!("{name}.c"));
        assert!(source.is_file(), "missing original corpus fixture: {}", source.display());
        let cc_output = match file(&source, &reference) {
            Ok(text) => text,
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => {
                reference_failures.push(format!("{name}: {error}"));
                continue;
            }
        };
        assert!(!cc_output.is_empty(), "GCC emitted no content for {name}");
        reference_pass += 1;
        match file(&source, &internal) {
            Ok(text) if !text.is_empty() => internal_pass += 1,
            Ok(_) => mismatches.push(format!("{name}: internal output was empty")),
            Err(error) => mismatches.push(format!("{name}: {error}")),
        }
    }
    eprintln!(
        "original musl S10 preprocessor corpus: {internal_pass}/{reference_pass} \
         GCC-accepted sources passed internal preprocessing ({} unavailable references)",
        reference_failures.len()
    );
    for skipped in &reference_failures {
        eprintln!("reference unavailable: {skipped}");
    }
    assert!(reference_pass > 0, "hosted reference accepted no corpus sources");
    assert!(mismatches.is_empty(),
        "internal preprocessing rejected {} GCC-accepted originals:\n{}",
        mismatches.len(), mismatches.join("\n"));
}

#[test]
fn hosted_target_integer_predefines_match_gcc_and_clang() {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "mini-c-cpp-hosted-target-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("probe.c");
    // These compiler-generated target facts are used by real libc headers.
    // Compare the *expanded initializer expressions* only: glibc may emit
    // unrelated declaration/attribute text that intentionally differs from
    // the independent reference preprocessing implementations.
    let probes = [
        ("probe_char_bit", "__CHAR_BIT__"),
        ("probe_int_max", "__INT_MAX__"),
        ("probe_long_max", "__LONG_MAX__"),
        ("probe_long_long_max", "__LONG_LONG_MAX__"),
        ("probe_byte_order", "__BYTE_ORDER__"),
        ("probe_order_little", "__ORDER_LITTLE_ENDIAN__"),
        ("probe_sizeof_pointer", "__SIZEOF_POINTER__"),
    ];
    let mut text = String::from("#include <limits.h>\n#include <stdint.h>\n");
    for (name, macro_name) in probes {
        text.push_str(&format!("unsigned long {name} = {macro_name};\n"));
    }
    std::fs::write(&source, text).unwrap();
    let run = |program: &str| file(&source, &Config {
        executable: OsString::from(program),
        args: if program == "internal" {
            vec![OsString::from("-E"), OsString::from("-D_GNU_SOURCE")]
        } else {
            vec![
                OsString::from("-E"), OsString::from("-std=gnu99"),
                OsString::from("-D_GNU_SOURCE"),
            ]
        },
    });
    let gcc = run("cc").expect("GCC must preprocess hosted target fixture");
    let internal = run("internal").expect("internal must preprocess hosted target fixture");
    let clang = run("clang");
    std::fs::remove_dir_all(&dir).unwrap();

    fn rhs(text: &str, name: &str) -> Vec<String> {
        let toks = mini_c_syntax::lexer::scan(text)
            .expect("valid preprocessing output");
        let i = toks.iter().position(|t| t.text == name)
            .unwrap_or_else(|| panic!("missing {name} in preprocessed output"));
        assert_eq!(toks[i+1].text, "=");
        toks[i+2..].iter().take_while(|tok| tok.text != ";")
            .map(|tok| tok.text.to_owned()).collect()
    }
    // GCC often spells __INT_MAX__ as 0x7fffffff while Clang and the
    // internal engine may use 2147483647. Both denote the same integer
    // and type; compare ABI integer values rather than textual spelling.
    fn literal_value(tokens: &[String]) -> u128 {
        assert_eq!(tokens.len(), 1, "expected one integer literal: {tokens:?}");
        let literal = tokens[0].trim_end_matches(['u', 'U', 'l', 'L']);
        if let Some(hex) = literal.strip_prefix("0x").or_else(|| literal.strip_prefix("0X")) {
            u128::from_str_radix(hex, 16).expect("hex macro value")
        } else {
            literal.parse::<u128>().expect("decimal macro value")
        }
    }
    for (name, macro_name) in probes {
        let expected = literal_value(&rhs(&gcc, name));
        let actual = literal_value(&rhs(&internal, name));
        assert_eq!(actual, expected,
            "target macro {macro_name} differs from hosted GCC reference");
        if let Ok(ref clang) = clang {
            assert_eq!(actual, literal_value(&rhs(clang, name)),
                "target macro {macro_name} differs from Clang reference");
        }
    }
}


#[test]
fn common_c99_and_posix_hosted_headers_preprocess_individually() {
    if !cfg!(target_os = "linux") || !Path::new("/usr/include/stdio.h").is_file() {
        return;
    }
    // These common hosted C99/POSIX headers are tested at the preprocessing
    // boundary: Mini-C checker and native codegen have separate acceptance
    // tests. Keep every header unchanged, using the host's normal sysroot.
    const HEADERS: &[&str] = &[
        "assert.h", "complex.h", "ctype.h", "dirent.h", "errno.h",
        "fenv.h", "float.h", "inttypes.h", "limits.h", "locale.h",
        "math.h", "setjmp.h", "signal.h", "stdarg.h", "stdbool.h",
        "stddef.h", "stdint.h", "stdio.h", "stdlib.h", "string.h",
        "strings.h", "sys/stat.h", "sys/types.h", "tgmath.h",
        "time.h", "unistd.h", "wchar.h", "wctype.h",
    ];
    let dir = std::env::temp_dir().join(format!(
        "mini-c-cpp-hosted-headers-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("include.c");
    let internal = Config {
        executable: OsString::from("internal"),
        args: vec![OsString::from("-E"), OsString::from("-D_GNU_SOURCE")],
    };
    let gcc = Config {
        executable: OsString::from("cc"),
        args: vec![
            OsString::from("-E"), OsString::from("-std=gnu11"),
            OsString::from("-D_GNU_SOURCE"),
        ],
    };
    let mut reference_pass = 0;
    let mut accepted = 0;
    let mut failures = Vec::new();
    for header in HEADERS {
        std::fs::write(
            &source, format!("#include <{header}>\nint header_probe = 7;\n")
        ).unwrap();
        let reference = match file(&source, &gcc) {
            Ok(result) => result,
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::remove_dir_all(&dir).unwrap();
                return;
            }
            Err(error) => {
                failures.push(format!("{header}: GCC rejected host header: {error}"));
                continue;
            }
        };
        assert!(reference.contains("header_probe"), "GCC lost {header}'s source");
        reference_pass += 1;
        match file(&source, &internal) {
            Ok(output) if output.contains("header_probe") => accepted += 1,
            Ok(_) => failures.push(format!("{header}: internal output lost source")),
            Err(error) => failures.push(format!("{header}: {error}")),
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
    eprintln!("hosted C99/POSIX header corpus: {accepted}/{reference_pass} GCC-accepted");
    assert_eq!(reference_pass, HEADERS.len(), "reference environment is incomplete: {failures:?}");
    assert!(failures.is_empty(), "internal hosted-header failures:\n{}", failures.join("\n"));
}
