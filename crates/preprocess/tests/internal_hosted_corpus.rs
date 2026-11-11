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
