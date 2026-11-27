//! Exact checked C prototype admission for a source musttail return.
//! Register capacity remains target ABI-owned; syntax never guesses it.
use mini_c_sema::{Checker,Target,CheckedItem,CheckedStmt,CheckedExprKind,check_stream};
use mini_c_syntax::parser::Parser;

fn check(src:&str)->Result<Vec<CheckedItem>,String>{
    let mut parser=Parser::new(src);
    let mut checker=Checker::new(Target::default()).map_err(|e|e.message)?;
    let mut out=Vec::new();
    check_stream(&mut parser,&mut checker,|item|{
        out.push(item);
        Ok(())
    }).map_err(|e|e.message)?;
    Ok(out)
}

#[test]
fn preserves_checked_required_scalar_and_void_calls() {
    let examples=[
        "long rec(long x){ [[clang::musttail]] return rec(x); }",
        "int rec(int x){ [[clang::musttail]] return rec(x); }",
        "void consume(int x); void wrapper(int x){ [[clang::musttail]] return consume(x); }",
        "int add(int a,int b); int rev(int a,int b){ [[clang::musttail]] return add(b,a); }",
        "int (*callback)(int); int wrapper(int x){ [[clang::musttail]] return callback(x); }",
        "int add(int); int wrapper(int x){ int (*fn)(int)=add; [[clang::musttail]] return (*fn)(x); }",
        "void (*sink)(long); void wrapper(long x){ [[clang::musttail]] return sink(x); }",
    ];
    for source in examples {
        let items=check(source).unwrap_or_else(|e|panic!("{source}: {e}"));
        let CheckedItem::Function(f)=items.last().unwrap() else{panic!("function expected")};
        let CheckedStmt::Block(stmts)=&f.body else {panic!("block expected")};
        assert!(stmts.iter().any(|stmt|matches!(stmt,
            CheckedStmt::MustTailReturn(e)
                if matches!(e.kind,CheckedExprKind::Call(..)))), "{source}");
    }
}

#[test]
fn refuses_mismatched_prototypes_and_noncall_returns() {
    let cases=[
        "long f(long x); long g(int x){ [[clang::musttail]] return f(x); }",
        "int f(int x); long g(long x){ [[clang::musttail]] return f(x); }",
        "int f(int x){ [[clang::musttail]] return x; }",
        "int f(int x){ [[clang::musttail]] return 1+f(x); }",
        "int f(int x){ [[clang::musttail]] return (int)f(x); }",
        "int f(int x){ [[clang::musttail]] return x?f(x):0; }",
        "float f(float x){ [[clang::musttail]] return f(x); }",
        "short f(short x){ [[clang::musttail]] return f(x); }",
        // The callee is callable, but its one-parameter prototype differs
        // from the caller's two-parameter prototype.
        "int f(int (*p)(int),int x){ [[clang::musttail]] return p(x); }",
        "long (*callback)(long); int wrapper(int x){ [[clang::musttail]] return callback(x); }",
        "int (*callback)(int,int); int wrapper(int x){ [[clang::musttail]] return callback(x,x); }",
        "int (*callback)(int,...); int wrapper(int x){ [[clang::musttail]] return callback(x); }",
    ];
    for source in cases {
        let err=check(source).expect_err(source);
        assert!(err.contains("musttail"),"{source}: {err}");
    }
}

#[test]
fn ordinary_returns_do_not_inherit_required_tail_semantics() {
    let items=check("int f(int x){return f(x);}").unwrap();
    let CheckedItem::Function(f)=&items[0] else {panic!("function")};
    let CheckedStmt::Block(stmts)=&f.body else {panic!("block")};
    assert!(matches!(&stmts[0],CheckedStmt::Return(Some(_))));
}
