//! C11 atomic-qualified integer objects may be source checked, but this
//! Cranelift reference path does not yet lower their memory effects to TypeId.
//! Never emit ordinary loads/stores or a linkable ELF as an accidental fallback.

use mini_c_cranelift::compile_source;

#[test]
fn checked_c11_atomic_objects_reject_before_native_elf() {
    for source in [
        "_Atomic int x;",
        "const _Atomic int x=3;",
        "_Atomic(short) x;",
        "_Atomic(long long) x;",
        "int f(void){_Atomic int x; return 0;}",
        "int f(_Atomic int *p){return *p;}",
        "void f(_Atomic int *p){*p=3;}",
        "typedef _Atomic(int) AI; AI x;",
        "struct S {_Atomic int state;}; int f(struct S *p){return p->state;}",
    ] {
        let error=compile_source(source)
            .expect_err("atomic-qualified source must not emit ordinary ELF");
        assert!(error.contains("C11 _Atomic object requires native TypeId atomic lowering"),
            "unexpected error for {source}: {error}");
    }
}

#[test]
fn non_atomic_scalar_objects_still_emit_elf() {
    let object=compile_source("int f(int *p){*p=9; return *p;}").unwrap();
    assert_eq!(&object[..4],b"\x7fELF");
}
