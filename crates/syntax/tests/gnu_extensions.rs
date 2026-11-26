use mini_c_syntax::{parser::{Parser,GnuExtensions},stream::ItemSource};
fn parse(source:&str,mode:GnuExtensions)->Result<(),String>{
    let mut p=Parser::new(source).with_gnu_extensions(mode);
    while p.next_item().map_err(|e|e.message)?.is_some(){}
    Ok(())
}
#[test]
fn extension_permissions_are_lexically_scoped(){
    use GnuExtensions::*;
    for (raw,marked) in [
        ("int f(void){__auto_type a=1;return a;}", "int f(void){__extension__ __auto_type a=1;return a;}"),
        ("__thread int a;", "__extension__ __thread int a;"),
        ("int f(void){asm(\"nop\");return 0;}", "int f(void){__extension__ asm(\"nop\");return 0;}"),
        ("int f(void) __attribute__((unused));","__extension__ int f(void) __attribute__((unused));"),
        ("struct S { int x __attribute__((aligned(4))); };","struct S { __extension__ int x __attribute__((aligned(4))); };"),
    ] {
        assert!(parse(raw,Disabled).is_err(),"{raw}");
        assert!(parse(raw,Enabled).is_ok(),"{raw}");
        assert!(parse(marked,Disabled).is_ok(),"{marked}");
    }
    assert!(parse("int f(void){__extension__ __auto_type x=1;__auto_type y=2;return x+y;}",Disabled).is_err());
    assert!(parse("__extension__ __thread int a,b; __thread int c;",Disabled).is_err());
    assert!(parse("int f(void){return __extension__ (1+2);}",Disabled).is_ok());
}
