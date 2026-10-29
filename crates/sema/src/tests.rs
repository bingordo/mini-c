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
fn indexing_checks_lvalue_and_pointee_arithmetic() {
    let items = check(
        "int use(int p[4], int i) { p[i] = 9; *(1 + p) = 7; return i[p]; }"
    ).unwrap();
    let CheckedItem::Function(f) = &items[0] else { panic!("function expected") };
    let CheckedStmt::Block(stmts) = &f.body else { panic!("block expected") };
    let CheckedStmt::Expr(Some(set)) = &stmts[0] else { panic!("assignment expected") };
    assert!(matches!(set.kind, CheckedExprKind::Assign(_, _)));
    check("int read(void) { int a[3]; int *p = a; return (p + 1)[1]; }").unwrap();
    check("int multidim(void) { int a[2][3]; int *row = a[1]; return row[0]; }").unwrap();
    reject("int x; int f(void) { return x[1]; }", "subscript requires pointer");
    reject("int f(int *p) { return p[p]; }", "integer offset");
    reject("int f(void *p) { return p[0]; }", "complete object type");
    reject("int f(int *p) { p[0] = p; return 0; }", "incompatible assignment");
    reject("int f(void) { int a[2][3]; a[0] = a[1]; return 0; }", "modifiable lvalue");
}

#[test]
fn array_types_and_parameter_adjustments() {
    let items = check("int a[3]; typedef int Matrix[2][4]; Matrix m; int use(int v[3]) { int *p = v; return *p; }").unwrap();
    let CheckedItem::Declaration(a) = &items[0] else { panic!("array declaration expected"); };
    assert_eq!(a.ty, CType::Array(Box::new(CType::int()), Some(3)));
    let CheckedItem::Typedef { ty: matrix, .. } = &items[1] else { panic!("typedef expected"); };
    let matrix_ty = CType::Array(Box::new(
        CType::Array(Box::new(CType::int()), Some(4)),
    ), Some(2));
    assert_eq!(*matrix, matrix_ty);
    let CheckedItem::Declaration(m) = &items[2] else { panic!("object expected"); };
    assert_eq!(m.ty, matrix_ty);
    let CheckedItem::Function(f) = &items[3] else { panic!("function expected"); };
    assert_eq!(f.signature, CType::Function {
        result: Box::new(CType::int()),
        params: vec![CType::Pointer(Box::new(CType::int()))],
    });
    assert_eq!(f.params[0].ty, CType::Pointer(Box::new(CType::int())));
    check("int from_ptr(int *a); int from_ptr(int a[9]) { return *a; }").unwrap();
    check("int copy(void) { int a[3]; int *p = a; int (*whole)[3] = &a; return 0; }").unwrap();
}

#[test]
fn invalid_array_types_and_assignments() {
    reject("int a[0];", "positive");
    reject("int a[-1];", "positive");
    reject("int a[1 / 0];", "array bound requires");
    reject("int n; int a[n];", "integer constant expression");
    reject("void a[2];", "array element requires");
    reject("int f(void) { int a[2]; a = a; return 0; }", "modifiable lvalue");
    reject("int f(void) { int a[2]; int b[2] = a; return 0; }", "incompatible assignment");
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
    // Pointer addition is now valid. Returning a pointer from an int
    // function still violates assignment compatibility.
    reject("int f(void) { int *p; return p + 1; }", "incompatible assignment");
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
    let inherited = check("static int file_var; int f(void) { extern int file_var; return file_var; }").unwrap();
    let CheckedItem::Function(f) = &inherited[1] else { panic!("function expected"); };
    let CheckedStmt::Block(stmts) = &f.body else { panic!("block expected"); };
    let CheckedStmt::Decl(local) = &stmts[0] else { panic!("extern declaration expected"); };
    assert_eq!(local.linkage, Linkage::Internal);
    reject("extern int n; static int n;", "conflicting linkage");
    check("int f(void) { static int n = 2 + 1; return n; }").unwrap();
    reject("int f(void) { static int n = f(); return n; }", "constant expression");
    reject("int f(void) { extern int n = 3; return n; }", "cannot have an initializer");
}

#[test]
fn unnamed_prototype_parameters_and_function_typedefs() {
    check("int zero(void); int f(int, void *); int f(int x, void *p) { return x + (p == 0); }").unwrap();
    check("typedef int Callback(int); Callback called; int called(int x) { return x; }").unwrap();
    reject("int bad(int) { return 0; }", "requires parameter names");
    reject("typedef int wrong(void) { return 0; }", "typedef cannot define a function");
    reject("int invalid(void x);", "parameter cannot have void");
}

