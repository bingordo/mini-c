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
fn global_pointer_initializer_emits_relocation() {
    emits_elf("int g; int *p=&g; int main(void) { return *p; }");
}

#[test]
fn unresolved_extern_global_emits_relocatable_object() {
    emits_elf("extern int shared; int get(void) { return shared; }");
}

#[test]
fn extern_declaration_then_definition_compiles() {
    emits_elf("extern int shared; int shared=42; int main(void) { return shared-42; }");
}

#[test]
fn repeated_tentative_definitions_compile() {
    emits_elf("int shared; int shared; int main(void) { shared=5; return shared-5; }");
}

#[test]
fn local_variable_shadows_global_object() {
    emits_elf("int x=13; int main(void) { int x=7; return x-7; }");
}

#[test]
fn null_pointer_global_initializer_compiles() {
    emits_elf("int *p=0; int main(void) { return p==0; }");
}

#[test]
fn pointer_global_tentative_definition_compiles() {
    emits_elf("int *p; int main(void) { return p==0; }");
}

#[test]
fn global_pointer_to_named_global_emits_relocation() {
    emits_elf("int value=37; int *ptr=&value; int main(void) { return *ptr-37; }");
}

#[test]
fn global_pointer_to_extern_emits_relocation() {
    emits_elf("extern int value; int *ptr=&value; int main(void) { return *ptr; }");
}

#[test]
fn global_array_decay_pointer_initializer_emits_relocation() {
    emits_elf("int values[3]; int *p=values; int main(void) { p[1]=7; return values[1]-7; }");
}

#[test]
fn extern_array_decay_pointer_initializer_emits_relocation() {
    emits_elf("extern int values[3]; int *p=values; int main(void) { return p[1]; }");
}


#[test]
fn local_string_pointer_and_character_index_compile() {
    emits_elf(r#"int main(void) { char *p="hello"; return p[1]-'e'; }"#);
}

#[test]
fn string_literal_as_function_argument_compiles() {
    emits_elf(r#"int second(char *p) { return p[1]; } int main(void) { return second("abc")-'b'; }"#);
}

#[test]
fn global_string_pointer_initializer_emits_relocation() {
    emits_elf(r#"char *p="hello"; int main(void) { return p[4]-'o'; }"#);
}

#[test]
fn string_literal_escape_bytes_and_embedded_nul_compile() {
    emits_elf(r#"int main(void) { char *p="a\n\x42\0z"; return p[1]+p[2]+p[3]-76; }"#);
}

#[test]
fn global_character_array_from_string_emits_elf() {
    emits_elf(r#"char text[4]="abc"; int main(void) { return text[2]-'c'; }"#);
    emits_elf(r#"char inferred[]="abc"; int main(void) { return inferred[3]; }"#);
    emits_elf(r#"unsigned char padded[8]="hi"; int main(void) { return padded[7]; }"#);
    emits_elf(r#"char exact[3]="abc"; int main(void) { return exact[2]-'c'; }"#);
}

#[test]
fn local_character_array_from_string_emits_elf() {
    emits_elf(r#"int main(void) { char text[]="abc"; return text[3]; }"#);
    emits_elf(r#"int main(void) { char short_string[3]="abc"; return short_string[2]-'c'; }"#);
    emits_elf(r#"int main(void) { char padded[13]="ok"; return padded[12]; }"#);
    emits_elf(r#"int main(void) { unsigned char bytes[4]="\377"; return bytes[0]-255; }"#);
}

#[test]
fn rejects_oversized_character_array_initializer() {
    let err=compile_source(r#"char text[2]="abc"; int main(void) { return text[0]; }"#)
        .unwrap_err();
    assert!(err.contains("string literal is too long"), "{err}");
}

#[test]
fn incomplete_char_array_redeclarations_compile() {
    emits_elf(r#"extern char text[]; char text[]="abc";
        int main(void) { return text[3]; }"#);
    emits_elf(r#"char text[]="abc"; extern char text[];
        int main(void) { return text[0]-'a'; }"#);
    emits_elf(r#"char text[4]; char text[]="abc";
        int main(void) { return text[2]-'c'; }"#);
}


#[test]
fn inline_structure_members_and_padding_compile() {
    emits_elf("int main(void) { struct Pair { char a; int b; long c; } v; v.a=2; v.b=17; v.c=19; return v.a+v.b+v.c-38; }");
    emits_elf("int main(void) { struct { int x; long y; } item; (&item)->x=11; (&item)->y=37; return item.x+item.y-48; }");
}

#[test]
fn nested_structs_and_arrays_compile() {
    emits_elf("int main(void) { struct { char first; struct { int a; long b; } inner; } item; item.inner.a=23; item.inner.b=41; return item.inner.a+item.inner.b-64; }");
    emits_elf("int main(void) { struct { char first; long value; } items[2]; items[1].value=27; items[0].first=5; return items[1].value+items[0].first-32; }");
}

#[test]
fn tentative_global_structure_compiles() {
    emits_elf("struct Record { char a; int b; long c; } global; int main(void) { global.b=17; global.c=19; return global.a+global.b+global.c-36; }");
}

#[test]
fn structure_assignment_and_initialization_copies_every_field() {
    emits_elf("int main(void) { struct { char c; int i; long n; } a[2]; a[0].c=7; a[0].i=19; a[0].n=31; a[1]=a[0]; return a[1].c+a[1].i+a[1].n-57; }");
    emits_elf("int main(void) { struct { int a; long b; } x[2]; x[0].a=17; x[0].b=29; { struct { int a; long b; } *unused=0; } x[1]=x[0]; return x[1].a+x[1].b-46; }");
}

#[test]
fn aggregate_brace_initializers_emit_elf() {
    emits_elf("int a[4]={3,5}; int main(void) { return a[0]+a[1]+a[2]+a[3]-8; }");
    emits_elf("int a[]={3,5,7}; int main(void) { return a[2]-7; }");
    emits_elf("struct { char c; long n; int x; } g={3,19}; int main(void) { return g.c+g.n+g.x-22; }");
    emits_elf("int main(void) { struct { int a; long b; } v={17,29}; return v.a+v.b-46; }");
    emits_elf("int main(void) { int v[2][3]={{1,2,3},{4,5,6}}; return v[1][2]-6; }");
}

#[test]
fn union_objects_and_first_member_init_compile() {
    emits_elf("union { int i; long l; } v={7}; int main(void) { return v.i-7; }");
    emits_elf("int main(void) { union { char c; long l; } v; v.l=27; return v.l-27; }");
    emits_elf("int main(void) { union { int i; long l; } a[2]; a[0].i=17; a[1]=a[0]; return a[1].i-17; }");
    emits_elf("int main(void) { struct { char flag; union { long x; int y; } u; } v; v.flag=3; v.u.x=19; return v.flag+v.u.x-22; }");
}

#[test]
fn named_struct_tags_recursive_fields_and_forward_declarations_compile() {
    emits_elf("struct Node; struct Node { int value; struct Node *next; }; struct Node root; int main(void) { root.value=19; root.next=&root; return root.next->value-19; }");
    emits_elf("typedef struct Item Item; struct Item { long amount; }; Item value; int main(void) { value.amount=31; return value.amount-31; }");
    emits_elf("union U; union U { int x; long y; }; union U word; int main(void) { word.x=17; return word.x-17; }");
}
