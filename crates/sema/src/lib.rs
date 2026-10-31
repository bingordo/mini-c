//! Streaming, backend-neutral semantic analysis for the supported C subset.
//! A caller delivers one external item at a time; completed function ASTs are
//! never retained in the checker. No filesystem, global state or output side effects.
pub mod types;
pub mod const_eval;

use std::collections::HashMap;
use mini_c_syntax::ast::{BinOp, Decl, Expr, Function, Span, Stmt, StorageClass, Type, UnOp};
use mini_c_syntax::diag::{Diagnostic, DiagnosticCode};
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
    /// Array lvalue conversion yields a pointer to the first element,
    /// without loading the entire aggregate into a scalar.
    ArrayDecay(Box<CheckedExpr>),
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
            match &self.ty {
                CType::Array(element, _) => Self::value(
                    CType::Pointer(element.clone()),
                    CheckedExprKind::ArrayDecay(Box::new(self)),
                ),
                _ => {
                    let ty = self.ty.clone();
                    Self::value(ty, CheckedExprKind::Load(Box::new(self)))
                }
            }
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
    // Enabled only by the recovery API. Normal compilation has no
    // transactional logging or symbol-table copying overhead.
    rollback_log: Option<HashMap<String, Option<Symbol>>>,
    statement_undo: Option<HashMap<String, Option<Symbol>>>,
    recovery_errors: Option<Vec<Diagnostic>>,
    recovery_limit: usize,
    recovery_depth: usize,
}

