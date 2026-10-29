//! Executable-object compilation smoke tests for the next C memory milestone.
//! Global object tests exercise data emission and relocations.
use mini_c_cranelift::compile_source;

fn emits_elf(src: &str) {
    let obj = compile_source(src).unwrap_or_else(|e| panic!("failed to compile {src:?}: {e}"));
    assert!(obj.len() > 64);
    assert_eq!(&obj[..4], b"\x7fELF");
}

#[test]
fn pointer_aliasing_and_array_indexing_compile() {
    emits_elf("int main(void) { int a[4]; int *p=a; p[1]=11; a[2]=7; return a[1]+p[2]-18; }");
}

#[test]
fn nested_arrays_and_pointer_parameter_compile() {
    emits_elf("int put(int *p,int i) { p[i]=19; return p[i]; } int main(void) { int a[2][3]; return put(a[1],2)-19; }");
}

#[test]
fn local_address_taken_across_direct_call_compiles() {
    emits_elf("int change(int *p) { *p=17; return 0; } int main(void) { int x=3; change(&x); return x-17; }");
}

#[test]
fn rejects_incomplete_local_array() {
    let err=compile_source("int main(void) { int a[]; return 0; }").unwrap_err();
    assert!(err.contains("incomplete"), "{err}");
}

#[test]
fn initialized_global_object_compiles() {
    emits_elf("int g=7; int main(void) { return g-7; }");
}

#[test]
fn internal_global_and_extern_reference_compile() {
    emits_elf("static int g=9; extern int g; int main(void) { return g-9; }");
}

#[test]
fn mutable_global_object_compiles() {
    emits_elf("int counter=10; int increment(void) { counter=counter+1; return counter; } int main(void) { return increment()-11; }");
}

#[test]
fn tentative_global_zero_initialization_compiles() {
    emits_elf("int counter; int main(void) { return counter; }");
}

#[test]
fn global_pointer_initializer_is_explicitly_rejected() {
    let err = compile_source("int g; int *p=&g; int main(void) { return *p; }").unwrap_err();
    assert!(err.contains("integer scalar support"), "{err}");
}
