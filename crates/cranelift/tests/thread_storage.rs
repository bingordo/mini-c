//! Independently link Cranelift TLS objects with GCC TLS definitions and
//! exercise per-thread address identity, initialized/zero storage and imports.
use mini_c_cranelift::compile_source;
use std::{fs, process::Command};

#[test]
fn gnu_and_c11_tls_interoperate_across_threads() {
    let source = r#"
        extern __thread int imported;
        __thread int exported=7;
        static _Thread_local int local;
        _Thread_local int zero;
        int *addr(void){return &exported;}
        int step(void){return ++exported + ++local + ++zero + ++imported;}
    "#;
    let object = compile_source(source).expect("TLS lowering");
    let dir=std::env::temp_dir().join(format!("minic-tls-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("native.o"),object).unwrap();
    fs::write(dir.join("oracle.c"),r#"
        #include <pthread.h>
        #include <assert.h>
        __thread int imported=11;
        extern __thread int exported, zero;
        extern int *addr(void);
        extern int step(void);
        static int *main_addr;
        static void *worker(void *unused) {
            (void)unused;
            assert(addr()!=main_addr && addr()==&exported);
            assert(exported==7 && zero==0 && imported==11);
            assert(step()==22 && step()==26);
            return 0;
        }
        int main(void) {
            main_addr=addr();
            assert(main_addr==&exported);
            assert(step()==22);
            pthread_t t;
            assert(!pthread_create(&t,0,worker,0));
            assert(!pthread_join(t,0));
            assert(exported==8 && zero==1 && imported==12);
            assert(step()==26);
            return 0;
        }
    "#).unwrap();
    let link=Command::new("cc").current_dir(&dir).args(["-pthread","oracle.c","native.o","-o","oracle"]).output().unwrap();
    assert!(link.status.success(),"{}",String::from_utf8_lossy(&link.stderr));
    let run=Command::new(dir.join("oracle")).output().unwrap();
    assert!(run.status.success(),"{}",String::from_utf8_lossy(&run.stderr));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn invalid_tls_declarations_and_static_addresses_reject() {
    for source in [
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