#[test]
fn static_initializers_respect_target_arithmetic_and_short_circuit() {
    check("int x = 0 && (1 / 0); int y = 1 || (1 / 0); unsigned int u = 0 - 1;").unwrap();
    reject("int bad = 2147483647 + 1;", "signed integer overflow");
    reject("int bad = 1 / 0;", "integer constant expression");
    check("int global; int *pointer = &global; int *nil = 0;").unwrap();
    reject("int global; int *copy = &global; int *bad = copy;", "pointer initializer requires");
    reject("int f(void) { int auto_var; static int *bad = &auto_var; return 0; }", "pointer initializer requires");
}

#[test]
fn explicit_signed_char_overrides_target_plain_char() {
    let target = Target { char_is_signed: false, ..Target::default() };
    let mut checker = Checker::new(target).unwrap();
    let mut parsed = Parser::new("char plain; signed char signed_value; unsigned char unsigned_value;");
    let mut types = Vec::new();
    check_stream(&mut parsed, &mut checker, |item| {
        if let CheckedItem::Declaration(decl) = item { types.push(decl.ty); }
        Ok(())
    }).unwrap();
    assert_eq!(types, vec![
        CType::Integer { rank: IntRank::Char, unsigned: true },
        CType::Integer { rank: IntRank::Char, unsigned: false },
        CType::Integer { rank: IntRank::Char, unsigned: true },
    ]);
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

#[test]
fn diagnostic_collection_recovers_syntax_and_semantic_errors() {
    use mini_c_syntax::diag::DiagnosticCode;
    let source = concat!(
        "int syntax_bad = ;\n",
        "int good(void) { return 1; }\n",
        "int semantic_bad(void) { return not_declared; }\n",
        "int still_good(void) { return good(); }\n",
    );
    let mut parser = Parser::new(source);
    let mut checker = Checker::new(Target::default()).unwrap();
    let errors = diagnose_stream_recovering(&mut parser, &mut checker, 8);
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert_eq!(errors[0].code, DiagnosticCode::SyntaxExpectedExpression);
    assert_eq!(errors[1].code, DiagnosticCode::SemanticUndeclaredIdentifier);
    assert!(errors[0].span.lo < errors[1].span.lo);
    assert!(checker.global("syntax_bad").is_none());
    assert!(checker.global("semantic_bad").is_none());
    assert!(checker.global("good").is_some());
    assert!(checker.global("still_good").is_some());
}

#[test]
fn semantic_rollback_restores_preexisting_globals_and_nested_externs() {
    let source = concat!(
        "int x;\n",
        "int broken(void) { extern int future; return missing; }\n",
        "int after(void) { return x; }\n",
    );
    let mut parser = Parser::new(source);
    let mut checker = Checker::new(Target::default()).unwrap();
    let errors = diagnose_stream_recovering(&mut parser, &mut checker, 8);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].message.contains("undeclared identifier"));
    assert!(checker.global("broken").is_none());
    assert!(checker.global("future").is_none());
    assert!(checker.global("x").is_some());
    assert!(checker.global("after").is_some());
}

#[test]
fn semantic_rollback_does_not_mark_failed_global_definition_as_defined() {
    let source = "int value = unknown; int value = 3;";
    let mut parser = Parser::new(source);
    let mut checker = Checker::new(Target::default()).unwrap();
    let errors = diagnose_stream_recovering(&mut parser, &mut checker, 4);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].message.contains("undeclared identifier"));
    assert!(checker.global("value").is_some_and(|symbol| symbol.defined));
}

#[test]
fn semantic_error_limit_is_respected() {
    let mut parser = Parser::new("int a = missing; int b = also_missing; int good;");
    let mut checker = Checker::new(Target::default()).unwrap();
    let errors = diagnose_stream_recovering(&mut parser, &mut checker, 1);
    assert_eq!(errors.len(), 1);
    assert!(checker.global("a").is_none());
    assert!(checker.global("good").is_none());
}

