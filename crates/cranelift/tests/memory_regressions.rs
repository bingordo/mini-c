//! Executable-object compilation smoke tests for the next C memory milestone.
//! Global object tests exercise data emission and relocations.
use mini_c_cranelift::compile_source;

fn emits_elf(src: &str) {
    let obj = compile_source(src).unwrap_or_else(|e| panic!("failed to compile {src:?}: {e}"));
    assert!(obj.len() > 64);
    assert_eq!(&obj[..4], b"\x7fELF");
}

#[test]
fn explicit_pointer_to_uintptr_casts_compile_for_lp64() {
    emits_elf("typedef unsigned long uintptr_t; \
        uintptr_t addr(char *p) { return (uintptr_t)p; } \
        int main(void) { char s[8]; return ((uintptr_t)s & 7) == 0; }");
}

#[test]
fn c99_switch_direct_labels_fallthrough_and_default_emit_elf() {
    emits_elf(
        "int choose(int x) { int total=0; switch(x) { \
         case 1: total=3; break; \
         case 2: total=4; case 3: total+=5; break; \
         default: total=6; } return total; } \
         int main(void) { return choose(1)!=3 || choose(2)!=9 || \
         choose(3)!=5 || choose(99)!=6; }"
    );
}

#[test]
fn c99_switch_nested_loop_break_continue_and_unsigned_cases_emit_elf() {
    emits_elf(
        "int main(void) { unsigned long x=~0UL; int result=0; \
         switch(x) { case 18446744073709551615UL: result=2; break; default: result=7; } \
         for(int i=0;i<4;i++){switch(i){case 1: continue; default: result+=i;} result+=3;} \
         return result != 16; }"
    );
    emits_elf(
        "int main(void) { int value=0; switch(1) { \
         case 1: switch(2) { case 2: value=7; break; default: value=9; } \
         value+=3; break; default: value=11; } return value!=10; }"
    );
}

#[test]
fn c99_switch_nested_case_and_skipped_local_emit_elf() {
    emits_elf(
        "int f(int x){switch(x){if(x){case 1: return 1;} default: return 0;}}"
    );
    // A selected case can bypass the initializer. Assign before reading the
    // local to keep the executed C program defined.
    emits_elf(
        "int f(int x){switch(x){int y=3; case 1: y=7; return y; default: return 0;}}"
    );
    emits_elf(concat!(
        "int f(int x){switch(x){for(int i=0;i<3;i++){case 1: i=2; break;} ",
        "break; default: return 0;}return 3;}"
    ));
}

#[test]
fn c99_switch_goto_arm_labels_emit_elf() {
    emits_elf(
        "int f(int x){goto done;switch(x){case 1: done: return 1;default:return 0;}}"
    );
    emits_elf(
        "int f(int x){goto done;switch(x){int y=8; done: y=3; break;}return 3;}"
    );
    emits_elf(
        "int f(int x){goto done;switch(x){case 1: if(x){done: return 9;} break;}return 0;}"
    );
}

#[test]
fn c99_bool_pointer_scalar_conversions_compile() {
    emits_elf("int nonnull(_Bool b) { return b; } \
        _Bool is_nonnull(int *p) { return p; } \
        int main(void) { int x=0; int *p=&x; \
          _Bool a=p, b=(_Bool)p, c=(int*)0, d=0; \
          d=p; return nonnull(p) && is_nonnull(p) && a && b && d && !c ? 0 : 1; }");
}

#[test]
fn c99_sizeof_named_aggregate_type_name_emits_elf() {
    emits_elf("struct Flags { _Bool first; int middle; _Bool last; }; \
        int main(void) { return sizeof(struct Flags)==12 ? 0 : 1; }");
}

#[test]
fn c99_bool_struct_fields_and_arrays_compile() {
    emits_elf("struct Flags { _Bool active; int count; _Bool ready; }; \
        int main(void) { struct Flags f; _Bool flags[3]; \
        f.active=256; f.count=17; f.ready=0; \
        flags[0]=0; flags[1]=256; flags[2]=3; \
        return sizeof(f)!=12 || sizeof(flags)!=3 || \
            f.active!=1 || f.count!=17 || f.ready!=0 || \
            flags[0]!=0 || flags[1]!=1 || flags[2]!=1; }");
}

#[test]
fn c99_bool_truth_conversion_and_promotion_compile() {
    emits_elf("_Bool g = 256; int main(void) { _Bool x = 2; \
        _Bool y = 256; return x == 1 && y == 1 && g == 1 ? 0 : 1; }");
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

#[test]
fn enumerations_and_signed_values_compile() {
    emits_elf("enum Mode { OFF, ON=7, NEXT=ON+3, NEGATIVE=-1 }; enum Mode m=NEXT; int main(void) { return m-10; }");
    emits_elf("int main(void) { enum Local { FIRST=-1, SECOND, THIRD=SECOND+11 }; enum Local value=THIRD; return value-10; }");
}

#[test]
fn aggregate_pointer_fields_emit_multiple_relocations() {
    emits_elf("int target=19; struct Holder { int *ptr; char *text; } value={&target,\"hi\"}; int main(void) { return *value.ptr+value.text[1]-124; }");
    emits_elf("char *messages[2]={\"red\",\"blue\"}; int main(void) { return messages[0][0]+messages[1][0]-212; }");
    emits_elf("struct Node; struct Node { int value; struct Node *next; }; struct Node head={17,&head}; int main(void) { return head.next->value-17; }");
}

#[test]
fn by_value_aggregate_abi_accepts_register_and_memory_classes() {
    // INTEGER-class one/two-eightbyte aggregate imports can now be
    // referenced by address or called with their real ABI signatures.
    emits_elf("struct T { int x; }; struct T copy(struct T x);");
    emits_elf("union U { int x; long y; }; union U make(void);");
    emits_elf("struct T { int x; }; struct T copy(struct T x); int main(void) { return &copy == 0; }");
    emits_elf("union U { int x; long y; }; union U make(void); int main(void) { make(); return 0; }");

    // MEMORY-class results are passed via a hidden sret pointer. Unused
    // hosted prototypes still need no code generation.
    emits_elf("struct Large { long a,b,c; }; struct Large make_large(void);");
    emits_elf("struct Large { long a,b,c; }; struct Large make_large(void); int main(void) { make_large(); return 0; }");
    emits_elf("struct Large { long a,b,c; }; long consume(struct Large); int main(void) { struct Large x={1,2,3}; return consume(x); }");
    // Until a generic memcpy lowering exists, cap giant arguments.
    let err = compile_source(
        "struct Huge { long v[600]; }; long consume(struct Huge); int main(void) { struct Huge x; return consume(x); }"
    ).unwrap_err();
    assert!(err.contains("unsupported SysV"), "{err}");
}

#[test]
fn enum_tagged_parameter_and_return_uses_scalar_abi() {
    emits_elf("enum Status { FIRST=3, SECOND=7 }; enum Status inc(enum Status x) { return x+1; } int main(void) { return inc(FIRST)-4; }");
}
