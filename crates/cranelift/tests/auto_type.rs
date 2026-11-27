//! GCC inferred local types use the existing checked declaration lowering.
use mini_c_cranelift::compile_source;
use std::{fs,process::Command};

#[test]
fn inferred_locals_preserve_value_type_scope_and_evaluation() {
    let source=r#"
      int f(int n) {
        int x=n;
        __auto_type p=&x;
        __auto_type old=(*p)++;
        int arr[2]={4,5};
        __auto_type a=arr;
        { __auto_type x=x+3; *p+=x; }
        return old+*p+a[1]+sizeof(*a);
      }
    "#;
    let bytes=compile_source(source).expect("inferred locals");
    let dir=std::env::temp_dir().join(format!("minic-auto-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("native.o"),bytes).unwrap();
    fs::write(dir.join("ref.c"),source.replace("int f(","int ref_f(")).unwrap();
    fs::write(dir.join("oracle.c"),"int f(int),ref_f(int); int main(void){for(int n=-50;n<50;n++)if(f(n)!=ref_f(n))return 1;return 0;}").unwrap();
    let link=Command::new("cc").current_dir(&dir).args(["-std=gnu11","ref.c","oracle.c","native.o","-o","oracle"]).output().unwrap();
    assert!(link.status.success(),"{}",String::from_utf8_lossy(&link.stderr));
    assert!(Command::new(dir.join("oracle")).status().unwrap().success());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn invalid_inferred_declarations_reject() {
    for s in [
        "int f(void){__auto_type x;return 0;}",
        "int f(void){__auto_type *x=0;return 0;}",
        "int f(void){__auto_type x=1,y=2;return 0;}",
        "int f(void){__auto_type x=x;return 0;}",
        "int f(void){typedef __auto_type x=1;return 0;}",
        "int f(void){__auto_type x=(void)0;return 0;}",
        "int f(void){struct S{int b:3;} s={0};__auto_type x=s.b;return x;}",
    ] {assert!(compile_source(s).is_err(),"{s}");}
}

#[test]
fn statement_expressions_match_gcc_on_scopes_and_lazy_effects() {
    let source = r#"
      int f(int n) {
        int x=n, effects=0;
        int a=__extension__ ({ __auto_type x=x+1; effects++; x+=2; x; });
        int z=({ __auto_type p=&x; *p+=1; ({ __auto_type q=*p; q+ ++effects; }); });
        int shorted=0 && ({ effects++; 1; });
        int choice=n>0 ? ({effects+=2; x;}) : ({effects+=3; a;});
        int branch=({int v=0; if(n>0) v=1; else v=2; v;});
        int looped=({int v=0; for(int i=0;i<3;i++) v+=i; v;});
        int uneval=sizeof ({effects++; x;});
        (void) ({effects++; (void)0;});
        ({});
        return a+z+x+shorted+choice+branch+looped+uneval+effects*1000;
      }
    "#;
    let bytes=compile_source(source).expect("statement expressions");
    let dir=std::env::temp_dir().join(format!("minic-stmt-expr-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("native.o"),bytes).unwrap();
    fs::write(dir.join("ref.c"),source.replace("int f(","int ref_f(")).unwrap();
    fs::write(dir.join("oracle.c"),"int f(int),ref_f(int); int main(void){for(int n=-50;n<50;n++)if(f(n)!=ref_f(n))return 1;return 0;}").unwrap();
    for opt in ["-O0","-O2"] {
        let link=Command::new("cc").current_dir(&dir).args(["-std=gnu11",opt,"ref.c","oracle.c","native.o","-o","oracle"]).output().unwrap();
        assert!(link.status.success(),"{}",String::from_utf8_lossy(&link.stderr));
        assert!(Command::new(dir.join("oracle")).status().unwrap().success(),"{opt}");
    }
    fs::remove_dir_all(dir).unwrap();
}
