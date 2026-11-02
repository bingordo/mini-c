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
    std::fs::write(dir.join("inc folder").join("unguarded.h"),
        "int repeated_header;\n").unwrap();
    std::fs::write(dir.join("inc folder").join("once.h"),
        "#pragma once\nint only_once;\n").unwrap();
    let path = dir.join("sample.c");
    std::fs::write(&path, fixture).unwrap();
    let args: Vec<OsString> = std::iter::once(OsString::from("-E"))
        .chain(std::iter::once(OsString::from(
            format!("-I{}", dir.join("inc folder").display()))))
        .chain(flags.iter().map(OsString::from)).collect();
    let external = file(Path::new(&path), &Config {
        executable: OsString::from("cc"), args: args.clone(),
    });
    let clang = file(Path::new(&path), &Config {
        executable: OsString::from("clang"), args: args.clone(),
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
    let result = tokens(&internal);
    assert_eq!(result, tokens(&external),
        "internal/GCC preprocessing mismatch\nsource:\n{fixture}\ninternal:\n{internal}\nGCC:\n{external}");
    match clang {
        Ok(clang) => assert_eq!(result, tokens(&clang),
            "internal/Clang preprocessing mismatch\nsource:\n{fixture}\ninternal:\n{internal}\nClang:\n{clang}"),
        Err(mini_c_preprocess::Error::Io(e))
            if e.kind() == std::io::ErrorKind::NotFound => {},
        Err(error) => panic!("Clang rejected fixture: {error:?}\n{fixture}"),
    }
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

#[test]
fn zero_argument_macros_and_empty_arguments() {
    compare(r#"
#define VOID()
#define VALUE() 17
#define ID(x) x
#define ARG2(a,b) a + b
int first = VALUE();
int second = ID(23);
int third = ARG2(ID(3), ID(5));
int fourth = VALUE() VOID() + 1;
"#, &[]);
}

#[test]
fn conditional_bitwise_and_ternary_precedence() {
    compare(r#"
#define A 0x17
#define B 5
#if defined(A) && !defined(MISSING) && ((A >> 1) >= B) && ((A & 3) == 3)
int included = 11;
#elif 1
int wrong_branch = 0;
#else
#error unreachable
#endif
#if (0 ? 1 : (3 + 4 * 2)) == 11
int ternary_result = 7;
#endif
"#, &[]);
}

#[test]
fn comments_and_backslash_newline_translation_phases() {
    compare("#define ADD(a,b) ((a) + \\\n (b))\n\
        #define REPL /* comment */ 27\n\
        int answer = ADD(REPL, 3); // comment\n\
        int another = 1/**/+/**/2;\n", &[]);
}

#[test]
fn unguarded_headers_expand_multiple_times_but_pragma_once_does_not() {
    compare(r#"
#include <unguarded.h>
#include <unguarded.h>
#include <once.h>
#include <once.h>
int done = 1;
"#, &[]);
}

#[test]
fn adjacent_paste_and_stringification_preserve_c99_rules() {
    compare(r#"
#define CAT(a,b) a##b
#define XCAT(a,b) CAT(a,b)
#define STRING(x) #x
#define EXPAND_STRING(x) STRING(x)
#define VAL value
int CAT(pre_,VAL) = 1;
int XCAT(pre_,VAL) = 2;
char *s = STRING(VAL + 3);
char *t = EXPAND_STRING(VAL);
"#, &[]);
}

#[test]
fn line_directives_and_direct_builtin_line_values() {
    compare(r#"
#line 100 "virtual name.c"
int first = __LINE__;
int second = __LINE__;
"#, &[]);
}

#[test]
fn line_and_file_builtins_in_object_function_and_indirect_macros() {
    compare(r#"
#line 100 "virtual name.c"
#define DIRECT_LINE __LINE__
#define WRAPPED_LINE(x) x
#define INDIRECT_LINE DIRECT_LINE
#define DIRECT_FILE __FILE__
#define WRAPPED_FILE(x) x
int first = DIRECT_LINE;
int second = WRAPPED_LINE(__LINE__);
int third = INDIRECT_LINE;
char *name1 = DIRECT_FILE;
char *name2 = WRAPPED_FILE(__FILE__);
"#, &[]);
}

#[test]
fn line_builtin_inside_macro_matches_gcc_when_invocation_is_fixed() {
    compare(r#"
#line 100 "virtual name.c"
int first = __LINE__;
int second = __LINE__;
#define WHICH_LINE __LINE__
int third = WHICH_LINE;
"#, &[]);
}

#[test]
fn macro_builtin_in_nested_header_uses_invocation_not_definition_location() {
    // Header macro definition has a different physical filename and line
    // from the call site. Both __FILE__ and __LINE__ must use the caller.
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "mini-c-macro-origins-{}-{nonce}", std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let header = dir.join("macro.h");
    let source = dir.join("source.c");
    std::fs::write(&header, "#define SHOW_LINE __LINE__\n#define SHOW_FILE __FILE__\n").unwrap();
    std::fs::write(&source,
        "#include \"macro.h\"\n#line 41 \"virtual-caller.c\"\nint n=SHOW_LINE;\nchar *f=SHOW_FILE;\n"
    ).unwrap();
    let args = vec![OsString::from("-E")];
    let reference = file(&source, &Config {
        executable: OsString::from("cc"), args: args.clone(),
    });
    let internal = file(&source, &Config {
        executable: OsString::from("internal"), args,
    });
    let _ = std::fs::remove_dir_all(dir);
    let reference = match reference {
        Err(mini_c_preprocess::Error::Io(e))
            if e.kind() == std::io::ErrorKind::NotFound => return,
        other => other.unwrap(),
    };
    let internal = internal.unwrap();
    assert_eq!(tokens(&internal), tokens(&reference),
        "header macro invocation mismatch\ninternal:\n{internal}\nGCC:\n{reference}");
}

#[test]
fn command_line_defines_and_undefines_follow_last_operation() {
    // GCC applies -D and -U in argument order, including redefinition.
    // rcc's original options representation groups these by kind; the
    // Mini-C adapter must not reorder the user's command-line semantics.
    compare(r#"
#if FEATURE == 7
int final_feature = 7;
#else
#error macro override was not applied
#endif
"#, &["-DFEATURE=1", "-UFEATURE", "-DFEATURE=7"]);

    compare(r#"
#ifdef FEATURE
#error feature should have been undefined
#else
int feature_is_missing = 1;
#endif
"#, &["-DFEATURE=1", "-UFEATURE"]);
}

#[test]
fn command_line_macro_redefinitions_keep_last_value() {
    compare("int last = NUMBER;\n", &[
        "-DNUMBER=17", "-DNUMBER=23",
    ]);
}

fn next_random(state: &mut u64) -> u64 {
    // Deterministic xorshift64, with no external rand dependency.
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn macro_graph_fixture(seed: u64) -> String {
    let mut random = seed | 1;
    let a = next_random(&mut random) % 29 + 1;
    let b = next_random(&mut random) % 13 + 1;
    let c = next_random(&mut random) % 11 + 1;
    let depth = (next_random(&mut random) % 5 + 1) as usize;
    let mut fixture = format!(
        "/* seed: {seed} */\n\
         #define BASE {a}\n\
         #define FACTOR {b}\n\
         #define ADD(x,y) ((x)+(y))\n\
         #define MULTIPLY(x,y) ((x)*(y))\n\
         #define STRINGIZE(x) #x\n\
         #define EXPAND_STRINGIZE(x) STRINGIZE(x)\n\
         #define CONCAT(x,y) x##y\n\
         #define INDIRECT_CONCAT(x,y) CONCAT(x,y)\n\
         #define SUFFIX {c}\n"
    );
    for level in 0..depth {
        let prior = if level == 0 {
            "BASE".to_owned()
        } else {
            format!("LEVEL{}", level - 1)
        };
        // Each stage uses the preceding macro as a complete argument,
        // exercising nested rescanning and whitespace preservation.
        fixture.push_str(&format!(
            "#define LEVEL{level} ADD({prior}, FACTOR)\n"
        ));
    }
    fixture.push_str(&format!(
        "int result = MULTIPLY(LEVEL{}, SUFFIX);\n",
        depth - 1,
    ));
    fixture.push_str(
        "int CONCAT(raw_,SUFFIX) = 7;\n\
         int INDIRECT_CONCAT(expanded_,SUFFIX) = 11;\n\
         char *direct = STRINGIZE(SUFFIX);\n\
         char *indirect = EXPAND_STRINGIZE(SUFFIX);\n\
         #if defined(BASE) && ((BASE + FACTOR) > BASE)\n\
         int conditional = 1;\n\
         #else\n\
         #error wrong expansion result\n\
         #endif\n",
    );
    fixture
}

#[test]
fn seeded_macro_graph_differential_regressions() {
    // The default adds only four small compiler invocations to frequent CI.
    // A manual extended run can use MINI_C_CPP_SEED_COUNT=128; a failing
    // fixture prints its seed for deterministic, one-case reproduction.
    let count = std::env::var("MINI_C_CPP_SEED_COUNT")
        .ok().and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(4).clamp(1, 128);
    for index in 0..count {
        let seed = 0x619A_5A31_A4C3_E19Fu64
            .wrapping_add((index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let fixture = macro_graph_fixture(seed);
        compare(&fixture, &[]);
    }
}

#[test]
fn seeded_macro_graph_generator_is_reproducible() {
    let first = macro_graph_fixture(0x1234_5678);
    assert_eq!(first, macro_graph_fixture(0x1234_5678));
    assert_ne!(first, macro_graph_fixture(0x1234_5679));
    assert!(first.contains("/* seed: 305419896 */"));
    assert!(first.contains("#define ADD(x,y)"));
}
