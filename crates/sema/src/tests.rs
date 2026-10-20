use super::*;
use mini_c_syntax::parser::Parser;

fn check(src: &str) -> Result<Vec<CheckedItem>, Diagnostic> {
    let mut parser = Parser::new(src);
    let mut checker = Checker::new(Target::default()).unwrap();
    let mut items = Vec::new();
    check_stream(&mut parser, &mut checker, |item| { items.push(item); Ok(()) })?;
    Ok(items)
}

fn reject(src: &str, message: &str) {
    match check(src) {
        Ok(_) => panic!("expected semantic failure containing '{message}' for: {src}"),
        Err(error) => assert!(error.message.contains(message),
            "expected '{message}', got {:?} for {src}", error),
    }
}

#[test]
fn typed_parameters_arithmetic_and_returns() {
    let result = check("int twice(int x) { int y = x * 2; return y; }").unwrap();
    let CheckedItem::Function(f) = &result[0] else { panic!("expected function"); };
    assert_eq!(f.signature, CType::Function { result: Box::new(CType::int()), params: vec![CType::int()] });
    let CheckedStmt::Block(body) = &f.body else { panic!("expected body"); };
    let CheckedStmt::Decl(d) = &body[0] else { panic!("expected local"); };
    assert_eq!(d.linkage, Linkage::None);
    assert_eq!(d.ty, CType::int());
    assert!(matches!(body[1], CheckedStmt::Return(Some(_))));
}

#[test]
fn recursive_function_and_prototype() {
    let result = check("int f(int n); int f(int n) { if (n) return f(n - 1); return 0; }").unwrap();
    assert!(matches!(result[0], CheckedItem::Declaration(_)));
    assert!(matches!(result[1], CheckedItem::Function(_)));
    reject("int f(int x) { return x; } int f(int y) { return y; }", "multiple definitions");
    reject("int f(int x); long f(int x) { return x; }", "conflicting declaration");
    reject("int f(int x); int f(int x, int y) { return x; }", "conflicting declaration");
}

#[test]
fn scoping_and_shadowing() {
    check("int x = 2; int f(int x) { { long x = 4; x = x + 1; } return x; }").unwrap();
    reject("int f(int x) { int x; return x; }", "duplicate declaration");
    reject("int f(void) { int x; int x; return 0; }", "duplicate declaration");
    reject("int f(void) { { int t = 2; } return t; }", "undeclared identifier");
    reject("int f(void) { return mystery; }", "undeclared identifier");
}

#[test]
fn return_and_control_flow_legality() {
    check("void run(void) { int x = 2; while (x) { x = x - 1; if (x) continue; break; } return; }").unwrap();
    reject("void f(void) { return 3; }", "void function cannot return a value");
    reject("int f(void) { return; }", "non-void function must return a value");
    reject("int f(void) { break; return 0; }", "break outside loop");
    reject("int f(void) { continue; return 0; }", "continue outside loop");
}

#[test]
fn calls_and_explicit_conversions() {
    let result = check("long twice(long a) { return a + a; } int f(void) { return twice(3); }").unwrap();
    let CheckedItem::Function(f) = &result[1] else { panic!("function expected"); };
    let CheckedStmt::Block(body) = &f.body else { panic!("body expected"); };
    let CheckedStmt::Return(Some(expr)) = &body[0] else { panic!("return expected"); };
    assert_eq!(expr.ty, CType::int());
    assert!(matches!(expr.kind, CheckedExprKind::Convert(_)));
    reject("int f(int x) { return f(); }", "expects 1 arguments");
    reject("int f(void) { int x; return x(); }", "not a function");
    reject("int f(void) { return 3(2); }", "not a function");
}

#[test]
fn lvalue_and_pointer_constraints() {
    check("int f(int *p) { return *p; } int x; int *q = 0;").unwrap();
    reject("int f(void) { 1 = 2; return 0; }", "modifiable lvalue");
    reject("int f(void) { int x; (x + 1) = 2; return x; }", "modifiable lvalue");
    reject("int f(void) { int x; return *x; }", "requires a pointer");
    reject("int f(void) { int *p; p = 5; return 0; }", "incompatible assignment");
    reject("int f(void) { int *p; return p + 1; }", "integer operands");
}

