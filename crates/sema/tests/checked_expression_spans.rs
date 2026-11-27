//! The opt-in syntax parser emits exact byte spans for checked expression
//! trees. Unannotated/implicitly generated expressions never invent ranges.
use mini_c_sema::{check_stream,CheckedExprKind,CheckedItem,CheckedStmt,Checker,Target};
use mini_c_syntax::parser::Parser;

fn checked(source:&str,with_spans:bool)->Vec<CheckedItem>{
    let mut parser=if with_spans {
        Parser::new_with_spans(source)
    } else {Parser::new(source)};
    let mut checker=Checker::new(Target::default()).unwrap();
    let mut result=Vec::new();
    check_stream(&mut parser,&mut checker,|item|{
        result.push(item); Ok(())
    }).unwrap();
    result
}

fn returned(items:&[CheckedItem])->&mini_c_sema::CheckedExpr{
    let CheckedItem::Function(f)=&items[0] else{panic!("function")};
    let CheckedStmt::Block(stmts)=&f.body else{panic!("block")};
    let CheckedStmt::Return(Some(expr))=&stmts[0] else{panic!("return")};
    expr
}

#[test]
fn precise_source_ranges_survive_checked_arithmetic_and_lvalue_load() {
    let source="int f(int x) {\n  return x + 3;\n}\n";
    let parsed=checked(source,true);
    let expr=returned(&parsed);
    let span=expr.source_span.expect("checked binary must preserve parser span");
    assert_eq!(&source[span.lo..span.hi],"x + 3");
    let CheckedExprKind::Binary(_,left,right)=&expr.kind else{
        panic!("checked expression is not binary: {:?}",expr.kind)
    };
    let lhs=left.source_span.expect("lvalue conversion must preserve x source");
    assert_eq!(&source[lhs.lo..lhs.hi],"x");
    let rhs=right.source_span.expect("literal must preserve its exact bytes");
    assert_eq!(&source[rhs.lo..rhs.hi],"3");
}

#[test]
fn unannotated_parser_never_manufactures_checked_expression_spans() {
    let src="int f(int x) { return x + 3; }";
    let parsed=checked(src,false);
    let expr=returned(&parsed);
    assert_eq!(expr.source_span,None);
    let CheckedExprKind::Binary(_,left,right)=&expr.kind else{panic!("binary")};
    assert_eq!(left.source_span,None);
    assert_eq!(right.source_span,None);
}