#[test]
fn fine_grained_semantic_diagnostic_codes() {
    use mini_c_syntax::diag::DiagnosticCode;
    let cases = [
        (
            "int f(void) { return missing; }",
            DiagnosticCode::SemanticUndeclaredIdentifier, "T0105",
        ),
        (
            "int f(void) { int x; int x; return 0; }",
            DiagnosticCode::SemanticDuplicateDeclaration, "T0102",
        ),
        (
            "int x; long x;",
            DiagnosticCode::SemanticConflictingDeclaration, "T0103",
        ),
        (
            "int f(void) { return 1; } int f(void) { return 2; }",
            DiagnosticCode::SemanticMultipleDefinitions, "T0104",
        ),
        (
            "int f(void) { 1 = 2; return 0; }",
            DiagnosticCode::SemanticInvalidAssignmentTarget, "T0106",
        ),
        (
            "int f(int x) { return x; } int g(void) { return f(); }",
            DiagnosticCode::SemanticWrongArgumentCount, "T0107",
        ),
        (
            "int f(void) { int *p; p = 5; return 0; }",
            DiagnosticCode::SemanticIncompatibleAssignment, "T0108",
        ),
    ];
    for (source, expected, code) in cases {
        let error = check(source).unwrap_err();
        assert_eq!(error.code, expected, "{source:?}: {error:?}");
        assert_eq!(error.code.as_str(), code);
    }
    assert_eq!(
        mini_c_syntax::diag::Diagnostic::semantic(Default::default(), "other").code.as_str(),
        "T0001",
    );
}

#[test]
fn collects_two_independent_semantic_errors_in_one_function() {
    use mini_c_syntax::diag::DiagnosticCode;
    let source = concat!(
        "int fail(void) {\n",
        "  int good = 1;\n",
        "  int x = missing_x;\n",
        "  int y = missing_y;\n",
        "  return good;\n",
        "}\n",
        "int later(void) { return 0; }\n",
    );
    let mut parser = Parser::new(source);
    let mut checker = Checker::new(Target::default()).unwrap();
    let errors = diagnose_stream_recovering(&mut parser, &mut checker, 4);
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors.iter().all(|e|
        e.code == DiagnosticCode::SemanticUndeclaredIdentifier
    ));
    assert!(errors[0].message.contains("missing_x"), "{errors:?}");
    assert!(errors[1].message.contains("missing_y"), "{errors:?}");
    assert!(checker.global("fail").is_none());
    assert!(checker.global("later").is_some());
}

#[test]
fn failed_statement_does_not_declare_a_local_for_following_statements() {
    use mini_c_syntax::diag::DiagnosticCode;
    let source = "int broken(void) { int bad = unknown; return bad; } int fine;";
    let mut parser = Parser::new(source);
    let mut checker = Checker::new(Target::default()).unwrap();
    let errors = diagnose_stream_recovering(&mut parser, &mut checker, 4);
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors.iter().all(|e|
        e.code == DiagnosticCode::SemanticUndeclaredIdentifier
    ));
    assert!(checker.global("broken").is_none());
    assert!(checker.global("fine").is_some());
}

#[test]
fn function_recovery_preserves_outer_locals_and_respects_total_error_limit() {
    let source = concat!(
        "int f(void) { int ok = 1; int a = unknown; int b = missing; return ok; }\n",
        "int g(void) { return absent; }\n",
    );
    let mut parser = Parser::new(source);
    let mut checker = Checker::new(Target::default()).unwrap();
    let errors = diagnose_stream_recovering(&mut parser, &mut checker, 2);
    assert_eq!(errors.len(), 2);
    assert!(errors[0].message.contains("unknown"));
    assert!(errors[1].message.contains("missing"));
    assert!(checker.global("f").is_none());
    assert!(checker.global("g").is_none());
}

#[test]
fn no_extra_diagnostics_from_successful_functions() {
    let source = "int f(void) { int a = 2; return a; } int g(void) { return f(); }";
    let mut parser = Parser::new(source);
    let mut checker = Checker::new(Target::default()).unwrap();
    assert!(diagnose_stream_recovering(&mut parser, &mut checker, 5).is_empty());
    assert!(checker.global("f").is_some());
    assert!(checker.global("g").is_some());
}

#[test]
fn musl_isblank_with_original_c_character_literals_typechecks(){
    let source="int isblank(int c)\n{\n\treturn (c == ' ' || c == '\\t');\n}\n";
    let checked=check(source).unwrap();
    assert_eq!(checked.len(),1);
    let CheckedItem::Function(function)=&checked[0] else {panic!("function expected")};
    assert_eq!(function.name,"isblank");
    assert_eq!(function.signature,CType::Function {
        result:Box::new(CType::int()),params:vec![CType::int()]
    });
}

#[test]
fn global_array_scalar_initializer_is_rejected_until_aggregate_support() {
    reject("int a[3] = 1;", "array initializer requires aggregate");
    reject("int a[3] = 0;", "array initializer requires aggregate");
    // An uninitialized file-scope array remains a valid tentative definition.
    check("int a[3]; extern int a[3];").unwrap();
    // Scalar constant initializers continue to work.
    check("int n = 3; unsigned long m = 7;").unwrap();
}
