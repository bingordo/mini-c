//! P11 parser regressions: comma-delimited GNU output operands are syntax,
//! independent of target-owned machine and ABI admission policies.

use mini_c_syntax::{parser::Parser,stream::{ItemSource,ExternalItem},ast::Stmt};

#[test]
fn parses_named_mixed_gp_memory_outputs_in_both_orders() {
    for (src,first,second) in [
        (r#"int f(int *p,int x){int y;asm volatile("movl %[in],%[mem]; movl %[mem],%[out]":[out]"=&r"(y),[mem]"=m"(*p):[in]"r"(x):"cc","memory");return y;}"#,"out","mem"),
        (r#"int f(int *p,int x){int y;asm volatile("addl %[in],%[mem];movl %[mem],%[out]":[mem]"+m"(*p),[out]"=r"(y):[in]"r"(x):"cc");return y;}"#,"mem","out"),
    ] {
        let Some(ExternalItem::Function(function))=Parser::new(src).next_item().unwrap()
        else {panic!("expected a C function")};
        let Stmt::Block(stmts)=function.body else {panic!("expected function body")};
        let Some(Stmt::ExtendedAsm{outputs,inputs,template,..})=
            stmts.iter().find(|item|matches!(item,Stmt::ExtendedAsm{..}))
        else {panic!("expected extended asm")};
        assert_eq!(outputs.len(),2);
        assert_eq!(outputs[0].name.as_deref(),Some(first));
        assert_eq!(outputs[1].name.as_deref(),Some(second));
        assert_eq!(inputs.len(),1);
        assert_eq!(inputs[0].name.as_deref(),Some("in"));
        assert!(template.contains("%[mem]"));
    }
}

#[test]
fn rejects_malformed_mixed_output_delimiters() {
    for src in [
        r#"int f(void){int x,y;asm("nop":"=r"(x),:"r"(y));}"#,
        r#"int f(void){int x,y;asm("nop":"=r"(x),,: "r"(y));}"#,
        r#"int f(void){int x,y;asm("nop":"=r"(x),"=m"(y),:"r"(x));}"#,
        r#"int f(void){int x,y;asm("nop":"=r"(x) "m"(y):"r"(x));}"#,
    ] {
        assert!(Parser::new(src).next_item().is_err(),
            "malformed GNU asm output delimiter accepted: {src}");
    }
}