impl Checker {
    pub fn new(target: Target) -> Result<Self, Diagnostic> {
        Ok(Self { target: target.validate()?, globals: HashMap::new(), locals: Vec::new(), result_ty: None, loop_depth: 0, rollback_log: None, statement_undo: None,
            recovery_errors: None, recovery_limit: 0, recovery_depth: 0 })
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
            Type::Signed(inner) => match self.resolve(inner, span)? {
                CType::Integer { rank, .. } => CType::Integer { rank, unsigned: false },
                _ => return Err(type_error(span, "signed requires an integer type")),
            },
            Type::Unsigned(inner) => match self.resolve(inner, span)? {
                CType::Integer { rank, .. } => CType::Integer { rank, unsigned: true },
                _ => return Err(type_error(span, "unsigned requires an integer type")),
            },
            Type::Alias(name) => {
                let symbol = self.lookup(name).ok_or_else(|| Diagnostic::coded(span, DiagnosticCode::SemanticUnknownTypedef, format!("unknown typedef '{name}'")))?;
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
                    let ty = self.adjust_parameter(self.resolve(param, span)?);
                    if !ty.is_object() {
                        return Err(type_error(span, "parameter requires a complete non-void object type"));
                    }
                    converted.push(ty);
                }
                CType::Function { result: Box::new(result), params: converted }
            }
            Type::Array(element, bound) => {
                let element = self.resolve(element, span)?;
                if !element.is_object() || matches!(element, CType::Array(_, None)) {
                    return Err(type_error(span, "array element requires a complete object type"));
                }
                let bound = bound.as_deref().map(|expr| -> Result<u64, Diagnostic> {
                    let typed = self.check_expr(expr, span)?.rvalue();
                    if !typed.ty.is_integer() {
                        return Err(type_error(span, "array bound requires an integer constant expression"));
                    }
                    let value = const_eval::evaluate(&typed, self.target)
                        .map_err(|reason| type_error(span, format!("array bound requires an integer constant expression: {reason}")))?;
                    if value <= 0 {
                        return Err(type_error(span, "array bound must be positive"));
                    }
                    u64::try_from(value).map_err(|_| type_error(span, "array bound exceeds the supported range"))
                }).transpose()?;
                CType::Array(Box::new(element), bound)
            },
        })
    }

    /// C adjusts array and function parameter declarations to pointers.
    /// Do this at both prototype resolution and definition checking.
    fn adjust_parameter(&self, ty: CType) -> CType {
        match ty {
            CType::Array(element, _) => CType::Pointer(element),
            CType::Function { .. } => CType::Pointer(Box::new(ty)),
            other => other,
        }
    }

    fn define_local(&mut self, name: &str, ty: CType, kind: SymbolKind, linkage: Linkage, span: Span) -> Result<(), Diagnostic> {
        let scope = self.locals.last_mut().expect("local declaration outside a function");
        if scope.contains_key(name) {
            return Err(Diagnostic::coded(span, DiagnosticCode::SemanticDuplicateDeclaration, format!("duplicate declaration of '{name}'")));
        }
        scope.insert(name.to_owned(), Symbol { ty, kind, linkage, defined: true });
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
        if self.rollback_log.is_some() || self.statement_undo.is_some() {
            // Log the first value before a tentative mutation. The outer
            // transaction covers the whole external item; the inner one
            // covers a single statement during diagnostic recovery.
            let previous = self.globals.get(name).cloned();
            if let Some(log) = &mut self.rollback_log {
                log.entry(name.to_owned()).or_insert_with(|| previous.clone());
            }
            if let Some(log) = &mut self.statement_undo {
                log.entry(name.to_owned()).or_insert(previous);
            }
        }
        if let Some(prior) = self.globals.get_mut(name) {
            if prior.kind != kind || prior.ty != ty {
                return Err(Diagnostic::coded(span, DiagnosticCode::SemanticConflictingDeclaration, format!("conflicting declaration of '{name}'")));
            }
            // An extern redeclaration inherits a previous internal linkage.
            // Explicit static cannot follow a prior external declaration.
            if prior.linkage != requested && storage != StorageClass::Extern {
                return Err(type_error(span, format!("conflicting linkage for '{name}'")));
            }
            if defining && prior.defined {
                return Err(Diagnostic::coded(span, DiagnosticCode::SemanticMultipleDefinitions, format!("multiple definitions of '{name}'")));
            }
            prior.defined |= defining;
        } else {
            self.globals.insert(name.to_owned(), Symbol {
                ty, kind, linkage: requested, defined: defining,
            });
        }
        Ok(())
    }

    fn restore_symbols(&mut self, changed: HashMap<String, Option<Symbol>>) {
        for (name, previous) in changed {
            match previous {
                Some(symbol) => { self.globals.insert(name, symbol); }
                None => { self.globals.remove(&name); }
            }
        }
    }

    /// Diagnostic-only entry point. Independent top-level statements inside
    /// each function body can produce separate errors, while the failed
    /// function is never committed to the persistent symbol table.
    /// A successful function still commits normally.
    pub fn check_item_diagnostics(
        &mut self, item: ExternalItem, max_errors: usize
    ) -> Vec<Diagnostic> {
        if max_errors == 0 { return Vec::new(); }
        assert!(self.rollback_log.is_none() && self.recovery_errors.is_none(),
            "nested semantic diagnostic recovery is unsupported");
        self.rollback_log = Some(HashMap::new());
        self.recovery_errors = Some(Vec::new());
        self.recovery_limit = max_errors;
        self.recovery_depth = 0;
        let result = self.check_item(item);
        let mut errors = self.recovery_errors.take().expect("diagnostic collection");
        if let Err(error) = result {
            if errors.len() < max_errors { errors.push(error); }
        }
        let changed = self.rollback_log.take().expect("symbol transaction");
        if !errors.is_empty() {
            self.restore_symbols(changed);
        }
        self.statement_undo = None;
        self.recovery_limit = 0;
        self.recovery_depth = 0;
        errors
    }

    /// Check an external item transactionally, rolling back only the globals
    /// touched on failure. The normal fail-fast entry point stays allocation
    /// free beyond the compilation's necessary symbol table operations.
    pub fn check_item_recovering(
        &mut self, item: ExternalItem
    ) -> Result<CheckedItem, Diagnostic> {
        assert!(self.rollback_log.is_none(), "nested semantic transactions are unsupported");
        self.rollback_log = Some(HashMap::new());
        let result = self.check_item(item);
        let changed = self.rollback_log.take().expect("transaction log");
        if result.is_err() {
            for (name, previous) in changed {
                match previous {
                    Some(symbol) => { self.globals.insert(name, symbol); }
                    None => { self.globals.remove(&name); }
                }
            }
        }
        result
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
            let expression = self.check_expr(initializer, decl.span)?;
            self.validate_static_initializer(&expression, initializer, &ty, decl.span)?;
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
            let ty = self.adjust_parameter(self.resolve(&param.ty, param.span)?);
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
            if let Err(err) = self.define_local(&param.name, param.ty.clone(), SymbolKind::Object, Linkage::None, param.span) {
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
        // In normal compilation, or while inside an already checked nested
        // statement, keep the original fail-fast behavior and no snapshots.
        if self.recovery_errors.is_none() || self.recovery_depth != 0 {
            return statements.iter().map(|s| self.check_stmt(s, span)).collect();
        }
        let mut checked = Vec::with_capacity(statements.len());
        for statement in statements {
            if self.recovery_errors.as_ref()
                .is_some_and(|errors| errors.len() >= self.recovery_limit) {
                break;
            }
            // Recovery-only local snapshot. Global changes use undo logs,
            // avoiding O(total globals) clones on every statement.
            let local_snapshot = self.locals.clone();
            let loop_depth = self.loop_depth;
            self.statement_undo = Some(HashMap::new());
            self.recovery_depth += 1;
            let result = self.check_stmt(statement, span);
            self.recovery_depth -= 1;
            let changed = self.statement_undo.take().expect("statement transaction");
            match result {
                Ok(stmt) => checked.push(stmt),
                Err(error) => {
                    self.locals = local_snapshot;
                    self.loop_depth = loop_depth;
                    self.restore_symbols(changed);
                    self.recovery_errors.as_mut().expect("diagnostic collection").push(error);
                }
            }
        }
        Ok(checked)
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
                let linkage = if decl.storage == StorageClass::Extern {
                    self.global(&decl.name).expect("registered extern").linkage
                } else { Linkage::None };
                self.define_local(&decl.name, ty.clone(), SymbolKind::Object, linkage, decl.span)?;
                let initializer = decl.init.as_ref().map(|value| -> Result<_, Diagnostic> {
                    let expression = self.check_expr(value, decl.span)?;
                    if decl.storage == StorageClass::Static {
                        self.validate_static_initializer(&expression, value, &ty, decl.span)?;
                    }
                    self.convert_for_assignment(expression, &ty, value, decl.span)
                }).transpose()?;
                CheckedStmt::Decl(CheckedDecl {
                    storage: decl.storage, name: decl.name.clone(), ty,
                    linkage,
                    initializer, span: decl.span,
                })
            }
            Stmt::Typedef(decl) => {
                let ty = self.resolve(&decl.ty, decl.span)?;
                self.define_local(&decl.name, ty.clone(), SymbolKind::Typedef, Linkage::None, decl.span)?;
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

    fn validate_static_initializer(
        &self, expression: &CheckedExpr, original: &Expr, ty: &CType, span: Span
    ) -> Result<(), Diagnostic> {
        if matches!(ty, CType::Array(..)) {
            // Array initialization needs aggregate/string initializer syntax and
            // element-wise checking. Never accept a scalar expression as a
            // complete array initializer just because it is constant.
            return Err(type_error(span, "array initializer requires aggregate initialization support"));
        }
        if ty.is_integer() {
            const_eval::evaluate(expression, self.target).map_err(|why|
                type_error(span, format!("initializer is not a valid integer constant expression: {why}")))?;
        } else if matches!(ty, CType::Pointer(_)) {
            let null = const_eval::evaluate(expression, self.target) == Ok(0);
            let address_of_global = match original {
                Expr::Unary(UnOp::Address, inner) => match inner.as_ref() {
                    Expr::Name(name) => self.globals.get(name).is_some_and(|s| s.kind != SymbolKind::Typedef),
                    _ => false,
                },
                Expr::Name(name) => self.globals.get(name).is_some_and(|s|
                    matches!(&s.ty, CType::Array(..))),
                _ => false,
            };
            if !null && !address_of_global {
                return Err(type_error(span, "pointer initializer requires a null constant or address of global symbol"));
            }
        }
        Ok(())
    }

    fn convert(&self, expr: CheckedExpr, ty: &CType) -> CheckedExpr {
        let expr = expr.rvalue();
        if &expr.ty == ty { return expr; }
        CheckedExpr::value(ty.clone(), CheckedExprKind::Convert(Box::new(expr)))
    }

    fn convert_for_assignment(&self, expr: CheckedExpr, ty: &CType, _original: &Expr, span: Span) -> Result<CheckedExpr, Diagnostic> {
        // The source array undergoes the standard array-to-pointer conversion
        // before checking scalar assignment compatibility.
        let expr = expr.rvalue();
        let compatible = match (ty, &expr.ty) {
            (CType::Integer { .. }, CType::Integer { .. }) => true,
            (CType::Pointer(lhs), CType::Pointer(rhs)) =>
                lhs == rhs || **lhs == CType::Void || **rhs == CType::Void,
            (CType::Pointer(_), CType::Integer { .. }) => const_eval::evaluate(&expr, self.target) == Ok(0),
            _ => false,
        };
        if !compatible {
            return Err(Diagnostic::coded(span, DiagnosticCode::SemanticIncompatibleAssignment, format!("incompatible assignment: {:?} to {:?}", expr.ty, ty)));
        }
        Ok(self.convert(expr, ty))
    }

    /// Form a checked pointer + integer or pointer - integer expression.
    /// Address scaling is postponed to lowering, where the pointee size is known.
    fn pointer_offset(
        &self, op: BinOp, pointer: CheckedExpr, integer: CheckedExpr, span: Span
    ) -> Result<CheckedExpr, Diagnostic> {
        let CType::Pointer(element) = &pointer.ty else {
            return Err(type_error(span, "pointer arithmetic requires a pointer"));
        };
        if element.size_bytes(self.target).is_none() {
            return Err(type_error(span, "pointer arithmetic requires a complete object type"));
        }
        if !integer.ty.is_integer() {
            return Err(type_error(span, "pointer arithmetic requires an integer offset"));
        }
        let integer_ty = self.target.promote(&integer.ty);
        let integer = self.convert(integer, &integer_ty);
        Ok(CheckedExpr::value(pointer.ty.clone(), CheckedExprKind::Binary(
            op, Box::new(pointer), Box::new(integer),
        )))
    }

    fn check_expr(&self, expr: &Expr, span: Span) -> Result<CheckedExpr, Diagnostic> {
        match expr {
            Expr::Int(number) => {
                let ty = self.target.decimal_literal(*number).ok_or_else(|| type_error(span, "decimal literal exceeds supported signed integer types"))?;
                Ok(CheckedExpr::value(ty, CheckedExprKind::Literal(*number)))
            }
            Expr::Name(name) => {
                let symbol = self.lookup(name).ok_or_else(|| Diagnostic::coded(span, DiagnosticCode::SemanticUndeclaredIdentifier, format!("undeclared identifier '{name}'")))?;
                let category = match symbol.kind {
                    SymbolKind::Typedef => return Err(type_error(span, format!("typedef name '{name}' used as expression"))),
                    SymbolKind::Function => Category::Function,
                    SymbolKind::Object => Category::LValue,
                };
                Ok(CheckedExpr { ty: symbol.ty.clone(), category, kind: CheckedExprKind::Name(name.clone()) })
            }
            Expr::Index(base, offset) => {
                // C defines E1[E2] as *((E1) + (E2)); either operand may
                // provide the pointer, including multidimensional array decay.
                let base = self.check_expr(base, span)?.rvalue();
                let offset = self.check_expr(offset, span)?.rvalue();
                let (ptr, idx) = if matches!(base.ty, CType::Pointer(_)) {
                    (base, offset)
                } else if matches!(offset.ty, CType::Pointer(_)) {
                    (offset, base)
                } else {
                    return Err(type_error(span, "subscript requires pointer and integer operands"));
                };
                let added = self.pointer_offset(BinOp::Add, ptr, idx, span)?;
                let CType::Pointer(element) = &added.ty else { unreachable!() };
                Ok(CheckedExpr {
                    ty: element.as_ref().clone(),
                    category: Category::LValue,
                    kind: CheckedExprKind::Unary(UnOp::Deref, Box::new(added)),
                })
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
                if relation && (matches!((&left.ty, &right.ty), (CType::Pointer(a), CType::Pointer(b))
                        if a == b || **a == CType::Void || **b == CType::Void)
                    || (matches!(op, BinOp::Eq | BinOp::Ne)
                        && ((matches!(left.ty, CType::Pointer(_)) && const_eval::evaluate(&right, self.target) == Ok(0))
                            || (matches!(right.ty, CType::Pointer(_)) && const_eval::evaluate(&left, self.target) == Ok(0))))) {
                    return Ok(CheckedExpr::value(CType::int(), CheckedExprKind::Binary(*op, Box::new(left), Box::new(right))));
                }
                // C pointer arithmetic allows p + n, n + p, and p - n.
                // Pointer difference p - q and relational pointer semantics are
                // separate follow-ups; never treat addresses as ordinary ints.
                if matches!(op, BinOp::Add | BinOp::Sub) {
                    if matches!(left.ty, CType::Pointer(_)) && right.ty.is_integer() {
                        return self.pointer_offset(*op, left, right, span);
                    }
                    if *op == BinOp::Add && left.ty.is_integer()
                        && matches!(right.ty, CType::Pointer(_))
                    {
                        return self.pointer_offset(*op, right, left, span);
                    }
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
                if lhs.category != Category::LValue || matches!(lhs.ty, CType::Array(..)) {
                    return Err(Diagnostic::coded(span, DiagnosticCode::SemanticInvalidAssignmentTarget, "left side of assignment must be a modifiable lvalue"));
                }
                let rhs_typed = self.check_expr(rhs, span)?;
                let rhs_typed = self.convert_for_assignment(rhs_typed, &lhs.ty, rhs, span)?;
                Ok(CheckedExpr::value(lhs.ty.clone(), CheckedExprKind::Assign(Box::new(lhs), Box::new(rhs_typed))))
            }
            Expr::Call(callee, args) => {
                let callee = self.check_expr(callee, span)?.rvalue();
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
                    return Err(Diagnostic::coded(span, DiagnosticCode::SemanticWrongArgumentCount, format!("function expects {} arguments, got {}", params.len(), args.len())));
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

/// Opt-in bounded syntax + semantic error collection. Each completed function
/// is released immediately; a failed semantic item does not poison later items.
/// The checker retains only committed file-scope declarations.
pub fn diagnose_stream_recovering(
    parser: &mut mini_c_syntax::parser::Parser<'_>,
    checker: &mut Checker,
    max_errors: usize,
) -> Vec<Diagnostic> {
    parser.for_each_item_collecting(max_errors, |item, remaining| {
        checker.check_item_diagnostics(item, remaining)
    })
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
