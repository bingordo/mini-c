//! Streaming, backend-neutral semantic analysis for the supported C subset.
//! A caller delivers one external item at a time; completed function ASTs are
//! never retained in the checker. No filesystem, global state or output side effects.
pub mod types;
pub mod const_eval;

use std::collections::HashMap;
use std::cell::{Cell, RefCell};
use mini_c_syntax::ast::{BinOp, Decl, Expr, Function, Span, Stmt, StorageClass, TagKind, Type, UnOp};
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
    EnumConstant(i64),
    /// Narrow C string including its trailing NUL; storage is emitted by the backend.
    StringLiteral(Vec<u8>),
    /// Typed, positional aggregate initializer values, in member order.
    Initializer(Vec<CheckedExpr>),
    Name(String),
    Unary(UnOp, Box<CheckedExpr>),
    Binary(BinOp, Box<CheckedExpr>, Box<CheckedExpr>),
    /// Sequenced left-to-right comma; result is always a non-lvalue.
    Comma(Box<CheckedExpr>, Box<CheckedExpr>),
    /// Lazy, non-lvalue C conditional; both arms are coerced to one result type.
    Conditional(Box<CheckedExpr>, Box<CheckedExpr>, Box<CheckedExpr>),
    Assign(Box<CheckedExpr>, Box<CheckedExpr>),
    /// The compound operator is applied to the old lvalue and right operand
    /// at computation_ty, then narrowed once to the lvalue type.
    CompoundAssign(BinOp, Box<CheckedExpr>, Box<CheckedExpr>, CType),
    /// Prefix/postfix ++/--, preserving a single lvalue and its computation type.
    Update { target: Box<CheckedExpr>, increment: bool, postfix: bool, computation_ty: CType },
    Call(Box<CheckedExpr>, Vec<CheckedExpr>),
    Convert(Box<CheckedExpr>),
    Load(Box<CheckedExpr>),
    /// Array lvalue conversion yields a pointer to the first element,
    /// without loading the entire aggregate into a scalar.
    ArrayDecay(Box<CheckedExpr>),
    /// Addressable member of a structure, with its checked byte offset.
    Member(Box<CheckedExpr>, u64),
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
        // A function designator converts to a pointer to that function in
        // value contexts, except for the operand of the address-of operator.
        if self.category == Category::Function {
            let ty = CType::Pointer(Box::new(self.ty.clone()));
            return Self::value(ty, CheckedExprKind::Unary(UnOp::Address, Box::new(self)));
        }
        if self.category == Category::LValue {
            match self.ty.unqualified() {
                CType::Array(element, _) => Self::value(
                    CType::Pointer(element.clone()),
                    CheckedExprKind::ArrayDecay(Box::new(self)),
                ),
                _ => {
                    // C lvalue conversion removes top-level const but keeps
                    // the pointee qualifier of any pointer value.
                    let ty = self.ty.unqualified().clone();
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
    Tag,
    Expr(Option<CheckedExpr>),
    Return(Option<CheckedExpr>),
    If { cond: CheckedExpr, then: Box<CheckedStmt>, otherwise: Option<Box<CheckedStmt>> },
    While { cond: CheckedExpr, body: Box<CheckedStmt> },
    DoWhile { body: Box<CheckedStmt>, cond: CheckedExpr },
    For(Box<CheckedFor>),
    Break,
    Continue,
}

/// Keep the relatively large for-loop payload out of the main statement enum.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckedFor {
    pub init: Option<Box<CheckedStmt>>,
    pub cond: Option<CheckedExpr>,
    pub step: Option<CheckedExpr>,
    pub body: Box<CheckedStmt>,
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
    Tag,
    Function(CheckedFunction),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind { Object, Function, Typedef, Enumerator }

#[derive(Debug, Clone)]
pub struct Symbol {
    pub ty: CType,
    pub kind: SymbolKind,
    pub linkage: Linkage,
    pub defined: bool,
    pub enum_value: Option<i64>,
}

#[derive(Clone)]
struct TagEntry {
    kind: TagKind,
    identity: u64,
    complete: Option<CType>,
}

/// Checked expressions carry their converted value types and lvalue category.
/// Scopes and persistent file symbols are local to this Checker instance.
pub struct Checker {
    target: Target,
    next_tag_identity: Cell<u64>,
    /// Independent tag namespace, parallel to ordinary identifier scopes.
    tags: RefCell<Vec<HashMap<String, TagEntry>>>,
    /// Stable definition lookup independent of currently visible shadowing tags.
    tag_definitions: RefCell<HashMap<u64, CType>>,
    globals: HashMap<String, Symbol>,
    locals: Vec<HashMap<String, Symbol>>,
    result_ty: Option<CType>,
    loop_depth: usize,
    // Enabled only by the recovery API. Normal compilation has no
    // transactional logging or symbol-table copying overhead.
    rollback_log: Option<HashMap<String, Option<Symbol>>>,
    // One undo map per active statement. Changes are recorded in every
    // ancestor map as well, so a failing enclosing statement can roll back
    // successful nested declarations without losing earlier symbol values.
    statement_undo: Vec<HashMap<String, Option<Symbol>>>,
    recovery_errors: Option<Vec<Diagnostic>>,
    recovery_limit: usize,
}

impl Checker {
    pub fn new(target: Target) -> Result<Self, Diagnostic> {
        Ok(Self { target: target.validate()?, next_tag_identity: Cell::new(1), tags: RefCell::new(vec![HashMap::new()]), tag_definitions: RefCell::new(HashMap::new()), globals: HashMap::new(), locals: Vec::new(), result_ty: None, loop_depth: 0, rollback_log: None, statement_undo: Vec::new(),
            recovery_errors: None, recovery_limit: 0 })
    }

    pub fn global(&self, name: &str) -> Option<&Symbol> { self.globals.get(name) }

    /// Query the completed file-scope type after all external declarations.
    /// Incomplete tagged tentative objects may acquire their layout later
    /// without an explicit object redeclaration.
    pub fn completed_global_type(&self, name: &str) -> Option<CType> {
        let symbol = self.global(name)?;
        self.try_complete_tag_type(symbol.ty.clone()).ok()
    }
    pub fn target(&self) -> Target { self.target }

    fn lookup(&self, name: &str) -> Option<&Symbol> {
        self.locals.iter().rev().find_map(|scope| scope.get(name))
            .or_else(|| self.globals.get(name))
    }

    fn pop_tag_scope(&self) {
        let scope = self.tags.borrow_mut().pop().expect("tag scope");
        let mut definitions = self.tag_definitions.borrow_mut();
        for entry in scope.values() {
            definitions.remove(&entry.identity);
        }
    }

    fn new_tag_identity(&self) -> u64 {
        let id = self.next_tag_identity.get();
        self.next_tag_identity.set(id.saturating_add(1));
        id
    }

    fn lookup_tag(&self, kind: TagKind, name: &str, span: Span)
        -> Result<Option<TagEntry>, Diagnostic>
    {
        let tags = self.tags.borrow();
        for scope in tags.iter().rev() {
            if let Some(entry) = scope.get(name) {
                if entry.kind != kind {
                    return Err(type_error(span, format!("conflicting tag kind for '{name}'")));
                }
                return Ok(Some(entry.clone()));
            }
        }
        Ok(None)
    }

    fn reference_tag(&self, kind: TagKind, name: &str, span: Span)
        -> Result<CType, Diagnostic>
    {
        let entry = if let Some(entry) = self.lookup_tag(kind, name, span)? {
            entry
        } else {
            let entry = TagEntry {
                kind, identity: self.new_tag_identity(), complete: None,
            };
            self.tags.borrow_mut().last_mut().expect("tag scope")
                .insert(name.to_owned(), entry.clone());
            entry
        };
        Ok(CType::TagRef { kind, name: name.to_owned(), identity: entry.identity })
    }

    fn declare_tag_only(&mut self, ty: &Type, span: Span) -> Result<(), Diagnostic> {
        match ty {
            Type::TagRef(kind, name) => {
                let current = self.tags.borrow().last().expect("tag scope")
                    .get(name).cloned();
                if let Some(entry) = current {
                    if entry.kind != *kind {
                        return Err(type_error(span, format!("conflicting tag kind for '{name}'")));
                    }
                } else {
                    let identity = self.new_tag_identity();
                    self.tags.borrow_mut().last_mut().expect("tag scope")
                        .insert(name.clone(), TagEntry {
                            kind: *kind, identity, complete: None,
                        });
                }
                Ok(())
            }
            Type::Struct { .. } | Type::Union { .. } | Type::Enum { .. } => {
                self.resolve(ty, span)?;
                Ok(())
            }
            _ => Err(type_error(span, "tag declaration requires struct or union")),
        }
    }

    fn complete_tag_type(&self, ty: CType, span: Span) -> Result<CType, Diagnostic> {
        match ty {
            CType::Qualified(inner) => Ok(self.complete_tag_type(*inner, span)?.with_const()),
            CType::TagRef { name, identity, .. } => self.tag_definitions.borrow()
                .get(&identity).cloned().ok_or_else(|| type_error(span,
                    format!("incomplete struct, union or enum tag '{name}'"))),
            CType::Array(element, count) => Ok(CType::Array(
                Box::new(self.complete_tag_type(*element, span)?), count)),
            other => Ok(other),
        }
    }

    fn try_complete_tag_type(&self, ty: CType) -> Result<CType, Diagnostic> {
        match ty {
            CType::Qualified(inner) => Ok(self.try_complete_tag_type(*inner)?.with_const()),
            CType::TagRef { kind, name, identity } => Ok(self.tag_definitions.borrow()
                .get(&identity).cloned()
                .unwrap_or(CType::TagRef { kind, name, identity })),
            CType::Array(element, count) => Ok(CType::Array(
                Box::new(self.try_complete_tag_type(*element)?), count)),
            other => Ok(other),
        }
    }

    fn define_enumerator(
        &mut self, name: &str, value: i64, span: Span,
    ) -> Result<(), Diagnostic> {
        if self.locals.is_empty() {
            self.declare_global(name, CType::int(), SymbolKind::Enumerator,
                true, StorageClass::Default, span)?;
            self.globals.get_mut(name).expect("registered enumerator").enum_value = Some(value);
        } else {
            self.define_local(name, CType::int(), SymbolKind::Enumerator,
                Linkage::None, span)?;
            self.locals.last_mut().expect("local scope")
                .get_mut(name).expect("registered enumerator").enum_value = Some(value);
        }
        Ok(())
    }

    fn resolve(&mut self, ty: &Type, span: Span) -> Result<CType, Diagnostic> {
        Ok(match ty {
            Type::Qualified(inner, qualifiers) => {
                let base = self.resolve(inner, span)?;
                if qualifiers.is_restrict && !matches!(base.unqualified(), CType::Pointer(_)) {
                    return Err(type_error(span, "restrict requires a pointer object type"));
                }
                if qualifiers.is_const {
                    if matches!(base.unqualified(), CType::Function { .. }) {
                        return Err(type_error(span, "const cannot qualify a function type"));
                    }
                    // Qualifiers of typedef array types apply to their elements.
                    match base {
                        CType::Array(element, count) => CType::Array(
                            Box::new((*element).with_const()), count),
                        other => other.with_const(),
                    }
                } else { base }
            }
            Type::Void => CType::Void,
            Type::Char => CType::Integer { rank: IntRank::Char, unsigned: !self.target.char_is_signed },
            Type::Short => CType::Integer { rank: IntRank::Short, unsigned: false },
            Type::Short => CType::Integer { rank: IntRank::Short, unsigned: false },
            Type::Int => CType::int(),
            Type::Long => CType::Integer { rank: IntRank::Long, unsigned: false },
            Type::LongLong => CType::Integer { rank: IntRank::LongLong, unsigned: false },
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
            Type::Enum { tag, enumerators } => {
                let kind = TagKind::Enum;
                let identity = if let Some(name) = tag {
                    let current = self.tags.borrow().last().expect("tag scope")
                        .get(name).cloned();
                    if let Some(entry) = current {
                        if entry.kind != kind {
                            return Err(type_error(span, format!("conflicting tag kind for '{name}'")));
                        }
                        if entry.complete.is_some() {
                            return Err(type_error(span, format!("redefinition of aggregate tag '{name}'")));
                        }
                        entry.identity
                    } else {
                        let identity = self.new_tag_identity();
                        self.tags.borrow_mut().last_mut().expect("tag scope")
                            .insert(name.clone(), TagEntry { kind, identity, complete: None });
                        identity
                    }
                } else { self.new_tag_identity() };
                let bits = self.target.bits(IntRank::Int);
                let bound = 1i128 << (bits - 1);
                let mut next = 0i128;
                for (name, expr) in enumerators {
                    let value = if let Some(expr) = expr {
                        let typed = self.check_expr(expr, span)?.rvalue();
                        if !typed.ty.is_integer() {
                            return Err(type_error(span, "enumerator requires an integer constant expression"));
                        }
                        const_eval::evaluate(&typed, self.target)
                            .map_err(|e| type_error(span, format!("enumerator requires an integer constant expression: {e}")))?
                    } else { next };
                    if value < -bound || value >= bound {
                        return Err(type_error(span, "enumerator value exceeds supported signed int range"));
                    }
                    let value = i64::try_from(value)
                        .map_err(|_| type_error(span, "enumerator value out of range"))?;
                    self.define_enumerator(name, value, span)?;
                    next = i128::from(value) + 1;
                }
                if let Some(name) = tag {
                    self.tags.borrow_mut().last_mut().expect("tag scope")
                        .insert(name.clone(), TagEntry {
                            kind, identity, complete: Some(CType::int()),
                        });
                }
                if tag.is_some() {
                    self.tag_definitions.borrow_mut().insert(identity, CType::int());
                }
                CType::int()
            }
            Type::TagRef(kind, name) => {
                let reference = self.reference_tag(*kind, name, span)?;
                if *kind == TagKind::Enum {
                    self.try_complete_tag_type(reference)?
                } else { reference }
            },
            Type::Struct { tag, members } | Type::Union { tag, members } => {
                let kind = if matches!(ty, Type::Union { .. }) {
                    TagKind::Union
                } else { TagKind::Struct };
                let identity = if let Some(name) = tag {
                    let current = self.tags.borrow().last().expect("tag scope")
                        .get(name).cloned();
                    if let Some(entry) = current {
                        if entry.kind != kind {
                            return Err(type_error(span, format!("conflicting tag kind for '{name}'")));
                        }
                        if entry.complete.is_some() {
                            return Err(type_error(span, format!("redefinition of aggregate tag '{name}'")));
                        }
                        entry.identity
                    } else {
                        let identity = self.new_tag_identity();
                        self.tags.borrow_mut().last_mut().expect("tag scope")
                            .insert(name.clone(), TagEntry { kind, identity, complete: None });
                        identity
                    }
                } else { self.new_tag_identity() };
                let mut fields = Vec::with_capacity(members.len());
                for member in members {
                    if member.init.is_some() || member.storage != StorageClass::Default {
                        return Err(type_error(member.span, "aggregate members cannot have initializers or storage classes"));
                    }
                    fields.push((member.name.clone(), self.resolve(&member.ty, member.span)?));
                }
                let mut complete = if kind == TagKind::Union {
                    types::layout_union(tag.clone(), fields, self.target)
                } else {
                    types::layout_struct(tag.clone(), fields, self.target)
                }.map_err(|reason| type_error(span, reason))?;
                match &mut complete {
                    CType::Struct { identity: id, .. } |
                    CType::Union { identity: id, .. } => *id = identity,
                    _ => unreachable!(),
                }
                if let Some(name) = tag {
                    self.tags.borrow_mut().last_mut().expect("tag scope")
                        .insert(name.clone(), TagEntry {
                            kind, identity, complete: Some(complete.clone()),
                        });
                }
                if tag.is_some() {
                    self.tag_definitions.borrow_mut().insert(identity, complete.clone());
                }
                complete
            }
            Type::Pointer(inner) => CType::Pointer(Box::new(self.resolve(inner, span)?)),
            Type::Function { result, params } => {
                let result = self.resolve(result, span)?;
                if matches!(result, CType::Function { .. }) {
                    return Err(type_error(span, "function cannot return a function"));
                }
                let mut converted = Vec::with_capacity(params.len());
                for param in params {
                    let resolved = self.resolve(param, span)?;
                    let ty = self.adjust_parameter(resolved);
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
        scope.insert(name.to_owned(), Symbol {
            ty, kind, linkage, defined: true, enum_value: None,
        });
        Ok(())
    }

    fn declare_global(
        &mut self, name: &str, ty: CType, kind: SymbolKind,
        defining: bool, storage: StorageClass, span: Span
    ) -> Result<(), Diagnostic> {
        let requested = if matches!(kind, SymbolKind::Typedef | SymbolKind::Enumerator) {
            Linkage::None
        } else if storage == StorageClass::Static {
            Linkage::Internal
        } else {
            Linkage::External
        };
        if self.rollback_log.is_some() || !self.statement_undo.is_empty() {
            // Log the first value at every active recovery checkpoint.
            // A failure in a nested statement restores just that statement,
            // while failure of the containing statement restores all of it.
            let previous = self.globals.get(name).cloned();
            if let Some(log) = &mut self.rollback_log {
                log.entry(name.to_owned()).or_insert_with(|| previous.clone());
            }
            for log in &mut self.statement_undo {
                log.entry(name.to_owned()).or_insert_with(|| previous.clone());
            }
        }
        if let Some(prior) = self.globals.get_mut(name) {
            let Some(composite) = prior.ty.composite(&ty).filter(|_| prior.kind == kind) else {
                return Err(Diagnostic::coded(span, DiagnosticCode::SemanticConflictingDeclaration, format!("conflicting declaration of '{name}'")));
            };
            // An extern redeclaration inherits a previous internal linkage.
            // Explicit static cannot follow a prior external declaration.
            if prior.linkage != requested && storage != StorageClass::Extern {
                return Err(type_error(span, format!("conflicting linkage for '{name}'")));
            }
            if defining && prior.defined {
                return Err(Diagnostic::coded(span, DiagnosticCode::SemanticMultipleDefinitions, format!("multiple definitions of '{name}'")));
            }
            prior.ty = composite;
            prior.defined |= defining;
        } else {
            self.globals.insert(name.to_owned(), Symbol {
                ty, kind, linkage: requested, defined: defining, enum_value: None,
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
        let saved_tags = self.tags.borrow().clone();
        let saved_definitions = self.tag_definitions.borrow().clone();
        self.rollback_log = Some(HashMap::new());
        self.recovery_errors = Some(Vec::new());
        self.recovery_limit = max_errors;
        let result = self.check_item(item);
        let mut errors = self.recovery_errors.take().expect("diagnostic collection");
        if let Err(error) = result {
            if errors.len() < max_errors { errors.push(error); }
        }
        let changed = self.rollback_log.take().expect("symbol transaction");
        if !errors.is_empty() {
            self.restore_symbols(changed);
            *self.tags.borrow_mut() = saved_tags;
            *self.tag_definitions.borrow_mut() = saved_definitions;
        }
        debug_assert!(self.statement_undo.is_empty());
        self.statement_undo.clear();
        self.recovery_limit = 0;
        errors
    }

    /// Check an external item transactionally, rolling back only the globals
    /// touched on failure. The normal fail-fast entry point stays allocation
    /// free beyond the compilation's necessary symbol table operations.
    pub fn check_item_recovering(
        &mut self, item: ExternalItem
    ) -> Result<CheckedItem, Diagnostic> {
        assert!(self.rollback_log.is_none(), "nested semantic transactions are unsupported");
        let saved_tags = self.tags.borrow().clone();
        let saved_definitions = self.tag_definitions.borrow().clone();
        self.rollback_log = Some(HashMap::new());
        let result = self.check_item(item);
        let changed = self.rollback_log.take().expect("transaction log");
        if result.is_err() {
            *self.tags.borrow_mut() = saved_tags;
            *self.tag_definitions.borrow_mut() = saved_definitions;
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
            ExternalItem::Tag(ty) => {
                self.declare_tag_only(&ty, Span::default())?;
                Ok(CheckedItem::Tag)
            }
            ExternalItem::Typedef(decl) => {
                let ty = self.resolve(&decl.ty, decl.span)?;
                self.declare_global(&decl.name, ty.clone(), SymbolKind::Typedef, false, decl.storage,
                    decl.name_span.unwrap_or(decl.span))?;
                Ok(CheckedItem::Typedef { name: decl.name, ty })
            }
            ExternalItem::Declaration(decl) => self.check_global(decl).map(CheckedItem::Declaration),
            ExternalItem::Function(func) => self.check_function(func).map(CheckedItem::Function),
        }
    }

    /// A narrow string initializes a character array, not a scalar pointer.
    /// For an incomplete array the NUL is included in the inferred bound;
    /// an explicitly sized array may omit the final NUL, as required by C.
    fn complete_string_array(
        &self, ty: CType, initializer: Option<&Expr>, span: Span
    ) -> Result<CType, Diagnostic> {
        if let (CType::Array(element, bound), Some(Expr::StringLiteral(bytes))) =
            (&ty, initializer.map(Expr::without_location)) {
            if matches!(element.as_ref(), CType::Integer { rank: IntRank::Char, .. }) {
                let len = u64::try_from(bytes.len())
                    .map_err(|_| type_error(span, "string literal exceeds supported array size"))?;
                if bound.as_ref().is_some_and(|&count| count < len.saturating_sub(1)) {
                    return Err(type_error(span, "string literal is too long for character array"));
                }
                if bound.is_none() {
                    return Ok(CType::Array(element.clone(), Some(len)));
                }
            }
        }
        if let (CType::Array(element, None), Some(Expr::Initializer(values))) =
            (&ty, initializer.map(Expr::without_location)) {
            let count = u64::try_from(values.len())
                .map_err(|_| type_error(span, "array initializer exceeds supported bounds"))?;
            return Ok(CType::Array(element.clone(), Some(count)));
        }
        Ok(ty)
    }

    fn check_object_initializer(
        &self, original: &Expr, ty: &CType, span: Span, is_static: bool
    ) -> Result<CheckedExpr, Diagnostic> {
        let span = original.location().unwrap_or(span);
        let original = original.without_location();
        if let Expr::Initializer(values) = original {
            let elements: Vec<&CType> = match ty {
                CType::Struct { members, .. } => {
                    if values.len() > members.len() {
                        return Err(type_error(span, "too many struct initializers"));
                    }
                    members.iter().map(|m| &m.ty).collect()
                }
                CType::Union { members, .. } => {
                    if values.len() > 1 {
                        return Err(type_error(span, "union initializer selects only its first member"));
                    }
                    members.iter().take(1).map(|m| &m.ty).collect()
                }
                CType::Array(element, Some(count)) => {
                    let count = usize::try_from(*count)
                        .map_err(|_| type_error(span, "array initializer exceeds supported bounds"))?;
                    if values.len() > count {
                        return Err(type_error(span, "too many array initializers"));
                    }
                    vec![element.as_ref(); values.len()]
                }
                CType::Array(_, None) => {
                    return Err(type_error(span, "incomplete array initializer"));
                }
                _ => {
                    if values.len() != 1 {
                        return Err(type_error(span, "scalar initializer requires one expression"));
                    }
                    return self.check_object_initializer(&values[0], ty, span, is_static);
                }
            };
            let checked = values.iter().zip(elements).map(|(value, element)|
                self.check_object_initializer(value, element, span, is_static))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(CheckedExpr::value(ty.clone(), CheckedExprKind::Initializer(checked)));
        }
        let expression = self.check_expr(original, span)?;
        if is_static {
            self.validate_static_initializer(&expression, original, ty, span)?;
        }
        if matches!((ty, original),
            (CType::Array(element, Some(_)), Expr::StringLiteral(_))
            if matches!(element.as_ref(), CType::Integer { rank: IntRank::Char, .. }))
        {
            // Retain literal bytes in checked IR; generic array-to-pointer
            // decay would lose the value needed to initialize the array.
            Ok(expression)
        } else {
            self.convert_for_assignment(expression, ty, original, span)
        }
    }

    fn check_global(&mut self, decl: Decl) -> Result<CheckedDecl, Diagnostic> {
        let raw = self.resolve(&decl.ty, decl.span)?;
        let raw = self.try_complete_tag_type(raw)?;
        let ty = self.complete_string_array(raw, decl.init.as_ref(), decl.span)?;
        let kind = if matches!(ty, CType::Function { .. }) { SymbolKind::Function } else { SymbolKind::Object };
        if kind == SymbolKind::Object && !ty.is_object() {
            return Err(type_error(decl.span, "object cannot have void or function type"));
        }
        if kind == SymbolKind::Function && decl.init.is_some() {
            return Err(type_error(decl.span, "function declaration cannot have an initializer"));
        }
// C requires a file-scope object with internal linkage to have a
        // complete type at its declaration, not merely at TU finalization.
        // This differs from external-linkage tentative definitions, which
        // may be completed by a later tag definition.
        if kind == SymbolKind::Object && decl.storage == StorageClass::Static
            && ty.size_bytes(self.target).is_none()
        {
            return Err(type_error(decl.span,
                "file-scope static object requires a complete type"));
        }
        let defining = decl.init.is_some();
        self.declare_global(&decl.name, ty.clone(), kind, defining, decl.storage,
            decl.name_span.unwrap_or(decl.span))?;
        // A prior incomplete array declaration can be completed here; publish
        // the composite type to both the checked item and later references.
        let ty = self.global(&decl.name).expect("registered global").ty.clone();
        let initializer = decl.init.as_ref()
            .map(|value| self.check_object_initializer(value, &ty, decl.span, true))
            .transpose()?;
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
            let resolved = self.resolve(&param.ty, param.span)?;
            let ty = self.adjust_parameter(resolved);
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
            params: params.iter().map(|p| p.ty.unqualified().clone()).collect(),
        };
        // Register before checking the body: recursive calls are legal.
        self.declare_global(&func.name, signature.clone(), SymbolKind::Function, true, func.storage,
            func.name_span.unwrap_or(func.span))?;
        self.locals.push(HashMap::new());
        self.tags.borrow_mut().push(HashMap::new());
        self.result_ty = Some(result);
        for (parsed_param, param) in func.params.iter().zip(&params) {
            if let Err(err) = self.define_local(&param.name, param.ty.clone(),
                SymbolKind::Object, Linkage::None,
                parsed_param.name_span.unwrap_or(param.span)) {
                self.pop_tag_scope();
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
        self.pop_tag_scope();
        self.locals.pop();
        self.result_ty = None;
        let body = checked?;
        let linkage = self.global(&func.name).expect("registered function").linkage;
        Ok(CheckedFunction { linkage, name: func.name, signature, params, body, span: func.span })
    }

    fn check_statements(&mut self, statements: &[Stmt], span: Span) -> Result<Vec<CheckedStmt>, Diagnostic> {
        // Ordinary compilation retains the fast, fail-first behavior.
        // Diagnostic recovery can instead continue through independent
        // statements at every brace nesting depth, including if/while bodies.
        if self.recovery_errors.is_none() {
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
            let tag_snapshot = self.tags.borrow().clone();
            let definitions_snapshot = self.tag_definitions.borrow().clone();
            let loop_depth = self.loop_depth;
            self.statement_undo.push(HashMap::new());
            let result = self.check_stmt(statement, span);
            let changed = self.statement_undo.pop().expect("statement transaction");
            match result {
                Ok(stmt) => checked.push(stmt),
                Err(error) => {
                    self.locals = local_snapshot;
                    *self.tags.borrow_mut() = tag_snapshot;
                    *self.tag_definitions.borrow_mut() = definitions_snapshot;
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
                self.tags.borrow_mut().push(HashMap::new());
                let inner = self.check_statements(stmts, span);
                self.pop_tag_scope();
                self.locals.pop();
                CheckedStmt::Block(inner?)
            }
            Stmt::Tag(ty) => {
                self.declare_tag_only(ty, span)?;
                CheckedStmt::Tag
            }
            Stmt::Decl(decl) => {
                let resolved = self.resolve(&decl.ty, decl.span)?;
                let ty = self.complete_string_array(
                    self.try_complete_tag_type(resolved)?,
                    decl.init.as_ref(), decl.span)?;
                if decl.storage != StorageClass::Extern && ty.size_bytes(self.target).is_none() {
                    return Err(type_error(decl.span, "incomplete local object type"));
                }
                if !ty.is_object() {
                    return Err(type_error(decl.span, "local variable cannot have void or function type"));
                }
                if decl.storage == StorageClass::Extern {
                    if decl.init.is_some() {
                        return Err(type_error(decl.span, "block-scope extern cannot have an initializer"));
                    }
                    self.declare_global(&decl.name, ty.clone(), SymbolKind::Object, false, decl.storage,
                        decl.name_span.unwrap_or(decl.span))?;
                }
                let linkage = if decl.storage == StorageClass::Extern {
                    self.global(&decl.name).expect("registered extern").linkage
                } else { Linkage::None };
                self.define_local(&decl.name, ty.clone(), SymbolKind::Object, linkage,
                    decl.name_span.unwrap_or(decl.span))?;
                let initializer = decl.init.as_ref()
                    .map(|value| self.check_object_initializer(value, &ty, decl.span,
                        decl.storage == StorageClass::Static))
                    .transpose()?;
                CheckedStmt::Decl(CheckedDecl {
                    storage: decl.storage, name: decl.name.clone(), ty,
                    linkage,
                    initializer, span: decl.span,
                })
            }
            Stmt::Typedef(decl) => {
                let ty = self.resolve(&decl.ty, decl.span)?;
                self.define_local(&decl.name, ty.clone(), SymbolKind::Typedef, Linkage::None,
                    decl.name_span.unwrap_or(decl.span))?;
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
                        Some(self.convert_for_assignment(expr, &expected, value,
                            value.location().unwrap_or(span))?)
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
            Stmt::DoWhile { body, cond } => {
                self.loop_depth += 1;
                let body_result = self.check_stmt(body, span);
                self.loop_depth -= 1;
                let body = Box::new(body_result?);
                let cond = self.check_condition(cond, span)?;
                CheckedStmt::DoWhile { body, cond }
            }
            Stmt::For { init, cond, step, body } => {
                // The initializer's identifiers and tags remain visible in
                // the condition, increment and body, but not after the loop.
                self.locals.push(HashMap::new());
                self.tags.borrow_mut().push(HashMap::new());
                let checked = (|| -> Result<CheckedStmt, Diagnostic> {
                    let init = init.as_deref()
                        .map(|stmt| self.check_stmt(stmt, span).map(Box::new))
                        .transpose()?;
                    let cond = cond.as_ref()
                        .map(|expr| self.check_condition(expr, span))
                        .transpose()?;
                    let step = step.as_ref()
                        .map(|expr| self.check_expr(expr, span).map(CheckedExpr::rvalue))
                        .transpose()?;
                    self.loop_depth += 1;
                    let body = self.check_stmt(body, span);
                    self.loop_depth -= 1;
                    Ok(CheckedStmt::For(Box::new(CheckedFor {
                        init, cond, step, body: Box::new(body?),
                    })))
                })();
                self.pop_tag_scope();
                self.locals.pop();
                checked?
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
        let span = cond.location().unwrap_or(span);
        let typed = self.check_expr(cond, span)?.rvalue();
        if !typed.ty.is_scalar() {
            return Err(Diagnostic::coded(span, DiagnosticCode::SemanticInvalidCondition,
                "condition requires a scalar type"));
        }
        Ok(typed)
    }

    fn validate_static_initializer(
        &self, expression: &CheckedExpr, original: &Expr, ty: &CType, span: Span
    ) -> Result<(), Diagnostic> {
        let span = original.location().unwrap_or(span);
        let original = original.without_location();
        // Const storage still has ordinary static initialization rules.
        let ty = ty.unqualified();
        if let CType::Array(element, Some(_)) = ty {
            if matches!(original, Expr::StringLiteral(_))
                && matches!(element.as_ref(), CType::Integer { rank: IntRank::Char, .. })
            {
                return Ok(());
            }
        }
        if matches!(ty, CType::Array(..)) {
            // Other aggregates still require dedicated initializer checking.
            return Err(type_error(span, "array initializer requires aggregate initialization support"));
        }
        if ty.is_integer() {
            const_eval::evaluate(expression, self.target).map_err(|why|
                type_error(span, format!("initializer is not a valid integer constant expression: {why}")))?;
        } else if matches!(ty, CType::Pointer(_)) {
            let null = const_eval::evaluate(expression, self.target) == Ok(0);
            let address_of_global = match original {
                Expr::Unary(UnOp::Address, inner) => match inner.without_location() {
                    Expr::Name(name) => self.globals.get(name).is_some_and(|s| s.kind != SymbolKind::Typedef),
                    _ => false,
                },
                Expr::Name(name) => self.globals.get(name).is_some_and(|s|
                    matches!(&s.ty, CType::Array(..)) ||
                    (s.kind == SymbolKind::Function && matches!(ty, CType::Pointer(inner)
                        if matches!(inner.as_ref(), CType::Function { .. })))),
                Expr::StringLiteral(_) => matches!(ty, CType::Pointer(element)
                    if matches!(element.as_ref(), CType::Integer { rank: IntRank::Char, .. } | CType::Void)),
                _ => false,
            };
            if !null && !address_of_global {
                return Err(type_error(span, "pointer initializer requires a null constant or address of global symbol"));
            }
        }
        Ok(())
    }

    /// Resolve the integer subset of C cast type-names without mutating tags
    /// or scopes; expression checking only holds an immutable checker borrow.
    fn cast_integer_type(&self, ty: &Type, span: Span) -> Result<CType, Diagnostic> {
        let resolved = match ty {
            Type::Char => CType::Integer {
                rank: IntRank::Char, unsigned: !self.target.char_is_signed,
            },
            Type::Qualified(inner, qualifiers) => {
                if qualifiers.is_restrict {
                    return Err(type_error(span, "restrict requires a pointer object type"));
                }
                return self.cast_integer_type(inner, span);
            },
            Type::Short => CType::Integer { rank: IntRank::Short, unsigned: false },
            Type::Int => CType::int(),
            Type::Long => CType::Integer { rank: IntRank::Long, unsigned: false },
            Type::LongLong => CType::Integer { rank: IntRank::LongLong, unsigned: false },
            Type::Signed(inner) | Type::Unsigned(inner) => {
                let CType::Integer { rank, .. } = self.cast_integer_type(inner, span)? else {
                    return Err(type_error(span, "integer cast requires an integer type"));
                };
                CType::Integer { rank, unsigned: matches!(ty, Type::Unsigned(_)) }
            }
            Type::Alias(name) => {
                let symbol = self.lookup(name).ok_or_else(||
                    type_error(span, format!("unknown cast typedef '{name}'")))?;
                if symbol.kind != SymbolKind::Typedef {
                    return Err(type_error(span, format!("'{name}' does not name a type")));
                }
                symbol.ty.clone()
            }
            _ => return Err(type_error(span, "unsupported cast type-name")),
        };
        if !resolved.is_integer() {
            return Err(type_error(span, "only integer casts are supported"));
        }
        Ok(resolved)
    }

    fn convert(&self, expr: CheckedExpr, ty: &CType) -> CheckedExpr {
        let expr = expr.rvalue();
        if &expr.ty == ty || (expr.ty.same_type(ty) &&
            matches!(ty, CType::Struct { .. } | CType::Union { .. })) {
            return expr;
        }
        CheckedExpr::value(ty.clone(), CheckedExprKind::Convert(Box::new(expr)))
    }

    fn convert_for_assignment(&self, expr: CheckedExpr, ty: &CType, _original: &Expr, span: Span) -> Result<CheckedExpr, Diagnostic> {
        // The source array undergoes the standard array-to-pointer conversion
        // before checking scalar assignment compatibility.
        let expr = expr.rvalue();
        // Initializing a const-qualified object is permitted. Only the
        // stored value type loses top-level const; nested pointer qualifiers
        // remain intact and cannot be discarded.
        let value_ty = ty.unqualified();
        let compatible = match (value_ty, &expr.ty) {
            (CType::Struct { .. }, CType::Struct { .. }) |
            (CType::Union { .. }, CType::Union { .. }) => ty.same_type(&expr.ty),
            (CType::Integer { .. }, CType::Integer { .. }) => true,
            (CType::Pointer(lhs), CType::Pointer(rhs)) =>
                CType::compatible_pointee_assignment(lhs, rhs),
            (CType::Pointer(_), CType::Integer { .. }) => const_eval::evaluate(&expr, self.target) == Ok(0),
            _ => false,
        };
        if !compatible {
            return Err(Diagnostic::coded(span, DiagnosticCode::SemanticIncompatibleAssignment, format!("incompatible assignment: {:?} to {:?}", expr.ty, ty)));
        }
        Ok(self.convert(expr, value_ty))
    }

    /// Form a checked pointer + integer or pointer - integer expression.
    /// Address scaling is postponed to lowering, where the pointee size is known.
    fn pointer_offset(
        &self, op: BinOp, pointer: CheckedExpr, integer: CheckedExpr, span: Span
    ) -> Result<CheckedExpr, Diagnostic> {
        let CType::Pointer(element) = &pointer.ty else {
            return Err(type_error(span, "pointer arithmetic requires a pointer"));
        };
        let complete = self.complete_tag_type(element.as_ref().clone(), span)?;
        if complete.size_bytes(self.target).is_none() {
            return Err(type_error(span, "pointer arithmetic requires a complete object type"));
        }
        let converted_pointer = CType::Pointer(Box::new(complete));
        let pointer = if pointer.ty == converted_pointer {
            pointer
        } else {
            self.convert(pointer, &converted_pointer)
        };
        if !integer.ty.is_integer() {
            return Err(Diagnostic::coded(span, DiagnosticCode::SemanticInvalidOperatorOperands, "pointer arithmetic requires an integer offset"));
        }
        let integer_ty = self.target.promote(&integer.ty);
        let integer = self.convert(integer, &integer_ty);
        Ok(CheckedExpr::value(pointer.ty.clone(), CheckedExprKind::Binary(
            op, Box::new(pointer), Box::new(integer),
        )))
    }

    fn check_expr(&self, expr: &Expr, span: Span) -> Result<CheckedExpr, Diagnostic> {
        match expr {
            Expr::Located(inner, located_span) => {
                self.check_expr(inner, *located_span).map_err(|mut error| {
                    // A field lookup has a distinct source token at the end
                    // of the member expression. Prefer that exact token over
                    // the complete base-and-member expression.
                    if error.code == DiagnosticCode::SemanticUnknownMember
                        && error.span == *located_span
                    {
                        if let Expr::Member(_, field, _) = inner.as_ref() {
                            error.span = Span {
                                lo: located_span.hi.saturating_sub(field.len()),
                                hi: located_span.hi,
                            };
                        }
                    }
                    error
                })
            },
            Expr::Initializer(_) => Err(type_error(span,
                "brace initializer is only valid in an object declaration")),
            Expr::StringLiteral(bytes) => {
                let len = u64::try_from(bytes.len())
                    .map_err(|_| type_error(span, "string literal exceeds target object size"))?;
                Ok(CheckedExpr {
                    ty: CType::Array(Box::new(CType::Integer {
                        rank: IntRank::Char, unsigned: !self.target.char_is_signed,
                    }), Some(len)),
                    category: Category::LValue,
                    kind: CheckedExprKind::StringLiteral(bytes.clone()),
                })
            },
            Expr::Int(number) => {
                let ty = self.target.decimal_literal(*number).ok_or_else(|| type_error(span, "decimal literal exceeds supported signed integer types"))?;
                Ok(CheckedExpr::value(ty, CheckedExprKind::Literal(*number)))
            }
            Expr::IntegerConstant(literal) => {
                let ty = self.target.integer_literal(*literal).ok_or_else(|| type_error(
                    span, "integer constant exceeds supported int/long type range"
                ))?;
                Ok(CheckedExpr::value(ty, CheckedExprKind::Literal(literal.value)))
            }
            Expr::Name(name) => {
                let symbol = self.lookup(name).ok_or_else(|| Diagnostic::coded(span, DiagnosticCode::SemanticUndeclaredIdentifier, format!("undeclared identifier '{name}'")))?;
                let category = match symbol.kind {
                    SymbolKind::Typedef => return Err(type_error(span, format!("typedef name '{name}' used as expression"))),
                    SymbolKind::Enumerator => return Ok(CheckedExpr::value(
                        CType::int(), CheckedExprKind::EnumConstant(
                            symbol.enum_value.expect("enumerator has a value")))),
                    SymbolKind::Function => Category::Function,
                    SymbolKind::Object => Category::LValue,
                };
                Ok(CheckedExpr { ty: symbol.ty.clone(), category, kind: CheckedExprKind::Name(name.clone()) })
            }
            Expr::Member(base, name, arrow) => {
                let base = self.check_expr(base, span)?;
                let object = if *arrow {
                    let base = base.rvalue();
                    let CType::Pointer(inner) = &base.ty else {
                        return Err(Diagnostic::coded(span, DiagnosticCode::SemanticInvalidMemberAccess, "arrow operator requires a pointer to struct"));
                    };
                    let complete = self.complete_tag_type(inner.as_ref().clone(), span)?;
                    if !matches!(complete.unqualified(), CType::Struct { .. } | CType::Union { .. }) {
                        return Err(Diagnostic::coded(span, DiagnosticCode::SemanticInvalidMemberAccess, "arrow operator requires a pointer to struct or union"));
                    }
                    CheckedExpr {
                        ty: complete,
                        category: Category::LValue,
                        kind: CheckedExprKind::Unary(UnOp::Deref, Box::new(base)),
                    }
                } else {
                    if base.category != Category::LValue {
                        return Err(Diagnostic::coded(span, DiagnosticCode::SemanticInvalidMemberAccess, "dot operator requires an addressable struct"));
                    }
                    base
                };
                let mut object = object;
                object.ty = self.complete_tag_type(object.ty.clone(), span)?;
                let members = match object.ty.unqualified() {
                    CType::Struct { members, .. } | CType::Union { members, .. } => members,
                    _ => return Err(Diagnostic::coded(span, DiagnosticCode::SemanticInvalidMemberAccess, "member access requires a struct or union")),
                };
                let field = members.iter().find(|m| m.name == *name).ok_or_else(||
                    Diagnostic::coded(span, DiagnosticCode::SemanticUnknownMember, format!("unknown struct member '{name}'")))?;
                // A member of a const aggregate is itself nonmodifiable.
                // Array member qualification applies to its element type.
                let field_ty = if object.ty.is_const() {
                    match field.ty.clone() {
                        CType::Array(element, count) =>
                            CType::Array(Box::new((*element).with_const()), count),
                        other => other.with_const(),
                    }
                } else { field.ty.clone() };
                let offset = field.offset;
                Ok(CheckedExpr {
                    ty: field_ty,
                    category: Category::LValue,
                    kind: CheckedExprKind::Member(Box::new(object), offset),
                })
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
                    return Err(Diagnostic::coded(span, DiagnosticCode::SemanticInvalidSubscript, "subscript requires pointer and integer operands"));
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
                            return Err(Diagnostic::coded(span, DiagnosticCode::SemanticInvalidOperatorOperands, "address-of requires an lvalue or function"));
                        }
                        let ty = CType::Pointer(Box::new(typed.ty.clone()));
                        Ok(CheckedExpr::value(ty, CheckedExprKind::Unary(*op, Box::new(typed))))
                    }
                    UnOp::Deref => {
                        // Function designators decay to pointers before unary *.
                        // This also permits (*function)(args), as required by C.
                        let typed = typed.rvalue();
                        let CType::Pointer(inner_ty) = &typed.ty else {
                            return Err(Diagnostic::coded(span, DiagnosticCode::SemanticInvalidOperatorOperands, "dereference requires a pointer"));
                        };
                        let ty = self.complete_tag_type((**inner_ty).clone(), span)?;
                        let category = if matches!(ty, CType::Function { .. }) { Category::Function } else if ty == CType::Void {
                            return Err(type_error(span, "cannot dereference void pointer"));
                        } else { Category::LValue };
                        Ok(CheckedExpr { ty, category, kind: CheckedExprKind::Unary(*op, Box::new(typed)) })
                    }
                    UnOp::Not => {
                        if !typed.ty.is_scalar() { return Err(Diagnostic::coded(span, DiagnosticCode::SemanticInvalidOperatorOperands, "logical not requires scalar operand")); }
                        Ok(CheckedExpr::value(CType::int(), CheckedExprKind::Unary(*op, Box::new(typed.rvalue()))))
                    }
                    UnOp::Plus | UnOp::Neg | UnOp::BitNot => {
                        if !typed.ty.is_integer() { return Err(Diagnostic::coded(span, DiagnosticCode::SemanticInvalidOperatorOperands, "arithmetic unary operator requires integer")); }
                        let ty = self.target.promote(&typed.ty);
                        Ok(CheckedExpr::value(ty.clone(), CheckedExprKind::Unary(*op, Box::new(self.convert(typed, &ty)))))
                    }
                }
            }
            Expr::Binary(op, lhs, rhs) => {
                let left = self.check_expr(lhs, span)?.rvalue();
                let right = self.check_expr(rhs, span)?.rvalue();
                let logical = matches!(op, BinOp::And | BinOp::Or);
                let bitwise = matches!(op, BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor);
                let relation = matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge);
                if logical {
                    if !(left.ty.is_scalar() && right.ty.is_scalar()) {
                        return Err(Diagnostic::coded(span, DiagnosticCode::SemanticInvalidOperatorOperands, "logical operator requires scalar operands"));
                    }
                    return Ok(CheckedExpr::value(CType::int(), CheckedExprKind::Binary(*op, Box::new(left), Box::new(right))));
                }
                if relation && (matches!((&left.ty, &right.ty), (CType::Pointer(a), CType::Pointer(b))
                        if a.unqualified().same_type(b.unqualified())
                            || *a.unqualified() == CType::Void
                            || *b.unqualified() == CType::Void)
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
                if matches!(op, BinOp::Shl | BinOp::Shr) {
                    if !(left.ty.is_integer() && right.ty.is_integer()) {
                        return Err(Diagnostic::coded(span, DiagnosticCode::SemanticInvalidOperatorOperands,
                            "shift operator requires integer operands"));
                    }
                    // Unlike arithmetic/bitwise binary operators, shifts apply
                    // integer promotion independently to each operand and
                    // retain the promoted LEFT operand's type.
                    let left_ty = self.target.promote(&left.ty);
                    let right_ty = self.target.promote(&right.ty);
                    return Ok(CheckedExpr::value(left_ty.clone(), CheckedExprKind::Binary(
                        *op, Box::new(self.convert(left, &left_ty)),
                        Box::new(self.convert(right, &right_ty)),
                    )));
                }
                if bitwise && !(left.ty.is_integer() && right.ty.is_integer()) {
                    return Err(Diagnostic::coded(span, DiagnosticCode::SemanticInvalidOperatorOperands,
                        "bitwise operator requires integer operands"));
                }
                let common = self.target.common_integer(&left.ty, &right.ty)
                    .ok_or_else(|| Diagnostic::coded(span, DiagnosticCode::SemanticInvalidOperatorOperands, "operator requires compatible integer operands"))?;
                let left = self.convert(left, &common);
                let right = self.convert(right, &common);
                let result = if relation { CType::int() } else { common };
                Ok(CheckedExpr::value(result, CheckedExprKind::Binary(*op, Box::new(left), Box::new(right))))
            }
            Expr::Conditional(condition, yes, no) => {
                let condition = self.check_condition(condition, span)?;
                // Type-check both alternatives, but only one will be executed.
                // Both are value contexts, including array/function decay.
                let yes = self.check_expr(yes, span)?.rvalue();
                let no = self.check_expr(no, span)?.rvalue();
                let ty = if let Some(integer) =
                    self.target.common_integer(&yes.ty, &no.ty)
                {
                    integer
                } else if yes.ty == CType::Void && no.ty == CType::Void {
                    CType::Void
                } else if matches!((&yes.ty, &no.ty),
                    (CType::Struct { .. }, CType::Struct { .. }) |
                    (CType::Union { .. }, CType::Union { .. }))
                    && yes.ty.same_type(&no.ty)
                {
                    yes.ty.clone()
                } else if let (CType::Pointer(a), CType::Pointer(b)) =
                    (&yes.ty, &no.ty)
                {
                    let base_a = a.unqualified();
                    let base_b = b.unqualified();
                    let unified = if base_a.same_type(base_b) {
                        base_a.clone()
                    } else if *base_a == CType::Void || *base_b == CType::Void {
                        CType::Void
                    } else {
                        return Err(Diagnostic::coded(span,
                            DiagnosticCode::SemanticInvalidOperatorOperands,
                            "conditional operands require compatible types"));
                    };
                    CType::Pointer(Box::new(
                        if a.is_const() || b.is_const() {
                            unified.with_const()
                        } else { unified }
                    ))
                } else if matches!(&yes.ty, CType::Pointer(_))
                    && no.ty.is_integer()
                    && const_eval::evaluate(&no, self.target) == Ok(0)
                {
                    yes.ty.clone()
                } else if matches!(&no.ty, CType::Pointer(_))
                    && yes.ty.is_integer()
                    && const_eval::evaluate(&yes, self.target) == Ok(0)
                {
                    no.ty.clone()
                } else {
                    return Err(Diagnostic::coded(span,
                        DiagnosticCode::SemanticInvalidOperatorOperands,
                        "conditional operands require compatible types"));
                };
                let yes = self.convert(yes, &ty);
                let no = self.convert(no, &ty);
                Ok(CheckedExpr::value(ty, CheckedExprKind::Conditional(
                    Box::new(condition), Box::new(yes), Box::new(no))))
            }
            Expr::Cast(ty, operand) => {
                let dest = self.cast_integer_type(ty, span)?;
                let value = self.check_expr(operand, span)?.rvalue();
                if !value.ty.is_integer() {
                    return Err(type_error(span, "integer cast requires integer operand"));
                }
                Ok(self.convert(value, &dest))
            }
            Expr::Comma(lhs, rhs) => {
                // Both operands undergo their ordinary value conversions.
                // The comma operator sequences them and returns the rvalue
                // of the right expression, including void and pointer types.
                let left = self.check_expr(lhs, span)?.rvalue();
                let right = self.check_expr(rhs, span)?.rvalue();
                Ok(CheckedExpr::value(right.ty.clone(), CheckedExprKind::Comma(
                    Box::new(left), Box::new(right),
                )))
            }
            Expr::Assign(lhs, rhs) => {
                let lhs_span = lhs.location().unwrap_or(span);
                let rhs_span = rhs.location().unwrap_or(span);
                let lhs = self.check_expr(lhs, span)?;
                if lhs.category != Category::LValue || lhs.ty.is_const()
                    || matches!(lhs.ty.unqualified(), CType::Array(..)) {
                    return Err(Diagnostic::coded(lhs_span, DiagnosticCode::SemanticInvalidAssignmentTarget, "left side of assignment must be a modifiable lvalue"));
                }
                let rhs_typed = self.check_expr(rhs, span)?;
                let rhs_typed = self.convert_for_assignment(rhs_typed, &lhs.ty, rhs, rhs_span)?;
                Ok(CheckedExpr::value(lhs.ty.clone(), CheckedExprKind::Assign(Box::new(lhs), Box::new(rhs_typed))))
            }
            Expr::CompoundAssign(op, lhs, rhs) => {
                let lhs_span = lhs.location().unwrap_or(span);
                let rhs_span = rhs.location().unwrap_or(span);
                let lhs = self.check_expr(lhs, span)?;
                let arithmetic = matches!(op,
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod);
                let pointer_arithmetic = matches!(op, BinOp::Add | BinOp::Sub)
                    && matches!(&lhs.ty, CType::Pointer(_));
                if lhs.category != Category::LValue || lhs.ty.is_const()
                    || !(lhs.ty.is_integer() || pointer_arithmetic)
                {
                    return Err(Diagnostic::coded(lhs_span,
                        DiagnosticCode::SemanticInvalidAssignmentTarget,
                        "compound assignment requires a modifiable integer lvalue or pointer +=/-="));
                }
                let rhs_checked = self.check_expr(rhs, span)?.rvalue();
                if !rhs_checked.ty.is_integer() {
                    return Err(Diagnostic::coded(rhs_span,
                        DiagnosticCode::SemanticInvalidOperatorOperands,
                        "compound assignment requires integer operands"));
                }
                let computation_ty = if pointer_arithmetic {
                    // C only permits +/- a scalar integer offset for pointers
                    // to complete object types, with element-sized scaling.
                    let CType::Pointer(element) = &lhs.ty else { unreachable!() };
                    let complete = self.complete_tag_type(element.as_ref().clone(), lhs_span)?;
                    if complete.size_bytes(self.target).is_none() {
                        return Err(type_error(lhs_span,
                            "pointer arithmetic requires a complete object type"));
                    }
                    CType::Pointer(Box::new(complete))
                } else if arithmetic || matches!(op,
                    BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor)
                {
                    self.target.common_integer(&lhs.ty, &rhs_checked.ty)
                        .ok_or_else(|| type_error(span, "incompatible compound assignment operands"))?
                } else if matches!(op, BinOp::Shl | BinOp::Shr) {
                    self.target.promote(&lhs.ty)
                } else {
                    return Err(type_error(span, "unsupported compound assignment operator"));
                };
                // Shift and pointer offsets promote their right operand
                // independently; all other operations apply the usual
                // arithmetic conversions to both operands.
                let rhs_ty = if pointer_arithmetic ||
                    matches!(op, BinOp::Shl | BinOp::Shr)
                {
                    self.target.promote(&rhs_checked.ty)
                } else {
                    computation_ty.clone()
                };
                let rhs_checked = self.convert(rhs_checked, &rhs_ty);
                let result_ty = lhs.ty.clone();
                Ok(CheckedExpr::value(result_ty, CheckedExprKind::CompoundAssign(
                    *op, Box::new(lhs), Box::new(rhs_checked), computation_ty)))
            }
            Expr::Update { target, increment, postfix } => {
                let target_span = target.location().unwrap_or(span);
                let checked = self.check_expr(target, target_span)?;
                if checked.category != Category::LValue || checked.ty.is_const()
                    || !(checked.ty.is_integer() || matches!(checked.ty, CType::Pointer(_)))
                {
                    return Err(Diagnostic::coded(target_span,
                        DiagnosticCode::SemanticInvalidAssignmentTarget,
                        "increment/decrement requires a modifiable integer or pointer lvalue"));
                }
                let computation_ty = if let CType::Pointer(pointee) = &checked.ty {
                    let complete = self.complete_tag_type(pointee.as_ref().clone(), target_span)?;
                    if complete.size_bytes(self.target).is_none() {
                        return Err(type_error(target_span,
                            "increment/decrement requires pointer to complete object type"));
                    }
                    CType::Pointer(Box::new(complete))
                } else {
                    self.target.promote(&checked.ty)
                };
                let ty = checked.ty.clone();
                Ok(CheckedExpr::value(ty, CheckedExprKind::Update {
                    target: Box::new(checked),
                    increment: *increment,
                    postfix: *postfix,
                    computation_ty,
                }))
            }
            Expr::Call(callee, args) => {
                let callee_span = callee.location().unwrap_or(span);
                let callee = self.check_expr(callee, span)?;
                // Keep direct named function calls in their existing IR form;
                // other function-value contexts use implicit pointer decay.
                let callee = if callee.category == Category::Function {
                    callee
                } else {
                    callee.rvalue()
                };
                let signature = match &callee.ty {
                    CType::Function { result, params } => Some((result.as_ref(), params.as_slice())),
                    CType::Pointer(inner) => match inner.as_ref() {
                        CType::Function { result, params } => Some((result.as_ref(), params.as_slice())),
                        _ => None,
                    },
                    _ => None,
                };
                let Some((result, params)) = signature else {
                    return Err(Diagnostic::coded(callee_span, DiagnosticCode::SemanticNotCallable,
                        "called expression is not a function"));
                };
                if args.len() != params.len() {
                    return Err(Diagnostic::coded(span, DiagnosticCode::SemanticWrongArgumentCount, format!("function expects {} arguments, got {}", params.len(), args.len())));
                }
                let mut checked_args = Vec::with_capacity(args.len());
                for (arg, ty) in args.iter().zip(params) {
                    let typed = self.check_expr(arg, span)?;
                    checked_args.push(self.convert_for_assignment(typed, ty, arg,
                        arg.location().unwrap_or(span))?);
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