#[test]
fn global_constant_initializers() {
    check("int x = 2 + 3 * 4; int y; int y; int f(void) { return x; }").unwrap();
    reject("int f(void) { return 1; } int x = f();", "constant expression");
    reject("int x = 2 / 0;", "constant expression");
    reject("int x = 1; int x = 2;", "multiple definitions");
    reject("void x;", "object cannot have void");
}

#[test]
fn typedef_resolution_and_use() {
    let result = check("typedef long Size; Size s; int f(void) { Size local = 3; return local; }").unwrap();
    assert!(matches!(&result[0], CheckedItem::Typedef { ty: CType::Integer { rank: IntRank::Long, unsigned: false }, .. }));
    let CheckedItem::Declaration(s) = &result[1] else { panic!("declaration expected"); };
    assert_eq!(s.ty, CType::Integer { rank: IntRank::Long, unsigned: false });
    reject("typedef int T; int T;", "conflicting declaration");
    reject("typedef int T; int f(void) { return T; }", "typedef name");
}

#[test]
fn unsigned_integer_model_and_promotions() {
    let items = check("unsigned char c = 1; unsigned int u = 2; unsigned long huge = 3; unsigned int f(unsigned char c, unsigned long l) { unsigned int x = c + u; return x + l; }").unwrap();
    let CheckedItem::Declaration(c) = &items[0] else { panic!("declaration expected"); };
    assert_eq!(c.ty, CType::Integer { rank: IntRank::Char, unsigned: true });
    let CheckedItem::Declaration(u) = &items[1] else { panic!("declaration expected"); };
    assert_eq!(u.ty, CType::Integer { rank: IntRank::Int, unsigned: true });
    let CheckedItem::Declaration(l) = &items[2] else { panic!("declaration expected"); };
    assert_eq!(l.ty, CType::Integer { rank: IntRank::Long, unsigned: true });
    check("typedef unsigned long Index; Index value; int f(void) { Index index = 3; return index; }").unwrap();
}

#[test]
fn local_typedef_scope_isolation() {
    check("typedef int T; int f(void) { typedef long T; T x = 4; { int T = 2; T = T + 1; } return x; } T g;").unwrap();
    reject("int f(void) { typedef int T; return T; }", "typedef name");
    reject("int f(void) { typedef int T; typedef long T; return 0; }", "duplicate declaration");
    check("int f(void) { typedef void Empty; return 0; }").unwrap();
}

#[test]
fn static_and_extern_linkage() {
    let items = check("static int counter = 3; extern int counter; static int helper(void) { static int x = 2; return counter + x; }").unwrap();
    let CheckedItem::Declaration(counter) = &items[0] else { panic!("declaration expected"); };
    assert_eq!(counter.linkage, Linkage::Internal);
    let CheckedItem::Function(helper) = &items[2] else { panic!("function expected"); };
    assert_eq!(helper.linkage, Linkage::Internal);
    check("int f(void) { extern int external; return external; } extern int external;").unwrap();
    reject("extern int n; static int n;", "conflicting linkage");
    check("int f(void) { static int n = 2 + 1; return n; }").unwrap();
    reject("int f(void) { static int n = f(); return n; }", "constant expression");
    reject("int f(void) { extern int n = 3; return n; }", "cannot have an initializer");
}

#[test]
fn checker_discards_completed_bodies() {
    let mut source = String::from("int shared;");
    for n in 0..200 {
        source.push_str(&format!("int f{n}(int x) {{ return x + shared; }}\n"));
    }
    let mut checker = Checker::new(Target::default()).unwrap();
    let mut count = 0;
    check_stream(&mut Parser::new(&source), &mut checker, |item| {
        if matches!(item, CheckedItem::Function(_)) { count += 1; }
        Ok(())
    }).unwrap();
    assert_eq!(count, 200);
    assert_eq!(checker.globals.len(), 201);
    assert!(checker.locals.is_empty());
}
