//! Candidate-neutral correctness gate: compare expanded preprocessing tokens,
//! never compiler-specific whitespace or #line marker formatting.
#![cfg(feature = "internal-rcc")]
use std::{ffi::OsString, path::Path, time::{SystemTime, UNIX_EPOCH}};
use mini_c_preprocess::{file, Config};

fn tokens(source: &str) -> Vec<String> {
    mini_c_syntax::lexer::scan(source).expect("valid C fixture tokens")
        .iter().map(|token| token.text.to_string()).collect()
}

fn compare(fixture: &str, flags: &[&str]) {
    let name = format!("mini-c-internal-cpp-{}-{}",
        std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos());
    let dir = std::env::temp_dir().join(name);
    std::fs::create_dir_all(dir.join("inc folder")).unwrap();
    std::fs::write(dir.join("inc folder").join("data.h"),
        "#ifndef DATA_H\n#define DATA_H\n#define HEADER_VALUE 23\n#endif\n").unwrap();
    let path = dir.join("sample.c");
    std::fs::write(&path, fixture).unwrap();
    let args: Vec<OsString> = std::iter::once(OsString::from("-E"))
        .chain(std::iter::once(OsString::from(
            format!("-I{}", dir.join("inc folder").display()))))
        .chain(flags.iter().map(OsString::from)).collect();
    let external = file(Path::new(&path), &Config {
        executable: OsString::from("cc"), args: args.clone(),
    });
    let internal = file(Path::new(&path), &Config {
        executable: OsString::from("internal"), args,
    });
    let _ = std::fs::remove_dir_all(dir);
    let external = match external {
        Err(mini_c_preprocess::Error::Io(e))
            if e.kind() == std::io::ErrorKind::NotFound => return,
        other => other.unwrap(),
    };
    let internal = internal.unwrap();
    assert_eq!(tokens(&internal), tokens(&external),
        "internal preprocessing mismatch\nsource:\n{fixture}\ninternal:\n{internal}\nexternal:\n{external}");
}

#[test]
fn macros_function_arguments_paste_and_stringify() {
    compare(r#"
#define JOIN(a,b) a##b
#define INDIRECT(a,b) JOIN(a,b)
#define VALUE test
#define STRINGIFY(a) #a
#define XSTR(a) STRINGIFY(a)
int JOIN(pre_,VALUE) = 1;
int INDIRECT(pre_,VALUE) = 2;
char *a = STRINGIFY(VALUE);
char *b = XSTR(VALUE);
"#, &[]);
}

#[test]
fn macro_recursion_and_nested_function_arguments() {
    compare(r#"
#define FIRST SECOND
#define SECOND FIRST
#define INC(x) ((x)+1)
#define TWICE(a) INC(INC(a))
int nested=TWICE(3);
int inert=FIRST;
"#, &[]);
}

#[test]
fn includes_conditions_and_cli_definitions() {
    compare(r#"
#include <data.h>
#include <data.h>
#if defined(FEATURE) && FEATURE == 17 && !defined(REMOVED)
int answer = HEADER_VALUE + FEATURE;
#else
#error missing macro state
#endif
"#, &["-DFEATURE=17", "-DREMOVED=1", "-UREMOVED"]);
}

#[test]
fn multiline_function_macros_and_variadics() {
    compare(r#"
#define ADD(a,b) ((a)+(b))
#define FORWARD(x,...) ADD(x, __VA_ARGS__)
int result = FORWARD(
    2,
    3
);
"#, &[]);
}
