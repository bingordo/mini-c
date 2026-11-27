//! Compare all GNU bit-count families with independently compiled GCC.
use mini_c_cranelift::compile_source;
use std::{fs,process::Command};

#[test]
fn bit_count_widths_and_single_evaluation_match_gcc() {
    let source=r#"
      int count(unsigned int x, unsigned long y, unsigned long long z) {
        return __builtin_clz(x)+__builtin_clzl(y)+__builtin_clzll(z)
          +__builtin_ctz(x)+__builtin_ctzl(y)+__builtin_ctzll(z)
          +__builtin_popcount(x)+__builtin_popcountl(y)+__builtin_popcountll(z);
      }
      int once(unsigned long *p) { return __builtin_clzl((*p)++); }
      int zero(void){return __builtin_popcount(0)+__builtin_popcountl(0)+__builtin_popcountll(0);}
    "#;
    let object=compile_source(source).unwrap();
    let dir=std::env::temp_dir().join(format!("minic-count-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("native.o"),object).unwrap();
    fs::write(dir.join("ref.c"),source.replace("count(","ref_count(").replace("__builtin_popref_count(","__builtin_popcount(").replace("once(","ref_once(").replace("zero(","ref_zero(")).unwrap();
    fs::write(dir.join("oracle.c"),r#"
      #include <assert.h>
      int count(unsigned int,unsigned long,unsigned long long),ref_count(unsigned int,unsigned long,unsigned long long);
      int once(unsigned long*),ref_once(unsigned long*),zero(void),ref_zero(void);
      int main(void){
        unsigned long x=1;
        for(unsigned i=0;i<4096;i++){
          x=x*6364136223846793005UL+1;
          assert(count((unsigned)x|1,x|1,(x>>1)|1)==ref_count((unsigned)x|1,x|1,(x>>1)|1));
          unsigned long a=x|1,b=a;
          assert(once(&a)==ref_once(&b) && a==b);
        }
        assert(zero()==ref_zero());
        return 0;
      }
    "#).unwrap();
    let link=Command::new("cc").current_dir(&dir).args(["-O2","ref.c","oracle.c","native.o","-o","oracle"]).output().unwrap();
    assert!(link.status.success(),"{}",String::from_utf8_lossy(&link.stderr));
    assert!(Command::new(dir.join("oracle")).status().unwrap().success());
    fs::remove_dir_all(dir).unwrap();
}
