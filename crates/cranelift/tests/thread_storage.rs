//! Check C11/GNU file-scope TLS storage without silently emitting ordinary data.
use mini_c_cranelift::compile_source;

#[test]
fn unused_extern_thread_storage_is_not_an_ordinary_object() {
    for source in [
        "extern __thread int value; int f(void){return 42;}",
        "extern _Thread_local int value; int f(void){return 42;}",
        "_Thread_local extern int value; int f(void){return 42;}",
        "extern __thread void *p, *q; int f(void){return 42;}",
    ] {
        assert!(compile_source(source).is_ok(), "{source}");
    }
}

#[test]
fn unsupported_thread_storage_never_emits_ordinary_object() {
    for source in [
        "__thread int value;",
        "static _Thread_local int value=7;",
        "extern __thread int value; int f(void){return value;}",
        "extern _Thread_local int value; int *f(void){return &value;}",
        "extern __thread int value; extern int value;",
        "extern int value; extern __thread int value;",
        "extern __thread int value; int *p=&value;",
        "__thread int f(void){return 0;}",
        "typedef __thread int T;",
        "extern __thread int value, f(void);",
        "extern __thread _Thread_local int value;",
    ] {
        assert!(compile_source(source).is_err(), "{source}");
    }
}
