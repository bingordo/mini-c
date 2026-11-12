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
fn c99_pointer_difference_is_signed_element_count_with_strict_pointee_constraints() {
    let items = check("long difference(int *p, const int *q) { return p-q; }")
        .expect("qualified compatible pointer subtraction");
    let CheckedItem::Function(f) = &items[0] else { panic!("function expected") };
    let CheckedStmt::Block(body) = &f.body else { panic!("body expected") };
    let CheckedStmt::Return(Some(value)) = &body[0] else { panic!("return expected") };
    let ptrdiff = CType::Integer { rank: IntRank::Long, unsigned: false };
    assert_eq!(value.ty, ptrdiff);
    assert!(matches!(&value.kind, CheckedExprKind::Binary(BinOp::Sub, _, _)));

    check("long distance(void) { short a[8]; return (&a[6] - &a[1]); }").unwrap();
    check("struct Pair { int x; long y; }; long f(struct Pair *p, struct Pair *q) { return p-q; }").unwrap();
    check("long f(char *p, char *q) { return p-q; }").unwrap();

    reject("long f(int *p, char *q) { return p-q; }", "compatible pointed-to types");
    reject("long f(void *p, void *q) { return p-q; }", "complete object type");
    reject("struct Missing; long f(struct Missing *p, struct Missing *q) { return p-q; }",
        "incomplete struct");
    reject("int id(int x) { return x; } long f(void) { return &id - &id; }",
        "complete object type");
    reject("long f(int *p, int *q) { return p+q; }", "compatible integer operands");
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
fn c99_function_scope_goto_labels_and_constraints() {
    let items = check("int f(int x) { goto done; x = 99; { done: return x; } }")
        .expect("forward goto and nested label");
    let CheckedItem::Function(function) = &items[0] else { panic!("expected function") };
    let CheckedStmt::Block(stmts) = &function.body else { panic!("expected body") };
    assert!(matches!(&stmts[0], CheckedStmt::Goto { target } if target == "done"));
    assert!(matches!(&stmts[2], CheckedStmt::Block(nested)
        if matches!(&nested[0], CheckedStmt::Label { name, .. } if name == "done")));

    // C has separate label and typedef namespaces.
    check("typedef int T; int f(void) { goto T; { T: return 1; } }").unwrap();
    check("int f(void) { again: goto again; } int g(void) { again: return 0; }").unwrap();
    reject("int f(void) { goto missing; return 0; }", "undefined label");
    reject("int f(void) { x: ; { x: ; } return 0; }", "duplicate label");
    reject("int f(void) { goto x; } int g(void) { x: return 0; }", "undefined label");
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
    check("int *global_ptr = (void*)0; void *global_void = (void*)0;").unwrap();
    check("typedef int *Ptr; Ptr global_alias = (Ptr)0; int **global_nested=(int**)0;")
        .unwrap();
    check("int *p=(int*)(unsigned long)0;").unwrap();
    reject("int *bad=(int*)1;", "pointer cast requires a pointer or null integer constant");
    reject("int global; int *copy = &global; int *bad2 = (int*)copy;",
        "pointer initializer requires");
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


#[test]
fn narrow_string_is_char_array_and_decays_to_pointer() {
    let items = check(r#"char *global="ab"; int main(void) {
        char *local="xy"; return local[1]-'y'+global[0]-'a';
    }"#).unwrap();
    let CheckedItem::Declaration(global) = &items[0] else { panic!("global expected") };
    let Some(init) = &global.initializer else { panic!("initializer expected") };
    assert_eq!(init.ty, CType::Pointer(Box::new(CType::Integer {
        rank: IntRank::Char, unsigned: false,
    })));
    let CheckedExprKind::ArrayDecay(string) = &init.kind else {
        panic!("string initializer must be array decay: {:?}", init.kind);
    };
    assert_eq!(string.ty, CType::Array(Box::new(CType::Integer {
        rank: IntRank::Char, unsigned: false,
    }), Some(3)));
    assert_eq!(string.category, Category::LValue);
    assert!(matches!(&string.kind, CheckedExprKind::StringLiteral(bytes) if bytes.as_slice() == &b"ab\0"[..]));
    reject(r#"int *p="invalid";"#, "pointer initializer requires");
    check(r#"char a[4]="abc";"#).unwrap();
    reject(r#"char a[2]="abc";"#, "string literal is too long");
    reject(r#"int a[4]="abc";"#, "array initializer requires");
}

#[test]
fn character_array_string_bounds_and_initializers() {
    let items = check(r#"char inferred[]="abc"; unsigned char padded[7]="hi";
        char no_nul[3]="abc"; int main(void) {
            char local[]="xy"; char exact[2]="xy"; return local[2]+exact[1]-'y';
        }"#).unwrap();
    let CheckedItem::Declaration(inferred) = &items[0] else { panic!("expected global"); };
    assert_eq!(inferred.ty, CType::Array(Box::new(CType::Integer {
        rank: IntRank::Char, unsigned: false,
    }), Some(4)));
    assert!(matches!(&inferred.initializer, Some(CheckedExpr {
        kind: CheckedExprKind::StringLiteral(bytes), ..
    }) if bytes.as_slice() == b"abc\0"));
    let CheckedItem::Declaration(padded) = &items[1] else { panic!("expected global"); };
    assert_eq!(padded.ty, CType::Array(Box::new(CType::Integer {
        rank: IntRank::Char, unsigned: true,
    }), Some(7)));
    let CheckedItem::Declaration(no_nul) = &items[2] else { panic!("expected global"); };
    assert!(matches!(no_nul.ty, CType::Array(_, Some(3))));
    let CheckedItem::Function(main) = &items[3] else { panic!("expected function"); };
    let CheckedStmt::Block(statements) = &main.body else { panic!("expected block"); };
    let CheckedStmt::Decl(local) = &statements[0] else { panic!("expected local"); };
    assert_eq!(local.ty, CType::Array(Box::new(CType::Integer {
        rank: IntRank::Char, unsigned: false,
    }), Some(3)));
    let CheckedStmt::Decl(exact) = &statements[1] else { panic!("expected local"); };
    assert!(matches!(exact.ty, CType::Array(_, Some(2))));
    reject(r#"char too_short[2]="abc";"#, "string literal is too long");
    reject(r#"int wrong_type[3]="ab";"#, "array initializer requires");
    reject(r#"int f(void) { char too_short[1]="abc"; return 0; }"#, "string literal is too long");
}

#[test]
fn global_array_redeclarations_preserve_completed_bounds() {
    let items = check(r#"extern char a[]; char a[]="abc"; extern char a[];"#).unwrap();
    for item in &items[1..] {
        let CheckedItem::Declaration(decl) = item else { panic!("expected declaration") };
        assert!(matches!(decl.ty, CType::Array(_, Some(4))));
    }
    check(r#"char a[]="abc"; extern char a[];"#).unwrap();
    check(r#"extern char a[]; char a[8]="abc";"#).unwrap();
    check(r#"char a[4]; char a[]="abc";"#).unwrap();
    reject(r#"extern char a[5]; char a[]="abc";"#, "conflicting declaration");
    reject(r#"extern int a[]; char a[]="abc";"#, "conflicting declaration");
}


#[test]
fn inline_struct_member_types_and_layout() {
    let items = check("struct Pair { char first; int middle; long last; } value;").unwrap();
    let CheckedItem::Declaration(value) = &items[0] else { panic!("struct global expected") };
    let CType::Struct { tag, members, size, align, .. } = &value.ty else { panic!("struct expected") };
    assert_eq!(tag.as_deref(), Some("Pair"));
    assert_eq!((*size, *align), (16, 8));
    assert_eq!(members.iter().map(|m| m.offset).collect::<Vec<_>>(), [0, 4, 8]);
    let checked = check("int main(void) { struct { char c; long x; } a; a.c=3; (&a)->x=19; return a.c + a.x; }").unwrap();
    let CheckedItem::Function(f) = &checked[0] else { panic!("function expected") };
    let CheckedStmt::Block(stmts) = &f.body else { panic!("block expected") };
    let CheckedStmt::Expr(Some(set)) = &stmts[2] else { panic!("assignment expected") };
    assert!(matches!(set.kind, CheckedExprKind::Assign(..)));
}

#[test]
fn inline_struct_member_errors() {
    reject("struct X { int a; int a; } x;", "duplicate structure member");
    reject("struct X { void a; } x;", "incomplete");
    reject("struct X { int a[]; } x;", "incomplete");
    reject("int main(void) { int n; return n.foo; }", "requires a struct");
    reject("int main(void) { struct { int a; } s; return s.missing; }",
        "unknown struct member");
    reject("int main(void) { struct { int a; } s; return s->a; }",
        "pointer to struct");
    reject("int main(void) { struct { int a; } s; return s.a[0]; }",
        "subscript requires pointer");
}

#[test]
fn structure_assignment_respects_type_identity() {
    check("int main(void) { struct { int x; long y; } a[2]; a[0].x=17; a[1]=a[0]; return a[1].x; }").unwrap();
    reject("int main(void) { struct { int x; } a; struct { int x; } b; a=b; return 0; }",
        "incompatible assignment");
}

#[test]
fn aggregate_initializer_bounds_and_types() {
    let items = check("int a[]={1,2,3}; struct { char a; long b; } g={4,5};").unwrap();
    let CheckedItem::Declaration(array) = &items[0] else { panic!("array expected") };
    assert_eq!(array.ty, CType::Array(Box::new(CType::int()), Some(3)));
    assert!(matches!(&array.initializer, Some(CheckedExpr {
        kind: CheckedExprKind::Initializer(values), ..
    }) if values.len() == 3));
    let CheckedItem::Declaration(aggregate) = &items[1] else { panic!("struct expected") };
    assert!(matches!(&aggregate.initializer, Some(CheckedExpr {
        kind: CheckedExprKind::Initializer(values), ..
    }) if values.len() == 2));
    check("int main(void) { int grid[2][3]={{1,2,3},{4,5,6}}; return grid[1][2]; }").unwrap();
    reject("int a[2]={1,2,3};", "too many array initializers");
    reject("struct {int x;} v={1,2};", "too many struct initializers");
    reject("int a[2]={bad,1};", "undeclared");
    reject("int main(void) { int x={1,2}; return x; }", "scalar initializer");
}

#[test]
fn union_members_layout_and_first_field_init() {
    let checked = check("union Word { char c; long l; } value;").unwrap();
    let CheckedItem::Declaration(decl) = &checked[0] else { panic!("global expected") };
    let CType::Union { members, size, align, .. } = &decl.ty else { panic!("union expected") };
    assert_eq!((*size,*align), (8,8));
    assert!(members.iter().all(|m| m.offset == 0));
    check("int main(void) { union { int i; long l; } u={7}; u.i=13; return u.i; }").unwrap();
    check("int main(void) { union { int i; long l; } a[2]; a[0].i=17; a[1]=a[0]; return a[1].i; }").unwrap();
    reject("union { int i; long l; } u={1,2};", "union initializer selects only its first member");
    reject("union { int i; int i; } u;", "duplicate union member");
}

#[test]
fn named_tags_forward_declarations_and_recursive_fields() {
    let src = "struct Node; struct Node { int value; struct Node *next; };         struct Node root; int main(void) { struct Node *p=&root; root.value=17;         root.next=p; return root.next->value-17; }";
    let items = check(src).unwrap();
    let CheckedItem::Declaration(root) = &items[2] else { panic!("root declaration expected") };
    let CType::Struct { members, size, .. } = &root.ty else { panic!("resolved struct expected") };
    assert_eq!(*size, 16);
    assert_eq!(members.iter().map(|m| m.offset).collect::<Vec<_>>(), [0,8]);
    check("typedef struct Record Record; struct Record { long count; }; Record record;").unwrap();
    check("union Data; union Data { int x; long y; }; union Data v;").unwrap();
    reject("struct T; union T { int a; };", "conflicting tag kind");
    reject("struct T { int a; }; struct T { long b; };", "redefinition");
}

#[test]
fn named_tags_have_separate_block_scopes() {
    check("struct T { int a; }; int f(void) {         struct T *outer=0; { struct T; struct T { long b; };         struct T inner; inner.b=17; } return 0; }").unwrap();
    reject("struct T; int f(void) { struct T value; return 0; }",
        "incomplete");
}

#[test]
fn enumerators_constant_expressions_and_scope() {
    let items=check("enum Kind { First, Second=7, Third=Second+2, Negative=-1 }; enum Kind value=Third;").unwrap();
    assert!(matches!(items[0], CheckedItem::Tag));
    let CheckedItem::Declaration(value) = &items[1] else { panic!("enum variable expected") };
    assert_eq!(value.ty, CType::int());
    assert!(matches!(&value.initializer, Some(CheckedExpr { kind: CheckedExprKind::EnumConstant(9), .. })));
    check("int main(void) { enum Local { A=-1, B, C=B+4 }; enum Local x=C; return x-A-5; }").unwrap();
    reject("enum Bad { A=2147483648 };", "exceeds supported signed int range");
    reject("enum Bad { A, A };", "multiple definitions");
    reject("int Same; enum Bad { Same };", "conflicting declaration");
    reject("struct Tag; enum Tag { E };", "conflicting tag kind");
}

#[test]
fn enum_tag_reference_uses_scalar_int_type() {
    let items = check("enum Status { A=3 }; enum Status inc(enum Status x) { return x+1; }").unwrap();
    let CheckedItem::Function(f) = &items[1] else { panic!("function expected") };
    assert_eq!(f.signature, CType::Function {
        result: Box::new(CType::int()), params: vec![CType::int()],
    });
}

#[test]
fn outer_aggregate_reference_remains_valid_under_tag_shadowing() {
    check("struct Node { int value; }; int f(void) { struct Node *p; { struct Node { long other; }; return p->value; } }").unwrap();
    check("struct Node { int value; }; int f(void) { struct Node *p; { union Node { long other; }; return p->value; } }").unwrap();
}

#[test]
fn tag_completion_rolls_back_after_failed_function() {
    let mut parser = Parser::new(
        "int bad(void) { struct Temp { int x; }; return missing; }");
    let mut checker = Checker::new(Target::default()).unwrap();
    let item = parser.next_item().unwrap().unwrap();
    assert!(checker.check_item_recovering(item).is_err());
    assert!(checker.tag_definitions.borrow().is_empty());
    assert_eq!(checker.tags.borrow().len(), 1);
}

#[test]
fn streaming_drops_local_tag_layouts() {
    let mut checker = Checker::new(Target::default()).unwrap();
    check_stream(&mut Parser::new("struct Global { int x; };"), &mut checker, |_| Ok(())).unwrap();
    assert_eq!(checker.tag_definitions.borrow().len(), 1);
    for i in 0..100 {
        let src = format!("int f{i}(void) {{ struct Local {{ int x; }} a; return 0; }}");
        check_stream(&mut Parser::new(&src), &mut checker, |_| Ok(())).unwrap();
        assert_eq!(checker.tag_definitions.borrow().len(), 1);
    }
}

#[test]
fn function_designators_decay_in_initializer_and_assignment_contexts() {
    check("int identity(int x) { return x; } \
           int (*global)(int) = identity; \
           int (*table[1])(int) = {identity}; \
           int main(void) { int (*local)(int) = identity; \
               local = identity; return local != 0; }").unwrap();
    reject("int identity(int x) { return x; } int *wrong = identity;",
        "pointer initializer requires");
}


#[test]
fn nested_block_recovery_collects_all_independent_errors_in_source_order() {
    use mini_c_syntax::diag::DiagnosticCode;
    let source = concat!(
        "int broken(void) {\n",
        "  if (1) {\n",
        "    int x = missing_one;\n",
        "    int y = missing_two;\n",
        "  }\n",
        "  int z = missing_three;\n",
        "  return 0;\n",
        "}\n",
        "int afterward(void) { return 7; }\n",
    );
    let mut checker = Checker::new(Target::default()).unwrap();
    let mut parser = Parser::new(source);
    let errors = diagnose_stream_recovering(&mut parser, &mut checker, 8);
    assert_eq!(errors.len(), 3, "{errors:?}");
    for (error, name) in errors.iter().zip([
        "missing_one", "missing_two", "missing_three",
    ]) {
        assert_eq!(error.code, DiagnosticCode::SemanticUndeclaredIdentifier);
        assert!(error.message.contains(name), "{error:?}");
    }
    assert!(checker.global("broken").is_none());
    assert!(checker.global("afterward").is_some());
}

#[test]
fn nested_loop_and_if_recovery_respect_the_shared_diagnostic_limit() {
    let source = concat!(
        "int broken(void) {\n",
        "  int count = 2;\n",
        "  while (count) {\n",
        "    if (1) {\n",
        "      int x = first_missing;\n",
        "      int y = second_missing;\n",
        "    }\n",
        "    int z = third_missing;\n",
        "    count = count - 1;\n",
        "  }\n",
        "  return count;\n",
        "}\n",
        "int after(void) { return 0; }\n",
    );
    let mut checker = Checker::new(Target::default()).unwrap();
    let mut parser = Parser::new(source);
    let errors = diagnose_stream_recovering(&mut parser, &mut checker, 2);
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors[0].message.contains("first_missing"), "{errors:?}");
    assert!(errors[1].message.contains("second_missing"), "{errors:?}");
    assert!(checker.global("broken").is_none());
    // The limit stops parsing before the next external declaration.
    assert!(checker.global("after").is_none());
}

#[test]
fn nested_recovery_does_not_leak_externs_or_tag_definitions() {
    let source = concat!(
        "int broken(void) {\n",
        "  if (1) {\n",
        "    extern int temporary;\n",
        "    struct Hidden { int member; } value;\n",
        "    int bad = unknown;\n",
        "  }\n",
        "  return 0;\n",
        "}\n",
        "int good(void) { return 1; }\n",
    );
    let mut checker = Checker::new(Target::default()).unwrap();
    let mut parser = Parser::new(source);
    let errors = diagnose_stream_recovering(&mut parser, &mut checker, 5);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(checker.global("broken").is_none());
    assert!(checker.global("temporary").is_none());
    assert!(checker.global("good").is_some());
    assert!(checker.tag_definitions.borrow().is_empty());
    assert_eq!(checker.tags.borrow().len(), 1);
}

#[test]
fn ordinary_check_stream_still_fails_fast_on_nested_semantic_errors() {
    let source = concat!(
        "int bad(void) { if (1) { int a=missing_a; int b=missing_b; } return 0; }\n",
        "int later(void) { return 1; }\n",
    );
    let mut checker = Checker::new(Target::default()).unwrap();
    let mut parser = Parser::new(source);
    let error = check_stream(&mut parser, &mut checker, |_| Ok(())).unwrap_err();
    assert!(error.message.contains("missing_a"), "{error:?}");
    assert!(checker.global("later").is_none());
}

#[test]
fn indirect_calls_accept_function_pointers_and_explicit_function_dereference() {
    check("int add(int x) { return x+1; } \
           int (*global)(int) = add; \
           struct Hooks { int (*invoke)(int); }; \
           struct Hooks hooks = {add}; \
           int main(void) { int (*local)(int) = add; \
               return local(1) + global(2) + hooks.invoke(3) + (*add)(4); }").unwrap();
    reject("int add(int x) { return x; } \
            int main(void) { int (*fp)(int)=add; return fp(); }",
        "expects 1 arguments");
}


#[test]
fn located_parser_reports_precise_undeclared_identifier_byte_span() {
    use mini_c_syntax::diag::DiagnosticCode;
    let source = "/* β */\nint f(void) { int valid=1; return valid + missing; }";
    let mut parser = Parser::new_with_spans(source);
    let mut checker = Checker::new(Target::default()).unwrap();
    let errors = diagnose_stream_recovering(&mut parser, &mut checker, 4);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, DiagnosticCode::SemanticUndeclaredIdentifier);
    let lo = source.find("missing").unwrap();
    assert_eq!(errors[0].span, mini_c_syntax::ast::Span { lo, hi: lo + 7 });
}

#[test]
fn located_parser_points_to_specific_bad_operand_not_whole_function() {
    use mini_c_syntax::diag::DiagnosticCode;
    let source = "int f(void) { int value=1; value=2; return value + missing; }";
    let mut parser = Parser::new_with_spans(source);
    let mut checker = Checker::new(Target::default()).unwrap();
    let error = diagnose_stream_recovering(&mut parser, &mut checker, 2)
        .into_iter().next().expect("semantic error");
    assert_eq!(error.code, DiagnosticCode::SemanticUndeclaredIdentifier);
    let lo = source.find("missing").unwrap();
    assert_eq!(error.span, mini_c_syntax::ast::Span { lo, hi: lo + 7 });
}

#[test]
fn located_parser_marks_invalid_assignment_lhs() {
    use mini_c_syntax::diag::DiagnosticCode;
    let source = "int f(void) { int x=0; 1 = x; return 0; }";
    let mut parser = Parser::new_with_spans(source);
    let mut checker = Checker::new(Target::default()).unwrap();
    let errors = diagnose_stream_recovering(&mut parser, &mut checker, 2);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, DiagnosticCode::SemanticInvalidAssignmentTarget);
    let lo = source.find("1 = x").unwrap();
    assert_eq!(errors[0].span, mini_c_syntax::ast::Span { lo, hi: lo + 1 });
}

#[test]
fn located_parser_preserves_array_string_and_function_pointer_initializers() {
    let source = concat!(
        "int items[] = {1,2,3};\n",
        "char text[] = \"hi\";\n",
        "int identity(int x) { return x; }\n",
        "int (*callback)(int) = identity;\n",
        "int f(void) { return items[1]; }\n",
    );
    let mut parser = Parser::new_with_spans(source);
    let mut checker = Checker::new(Target::default()).unwrap();
    let errors = diagnose_stream_recovering(&mut parser, &mut checker, 4);
    assert!(errors.is_empty(), "{errors:?}");
    assert!(checker.global("items").is_some());
    assert!(checker.global("text").is_some());
    assert!(checker.global("callback").is_some());
    assert!(checker.global("f").is_some());
}

#[test]
fn ordinary_parser_remains_unannotated() {
    let source = "int value=2+3;";
    let mut parser = Parser::new(source);
    let item = parser.next_item().unwrap().unwrap();
    let mini_c_syntax::stream::ExternalItem::Declaration(decl) = item else {
        panic!("expected declaration");
    };
    let value = decl.init.expect("initializer");
    assert!(value.location().is_none());
    assert!(matches!(value, Expr::Binary(..)));
}

#[test]
fn file_scope_internal_objects_require_complete_types() {
    reject("static int missing[];", "file-scope static object requires a complete type");
    reject("static struct Tag missing; struct Tag { int value; };",
        "file-scope static object requires a complete type");
    check("static int defined[] = {1,2}; \
           static struct Tag *pointer; \
           struct Tag { int value; }; \
           static struct Tag ready; \
           int main(void) { return defined[0] + ready.value; }").unwrap();
    check("struct Later; struct Later tentative; \
           struct Later { int value; };").unwrap();
    check("extern struct Missing external;").unwrap();
}


#[test]
fn located_operator_diagnostics_have_specific_codes_and_spans() {
    use mini_c_syntax::{ast::Span, diag::DiagnosticCode};
    let cases = [
        (
            "int f(void) { int x; return x(); }",
            DiagnosticCode::SemanticNotCallable, "T0109", "x()",
            1usize,
        ),
        (
            "struct S { int good; }; int f(void) { struct S s; return s.missing; }",
            DiagnosticCode::SemanticUnknownMember, "T0110", "missing",
            "missing".len(),
        ),
        (
            "int f(void) { int x; return x.foo; }",
            DiagnosticCode::SemanticInvalidMemberAccess, "T0111", "x.foo",
            "x.foo".len(),
        ),
        (
            "int f(void) { int x; return x[1]; }",
            DiagnosticCode::SemanticInvalidSubscript, "T0112", "x[1]",
            "x[1]".len(),
        ),
        (
            "int f(void) { int *p; return p * 2; }",
            DiagnosticCode::SemanticInvalidOperatorOperands, "T0113", "p * 2",
            "p * 2".len(),
        ),
        (
            "struct S { int field; }; int f(void) { struct S s; if(s) return 1; return 0; }",
            DiagnosticCode::SemanticInvalidCondition, "T0114", "if(s)",
            1usize,
        ),
        (
            "int take(int *p) { return 0; } int f(void) { return take(42); }",
            DiagnosticCode::SemanticIncompatibleAssignment, "T0108", "42",
            2usize,
        ),
        (
            "int take(int x) { return x; } int f(void) { return take(); }",
            DiagnosticCode::SemanticWrongArgumentCount, "T0107", "take()",
            "take()".len(),
        ),
        (
            "int f(void) { return *2; }",
            DiagnosticCode::SemanticInvalidOperatorOperands, "T0113", "*2",
            2usize,
        ),
    ];
    for (source, code, stable, search, len) in cases {
        let mut parser = Parser::new_with_spans(source);
        let mut checker = Checker::new(Target::default()).unwrap();
        let errors = diagnose_stream_recovering(&mut parser, &mut checker, 2);
        assert_eq!(errors.len(), 1, "{source:?}: {errors:?}");
        let error = &errors[0];
        assert_eq!(error.code, code, "{source:?}: {error:?}");
        assert_eq!(error.code.as_str(), stable);
        let lo = if search == "if(s)" {
            source.find("if(s)").unwrap() + 3
        } else {
            source.rfind(search).unwrap()
        };
        assert_eq!(error.span, Span { lo, hi: lo + len }, "{source:?}: {error:?}");
    }
}

#[test]
fn invalid_nested_calls_are_reported_at_the_correct_callee() {
    use mini_c_syntax::{ast::Span, diag::DiagnosticCode};
    let source = "int f(void) { int n=1; return n() + n(); }";
    let mut parser = Parser::new_with_spans(source);
    let mut checker = Checker::new(Target::default()).unwrap();
    let errors = diagnose_stream_recovering(&mut parser, &mut checker, 5);
    // Recovery is per statement: the first invalid call is reported,
    // but the other operand in that statement is not independently retried.
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, DiagnosticCode::SemanticNotCallable);
    let lo = source.find("n()").unwrap();
    assert_eq!(errors[0].span, Span { lo, hi: lo + 1 });
}

#[test]
fn plain_parser_preserves_legacy_fallback_spans_and_errors() {
    use mini_c_syntax::diag::DiagnosticCode;
    let src = "int f(void) { int x; return x[1]; }";
    let mut parser = Parser::new(src);
    let mut checker = Checker::new(Target::default()).unwrap();
    let error = check_stream(&mut parser, &mut checker, |_| Ok(())).unwrap_err();
    assert_eq!(error.code, DiagnosticCode::SemanticInvalidSubscript);
    assert!(error.span.hi >= error.span.lo);
}


#[test]
fn bitwise_integer_promotions_and_pointer_rejection() {
    check("int f(int x) { return ~x & 255; }").unwrap();
    check("unsigned int f(unsigned char c) { return c | 16; }").unwrap();
    check("int f(int a, int b) { return (a ^ b) & 31; }").unwrap();
    check("int f(int a) { return (a & 3) == 2 || (a | 4) != 0; }").unwrap();
    reject("int f(int *p) { return p & 7; }", "bitwise operator requires integer");
    reject("int f(int *p) { return ~p; }", "arithmetic unary operator requires integer");
}


#[test]
fn located_declaration_name_errors_use_precise_token_spans() {
    use mini_c_syntax::{ast::Span, diag::DiagnosticCode};
    let cases = [
        ("int x; long x;", "x;", DiagnosticCode::SemanticConflictingDeclaration),
        ("int x=1; int x=2;", "x=2", DiagnosticCode::SemanticMultipleDefinitions),
        ("typedef int Alias; long Alias;", "Alias;", DiagnosticCode::SemanticConflictingDeclaration),
        ("int f(void) { int x; int x; return 0; }", "x; return", DiagnosticCode::SemanticDuplicateDeclaration),
        ("int f(void) { typedef int T; typedef long T; return 0; }", "T; return", DiagnosticCode::SemanticDuplicateDeclaration),
        ("int f(int x, int x) { return 0; }", "x) {", DiagnosticCode::SemanticDuplicateDeclaration),
        ("int f(void) { return 0; } int f(void) { return 1; }", "f(void) { return 1", DiagnosticCode::SemanticMultipleDefinitions),
        ("/* β */ int (*callback)(int); long (*callback)(int);", "callback)(int);", DiagnosticCode::SemanticConflictingDeclaration),
    ];
    for (source, needle, expected) in cases {
        let mut parser = Parser::new_with_spans(source);
        let mut checker = Checker::new(Target::default()).unwrap();
        let errors = diagnose_stream_recovering(&mut parser, &mut checker, 4);
        assert_eq!(errors.len(), 1, "{source:?}: {errors:?}");
        let error = &errors[0];
        assert_eq!(error.code, expected, "{source:?}: {error:?}");
        let lo = source.rfind(needle).unwrap();
        let len = error.message.split('\'').nth(1).expect("quoted declaration name").len();
        assert_eq!(error.span, Span { lo, hi: lo + len }, "{source:?}: {error:?}");
    }
}

#[test]
fn default_parser_keeps_name_spans_absent() {
    use mini_c_syntax::{ast::Stmt, stream::ExternalItem};
    let source = "int (*callback)(int); int f(int x) { int local; return x; }";
    let mut parser = Parser::new(source);
    let ExternalItem::Declaration(decl) = parser.next_item().unwrap().unwrap() else {
        panic!("expected declaration");
    };
    assert_eq!(decl.name, "callback");
    assert!(decl.name_span.is_none());
    let ExternalItem::Function(f) = parser.next_item().unwrap().unwrap() else {
        panic!("expected function");
    };
    assert!(f.name_span.is_none());
    assert!(f.params[0].name_span.is_none());
    let Stmt::Block(stmts) = f.body else { panic!("expected block") };
    let Stmt::Decl(local) = &stmts[0] else { panic!("expected local declaration") };
    assert!(local.name_span.is_none());
}

#[test]
fn located_parser_handles_nested_pointer_declarators_and_parameters() {
    use mini_c_syntax::{ast::{Span, Stmt}, stream::ExternalItem};
    let source = "int (*callback)(int); int f(int x) { int local; return x; }";
    let mut parser = Parser::new_with_spans(source);
    let ExternalItem::Declaration(decl) = parser.next_item().unwrap().unwrap() else {
        panic!("expected declaration");
    };
    let lo = source.find("callback").unwrap();
    assert_eq!(decl.name_span, Some(Span { lo, hi: lo + 8 }));
    let ExternalItem::Function(f) = parser.next_item().unwrap().unwrap() else {
        panic!("expected function");
    };
    let lo = source.find("f(int x)").unwrap();
    assert_eq!(f.name_span, Some(Span { lo, hi: lo + 1 }));
    let lo = source.find("x) {").unwrap();
    assert_eq!(f.params[0].name_span, Some(Span { lo, hi: lo + 1 }));
    let Stmt::Block(stmts) = f.body else { panic!("expected block") };
    let Stmt::Decl(local) = &stmts[0] else { panic!("expected local") };
    let lo = source.find("local").unwrap();
    assert_eq!(local.name_span, Some(Span { lo, hi: lo + 5 }));
}

#[test]
fn error_recovery_preserves_valid_declarations_after_conflicting_names() {
    use mini_c_syntax::diag::DiagnosticCode;
    let source = "int a; long a; int f(int x, int x) { return 1; } int good(void) { return 3; }";
    let mut parser = Parser::new_with_spans(source);
    let mut checker = Checker::new(Target::default()).unwrap();
    let errors = diagnose_stream_recovering(&mut parser, &mut checker, 5);
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert_eq!(errors[0].code, DiagnosticCode::SemanticConflictingDeclaration);
    assert_eq!(errors[1].code, DiagnosticCode::SemanticDuplicateDeclaration);
    assert!(checker.global("a").is_some());
    assert!(checker.global("f").is_none());
    assert!(checker.global("good").is_some());
}

#[test]
fn bounded_malformed_declaration_semantic_mutations_do_not_panic() {
    // Unlike parser-only fuzzing, exercise syntax recovery plus symbol rollback
    // on truncations and punctuation insertions through the semantic checker.
    let seeds = [
        "int x; int x; int f(int a, int b) { return a+b; }",
        "typedef int Value; Value n=1; int f(void) { Value v=2; return v; }",
        "int (*callback)(int); int f(void) { int a[2]={1,2}; return a[0]; }",
    ];
    for seed in seeds {
        for at in 0..=seed.len() {
            let truncated = &seed[..at];
            let mut inserted = seed.to_owned();
            inserted.insert(at, '@');
            for input in [truncated, inserted.as_str()] {
                let result = std::panic::catch_unwind(|| {
                    let mut parser = Parser::new_with_spans(input);
                    let mut checker = Checker::new(Target::default()).unwrap();
                    let errors = diagnose_stream_recovering(&mut parser, &mut checker, 5);
                    assert!(errors.len() <= 5);
                });
                assert!(result.is_ok(), "semantic recovery panic for {input:?}");
            }
        }
    }
}


#[test]
fn shift_operators_promote_independently_and_reject_pointer_operands() {
    check("unsigned long f(unsigned int x, unsigned long n) { return x << n; }").unwrap();
    check("int f(unsigned char x, long n) { return x >> n; }").unwrap();
    check("int f(int x, long n) { return x << n; }").unwrap();
    reject("int f(int *p) { return p << 3; }", "shift operator requires integer operands");
    reject("int f(int *p) { return 3 >> p; }", "shift operator requires integer operands");
    reject("int a = 1 << 32;", "shift count out of range");
    reject("int a = 1 >> -1;", "shift count out of range");
    reject("int a = 1 << 31;", "signed integer overflow");
}


#[test]
fn compound_bitwise_assignments_validate_lvalues_and_integer_promotions() {
    check("int f(int x, int y) { x &= y; x |= 3; x ^= 1; x <<= 2; x >>= 1; return x; }").unwrap();
    check("int f(unsigned char x, unsigned long shift) { x >>= shift; x <<= 1; x ^= 255; return x; }").unwrap();
    check("int f(int *p) { *p |= 1; return (*p ^= 3); }").unwrap();
    check("int f(int x, int y) { x &= y |= 4; return x; }").unwrap();
    reject("int f(int *p) { p &= 3; return 0; }", "modifiable integer lvalue");
    reject("int f(int x, int *p) { x ^= p; return x; }", "requires integer operands");
    reject("int f(int x) { (x + 1) <<= 2; return x; }", "modifiable integer lvalue");
}


#[test]
fn arithmetic_compound_assignments_support_integer_and_pointer_lvalues() {
    check("int f(unsigned char c, unsigned long n) { c += n; c *= 3; c -= 1; c /= 2; c %= 7; return c; }").unwrap();
    check("int f(int a, int b) { a += b *= 3; a -= 1; a /= 2; a %= 5; return a; }").unwrap();
    check("int f(int *p, int offset) { p += offset; p -= 1; return *p; }").unwrap();
    check("int f(void) { struct Data { int x; }; struct Data a[2]; struct Data *p=a; p += 1; p -= 1; return 0; }").unwrap();
    reject("int f(int *p) { p *= 2; return 0; }", "modifiable integer lvalue");
    reject("int f(int *p) { p += p; return 0; }", "requires integer operands");
    reject("int f(void *p) { p += 1; return 0; }", "complete object type");
    reject("int f(int x) { (x+1) += 2; return x; }", "modifiable integer lvalue");
}


#[test]
fn prefix_and_postfix_updates_validate_modifiable_scalar_lvalues() {
    check("int f(int *p) { int old=(*p)++; int new=++*p; return old+new; }").unwrap();
    check("int f(void) { unsigned char c=1; c++; --c; return c++; }").unwrap();
    check("int f(int *p) { p++; --p; return *p; }").unwrap();
    check("int f(void) { struct Pair { int x; int y; }; struct Pair a[2]; struct Pair *p=a; p++; --p; return 0; }").unwrap();
    reject("int f(int x) { ++(x+1); return x; }",
        "increment/decrement requires a modifiable");
    reject("int f(int x) { (x++)++; return x; }",
        "increment/decrement requires a modifiable");
    reject("int f(void *p) { p++; return 0; }",
        "pointer to complete object type");
    reject("int f(void) { int a[2]; a++; return 0; }",
        "increment/decrement requires a modifiable");
    reject("int x; int a=x++;", "integer constant expression");
}

#[test]
fn nested_assignments_preserve_types_and_value_categories() {
    check("int f(void) { int a=0; int b=0; return (a=b=5)+a+b; }").unwrap();
    check("int f(void) { unsigned char x=0; return (x=260) == 4; }").unwrap();
    reject("int f(void) { int a=0; (a=3)=4; return a; }",
        "modifiable lvalue");
}


#[test]
fn for_loop_declaration_scopes_and_optional_expressions() {
    check("int f(void) { int result=0; for (int i=0; i<5; i++) result+=i; return result; }").unwrap();
    check("int f(void) { int x=0; for (; x<4; x++) { if(x==2) continue; } return x; }").unwrap();
    check("int f(void) { for (;;) { break; } return 0; }").unwrap();
    check("int f(void) { int i=8; for (int i=0; i<2; i++) {} return i; }").unwrap();
    check("typedef int T; int f(void) { for (int T=0; T<2; T++) {} T x=1; return x; }").unwrap();
    check("int f(void) { int s=0; for (int i=0; i<3; i++) for (int j=0; j<3; j++) { if (j==1) continue; s+=i+j; } return s; }").unwrap();
    reject("int f(void) { for (int i=0; i<2; i++) {} return i; }", "undeclared identifier");
    reject("int f(void) { for (int i=0; i<2; i++) {} i++; return 0; }", "undeclared identifier");
    reject("int f(void) { for (int i=0; i<2; i++) { } return i; }", "undeclared identifier");
    reject("int f(void) { for (int i=0; i<2; i++) { } int j=i; return 0; }", "undeclared identifier");
}


#[test]
fn comma_expression_sequence_values_and_lvalue_rules() {
    check("int main(void) { int x=0; int y=0; return (x=2, y=5, x+y); }").unwrap();
    check("void touch(void) { return; } int main(void) { return (touch(), 7); }").unwrap();
    check("int main(void) { int x=3; int *p=0; return *(p=&x, p); }").unwrap();
    check("int f(int a,int b) { return a+b; } int main(void) { int a=0; return f((a=2, a), 3); }").unwrap();
    check("int main(void) { int a[2]={1,2}; return a[0]+a[1]; }").unwrap();
    check("int main(void) { int i=0; int j=0; int s=0; for (i=0,j=0; i<4; i++, j+=2) { s+=i+j; } return s; }").unwrap();
    reject("int main(void) { int x=0; int y=0; (x,y)=3; return 0; }",
        "modifiable lvalue");
    reject("int main(void) { int x=0; int y=0; (x,y)++; return 0; }",
        "modifiable");
    reject("int global=(1,2);", "integer constant expression");
    reject("int f(int a,int b) { return a+b; } int main(void) { return f(1,2,3); }",
        "expects 2 arguments");
}


#[test]
fn do_while_statements_are_post_test_loops_with_scoped_bodies() {
    check("int f(void) { int x=0; do { x++; } while (x<3); return x; }").unwrap();
    check("int f(void) { int x=0; do ; while (x++<2); return x; }").unwrap();
    check("int f(void) { int x=0; do { x++; if(x==2)continue; } while (x<4); return x; }").unwrap();
    check("int f(void) { int i=0; int checks=0; do { i++; } while (checks++,i<3); return checks; }").unwrap();
    check("int f(void) { int sum=0; for (int i=0; i<3; i++) { int j=0; do { j++; if(j==2)continue; sum+=i+j; } while (j<3); } return sum; }").unwrap();
    check("int f(void) { int x=7; do { int x=3; x++; } while (0); return x; }").unwrap();
    reject("int f(void) { struct S { int x; }; struct S s; do { } while (s); return 0; }",
        "condition requires a scalar type");
}


#[test]
fn conditional_operator_types_lazy_branches_and_null_pointers() {
    check("int main(void) { int x=0; return 1 ? 7 : (x=99); }").unwrap();
    check("int main(void) { int x=0; return 0 ? (x=99) : 7; }").unwrap();
    check("int main(void) { unsigned char c=255; long x=9; return 1 ? c : x; }").unwrap();
    check("int main(void) { int x=5; int *p=1 ? &x : 0; return *p; }").unwrap();
    check("int main(void) { int x=5; int *p=0 ? 0 : &x; return *p; }").unwrap();
    check("int main(void) { int x=5; void *p=0; int *q=&x; return (1 ? q : p) == q; }").unwrap();
    check("struct S { int x; }; int main(void) { struct S a={3}; struct S b={7}; struct S c=1 ? a : b; return c.x; }").unwrap();
    check("void touch(void) { return; } int main(void) { 1 ? touch() : touch(); return 0; }").unwrap();
    check("int main(void) { int x=2; return (1 ? x : x+1); }").unwrap();
    check("int a = 1 ? 9 : 1/0; int b = 0 ? 1/0 : 3;").unwrap();
    reject("int main(void) { int x=0; int y=1; (1 ? x : y) = 3; return x; }",
        "modifiable lvalue");
    reject("int main(void) { int x=0; int y=1; (1 ? x : y)++; return x; }",
        "modifiable");
    reject("struct S { int n; }; int main(void) { struct S s; return s ? 1 : 2; }",
        "condition requires a scalar type");
    reject("int main(void) { int x=0; int *p=&x; return 1 ? p : 3; }",
        "conditional operands require compatible types");
    reject("void mark(void) { return; } int main(void) { return 1 ? mark() : 3; }",
        "conditional operands require compatible types");
}

#[test]
fn c99_bool_accepts_pointer_scalars_but_not_implicit_pointer_to_integer() {
    let source = "int nonnull(_Bool b) { return b; } \
        _Bool is_nonnull(int *p) { return p; } \
        int main(void) { int x=0; int *p=&x; \
            _Bool from_ptr=p; _Bool explicit_ptr=(_Bool)p; \
            _Bool null_ptr=(int*)0; _Bool reassigned=0; \
            reassigned=p; return nonnull(p) && is_nonnull(p) \
                && from_ptr && explicit_ptr && reassigned && !null_ptr ? 0 : 1; }";
    let checked = check(source).expect("C99 pointer to _Bool conversions must type-check");
    let CheckedItem::Function(func) = &checked[1] else { panic!("expected is_nonnull"); };
    let CheckedStmt::Block(statements) = &func.body else { panic!("expected block"); };
    let CheckedStmt::Return(Some(return_value)) = &statements[0] else {
        panic!("expected return");
    };
    assert_eq!(return_value.ty, CType::Integer { rank: IntRank::Bool, unsigned: true });
    assert!(matches!(return_value.kind, CheckedExprKind::Convert(_)),
        "returning a pointer as _Bool must retain a checked conversion");
    reject("long wrong(int *p) { return p; }", "incompatible assignment");
    reject("int wrong(int *p) { return p; }", "incompatible assignment");
}

#[test]
fn c99_bool_one_byte_array_and_struct_layout() {
    let items = check("struct Flags { _Bool active; int count; _Bool ready; } f;").unwrap();
    let CheckedItem::Declaration(f) = &items[0] else { panic!("expected Flags object"); };
    let CType::Struct { members, size, align, .. } = &f.ty else {
        panic!("_Bool must be a distinct struct-member type");
    };
    assert_eq!((*size, *align), (12, 4));
    assert_eq!(members.iter().map(|m| m.offset).collect::<Vec<_>>(), [0, 4, 8]);
    assert_eq!(members[0].ty, CType::Integer { rank: IntRank::Bool, unsigned: true });
    assert_eq!(members[2].ty, CType::Integer { rank: IntRank::Bool, unsigned: true });
    let checked = check("int f(void) { _Bool flags[3]; return sizeof flags; }").unwrap();
    let CheckedItem::Function(f) = &checked[0] else { panic!("expected function"); };
    let CheckedStmt::Block(stmts) = &f.body else { panic!("expected body"); };
    let CheckedStmt::Decl(flags) = &stmts[0] else { panic!("expected array object"); };
    assert_eq!(flags.ty.size_bytes(Target::default()), Some(3));
}

#[test]
fn c99_bool_is_distinct_promoted_and_normalized() {
    let items = check("_Bool a = 2; _Bool b = 0; int f(_Bool v) { \
        _Bool local = 256; return sizeof(_Bool) == 1 && \
        sizeof(v+1) == sizeof(int) && (int)local == 1; }").unwrap();
    let CheckedItem::Declaration(first) = &items[0] else { panic!("bool global"); };
    assert_eq!(first.ty, CType::Integer { rank: IntRank::Bool, unsigned: true });
    let initializer = first.initializer.as_ref().expect("global initializer");
    assert_eq!(const_eval::evaluate(initializer, Target::default()), Ok(1));
    check("typedef _Bool flag; int f(int x) { flag a = (_Bool)x; return a; }").unwrap();
    check("int f(_Bool x) { switch (x) { case 0: return 0; case 1: return 1; } return 2; }").unwrap();
    reject("unsigned _Bool bad;", "expected identifier in declarator");
}

#[test]
fn explicit_integer_casts_handle_typedefs_and_unsigned_conversions() {
    check("int isdigit(int c) { return (unsigned)c-'0' < 10; }").unwrap();
    check("int isalpha(int c) { return ((unsigned)c|32)-'a' < 26; }").unwrap();
    check("typedef unsigned int U; int f(int x) { return (U)x + (int)(unsigned char)x; }").unwrap();
    check("int f(int x) { int y=(int)(unsigned)x; return y; }").unwrap();
    check("int f(int x) { return (unsigned long)(unsigned)x > 0; }").unwrap();
    check("int f(void) { int x=3; return (x) + 2; }").unwrap();
    check("int f(void) { unsigned int x=0; return (unsigned)(x=5); }").unwrap();
    check("int f(int *p) { return (unsigned)p; }").unwrap();
    reject("int f(int x) { return (void)x; }", "unsupported cast type-name");
    check("typedef int T; int f(void) { int T=3; return (T)+1; }").unwrap();
}

#[test]
fn c99_pointer_to_integer_explicit_casts_preserve_checked_conversion() {
    let source = "typedef unsigned long uintptr_t; \
        uintptr_t ptr_bits(int *p) { return (uintptr_t)p; } \
        int pointer_low_byte(char *p) { return (unsigned int)(uintptr_t)p & 255; }";
    let items = check(source).unwrap();
    let CheckedItem::Function(f) = &items[1] else { panic!("expected typed function"); };
    let CheckedStmt::Block(statements) = &f.body else { panic!("expected block"); };
    let CheckedStmt::Return(Some(result)) = &statements[0] else { panic!("expected return"); };
    assert!(matches!(result.kind, CheckedExprKind::Convert(_)));
    assert!(matches!(&result.ty,
        CType::Integer { rank: IntRank::Long, unsigned: true }),
        "pointer-to-uintptr_t must retain unsigned-long type: {:?}", result.ty);
    // Source pointer-to-integer casts and nested integer narrowing do not
    // silently turn into pointer arithmetic or an unevaluated constant.
    check("typedef unsigned long uintptr_t; uintptr_t align(char *p) { \
        return -(uintptr_t)p & 7; }").unwrap();
    // Implicit pointer-to-integer assignment remains invalid.
    reject("unsigned long bad(int *p) { unsigned long value=p; return value; }",
        "incompatible");
}

#[test]
fn m10_pointer_casts_preserve_c_types_and_null_semantics() {
    check("int f(void) { int *p = (void*)0; return p == (void*)0; }").unwrap();
    check("int f(void) { int x=9; void *q=(void*)&x; int *p=(int*)q; return *p; }")
        .unwrap();
    check("typedef int *Ptr; int f(void) { Ptr p=(Ptr)0; return p==0; }")
        .unwrap();
    check("int f(void) { int **p=(int **)0; return p == (void*)0; }").unwrap();
    check("int f(int x) { return (const int)x; }").unwrap();
    reject("int f(int x) { int *p = (int*)x; return p==0; }",
        "pointer cast requires a pointer or null integer constant");
    reject("int f(void) { int *p=(int*)1; return p==0; }",
        "pointer cast requires a pointer or null integer constant");
    reject("int f(void) { int *p=(int*)0.5f; return p==0; }",
        "pointer cast requires a pointer or null integer constant");
}

#[test]
fn pinned_s9_hex_integer_functions_typecheck_unchanged() {
    check("int isascii(int c) { return !(c&~0x7f); }").unwrap();
    check("int iscntrl(int c) { return (unsigned)c < 0x20 || c == 0x7f; }").unwrap();
    check("int isgraph(int c) { return (unsigned)c-0x21 < 0x5e; }").unwrap();
    check("int isprint(int c) { return (unsigned)c-0x20 < 0x5f; }").unwrap();
    check("int toascii(int c) { return c & 0x7f; }").unwrap();
    check("int ok(void) { return 0xFFFFFFFF > 0; }").unwrap();
    reject("int bad(void) { return 08; }", "integer constant");
}

#[test]
fn integer_constant_c_type_selection_and_unsigned_ranges() {
    use mini_c_syntax::integer_literal::decode_integer_literal;
    let target = Target::default();
    let cases = [
        ("2147483647", IntRank::Int, false),
        ("2147483648", IntRank::Long, false),
        ("0x7fffffff", IntRank::Int, false),
        ("0x80000000", IntRank::Int, true),
        ("0xffffffff", IntRank::Int, true),
        ("037777777777", IntRank::Int, true),
        ("0x100000000", IntRank::Long, false),
        ("0xffffffffffffffff", IntRank::Long, true),
        ("123U", IntRank::Int, true),
        ("123L", IntRank::Long, false),
        ("123UL", IntRank::Long, true),
        ("123Lu", IntRank::Long, true),
        ("4294967296u", IntRank::Long, true),
        ("18446744073709551615UL", IntRank::Long, true),
    ];
    for (source, rank, unsigned) in cases {
        let literal = decode_integer_literal(source).unwrap();
        assert_eq!(target.integer_literal(literal), Some(CType::Integer { rank, unsigned }), "{source}");
        let code = format!("int f(void) {{ return ({source}) != 0; }}");
        check(&code).unwrap();
    }
    check("int f(void) { return 0xffffffff < 0; }").unwrap();
    check("int f(void) { return 0xffffffffu > 0; }").unwrap();
    check("long f(void) { return 0xffffffffffffffffUL; }").unwrap();
    reject("long f(void) { return 9223372036854775808L; }",
        "integer constant exceeds supported");
    reject("long f(void) { return 18446744073709551615; }",
        "decimal literal exceeds supported");
    check("int f(void) { return 7llu; }").unwrap();
}

#[test]
fn nondecimal_type_selection_respects_nondefault_target_widths() {
    use mini_c_syntax::integer_literal::decode_integer_literal;
    let model = Target { char_is_signed: true, int_bits: 16, long_bits: 32 };
    let cases = [
        ("0x7fff", Some((IntRank::Int, false))),
        ("0x8000", Some((IntRank::Int, true))),
        ("0xffff", Some((IntRank::Int, true))),
        ("0x10000", Some((IntRank::Long, false))),
        ("0x80000000", Some((IntRank::Long, true))),
        ("0xffffffff", Some((IntRank::Long, true))),
        ("0x100000000", Some((IntRank::LongLong, false))),
        ("65535", Some((IntRank::Long, false))),
        ("65535U", Some((IntRank::Int, true))),
        ("65536U", Some((IntRank::Long, true))),
    ];
    for (spelling, expected) in cases {
        let literal = decode_integer_literal(spelling).unwrap();
        let selected = model.integer_literal(literal);
        let expected = expected.map(|(rank, unsigned)| CType::Integer { rank, unsigned });
        assert_eq!(selected, expected, "{spelling}");
    }
}

#[test]
fn long_long_distinct_rank_literal_and_usual_arithmetic_conversions() {
    use mini_c_syntax::integer_literal::decode_integer_literal;
    let model = Target { int_bits: 32, long_bits: 32, ..Target::default() };
    let ll = CType::Integer { rank: IntRank::LongLong, unsigned: false };
    let ull = CType::Integer { rank: IntRank::LongLong, unsigned: true };
    let ul = CType::Integer { rank: IntRank::Long, unsigned: true };
    assert_eq!(model.bits(IntRank::LongLong), 64);
    assert_eq!(model.bits(IntRank::Long), 32);
    for text in ["2147483648LL", "0x100000000LL", "9223372036854775807LL"] {
        assert_eq!(model.integer_literal(decode_integer_literal(text).unwrap()), Some(ll.clone()));
    }
    for text in ["18446744073709551615ULL", "0xffffffffffffffffULL"] {
        assert_eq!(model.integer_literal(decode_integer_literal(text).unwrap()), Some(ull.clone()));
    }
    assert_eq!(model.integer_literal(decode_integer_literal("4294967295UL").unwrap()), Some(ul.clone()));
    assert_eq!(model.integer_literal(decode_integer_literal("4294967296UL").unwrap()), Some(ull.clone()));
    assert_eq!(model.integer_literal(decode_integer_literal("0xffffffff").unwrap()), Some(CType::Integer { rank: IntRank::Int, unsigned: true }));
    assert_eq!(model.common_integer(&ul, &ll), Some(ll.clone()));
    assert_eq!(model.common_integer(&ull, &ll), Some(ull));
    check("long long f(long long x) { return x + 1LL; }").unwrap();
    check("unsigned long long f(unsigned long long x) { return x + 0xffffffffffffffffULL; }").unwrap();
    check("long long f(void) { return 9223372036854775807LL; }").unwrap();
    reject("long long f(void) { return 9223372036854775808LL; }", "integer constant exceeds");
    reject("int f(void) { return 1LLL; }", "suffix");
}

#[test]
fn const_objects_pointer_targets_and_restrict_are_semantically_checked() {
    check("int f(void) { const int x=7; return x; }").unwrap();
    check("int f(void) { int x=7; const int *p=&x; return *p; }").unwrap();
    check("int f(int *restrict p) { *p=3; return *p; }").unwrap();
    check("int f(const int *__restrict p) { return *p; }").unwrap();
    check("int f(void) { int x=2; int *const p=&x; *p=3; return x; }").unwrap();
    check("typedef int *Ptr; int f(void) { int x=3; Ptr const p=&x; *p=7; return x; }").unwrap();
    check("int f(const int *p) { const int *q=p; return *q; }").unwrap();
    reject("int f(void) { const int x=1; x=2; return x; }",
        "modifiable lvalue");
    reject("int f(void) { const int x=1; ++x; return x; }",
        "modifiable");
    reject("int f(void) { const int x=1; x+=2; return x; }",
        "modifiable");
    reject("int f(void) { const int x=1; const int *p=&x; *p=2; return x; }",
        "modifiable lvalue");
    reject("int f(void) { int x=1; int *const p=&x; p=0; return x; }",
        "modifiable lvalue");
    reject("int f(void) { const int x=1; int *p=&x; return *p; }",
        "incompatible assignment");
    reject("int f(void) { int x=1; int **p=0; const int **q=p; return 0; }",
        "incompatible assignment");
    reject("int f(void) { restrict int x=3; return x; }",
        "restrict requires a pointer");
}


#[test]
fn const_aggregate_members_and_pointer_conditional_qualifiers() {
    check("struct S { int x; }; int f(void) { struct S s={7}; const struct S *p=&s; return p->x; }").unwrap();
    check("int f(void) { int x=1; int *p=&x; const int *q=p; return (p==q) + (q==p); }").unwrap();
    check("int f(void) { int x=1; int *p=&x; const int *q=p; return *(1 ? p : q); }").unwrap();
    check("int f(void) { int x=1; const int *p=&x; const void *q=p; return q != 0; }").unwrap();
    reject("struct S { int x; }; int f(void) { struct S s={7}; const struct S *p=&s; p->x=9; return s.x; }", "modifiable lvalue");
    reject("int f(void) { int x=1; const int *q=&x; void *p=q; return 0; }", "incompatible assignment");
    reject("int f(void) { int x=1; int *p=&x; const int *q=p; int *r=(1?p:q); return 0; }", "incompatible assignment");
}


#[test]
fn m10_gnu_extension_suppresses_warnings_not_types_or_redeclaration_checks() {
    check(concat!(
        "__extension__ extern long long int llrint(double x) ",
        "__attribute__((__nothrow__,__leaf__));",
        "__extension__ extern long long int llrint(double x);",
        "int main(void){return 0;}"
    )).unwrap();
    reject(concat!(
        "__extension__ extern long long int llrint(double x);",
        "extern int llrint(double x);"
    ), "conflicting declaration");
    reject("__extension__ int x=1;int x=2;", "multiple definitions");
}

#[test]
fn m10_float128_hosted_signature_preserves_distinct_abi_identity() {
    use mini_c_syntax::ast::FloatRank;
    let checked = check(concat!(
        "extern int __fpclassifyf128 (_Float128 __value) ",
        "__attribute__((__nothrow__,__leaf__));",
        "extern int __isnanf128(_Float128 __value);"
    )).unwrap();
    let CheckedItem::Declaration(proto) = &checked[0] else {
        panic!("expected checked prototype");
    };
    assert_eq!(proto.ty, CType::Function {
        result: Box::new(CType::int()),
        params: vec![CType::Floating(FloatRank::Float128)],
    });
    let quad = CType::Floating(FloatRank::Float128);
    assert_eq!(quad.size_bytes(Target::default()), Some(16));
    assert_eq!(quad.align_bytes(Target::default()), Some(16));
    assert!(!quad.same_type(&CType::Floating(FloatRank::LongDouble)));
    reject("extern int f(_Float128); extern int f(long double);",
        "conflicting declaration");
}

#[test]
fn m10_float_prototypes_retain_types_and_target_layout() {
    use mini_c_syntax::ast::FloatRank;
    let src = concat!(
        "typedef float float_t;typedef double double_t;",
        "extern int __fpclassify(double __value) __attribute__((__nothrow__,__leaf__));",
        "extern double nexttoward(double x, long double y);",
        "extern long double sqrtl(long double x);",
        "extern float nexttowardf(float x, double long y);",
        "typedef float_t another_float;"
    );
    let checked = check(src).unwrap();
    let CheckedItem::Typedef { ty: float_ty, .. } = &checked[0] else {
        panic!("missing float typedef");
    };
    let CheckedItem::Typedef { ty: double_ty, .. } = &checked[1] else {
        panic!("missing double typedef");
    };
    assert_eq!(float_ty, &CType::Floating(FloatRank::Float));
    assert_eq!(double_ty, &CType::Floating(FloatRank::Double));
    assert!(!float_ty.same_type(double_ty));
    let CheckedItem::Declaration(proto) = &checked[3] else {
        panic!("missing prototype");
    };
    assert_eq!(proto.ty, CType::Function {
        result: Box::new(CType::Floating(FloatRank::Double)),
        params: vec![
            CType::Floating(FloatRank::Double),
            CType::Floating(FloatRank::LongDouble),
        ],
    });
    let target = Target::default();
    for (kind, bytes) in [
        (FloatRank::Float, 4),
        (FloatRank::Double, 8),
        (FloatRank::LongDouble, 16),
    ] {
        let ty = CType::Floating(kind);
        assert_eq!(ty.size_bytes(target), Some(bytes));
        assert_eq!(ty.align_bytes(target), Some(bytes));
        assert!(ty.is_scalar());
        assert!(!ty.is_integer());
    }
    reject("extern double x; extern int x;", "conflicting declaration");
    reject("int f(void){double x;return x;}", "incompatible");
}

#[test]
fn gnu_function_hints_preserve_checked_signatures() {
    check("int add(int x) __attribute__((__nothrow__,__leaf__)); int add(int x){return x+1;}").unwrap();
    check("int read(const int *p) __attribute__((__pure__,__nonnull__(1))); int read(const int *p){return *p;}").unwrap();
    check("typedef unsigned long size_t; extern void *copy(void *__restrict a, const void *__restrict b, size_t n) __attribute__((__nothrow__,__leaf__)) __attribute__((__nonnull__(1,2)));").unwrap();
}

#[test]
fn function_redeclarations_ignore_only_top_level_parameter_const() {
    // C17 6.7.6.3p15: qualifiers on the parameter object itself do
    // not distinguish function types across declarations.
    check("int f(const int); int f(int x) { return x; }").unwrap();
    check("int f(int *const); int f(int *p) { return *p; }").unwrap();
    check("int f(int *); int f(int *const p) { return *p; }").unwrap();

    // A qualifier on the pointee remains part of parameter type
    // compatibility, and must not be discarded with the top level.
    reject("int g(const int *); int g(int *p) { return *p; }",
        "conflicting declaration");
    reject("int g(int *); int g(const int *p) { return *p; }",
        "conflicting declaration");
}

#[test]
fn iso_c_void_pointer_conversion_and_relational_constraints() {
    check("int f(void) { int x=3; void *p=&x; int *q=p; return p==q; }").unwrap();
    reject("int g(void) { int (*fp)(void)=0; void *p=fp; return p!=0; }",
        "incompatible assignment");
    reject("int g(void) { void *p=0; int (*fp)(void)=p; return fp!=0; }",
        "incompatible assignment");
    check("int h(void) { int x=3; void *p=&x; int *q=&x; return p==q; }").unwrap();
    reject("int h(void) { int x=3; void *p=&x; int *q=&x; return p<q; }",
        "compatible integer operands");
    reject("int h(void) { void *p=0; void *q=0; return p<q; }",
        "compatible integer operands");
    check("int h(int *a, int *b) { return a<b; }").unwrap();
    // ?: follows the same object-pointer/void-pointer restriction.
    reject("int choose(int c) { int (*fp)(void)=0; void *vp=0; return (c ? fp : vp)!=0; }",
        "compatible types");
    check("int choose(int c) { int x=1; int *p=&x; void *v=p; return (c ? p : v)!=0; }").unwrap();
}

#[test]
fn compatible_function_pointer_parameter_qualification() {
    // Top-level parameter qualifiers do not distinguish function types,
    // including when the function type is referenced through a pointer.
    check("int id(int x){return x;} int f(void){int (*p)(const int)=id; return p(3); }").unwrap();
    check("int id(const int x){return x;} int f(void){int (*p)(int)=id; return p(3); }").unwrap();
    check("int id(int *const p){return *p;} int f(void){int (*q)(int *)=id; int x=4; return q(&x); }").unwrap();
    // Qualifiers on a pointer's target, unlike on the parameter
    // variable itself, remain part of the function signature.
    reject("int id(const int *p){return *p;} int f(void){int (*q)(int *)=id; return 0; }",
        "incompatible assignment");
    reject("int id(int *p){return *p;} int f(void){int (*q)(const int *)=id; return 0; }",
        "incompatible assignment");
}

#[test]
fn const_aggregate_and_character_array_initializers() {
    // Braces describe the aggregate even when its object type is const.
    check("struct S { int x; long y; }; const struct S s={3,4}; int f(void) { return s.x; }").unwrap();
    check("struct S { int x; }; int f(void) { struct S s={3}; const struct S q=s; return q.x; }").unwrap();
    check("const int values[2]={1,2}; int f(void) { return values[1]; }").unwrap();

    // Qualifying char-array elements must not disable string literal sizing
    // or the string literal's special array initialization handling.
    check("const char text[]=\"abc\"; int f(void) { return text[1]; }").unwrap();
    check("int f(void) { const char text[3]=\"hi\"; return text[0]; }").unwrap();
    reject("const char text[2]=\"abcd\";", "string literal is too long");

    // Array element qualifiers propagate through each dimension.
    check("struct S { int a[2][2]; }; int f(void) { struct S s={{{1,2},{3,4}}}; const struct S *p=&s; const int *q=p->a[0]; return q[0]; }").unwrap();
    reject("struct S { int a[2][2]; }; int f(void) { struct S s={{{1,2},{3,4}}}; const struct S *p=&s; int *q=p->a[0]; return q[0]; }",
        "incompatible assignment");
}

#[test]
fn s12_variadic_printf_semantics_and_promotion() {
    let source = r#"
        extern int printf(const char *restrict fmt, ...);
        int main(void) {
            char c = 65;
            short n = 17;
            return printf("%s %d %d", "hello", c, n);
        }
    "#;
    let items = check(source).unwrap();
    let CheckedItem::Declaration(decl) = &items[0] else {
        panic!("variadic printf declaration expected");
    };
    assert!(matches!(decl.ty, CType::VariadicFunction { .. }));
    let CheckedItem::Function(main) = &items[1] else {
        panic!("main definition expected");
    };
    let CheckedStmt::Block(stmts) = &main.body else { panic!("block expected") };
    let CheckedStmt::Return(Some(call)) = &stmts[2] else { panic!("call return expected") };
    let CheckedExprKind::Call(_, args) = &call.kind else { panic!("call expected") };
    assert_eq!(args.len(), 4);
    assert_eq!(args[2].ty, CType::int());
    assert_eq!(args[3].ty, CType::int());

    reject("extern int printf(const char *, ...); int main(void){return printf();}",
        "at least 1");
    reject("extern int printf(const char *, ...); int main(void){return printf(7, 9);}",
        "incompatible");
    reject("int f(int); int f(int, ...);", "conflicting declaration");
    reject("int f(int, ...); int f(int);", "conflicting declaration");
    check(r#"extern int printf(const char *, ...); int main(void){return printf("%s", "hi");}"#).unwrap();
}

#[test]
fn s12_gcc_builtin_va_list_layout_typedef_and_parameter_decay() {
    let items = check(
        "typedef __builtin_va_list __gnuc_va_list; "
    ).unwrap();
    let CheckedItem::Typedef { ty, .. } = &items[0] else {
        panic!("expected builtin typedef");
    };
    assert_eq!(ty.size_bytes(Target::default()), Some(24));
    assert_eq!(ty.align_bytes(Target::default()), Some(8));
    let CType::Array(element, Some(1)) = ty else {
        panic!("GCC SysV va_list is one-element tag array, not void*");
    };
    let CType::Struct { members, size, align, .. } = element.as_ref() else {
        panic!("GCC va_list contains struct tag");
    };
    assert_eq!((*size, *align), (24, 8));
    assert_eq!(
        members.iter().map(|m| (m.name.as_str(), m.offset)).collect::<Vec<_>>(),
        vec![("gp_offset", 0), ("fp_offset", 4),
             ("overflow_arg_area", 8), ("reg_save_area", 16)]
    );
    let items = check(
        "typedef __builtin_va_list __gnuc_va_list; typedef __gnuc_va_list va_list; extern int vprintf(const char *restrict fmt, va_list ap);"
    ).unwrap();
    let CheckedItem::Declaration(vprintf) = &items[2] else {
        panic!("expected vprintf prototype");
    };
    let CType::Function { params, .. } = &vprintf.ty else {
        panic!("expected fixed vprintf prototype");
    };
    assert!(matches!(&params[1], CType::Pointer(inner)
        if matches!(inner.as_ref(), CType::Struct { size: 24, .. })));
    reject("typedef __builtin_va_list va_list; typedef void *va_list;",
        "conflicting");
}

#[test]
fn s12_sizeof_type_folds_in_glibc_array_bounds() {
    let items = check(
        "typedef unsigned long size_t; "
        .to_owned().as_str()
    ).unwrap();
    assert_eq!(items.len(), 1);
    let items = check(concat!(
        "typedef unsigned long size_t; ",
        "struct FILE_like { ",
        "char unused[15 * sizeof(int) - 4 * sizeof(void *) - sizeof(size_t)]; ",
        "}; ",
        "int a[sizeof(int)]; ",
        "int b[sizeof(void *)]; ",
        "int c[sizeof(size_t)]; ",
        "int f(void) { return sizeof(int) == 4 && sizeof(void *) == 8; }"
    )).unwrap();
    let CheckedItem::Declaration(a) = &items[2] else { panic!("a declaration"); };
    let CheckedItem::Declaration(b) = &items[3] else { panic!("b declaration"); };
    let CheckedItem::Declaration(c) = &items[4] else { panic!("c declaration"); };
    assert!(matches!(a.ty, CType::Array(_, Some(4))));
    assert!(matches!(b.ty, CType::Array(_, Some(8))));
    assert!(matches!(c.ty, CType::Array(_, Some(8))));
    reject("int a[sizeof(void)];", "complete object type");
    reject("int a[sizeof(int[])];", "complete object type");
    reject("int a[sizeof(missing)];", "undeclared identifier");
}

#[test]
fn variadic_function_pointer_parameters_ignore_only_top_level_qualifiers() {
    check("extern int sink(int, ...); int f(void) { int (*p)(const int, ...) = sink; return p(1, 2); }").unwrap();
    check("extern int sink(int *const, ...); int f(void) { int (*p)(int *, ...) = sink; int n=7; return p(&n, 2); }").unwrap();
    // Fixed-arity and variadic signatures remain distinct.
    reject("extern int sink(int, ...); int f(void) { int (*p)(int) = sink; return 0; }",
        "incompatible assignment");
    // Const on the pointee remains part of a variadic function parameter type.
    reject("extern int sink(const int *, ...); int f(void) { int (*p)(int *, ...) = sink; return 0; }",
        "incompatible assignment");
}

#[test]
fn s12_gnu_assembler_alias_survives_semantics_and_redeclarations() {
    let items = check(concat!(
        "extern int localputs(const char *) __asm__(\"puts\");",
        "extern int localputs(const char *);",
        "int main(void) { return localputs(\"hello\"); }"
    )).unwrap();
    let CheckedItem::Declaration(first) = &items[0] else { panic!("declaration"); };
    let CheckedItem::Declaration(second) = &items[1] else { panic!("redeclaration"); };
    assert_eq!(first.name, "localputs");
    assert_eq!(first.asm_label.as_deref(), Some("puts"));
    assert_eq!(second.asm_label.as_deref(), Some("puts"));

    reject(concat!(
        "extern int f(void) __asm__(\"one\");",
        "extern int f(void) __asm__(\"two\");"
    ), "conflicting asm linkage");
    check("extern int f(void) __asm__(\"impl\");").unwrap();
}

#[test]
fn completed_aggregate_tagref_parameter_accepts_compatible_by_value_argument() {
    // The prototype can retain a TagRef even after the struct body becomes
    // complete. Argument compatibility must compare the same nominal tag.
    check("struct Cookie { long a; long b; }; extern int consume(struct Cookie); int main(void) { struct Cookie c={3,4}; return consume(c); }").unwrap();
    check("struct Cookie; extern int consume(struct Cookie); struct Cookie { long a; long b; }; int main(void) { struct Cookie c={3,4}; return consume(c); }").unwrap();
    check("union Value { long a; int b; }; extern int consume(union Value); int main(void) { union Value v={3}; return consume(v); }").unwrap();

    // Same layout is not sufficient: struct/union tags carry identity.
    reject("struct S { long a; }; struct T { long a; }; extern int consume(struct S); int main(void) { struct T t={1}; return consume(t); }",
        "incompatible assignment");
    reject("struct S { long a; }; union U { long a; }; extern int consume(struct S); int main(void) { union U u={1}; return consume(u); }",
        "incompatible assignment");
}

#[test]
fn s21_register_storage_is_automatic_and_nonaddressable() {
    let items = check("int f(register int x) { register int y=x; for(register int i=0;i<3;i++) y+=i; return y; }").unwrap();
    let CheckedItem::Function(func) = &items[0] else { panic!("expected function"); };
    assert_eq!(func.params[0].storage, StorageClass::Register);
    let CheckedStmt::Block(body) = &func.body else { panic!("expected function body"); };
    let CheckedStmt::Decl(y) = &body[0] else { panic!("expected local"); };
    assert_eq!(y.storage, StorageClass::Register);
    assert_eq!(y.linkage, Linkage::None);

    check("int f(void) { int x=1; { register int x=2; } int *p=&x; return *p; }").unwrap();
    check("int f(int *p) { register int *q=p; return *q; }").unwrap();
    reject("int f(register int n) { return *(&n); }", "cannot take address of register");
    reject("int f(void) { register int n=1; int *p=&n; return *p; }", "cannot take address of register");
    reject("int f(void) { register int a[2]; return 0; }", "register local currently requires");
    reject("register int object;", "register storage class is not allowed at file scope");
    reject("register int f(void) { return 0; }", "register storage class is not allowed at file scope");
}

#[test]
fn s21_gcc_ctz_builtin_preserves_type_and_single_operand() {
    let items = check("int f(int x) { return __builtin_ctz(x); }").unwrap();
    let CheckedItem::Function(fun) = &items[0] else { panic!("function"); };
    let CheckedStmt::Block(stmts) = &fun.body else { panic!("body"); };
    let CheckedStmt::Return(Some(result)) = &stmts[0] else { panic!("return"); };
    assert_eq!(result.ty, CType::int());
    let CheckedExprKind::BuiltinCtz(arg) = &result.kind else { panic!("builtin expected"); };
    assert_eq!(arg.ty, CType::Integer { rank: IntRank::Int, unsigned: true });

    check("int f(int x) { return x == 0 ? 0 : __builtin_ctz(x) + 1; }").unwrap();
    check("int f(void) { return __builtin_ctz(-1); }").unwrap();
    check("int f(void) { return __builtin_ctz(0x80000000UL); }").unwrap();
    // If a real declaration is in scope, ordinary C function checking wins.
    let items = check("extern int __builtin_ctz(int); int f(int x) { return __builtin_ctz(x); }").unwrap();
    let CheckedItem::Function(fun) = &items[1] else { panic!("function"); };
    let CheckedStmt::Block(stmts) = &fun.body else { panic!("body"); };
    let CheckedStmt::Return(Some(result)) = &stmts[0] else { panic!("return"); };
    assert!(matches!(result.kind, CheckedExprKind::Call(..)));
}

#[test]
fn s21_gcc_ctz_builtin_rejects_malformed_uses() {
    reject("int f(void) { return __builtin_ctz(); }", "expects exactly one argument");
    reject("int f(void) { return __builtin_ctz(1, 2); }", "expects exactly one argument");
    reject("int f(void) { int x=7; return __builtin_ctz(&x); }", "requires an integer argument");
    reject("int f(void) { return __builtin_ctzll(1); }", "undeclared identifier");
    reject("int __builtin_ctz; int f(void) { return __builtin_ctz(8); }", "not a function");
    // A builtin evaluation is not a C integer constant expression (even if
    // the backend might fold its bits); don't add GNU constant-folding here.
    reject("int value = __builtin_ctz(8);", "not an integer constant expression");
}

#[test]
fn m10_decimal_floating_constant_semantics_are_type_preserving() {
    for (source, ty, token) in [
        ("float f(void) { return 0.5f; }",
            CType::Floating(mini_c_syntax::ast::FloatRank::Float), "0.5f"),
        ("double f(void) { return 1e+12; }",
            CType::Floating(mini_c_syntax::ast::FloatRank::Double), "1e+12"),
        ("long double f(void) { return 1.5L; }",
            CType::Floating(mini_c_syntax::ast::FloatRank::LongDouble), "1.5L"),
    ] {
        let items = check(source).unwrap();
        let CheckedItem::Function(fun) = &items[0] else { panic!("function"); };
        let CheckedStmt::Block(stmts) = &fun.body else { panic!("body"); };
        let CheckedStmt::Return(Some(result)) = &stmts[0] else { panic!("return"); };
        assert_eq!(result.ty, ty);
        assert!(matches!(&result.kind, CheckedExprKind::FloatLiteral(s) if s == token));
    }
    reject("enum E { A=1.5f };", "integer constant expression");
}

#[test]
fn m10_floating_usual_arithmetic_preserves_checked_result_types() {
    use mini_c_syntax::ast::FloatRank;
    for (source, expected) in [
        ("float f(float a,float b){return +a + -b * 0.5f;}",
            CType::Floating(FloatRank::Float)),
        ("double f(float a,int b){return a + b * 2.0;}",
            CType::Floating(FloatRank::Double)),
        ("long double f(double a){return a + 2.0L;}",
            CType::Floating(FloatRank::LongDouble)),
        ("int f(float a,double b){return a < b;}", CType::int()),
        ("int f(float a){return a == 0;}", CType::int()),
    ] {
        let items = check(source).unwrap();
        let CheckedItem::Function(fun) = &items[0] else { panic!("function"); };
        let CheckedStmt::Block(stmts) = &fun.body else { panic!("block"); };
        let CheckedStmt::Return(Some(result)) = &stmts[0] else { panic!("return"); };
        assert_eq!(result.ty, expected, "{source}");
        assert!(matches!(result.kind, CheckedExprKind::Binary(..)), "{source}");
    }
}

#[test]
fn m10_floating_arithmetic_rejects_integer_only_and_pointer_operations() {
    reject("float f(float a){return a % 2.0f;}", "compatible arithmetic operands");
    reject("int f(float a){return a & 3;}", "bitwise operator requires integer");
    reject("int f(float a){return a << 1;}", "shift operator requires integer");
    reject("float f(float a){return ~a;}", "bitwise not requires integer");
    reject("float f(float a,int *p){return a + p;}",
        "compatible arithmetic operands");
    // Floating expressions are not integer constant expressions.
    reject("enum E { A=(1.5f + 2.5f) };", "integer constant expression");
}

#[test]
fn m10_floating_conditional_common_type_is_not_integer() {
    use mini_c_syntax::ast::FloatRank;
    for (source, expected) in [
        ("float f(int test,float a,float b){return test ? a : b;}",
            CType::Floating(FloatRank::Float)),
        ("double f(int test,float a,double b){return test ? a : b;}",
            CType::Floating(FloatRank::Double)),
        ("float f(int test,float a){return test ? a : 0;}",
            CType::Floating(FloatRank::Float)),
    ] {
        let items = check(source).unwrap();
        let CheckedItem::Function(fun) = &items[0] else { panic!("function"); };
        let CheckedStmt::Block(stmts) = &fun.body else { panic!("block"); };
        let CheckedStmt::Return(Some(result)) = &stmts[0] else { panic!("return"); };
        assert_eq!(result.ty, expected, "{source}");
        assert!(matches!(result.kind, CheckedExprKind::Conditional(..)), "{source}");
    }
}

#[test]
fn m10_checked_block_declaration_groups_preserve_scope_and_order() {
    check("int f(void){ int i=2, j=i+3; return j; }").unwrap();
    check("int f(void){ int i=4, *p=&i, k=*p+2; return k; }").unwrap();
    check("int f(void){ typedef int Word, Other; Word a=1, b=a+2; Other c=3; return b+c; }").unwrap();
    check("int f(void){ register int i=2, j=3; return i+j; }").unwrap();
    reject("int f(void){ int a=1, a=2; return a; }", "duplicate declaration");
    reject("int f(void){ int a=1,b=unknown; return a; }", "undeclared identifier");
    reject("int f(void){ const int a=1,b=a+2; b=3; return a; }", "modifiable lvalue");
}

#[test]
fn c99_switch_integral_promotions_fallthrough_and_checked_labels() {
    let items = check(
        "int select(unsigned char c) { switch (c) { \
         case 1+2: c=4; case 7: { c=9; break; } \
         default: return c; } return 0; }"
    ).unwrap();
    let CheckedItem::Function(function) = &items[0] else { panic!("function") };
    let CheckedStmt::Block(stmts) = &function.body else { panic!("body") };
    let CheckedStmt::Switch { control, body } = &stmts[0] else { panic!("switch") };
    assert_eq!(control.ty, CType::int(), "char must be promoted to int");
    let CheckedStmt::Block(arms) = body.as_ref() else { panic!("case block") };
    assert!(matches!(&arms[0], CheckedStmt::Case { value: 3, .. }));
    assert!(matches!(&arms[1], CheckedStmt::Case { value: 7, .. }));
    assert!(matches!(&arms[2], CheckedStmt::Default(_)));
}

#[test]
fn c99_switch_scope_case_ownership_and_break_continue_rules() {
    check("int f(int x) { switch (x) { case 1: while(x) { break; } break; default: break; } return 0; }").unwrap();
    check("int f(int x) { while(x) { switch(x) { case 1: continue; default: break; } break; } return 0; }").unwrap();
    check("int f(int x) { switch(x) { case 1: switch(x) { case 1: break; default: break; } case 2: break; } return 0; }").unwrap();
    check("int f(int x) { switch(x) { case 1: if (x) { case 2: x=3; } default: break; } return 0; }").unwrap();
    reject("int f(int x) { case 1: return x; }", "case label outside switch");
    reject("int f(int x) { default: return x; }", "default label outside switch");
    reject("int f(int x) { switch (x) { case 1: continue; } return 0; }", "continue outside loop");
    reject("int f(void) { break; }", "break outside loop");
}

#[test]
fn c99_switch_rejects_non_integer_or_non_constant_and_duplicates() {
    reject("int f(float f) { switch(f) { default: break; } return 0; }",
        "controlling expression requires an integer");
    reject("int f(int *p) { switch(p) { default: break; } return 0; }",
        "controlling expression requires an integer");
    reject("int f(int x) { switch(x) { case x: return 1; } return 0; }",
        "integer constant expression");
    reject("int f(int x) { switch(x) { case 1.5: return 1; } return 0; }",
        "integer constant expression");
    reject("int f(int x) { switch(x) { case 3: return 0; case 1+2: return 1; } return 2; }",
        "duplicate case");
    reject("int f(int x) { switch(x) { default: return 0; default: return 1; } }",
        "duplicate default");
    reject("int f(int x) { switch(x) { case 1/0: return 0; } }",
        "integer constant expression");
    // Conversion to unsigned long wraps a negative case constant before
    // duplicate checking. The two cases are equal in the controlling type.
    reject("int f(unsigned long x) { switch(x) { case -1: return 0; \
        case 18446744073709551615UL: return 1; } return 2; }",
        "duplicate case");
    // Distinct nested switches can reuse both a case value and default.
    check("int f(int x) { switch(x) { case 1: switch(x) { case 1: return 1; default: break; } \
        default: return 2; } return 0; }").unwrap();
}

#[test]
fn c99_sizeof_expression_preserves_array_types_and_is_unevaluated() {
    let items = check(concat!(
        "int a[7]; int changed; ",
        "int f(int *p) { ",
        "return sizeof a == 28 && sizeof(a) == 28 && ",
        "sizeof(a[0]) == 4 && sizeof(*p) == 4 && ",
        "sizeof p == 8 && sizeof(p+1) == 8 && ",
        "sizeof(changed++) == 4 && sizeof(++changed) == 4 && ",
        "sizeof(1.0f) == 4 && sizeof 2.0 == 8 && ",
        "sizeof \"abc\" == 4 && sizeof(int[3]) == 12; }"
    )).unwrap();
    let CheckedItem::Function(fun) = &items[2] else { panic!("expected function"); };
    let CheckedStmt::Block(stmts) = &fun.body else { panic!("expected body"); };
    let CheckedStmt::Return(Some(expr)) = &stmts[0] else { panic!("expected return"); };
    assert_eq!(expr.ty, CType::int());
    // Any side-effecting sizeof operand is discarded during checking.
    // Only a constant size_t value appears in the checked tree.
    fn count_updates(e: &CheckedExpr) -> usize {
        match &e.kind {
            CheckedExprKind::Update { .. } => 1,
            CheckedExprKind::Binary(_, a, b) => count_updates(a) + count_updates(b),
            CheckedExprKind::Convert(inner) | CheckedExprKind::Load(inner) =>
                count_updates(inner),
            _ => 0,
        }
    }
    assert_eq!(count_updates(expr), 0);
}

#[test]
fn c99_sizeof_named_aggregate_type_name_uses_visible_tag() {
    let items = check("struct Flags { _Bool first; int middle; _Bool last; }; \
        int size(void) { return sizeof(struct Flags); }").unwrap();
    let CheckedItem::Function(f) = &items[1] else { panic!("expected function"); };
    let CheckedStmt::Block(stmts) = &f.body else { panic!("expected block"); };
    let CheckedStmt::Return(Some(result)) = &stmts[0] else { panic!("expected return"); };
    assert_eq!(const_eval::evaluate(result, Target::default()), Ok(12));
    check("struct Forward; int f(void) { return sizeof(struct Forward *); }").unwrap();
    check("struct A { int x; }; int f(void) { \
        struct A { long y; }; return sizeof(struct A) == 8; }").unwrap();
    reject("struct Forward; int f(void) { return sizeof(struct Forward); }",
        "incomplete struct, union or enum tag");
    reject("int f(void) { return sizeof(struct Missing); }",
        "unknown aggregate tag");
}

#[test]
fn c99_sizeof_expression_typedef_disambiguation_and_constraints() {
    check(concat!(
        "typedef int T; int a[sizeof(T)]; ",
        "int f(void) { int Tval=1; return sizeof(Tval)==sizeof(T); }"
    )).unwrap();
    check("int f(int a[9]) { return sizeof(a) == sizeof(int *); }").unwrap();
    reject("int f(void) { return sizeof(missing); }", "undeclared identifier");
    reject("int f(void) { return sizeof(void); }", "complete object type");
    reject("int f(void) { return sizeof(int[]); }", "complete object type");
    reject("int g(void); int f(void) { return sizeof(g); }", "complete object type");
    reject("int f(void) { int a[]; return sizeof(a); }", "incomplete local object type");
}

#[test]
fn c99_f0_feature_matrix_checked_shapes_are_not_only_exit_codes() {
    use mini_c_syntax::ast::{BinOp, FloatRank};
    // tools/c99_features.json:sizeof-array. The checked representation must
    // be the size_t constant, not an address/array-decay or runtime load.
    let items = check("unsigned long f(void){int a[4]; return sizeof a;}").unwrap();
    let CheckedItem::Function(fun) = &items[0] else { panic!("function"); };
    let CheckedStmt::Block(stmts) = &fun.body else { panic!("body"); };
    let CheckedStmt::Return(Some(size)) = &stmts[1] else { panic!("return"); };
    assert_eq!(size.ty, CType::Integer { rank: IntRank::Long, unsigned: true });
    assert!(matches!(size.kind, CheckedExprKind::Literal(16)));
    assert_eq!(size.category, Category::Value);

    // tools/c99_features.json:float-arithmetic. The result and operands
    // remain floating typed; accepting syntax alone would miss wrong typing.
    let items = check("float f(float x){return x + 3.5f;}").unwrap();
    let CheckedItem::Function(fun) = &items[0] else { panic!("function"); };
    let CheckedStmt::Block(stmts) = &fun.body else { panic!("body"); };
    let CheckedStmt::Return(Some(sum)) = &stmts[0] else { panic!("return"); };
    assert_eq!(sum.ty, CType::Floating(FloatRank::Float));
    assert!(matches!(sum.kind, CheckedExprKind::Binary(BinOp::Add, _, _)));
    assert_eq!(sum.category, Category::Value);

    // tools/c99_features.json:conditional-integer. Check the actual lazy
    // conditional expression kind and integer result type.
    let items = check("int f(int x){return x ? 3 : 4;}").unwrap();
    let CheckedItem::Function(fun) = &items[0] else { panic!("function"); };
    let CheckedStmt::Block(stmts) = &fun.body else { panic!("body"); };
    let CheckedStmt::Return(Some(expr)) = &stmts[0] else { panic!("return"); };
    assert_eq!(expr.ty, CType::int());
    assert!(matches!(expr.kind, CheckedExprKind::Conditional(..)));
}
