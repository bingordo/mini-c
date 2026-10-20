//! Streaming, backend-neutral semantic analysis for the supported C subset.
//! A caller delivers one external item at a time; completed function ASTs are
//! never retained in the checker. No filesystem, global state or output side effects.
pub mod types;

use std::collections::HashMap;
use mini_c_syntax::ast::{BinOp, Decl, Expr, Function, Span, Stmt, StorageClass, Type, UnOp};
use mini_c_syntax::diag::Diagnostic;
use mini_c_syntax::stream::{ExternalItem, ItemSource};

pub use types::{CType, IntRank, Target};
use types::type_error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Linkage { None, Internal, External }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category { Value, LValue, Function }

#[derive(Debug, Clone, PartialEq)]
pub enum CheckedExprKind {
    Literal(u64),
    Name(String),
    Unary(UnOp, Box<CheckedExpr>),
    Binary(BinOp, Box<CheckedExpr>, Box<CheckedExpr>),
    Assign(Box<CheckedExpr>, Box<CheckedExpr>),
    Call(Box<CheckedExpr>, Vec<CheckedExpr>),
    Convert(Box<CheckedExpr>),
    Load(Box<CheckedExpr>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedExpr {
    pub ty: CType,
    pub category: Category,
    pub kind: CheckedExprKind,
}

impl CheckedExpr {
    fn value(ty: CType, kind: CheckedExprKind) -> Self {
        Self { ty, category: Category::Value, kind }
    }
    fn rvalue(self) -> Self {
        if self.category == Category::LValue {
            let ty = self.ty.clone();
            Self::value(ty, CheckedExprKind::Load(Box::new(self)))
        } else {
            self
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedDecl {
    pub storage: StorageClass,
    pub name: String,
    pub ty: CType,
    pub linkage: Linkage,
    pub initializer: Option<CheckedExpr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CheckedStmt {
    Block(Vec<CheckedStmt>),
    Decl(CheckedDecl),
    Typedef { name: String, ty: CType },
    Expr(Option<CheckedExpr>),
    Return(Option<CheckedExpr>),
    If { cond: CheckedExpr, then: Box<CheckedStmt>, otherwise: Option<Box<CheckedStmt>> },
    While { cond: CheckedExpr, body: Box<CheckedStmt> },
    Break,
    Continue,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedFunction {
    pub linkage: Linkage,
    pub name: String,
    pub signature: CType,
    pub params: Vec<CheckedDecl>,
    pub body: CheckedStmt,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CheckedItem {
    Declaration(CheckedDecl),
    Typedef { name: String, ty: CType },
    Function(CheckedFunction),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind { Object, Function, Typedef }

#[derive(Debug, Clone)]
pub struct Symbol {
    pub ty: CType,
    pub kind: SymbolKind,
    pub linkage: Linkage,
    pub defined: bool,
}

/// Checked expressions carry their converted value types and lvalue category.
/// Scopes and persistent file symbols are local to this Checker instance.
pub struct Checker {
    target: Target,
    globals: HashMap<String, Symbol>,
    locals: Vec<HashMap<String, Symbol>>,
    result_ty: Option<CType>,
    loop_depth: usize,
}

impl Checker {
    pub fn new(target: Target) -> Result<Self, Diagnostic> {
        Ok(Self { target: target.validate()?, globals: HashMap::new(), locals: Vec::new(), result_ty: None, loop_depth: 0 })
    }

    pub fn global(&self, name: &str) -> Option<&Symbol> { self.globals.get(name) }
    pub fn target(&self) -> Target { self.target }

    fn lookup(&self, name: &str) -> Option<&Symbol> {
        self.locals.iter().rev().find_map(|scope| scope.get(name))
            .or_else(|| self.globals.get(name))
    }

    fn resolve(&self, ty: &Type, span: Span) -> Result<CType, Diagnostic> {
        Ok(match ty {
            Type::Void => CType::Void,
            Type::Char => CType::Integer { rank: IntRank::Char, unsigned: !self.target.char_is_signed },
            Type::Int => CType::int(),
            Type::Long => CType::Integer { rank: IntRank::Long, unsigned: false },
            Type::Unsigned(inner) => match self.resolve(inner, span)? {
                CType::Integer { rank, .. } => CType::Integer { rank, unsigned: true },
                _ => return Err(type_error(span, "unsigned requires an integer type")),
            },
            Type::Alias(name) => {
                let symbol = self.lookup(name).ok_or_else(|| type_error(span, format!("unknown typedef '{name}'")))?;
                if symbol.kind != SymbolKind::Typedef {
                    return Err(type_error(span, format!("'{name}' does not name a type")));
                }
                symbol.ty.clone()
            }
            Type::Pointer(inner) => CType::Pointer(Box::new(self.resolve(inner, span)?)),
            Type::Function { result, params } => {
                let result = self.resolve(result, span)?;
                if matches!(result, CType::Function { .. }) {
                    return Err(type_error(span, "function cannot return a function"));
                }
                let mut converted = Vec::with_capacity(params.len());
                for param in params {
                    let ty = self.resolve(param, span)?;
                    if !ty.is_object() {
                        return Err(type_error(span, "parameter requires a complete non-void object type"));
                    }
                    converted.push(ty);
                }
                CType::Function { result: Box::new(result), params: converted }
            }
            Type::Array(..) => return Err(type_error(span, "array semantics are not implemented")),
        })
    }

    fn define_local(&mut self, name: &str, ty: CType, kind: SymbolKind, span: Span) -> Result<(), Diagnostic> {
        let scope = self.locals.last_mut().expect("local declaration outside a function");
        if scope.contains_key(name) {
            return Err(type_error(span, format!("duplicate declaration of '{name}'")));
        }
        scope.insert(name.to_owned(), Symbol { ty, kind, linkage: Linkage::None, defined: true });
        Ok(())
    }

    fn declare_global(
        &mut self, name: &str, ty: CType, kind: SymbolKind,
        defining: bool, storage: StorageClass, span: Span
    ) -> Result<(), Diagnostic> {
        let requested = if kind == SymbolKind::Typedef {
            Linkage::None
        } else if storage == StorageClass::Static {
            Linkage::Internal
        } else {
            Linkage::External
        };
        if let Some(prior) = self.globals.get_mut(name) {
            if prior.kind != kind || prior.ty != ty {
                return Err(type_error(span, format!("conflicting declaration of '{name}'")));
            }
            // An extern redeclaration inherits a previous internal linkage.
            // Explicit static cannot follow a prior external declaration.
            if prior.linkage != requested && storage != StorageClass::Extern {
                return Err(type_error(span, format!("conflicting linkage for '{name}'")));
            }
            if defining && prior.defined {
                return Err(type_error(span, format!("multiple definitions of '{name}'")));
            }
            prior.defined |= defining;
        } else {
            self.globals.insert(name.to_owned(), Symbol {
                ty, kind, linkage: requested, defined: defining,
            });
        }
        Ok(())
    }

    pub fn check_item(&mut self, item: ExternalItem) -> Result<CheckedItem, Diagnostic> {
        match item {
            ExternalItem::Typedef(decl) => {
                let ty = self.resolve(&decl.ty, decl.span)?;
                self.declare_global(&decl.name, ty.clone(), SymbolKind::Typedef, false, decl.storage, decl.span)?;
                Ok(CheckedItem::Typedef { name: decl.name, ty })
            }
            ExternalItem::Declaration(decl) => self.check_global(decl).map(CheckedItem::Declaration),
            ExternalItem::Function(func) => self.check_function(func).map(CheckedItem::Function),
        }
    }

    fn check_global(&mut self, decl: Decl) -> Result<CheckedDecl, Diagnostic> {
        let ty = self.resolve(&decl.ty, decl.span)?;
        let kind = if matches!(ty, CType::Function { .. }) { SymbolKind::Function } else { SymbolKind::Object };
        if kind == SymbolKind::Object && !ty.is_object() {
            return Err(type_error(decl.span, "object cannot have void or function type"));
        }
        if kind == SymbolKind::Function && decl.init.is_some() {
            return Err(type_error(decl.span, "function declaration cannot have an initializer"));
        }
        let defining = decl.init.is_some();
        self.declare_global(&decl.name, ty.clone(), kind, defining, decl.storage, decl.span)?;
        let initializer = if let Some(initializer) = &decl.init {
            // The supported global integer initializer must be a C constant expression.
            if ty.is_integer() && const_integer(initializer).is_none() {
                return Err(type_error(decl.span, "global initializer is not an integer constant expression"));
            }
            let expression = self.check_expr(initializer, decl.span)?;
            Some(self.convert_for_assignment(expression, &ty, initializer, decl.span)?)
        } else {
            None
        };
        let linkage = self.global(&decl.name).expect("registered global").linkage;
        Ok(CheckedDecl { storage: decl.storage, name: decl.name, ty, linkage, initializer, span: decl.span })
    }

    fn check_function(&mut self, func: Function) -> Result<CheckedFunction, Diagnostic> {
        let result = self.resolve(&func.result, func.span)?;
        if matches!(result, CType::Function { .. }) {
            return Err(type_error(func.span, "function cannot return a function"));
        }
        let mut params = Vec::new();
        for param in &func.params {
            let ty = self.resolve(&param.ty, param.span)?;
            if !ty.is_object() {
                return Err(type_error(param.span, "function parameter cannot have void or function type"));
            }
            params.push(CheckedDecl {
                storage: StorageClass::Default, name: param.name.clone(), ty,
                linkage: Linkage::None, initializer: None, span: param.span,
            });
        }
        let signature = CType::Function {
            result: Box::new(result.clone()),
            params: params.iter().map(|p| p.ty.clone()).collect(),
        };
        // Register before checking the body: recursive calls are legal.
        self.declare_global(&func.name, signature.clone(), SymbolKind::Function, true, func.storage, func.span)?;
        self.locals.push(HashMap::new());
        self.result_ty = Some(result);
        for param in &params {
            if let Err(err) = self.define_local(&param.name, param.ty.clone(), SymbolKind::Object, param.span) {
                self.locals.pop();
                self.result_ty = None;
                return Err(err);
            }
        }
        // Function parameters and its outermost compound statement share one scope.
        let checked = match &func.body {
            Stmt::Block(stmts) => self.check_statements(stmts, func.span).map(CheckedStmt::Block),
            other => self.check_stmt(other, func.span),
        };
        self.locals.pop();
        self.result_ty = None;
        let body = checked?;
        let linkage = self.global(&func.name).expect("registered function").linkage;
        Ok(CheckedFunction { linkage, name: func.name, signature, params, body, span: func.span })
    }

    fn check_statements(&mut self, statements: &[Stmt], span: Span) -> Result<Vec<CheckedStmt>, Diagnostic> {
        statements.iter().map(|s| self.check_stmt(s, span)).collect()
    }

    fn check_stmt(&mut self, stmt: &Stmt, span: Span) -> Result<CheckedStmt, Diagnostic> {
        Ok(match stmt {
            Stmt::Block(stmts) => {
                self.locals.push(HashMap::new());
                let inner = self.check_statements(stmts, span);
                self.locals.pop();
                CheckedStmt::Block(inner?)
            }
            Stmt::Decl(decl) => {
                let ty = self.resolve(&decl.ty, decl.span)?;
                if !ty.is_object() {
                    return Err(type_error(decl.span, "local variable cannot have void or function type"));
                }
                if decl.storage == StorageClass::Extern {
                    if decl.init.is_some() {
                        return Err(type_error(decl.span, "block-scope extern cannot have an initializer"));
                    }
                    self.declare_global(&decl.name, ty.clone(), SymbolKind::Object, false, decl.storage, decl.span)?;
                }
                self.define_local(&decl.name, ty.clone(), SymbolKind::Object, decl.span)?;
                if decl.storage == StorageClass::Static && decl.init.as_ref().is_some_and(|init| const_integer(init).is_none()) {
                    return Err(type_error(decl.span, "static initializer is not an integer constant expression"));
                }
                let initializer = decl.init.as_ref().map(|value| -> Result<_, Diagnostic> {
                    let value = self.check_expr(value, decl.span)?;
                    self.convert_for_assignment(value, &ty, decl.init.as_ref().expect("initializer exists"), decl.span)
                }).transpose()?;
                CheckedStmt::Decl(CheckedDecl {
                    storage: decl.storage, name: decl.name.clone(), ty,
                    linkage: if decl.storage == StorageClass::Extern { Linkage::External } else { Linkage::None },
                    initializer, span: decl.span,
                })
            }
            Stmt::Typedef(decl) => {
                let ty = self.resolve(&decl.ty, decl.span)?;
                self.define_local(&decl.name, ty.clone(), SymbolKind::Typedef, decl.span)?;
                CheckedStmt::Typedef { name: decl.name.clone(), ty }
            }
            Stmt::Expr(expr) => {
                CheckedStmt::Expr(expr.as_ref().map(|e| self.check_expr(e, span)).transpose()?)
            }
            Stmt::Return(value) => {
                let expected = self.result_ty.clone().expect("return outside function");
                let checked = match (value, &expected) {
                    (None, CType::Void) => None,
                    (None, _) => return Err(type_error(span, "non-void function must return a value")),
                    (Some(_), CType::Void) => return Err(type_error(span, "void function cannot return a value")),
                    (Some(value), _) => {
                        let expr = self.check_expr(value, span)?;
                        Some(self.convert_for_assignment(expr, &expected, value, span)?)
                    }
                };
                CheckedStmt::Return(checked)
            }
            Stmt::If { cond, then, otherwise } => {
                let cond = self.check_condition(cond, span)?;
                let then = Box::new(self.check_stmt(then, span)?);
                let otherwise = otherwise.as_ref().map(|s| self.check_stmt(s, span).map(Box::new)).transpose()?;
                CheckedStmt::If { cond, then, otherwise }
            }
            Stmt::While { cond, body } => {
                let cond = self.check_condition(cond, span)?;
                self.loop_depth += 1;
                let body_result = self.check_stmt(body, span);
                self.loop_depth -= 1;
                CheckedStmt::While { cond, body: Box::new(body_result?) }
            }
            Stmt::Break => {
                if self.loop_depth == 0 { return Err(type_error(span, "break outside loop")); }
                CheckedStmt::Break
            }
            Stmt::Continue => {
                if self.loop_depth == 0 { return Err(type_error(span, "continue outside loop")); }
                CheckedStmt::Continue
            }
        })
    }

    fn check_condition(&self, cond: &Expr, span: Span) -> Result<CheckedExpr, Diagnostic> {
        let typed = self.check_expr(cond, span)?.rvalue();
        if !typed.ty.is_scalar() {
            return Err(type_error(span, "condition requires a scalar type"));
        }
        Ok(typed)
    }

    fn convert(&self, expr: CheckedExpr, ty: &CType) -> CheckedExpr {
        let expr = expr.rvalue();
        if &expr.ty == ty { return expr; }
        CheckedExpr::value(ty.clone(), CheckedExprKind::Convert(Box::new(expr)))
    }

    fn convert_for_assignment(&self, expr: CheckedExpr, ty: &CType, original: &Expr, span: Span) -> Result<CheckedExpr, Diagnostic> {
        let compatible = match (ty, &expr.ty) {
            (CType::Integer { .. }, CType::Integer { .. }) => true,
            (CType::Pointer(lhs), CType::Pointer(rhs)) =>
                lhs == rhs || **lhs == CType::Void || **rhs == CType::Void,
            (CType::Pointer(_), CType::Integer { .. }) => const_integer(original) == Some(0),
            _ => false,
        };
        if !compatible {
            return Err(type_error(span, format!("incompatible assignment: {:?} to {:?}", expr.ty, ty)));
        }
        Ok(self.convert(expr, ty))
    }

    fn check_expr(&self, expr: &Expr, span: Span) -> Result<CheckedExpr, Diagnostic> {
        match expr {
            Expr::Int(number) => {
                let ty = self.target.decimal_literal(*number).ok_or_else(|| type_error(span, "decimal literal exceeds supported signed integer types"))?;
                Ok(CheckedExpr::value(ty, CheckedExprKind::Literal(*number)))
            }
            Expr::Name(name) => {
                let symbol = self.lookup(name).ok_or_else(|| type_error(span, format!("undeclared identifier '{name}'")))?;
                let category = match symbol.kind {
                    SymbolKind::Typedef => return Err(type_error(span, format!("typedef name '{name}' used as expression"))),
                    SymbolKind::Function => Category::Function,
                    SymbolKind::Object => Category::LValue,
                };
                Ok(CheckedExpr { ty: symbol.ty.clone(), category, kind: CheckedExprKind::Name(name.clone()) })
            }
            Expr::Unary(op, inner) => {
                let typed = self.check_expr(inner, span)?;
                match op {
                    UnOp::Address => {
                        if typed.category == Category::Value {
                            return Err(type_error(span, "address-of requires an lvalue or function"));
                        }
                        let ty = CType::Pointer(Box::new(typed.ty.clone()));
                        Ok(CheckedExpr::value(ty, CheckedExprKind::Unary(*op, Box::new(typed))))
                    }
                    UnOp::Deref => {
                        let CType::Pointer(inner_ty) = &typed.ty else {
                            return Err(type_error(span, "dereference requires a pointer"));
                        };
                        let ty = (**inner_ty).clone();
                        let category = if matches!(ty, CType::Function { .. }) { Category::Function } else if ty == CType::Void {
                            return Err(type_error(span, "cannot dereference void pointer"));
                        } else { Category::LValue };
                        Ok(CheckedExpr { ty, category, kind: CheckedExprKind::Unary(*op, Box::new(typed.rvalue())) })
                    }
                    UnOp::Not => {
                        if !typed.ty.is_scalar() { return Err(type_error(span, "logical not requires scalar operand")); }
                        Ok(CheckedExpr::value(CType::int(), CheckedExprKind::Unary(*op, Box::new(typed.rvalue()))))
                    }
                    UnOp::Plus | UnOp::Neg => {
                        if !typed.ty.is_integer() { return Err(type_error(span, "arithmetic unary operator requires integer")); }
                        let ty = self.target.promote(&typed.ty);
                        Ok(CheckedExpr::value(ty.clone(), CheckedExprKind::Unary(*op, Box::new(self.convert(typed, &ty)))))
                    }
                }
            }
            Expr::Binary(op, lhs, rhs) => {
                let left = self.check_expr(lhs, span)?.rvalue();
                let right = self.check_expr(rhs, span)?.rvalue();
                let logical = matches!(op, BinOp::And | BinOp::Or);
                let relation = matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge);
                if logical {
                    if !(left.ty.is_scalar() && right.ty.is_scalar()) {
                        return Err(type_error(span, "logical operator requires scalar operands"));
                    }
                    return Ok(CheckedExpr::value(CType::int(), CheckedExprKind::Binary(*op, Box::new(left), Box::new(right))));
                }
                if relation && (matches!((&left.ty, &right.ty), (CType::Pointer(a), CType::Pointer(b)) if a == b)
                    || (matches!(op, BinOp::Eq | BinOp::Ne)
                        && ((matches!(left.ty, CType::Pointer(_)) && const_integer(rhs) == Some(0))
                            || (matches!(right.ty, CType::Pointer(_)) && const_integer(lhs) == Some(0))))) {
                    return Ok(CheckedExpr::value(CType::int(), CheckedExprKind::Binary(*op, Box::new(left), Box::new(right))));
                }
                let common = self.target.common_integer(&left.ty, &right.ty)
                    .ok_or_else(|| type_error(span, "operator requires compatible integer operands"))?;
                let left = self.convert(left, &common);
                let right = self.convert(right, &common);
                let result = if relation { CType::int() } else { common };
                Ok(CheckedExpr::value(result, CheckedExprKind::Binary(*op, Box::new(left), Box::new(right))))
            }
            Expr::Assign(lhs, rhs) => {
                let lhs = self.check_expr(lhs, span)?;
                if lhs.category != Category::LValue {
                    return Err(type_error(span, "left side of assignment must be a modifiable lvalue"));
                }
                let rhs_typed = self.check_expr(rhs, span)?;
                let rhs_typed = self.convert_for_assignment(rhs_typed, &lhs.ty, rhs, span)?;
                Ok(CheckedExpr::value(lhs.ty.clone(), CheckedExprKind::Assign(Box::new(lhs), Box::new(rhs_typed))))
            }
            Expr::Call(callee, args) => {
                let callee = self.check_expr(callee, span)?;
                let signature = match &callee.ty {
                    CType::Function { result, params } => Some((result.as_ref(), params.as_slice())),
                    CType::Pointer(inner) => match inner.as_ref() {
                        CType::Function { result, params } => Some((result.as_ref(), params.as_slice())),
                        _ => None,
                    },
                    _ => None,
                };
                let Some((result, params)) = signature else {
                    return Err(type_error(span, "called expression is not a function"));
                };
                if args.len() != params.len() {
                    return Err(type_error(span, format!("function expects {} arguments, got {}", params.len(), args.len())));
                }
                let mut checked_args = Vec::with_capacity(args.len());
                for (arg, ty) in args.iter().zip(params) {
                    let typed = self.check_expr(arg, span)?;
                    checked_args.push(self.convert_for_assignment(typed, ty, arg, span)?);
                }
                let ty = result.clone();
                Ok(CheckedExpr::value(ty, CheckedExprKind::Call(Box::new(callee), checked_args)))
            }
        }
    }
}

/// Minimal integer constant-expression evaluator, deliberately independent of
/// the checked expression IR. Checked operations avoid host overflows/panics.
fn const_integer(expr: &Expr) -> Option<i128> {
    match expr {
        Expr::Int(v) => Some(i128::from(*v)),
        Expr::Unary(UnOp::Plus, e) => const_integer(e),
        Expr::Unary(UnOp::Neg, e) => const_integer(e)?.checked_neg(),
        Expr::Unary(UnOp::Not, e) => Some(i128::from(const_integer(e)? == 0)),
        Expr::Binary(op, a, b) => {
            let lhs = const_integer(a)?;
            let rhs = const_integer(b)?;
            match op {
                BinOp::Add => lhs.checked_add(rhs),
                BinOp::Sub => lhs.checked_sub(rhs),
                BinOp::Mul => lhs.checked_mul(rhs),
                BinOp::Div => lhs.checked_div(rhs),
                BinOp::Mod => lhs.checked_rem(rhs),
                BinOp::Eq => Some(i128::from(lhs == rhs)),
                BinOp::Ne => Some(i128::from(lhs != rhs)),
                BinOp::Lt => Some(i128::from(lhs < rhs)),
                BinOp::Le => Some(i128::from(lhs <= rhs)),
                BinOp::Gt => Some(i128::from(lhs > rhs)),
                BinOp::Ge => Some(i128::from(lhs >= rhs)),
                BinOp::And => Some(i128::from(lhs != 0 && rhs != 0)),
                BinOp::Or => Some(i128::from(lhs != 0 || rhs != 0)),
            }
        }
        _ => None,
    }
}

/// Consume one translation unit without collecting its function ASTs.
/// The caller can consume/drop each checked function immediately.
pub fn check_stream<S, F>(parser: &mut S, checker: &mut Checker, mut receive: F) -> Result<(), Diagnostic>
where
    S: ItemSource<Error = Diagnostic>,
    F: FnMut(CheckedItem) -> Result<(), Diagnostic>,
{
    while let Some(item) = parser.next_item()? {
        receive(checker.check_item(item)?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
