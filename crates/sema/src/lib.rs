//! Streaming, backend-neutral semantic analysis for the supported C subset.
//! A caller delivers one external item at a time; completed function ASTs are
//! never retained in the checker. No filesystem, global state or output side effects.
pub mod types;
pub mod const_eval;

use std::collections::{HashMap, HashSet};
use std::cell::{Cell, RefCell};
use mini_c_syntax::ast::{BinOp, Decl, Designator, Expr, Function, Span, Stmt, StorageClass, TagKind, Type, UnOp};
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
    /// Exact token retained until target-specific floating rounding is supported.
    FloatLiteral(String),
    EnumConstant(i64),
    /// Narrow C string including its trailing NUL; storage is emitted by the backend.
    StringLiteral(Vec<u8>),
    /// Typed, positional aggregate initializer values, in member order.
    Initializer(Vec<CheckedExpr>),
    /// Source-order explicit member/array-element initializers. Missing
    /// entries are zero-filled by codegen, without materializing huge holes.
    /// Duplicate target selection is currently rejected rather than
    /// silently applying implementation-defined override side effects.
    SparseInitializer(Vec<(usize, CheckedExpr)>),
    /// F3 nested field/array designators. Each complete path contains
    /// checked subobject indices from the outer aggregate to the initialized
    /// object; disjoint paths can share an outer aggregate without erasing
    /// previous writes or re-zeroing the enclosing subobject.
    DeepInitializer(Vec<(Vec<usize>, CheckedExpr)>),
    /// An addressable C99 compound literal with automatic block storage.
    /// Its initializer runs when control evaluates this expression; each
    /// syntactic occurrence owns its own stack storage.
    CompoundLiteral(Box<CheckedExpr>),
    /// File-scope C99 compound literal with static storage duration. The
    /// private symbol uniquely identifies the object across initializers;
    /// unlike an automatic literal, this object must be emitted into ELF
    /// data, not materialized in a stack slot on evaluation.
    StaticCompoundLiteral { symbol: String, initializer: Box<CheckedExpr> },
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
    /// GCC __builtin_ctz(unsigned int): the argument is evaluated exactly once.
    /// As in GCC, a zero operand has undefined behavior; callers must guard it.
    BuiltinCtz(Box<CheckedExpr>),
    Convert(Box<CheckedExpr>),
    Load(Box<CheckedExpr>),
    /// Array lvalue conversion yields a pointer to the first element,
    /// without loading the entire aggregate into a scalar.
    ArrayDecay(Box<CheckedExpr>),
    /// Addressable member of a structure, with its checked byte offset.
    Member(Box<CheckedExpr>, u64),
    /// A non-addressable C bit-field member, stored in one 32-bit unit.
    /// It retains integer lvalue semantics for assignment and ++/--.
    BitFieldMember(Box<CheckedExpr>, u64, u8, u8),
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
    /// External ELF linkage symbol, distinct from C source identifier.
    pub asm_label: Option<String>,
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
    /// Case and default labels remain in source order to preserve fallthrough.
    /// Dispatch uses the controlling expression after integer promotions.
    Switch { control: CheckedExpr, body: Box<CheckedStmt> },
    /// The case value has been constant-evaluated and converted to the
    /// promoted controlling integer type, so duplicate detection is exact.
    Case { value: i128, body: Box<CheckedStmt> },
    Default(Box<CheckedStmt>),
    DoWhile { body: Box<CheckedStmt>, cond: CheckedExpr },
    For(Box<CheckedFor>),
    Break,
    Continue,
    /// Resolved in the label namespace of the enclosing function.
    Label { name: String, body: Box<CheckedStmt> },
    Goto { target: String },
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
    pub asm_label: Option<String>,
    pub enum_value: Option<i64>,
    /// C register object: the address of this object cannot be computed.
    pub is_register: bool,
}

#[derive(Clone)]
struct TagEntry {
    kind: TagKind,
    identity: u64,
    complete: Option<CType>,
}

/// Lexically nearest active switch. Cases inside nested loops/blocks belong
/// to it; a nested switch has an independent label namespace.
#[derive(Clone)]
struct SwitchContext {
    control_type: CType,
    seen_cases: HashSet<i128>,
    seen_default: bool,
}

/// Checked expressions carry their converted value types and lvalue category.
/// Scopes and persistent file symbols are local to this Checker instance.
pub struct Checker {
    target: Target,
    next_tag_identity: Cell<u64>,
    next_static_compound_identity: Cell<u64>,
    /// Independent tag namespace, parallel to ordinary identifier scopes.
    tags: RefCell<Vec<HashMap<String, TagEntry>>>,
    /// Stable definition lookup independent of currently visible shadowing tags.
    tag_definitions: RefCell<HashMap<u64, CType>>,
    globals: HashMap<String, Symbol>,
    locals: Vec<HashMap<String, Symbol>>,
    result_ty: Option<CType>,
    loop_depth: usize,
    switches: Vec<SwitchContext>,
    /// Function-scope label namespace, separate from ordinary identifiers/tags.
    function_labels: HashMap<String, Span>,
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
        Ok(Self { target: target.validate()?, next_tag_identity: Cell::new(1), next_static_compound_identity: Cell::new(1), tags: RefCell::new(vec![HashMap::new()]), tag_definitions: RefCell::new(HashMap::new()), globals: HashMap::new(), locals: Vec::new(), result_ty: None, loop_depth: 0, switches: Vec::new(), function_labels: HashMap::new(), rollback_log: None, statement_undo: Vec::new(),
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
                Linkage::None, false, span)?;
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
                    if matches!(base.unqualified(), CType::Function { .. } | CType::VariadicFunction { .. }) {
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
            Type::BuiltinVaList => {
                // x86-64 SysV GCC: typedef struct __va_list_tag {
                //   unsigned int gp_offset, fp_offset;
                //   void *overflow_arg_area, *reg_save_area;
                // } __builtin_va_list[1];
                // This must remain an array and decay to a tag pointer in
                // function parameters, not be modeled as an ordinary void*.
                if self.target.int_bits != 32 || self.target.long_bits != 64 {
                    return Err(type_error(span,
                        "__builtin_va_list requires the x86-64 SysV LP64 data model"));
                }
                let u32_ty = CType::Integer { rank: IntRank::Int, unsigned: true };
                let void_ptr = CType::Pointer(Box::new(CType::Void));
                let mut tag = types::layout_struct(
                    Some("__va_list_tag".to_owned()),
                    vec![
                        ("gp_offset".to_owned(), u32_ty.clone()),
                        ("fp_offset".to_owned(), u32_ty),
                        ("overflow_arg_area".to_owned(), void_ptr.clone()),
                        ("reg_save_area".to_owned(), void_ptr),
                    ],
                    self.target,
                ).map_err(|reason| type_error(span, reason))?;
                // Reserved nominal identity, identical across GCC builtin uses
                // but disjoint from user-written structure declarations.
                if let CType::Struct { identity, .. } = &mut tag {
                    *identity = u64::MAX;
                }
                CType::Array(Box::new(tag), Some(1))
            }
            Type::Void => CType::Void,
            Type::Bool => CType::Integer { rank: IntRank::Bool, unsigned: true },
            Type::Char => CType::Integer { rank: IntRank::Char, unsigned: !self.target.char_is_signed },
            Type::Short => CType::Integer { rank: IntRank::Short, unsigned: false },
            Type::Floating(rank) => CType::Floating(*rank),
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
                    // A member's written type can refer to a tag or typedef
                    // completed earlier in this translation unit. Resolve its
                    // nominal reference to that completed layout *before*
                    // layout_struct/layout_union inspect its byte size and
                    // alignment. Keep pointer targets nominal, so legal
                    // self-referential `struct Node *next` fields do not
                    // require completing the enclosing Node prematurely.
                    // Truly incomplete by-value members still fail below.
                    let (declared, bit_width) = match &member.ty {
                        Type::BitField(base, width) => {
                            if member.name.is_empty() {
                                return Err(type_error(member.span,
                                    "unnamed/zero-width bit-fields require separate layout support"));
                            }
                            (base.as_ref(), Some(width.as_ref()))
                        }
                        _ => (&member.ty, None),
                    };
                    let resolved = self.resolve(declared, member.span)?;
                    let resolved = self.try_complete_tag_type(resolved)?;
                    let width = bit_width.map(|width| {
                        self.check_bit_width(width, &resolved, member.span)
                    }).transpose()?;
                    fields.push((member.name.clone(), resolved, width));
                }
                let mut complete = if kind == TagKind::Union {
                    types::layout_union_with_bits(tag.clone(), fields, self.target)
                } else {
                    types::layout_struct_with_bits(tag.clone(), fields, self.target)
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
            Type::BitField(..) => return Err(type_error(span,
                "bit-field declaration is only valid inside a struct or union")),
            Type::Pointer(inner) => CType::Pointer(Box::new(self.resolve(inner, span)?)),
            Type::Function { result, params } | Type::VariadicFunction { result, params } => {
                let result = self.resolve(result, span)?;
                if matches!(result, CType::Function { .. } | CType::VariadicFunction { .. }) {
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
                if matches!(ty, Type::VariadicFunction { .. }) {
                    CType::VariadicFunction { result: Box::new(result), params: converted }
                } else {
                    CType::Function { result: Box::new(result), params: converted }
                }
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
            CType::Function { .. } | CType::VariadicFunction { .. } => CType::Pointer(Box::new(ty)),
            other => other,
        }
    }

    fn define_local(&mut self, name: &str, ty: CType, kind: SymbolKind, linkage: Linkage, is_register: bool, span: Span) -> Result<(), Diagnostic> {
        let scope = self.locals.last_mut().expect("local declaration outside a function");
        if scope.contains_key(name) {
            return Err(Diagnostic::coded(span, DiagnosticCode::SemanticDuplicateDeclaration, format!("duplicate declaration of '{name}'")));
        }
        scope.insert(name.to_owned(), Symbol {
            ty, kind, linkage, defined: true, asm_label: None, enum_value: None, is_register,
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
                ty, kind, linkage: requested, defined: defining, asm_label: None, enum_value: None, is_register: false,
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
            if matches!(element.unqualified(), CType::Integer { rank: IntRank::Char, .. }) {
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
            let mut next = 0u64;
            let mut bound = 0u64;
            for entry in values {
                let designated = match entry.without_location() {
                    Expr::Designated { path, .. } => {
                        match path.first().ok_or_else(|| type_error(
                            span, "empty initializer designation"))? {
                            Designator::Index(index) => {
                                Some(self.initializer_index(index, span)?)
                            }
                            Designator::Field(_) => return Err(type_error(span,
                                "field designator requires struct or union initializer")),
                        }
                    }
                    _ => None,
                };
                let index = designated.unwrap_or(next);
                next = index.checked_add(1)
                    .ok_or_else(|| type_error(span, "array designator index overflow"))?;
                bound = bound.max(next);
            }
            if bound > 1_048_576 {
                return Err(type_error(span, "array initializer exceeds 1 MiB element guard"));
            }
            return Ok(CType::Array(element.clone(), Some(bound)));
        }
        Ok(ty)
    }

    /// Evaluate a C99 designator index as an integer constant expression.
    /// Negative indices and enormous sparse arrays are diagnosed before
    /// converting to host indexes or allocating initializer vectors.
    fn initializer_index(&self, expr: &Expr, span: Span) -> Result<u64, Diagnostic> {
        let typed = self.check_expr(expr, expr.location().unwrap_or(span))?;
        if !typed.ty.is_integer() {
            return Err(type_error(span, "array designator requires integer constant expression"));
        }
        let value = const_eval::evaluate(&typed, self.target)
            .map_err(|_| type_error(span, "array designator requires integer constant expression"))?;
        if !(0..=1_048_576).contains(&value) {
            return Err(type_error(span, "array designator index out of supported range"));
        }
        u64::try_from(value)
            .map_err(|_| type_error(span, "array designator index out of supported range"))
    }

    /// Resolve one C99 designator path through the current aggregate, with
    /// strict bounds and member-name constraints at *every* level.
    fn initializer_path(
        &self, ty: &CType, path: &[Designator], span: Span,
    ) -> Result<(Vec<usize>, CType), Diagnostic> {
        let mut current = ty;
        let mut slots = Vec::with_capacity(path.len());
        for part in path {
            let (index, next) = match (part, current.unqualified()) {
                (Designator::Field(name), CType::Struct { members, .. } |
                    CType::Union { members, .. }) => {
                    let i = members.iter().position(|m| m.name == *name)
                        .ok_or_else(|| type_error(span,
                            format!("unknown initializer member '{name}'")))?;
                    (i, &members[i].ty)
                }
                (Designator::Index(expr), CType::Array(element, Some(count))) => {
                    let i = self.initializer_index(expr, span)?;
                    if i >= *count {
                        return Err(type_error(span, "array designator index exceeds bound"));
                    }
                    (usize::try_from(i).map_err(|_| type_error(
                        span, "array designator index too large"))?, element.as_ref())
                }
                (Designator::Field(_), _) => return Err(type_error(span,
                    "field designator requires struct or union initializer")),
                (Designator::Index(_), _) => return Err(type_error(span,
                    "array designator requires array initializer")),
            };
            slots.push(index);
            current = next;
        }
        Ok((slots, current.clone()))
    }

    /// Overlapping whole subobjects and distinct members of the *same union*
    /// must not be accepted until override semantics are modeled. Independent
    /// paths to members of a struct/array are safe to emit in source order.
    fn initializer_paths_overlap(ty: &CType, a: &[usize], b: &[usize]) -> bool {
        let mut current = ty;
        for (left, right) in a.iter().zip(b) {
            if left != right {
                return matches!(current.unqualified(), CType::Union { .. });
            }
            current = match current.unqualified() {
                CType::Struct { members, .. } | CType::Union { members, .. } =>
                    match members.get(*left) {
                        Some(member) => &member.ty,
                        None => return true,
                    },
                CType::Array(element, _) => element.as_ref(),
                _ => return true,
            };
        }
        // Exact duplicate or parent/child overwrite.
        true
    }

    /// Use flat source-order paths for nested designations so two writes like
    /// `[1].a=3,[1].b=4` do not each reset the other's struct storage.
    /// C99 positional continuation *after* a multi-component designation
    /// needs a subobject cursor and stays fail-closed in this bounded slice.
    fn check_deep_initializer(
        &self, values: &[Expr], ty: &CType, span: Span, is_static: bool,
    ) -> Result<CheckedExpr, Diagnostic> {
        let mut checked_paths: Vec<(Vec<usize>, CheckedExpr)> = Vec::new();
        let mut cursor = 0usize;
        let mut last_nested = false;
        for value in values {
            let located = value.location().unwrap_or(span);
            let (path, initializer) = match value.without_location() {
                Expr::Designated { path, value } => {
                    let (slots, _) = self.initializer_path(ty, path, located)?;
                    (slots, value.as_ref())
                }
                _ => {
                    if last_nested {
                        return Err(type_error(located,
                            "positional continuation after nested designator is not yet supported"));
                    }
                    (vec![cursor], value)
                }
            };
            if path.is_empty() {
                return Err(type_error(located, "empty initializer designation"));
            }
            // Resolve positional elements with the same aggregate checks.
            let mut current = ty;
            for &slot in &path {
                current = match current.unqualified() {
                    CType::Struct { members, .. } | CType::Union { members, .. } =>
                        &members.get(slot).ok_or_else(|| type_error(located,
                            "too many struct or union initializers"))?.ty,
                    CType::Array(element, Some(count)) => {
                        if u64::try_from(slot).map_or(true, |i| i >= *count) {
                            return Err(type_error(located, "too many array initializers"));
                        }
                        element.as_ref()
                    }
                    _ => return Err(type_error(located,
                        "initializer designator requires aggregate subobject")),
                };
            }
            if checked_paths.iter().any(|(prior, _)|
                Self::initializer_paths_overlap(ty, prior, &path))
            {
                return Err(type_error(located,
                    "overlapping initializer targets require override handling"));
            }
            let checked = self.check_object_initializer(initializer, current, span, is_static)?;
            cursor = path[0].checked_add(1)
                .ok_or_else(|| type_error(located, "initializer cursor overflow"))?;
            last_nested = path.len() > 1;
            checked_paths.push((path, checked));
        }
        Ok(CheckedExpr::value(ty.clone(),
            CheckedExprKind::DeepInitializer(checked_paths)))
    }

    fn check_object_initializer(
        &self, original: &Expr, ty: &CType, span: Span, is_static: bool
    ) -> Result<CheckedExpr, Diagnostic> {
        let span = original.location().unwrap_or(span);
        let original = original.without_location();
        if let Expr::Initializer(values) = original {
            if values.iter().any(|value| matches!(
                value.without_location(), Expr::Designated { path, .. } if path.len() > 1
            )) {
                return self.check_deep_initializer(values, ty, span, is_static);
            }
            // Every entry has a source-order position. Designators can change
            // the current object; the next undesignated entry follows that
            // element, not the previous positional index.
            let mut entries = Vec::with_capacity(values.len());
            let mut seen = HashSet::new();
            let mut cursor = 0usize;
            let mut has_designators = false;
            let array = matches!(ty.unqualified(), CType::Array(..));
            for value in values {
                let (slot, input) = match value.without_location() {
                    Expr::Designated { path, value: inner } => {
                        has_designators = true;
                        if path.len() != 1 {
                            return Err(type_error(value.location().unwrap_or(span),
                                "nested designator paths are not yet supported"));
                        }
                        let slot = match (&path[0], ty.unqualified()) {
                            (Designator::Index(index), CType::Array(_, _)) =>
                                usize::try_from(self.initializer_index(index, span)?)
                                    .map_err(|_| type_error(span, "array designator index too large"))?,
                            (Designator::Field(field), CType::Struct { members, .. } |
                                CType::Union { members, .. }) =>
                                members.iter().position(|m| &m.name == field)
                                    .ok_or_else(|| type_error(span,
                                        format!("unknown initializer member '{field}'")))?,
                            (Designator::Field(_), _) => return Err(type_error(span,
                                "field designator requires struct or union initializer")),
                            (Designator::Index(_), _) => return Err(type_error(span,
                                "array designator requires array initializer")),
                        };
                        (slot, inner.as_ref())
                    }
                    _ => (cursor, value),
                };
                let element = match ty.unqualified() {
                    CType::Struct { members, .. } => members.get(slot).map(|m| &m.ty)
                        .ok_or_else(|| type_error(span, "too many struct initializers"))?,
                    CType::Union { members, .. } => members.get(slot).map(|m| &m.ty)
                        .ok_or_else(|| type_error(span, "too many union initializers"))?,
                    CType::Array(element, Some(count)) => {
                        if u64::try_from(slot).map_or(true, |i| i >= *count) {
                            return Err(type_error(span, if has_designators {
                                "array designator index exceeds bound"
                            } else { "too many array initializers" }));
                        }
                        element.as_ref()
                    }
                    CType::Array(_, None) => return Err(type_error(span,
                        "incomplete array initializer")),
                    _ => {
                        if has_designators {
                            return Err(type_error(span,
                                "designated initializer requires aggregate type"));
                        }
                        if values.len() != 1 {
                            return Err(type_error(span, "scalar initializer requires one expression"));
                        }
                        return self.check_object_initializer(value, ty, span, is_static);
                    }
                };
                if !seen.insert(slot) {
                    return Err(type_error(span,
                        "duplicate initializer target requires override handling"));
                }
                if !array && matches!(ty.unqualified(), CType::Union { .. }) && !entries.is_empty() {
                    return Err(type_error(span, if has_designators {
                        "multiple union member initializers require override handling"
                    } else { "union initializer selects only its first member" }));
                }
                let checked = self.check_object_initializer(input, element, span, is_static)?;
                entries.push((slot, checked));
                cursor = slot.checked_add(1)
                    .ok_or_else(|| type_error(span, "initializer cursor overflow"))?;
            }
            return Ok(CheckedExpr::value(ty.clone(),
                if has_designators {
                    CheckedExprKind::SparseInitializer(entries)
                } else {
                    CheckedExprKind::Initializer(
                        entries.into_iter().map(|(_, value)| value).collect())
                }));
        }
        if matches!(original, Expr::Designated { .. }) {
            return Err(type_error(span, "initializer designator requires enclosing braces"));
        }
        let expression = self.check_expr(original, span)?;
        if is_static {
            self.validate_static_initializer(&expression, original, ty, span)?;
        }
        if matches!((ty.unqualified(), original),
            (CType::Array(element, Some(_)), Expr::StringLiteral(_))
            if matches!(element.unqualified(), CType::Integer { rank: IntRank::Char, .. }))
        {
            // Retain literal bytes; array-to-pointer decay would lose the
            // source bytes needed to initialize the array.
            Ok(expression)
        } else {
            self.convert_for_assignment(expression, ty, original, span)
        }
    }

    fn check_global(&mut self, decl: Decl) -> Result<CheckedDecl, Diagnostic> {
        if decl.storage == StorageClass::Register {
            return Err(type_error(decl.span, "register storage class is not allowed at file scope"));
        }
        let raw = self.resolve(&decl.ty, decl.span)?;
        let raw = self.try_complete_tag_type(raw)?;
        let ty = self.complete_string_array(raw, decl.init.as_ref(), decl.span)?;
        let kind = if matches!(ty, CType::Function { .. } | CType::VariadicFunction { .. }) { SymbolKind::Function } else { SymbolKind::Object };
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
        if let Some(label) = &decl.asm_label {
            if kind != SymbolKind::Function || decl.storage == StorageClass::Static {
                return Err(type_error(decl.span,
                    "external asm linkage label requires an externally linked function"));
            }
            let global = self.globals.get_mut(&decl.name).expect("declared global");
            if global.asm_label.as_ref().is_some_and(|prior| prior != label) {
                return Err(Diagnostic::coded(decl.span,
                    DiagnosticCode::SemanticConflictingDeclaration,
                    format!("conflicting asm linkage symbol for '{}'", decl.name)));
            }
            global.asm_label = Some(label.clone());
        }
        // A prior incomplete array declaration can be completed here; publish
        // the composite type to both the checked item and later references.
        let ty = self.global(&decl.name).expect("registered global").ty.clone();
        let initializer = decl.init.as_ref()
            .map(|value| self.check_object_initializer(value, &ty, decl.span, true))
            .transpose()?;
        let global = self.global(&decl.name).expect("registered global");
        let linkage = global.linkage;
        let asm_label = global.asm_label.clone();
        Ok(CheckedDecl {
            storage: decl.storage, name: decl.name, ty, linkage, asm_label,
            initializer, span: decl.span,
        })
    }

    /// Discover all labels before checking jumps: forward targets are legal,
    /// and labels have function scope even when nested in compound statements.
    fn collect_function_labels(
        stmt: &Stmt, labels: &mut HashMap<String, Span>,
    ) -> Result<(), Diagnostic> {
        match stmt {
            Stmt::Label { name, span, body } => {
                if labels.insert(name.clone(), *span).is_some() {
                    return Err(type_error(*span, format!("duplicate label '{name}' in function")));
                }
                Self::collect_function_labels(body, labels)?;
            }
            Stmt::Block(stmts) => {
                for stmt in stmts {
                    Self::collect_function_labels(stmt, labels)?;
                }
            }
            Stmt::If { then, otherwise, .. } => {
                Self::collect_function_labels(then, labels)?;
                if let Some(otherwise) = otherwise {
                    Self::collect_function_labels(otherwise, labels)?;
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. }
            | Stmt::Switch { body, .. } | Stmt::Case { body, .. }
            | Stmt::Default(body) => Self::collect_function_labels(body, labels)?,
            Stmt::For { init, body, .. } => {
                if let Some(init) = init {
                    Self::collect_function_labels(init, labels)?;
                }
                Self::collect_function_labels(body, labels)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn check_function(&mut self, func: Function) -> Result<CheckedFunction, Diagnostic> {
        if func.storage == StorageClass::Register {
            return Err(type_error(func.span, "function cannot have register storage class"));
        }
        let result = self.resolve(&func.result, func.span)?;
        if matches!(result, CType::Function { .. } | CType::VariadicFunction { .. }) {
            return Err(type_error(func.span, "function cannot return a function"));
        }
        let mut params = Vec::new();
        for param in &func.params {
            if !matches!(param.storage, StorageClass::Default | StorageClass::Register) {
                return Err(type_error(param.span, "function parameter requires automatic or register storage"));
            }
            let resolved = self.resolve(&param.ty, param.span)?;
            let ty = self.adjust_parameter(resolved);
            if !ty.is_object() {
                return Err(type_error(param.span, "function parameter cannot have void or function type"));
            }
            if param.storage == StorageClass::Register && !ty.is_integer()
                && !matches!(ty.unqualified(), CType::Pointer(_))
            {
                return Err(type_error(param.span, "register parameter currently requires an integer or pointer type"));
            }
            params.push(CheckedDecl {
                storage: param.storage, name: param.name.clone(), ty,
                linkage: Linkage::None, asm_label: None, initializer: None, span: param.span,
            });
        }
        let signature = CType::Function {
            result: Box::new(result.clone()),
            params: params.iter().map(|p| p.ty.unqualified().clone()).collect(),
        };
        // Resolve labels independently of ordinary lexical identifier scopes.
        // A jump may target a label declared later in the same function.
        let mut function_labels = HashMap::new();
        Self::collect_function_labels(&func.body, &mut function_labels)?;
        // Register before checking the body: recursive calls are legal.
        self.declare_global(&func.name, signature.clone(), SymbolKind::Function, true, func.storage,
            func.name_span.unwrap_or(func.span))?;
        self.locals.push(HashMap::new());
        self.tags.borrow_mut().push(HashMap::new());
        self.function_labels = function_labels;
        self.result_ty = Some(result);
        for (parsed_param, param) in func.params.iter().zip(&params) {
            if let Err(err) = self.define_local(&param.name, param.ty.clone(),
                SymbolKind::Object, Linkage::None,
                parsed_param.storage == StorageClass::Register,
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
        self.function_labels.clear();
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
            let switches = self.switches.clone();
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
                    self.switches = switches;
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
                // Register is a storage-duration hint, not permission to take
                // the address of an object. Until aggregate handling can
                // enforce this recursively, limit it to integer/pointer scalars.
                if decl.storage == StorageClass::Register && !ty.is_integer()
                    && !matches!(ty.unqualified(), CType::Pointer(_))
                {
                    return Err(type_error(decl.span, "register local currently requires an integer or pointer type"));
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
                    decl.storage == StorageClass::Register,
                    decl.name_span.unwrap_or(decl.span))?;
                let initializer = decl.init.as_ref()
                    .map(|value| self.check_object_initializer(value, &ty, decl.span,
                        decl.storage == StorageClass::Static))
                    .transpose()?;
                CheckedStmt::Decl(CheckedDecl {
                    storage: decl.storage, name: decl.name.clone(), ty,
                    linkage, asm_label: None,
                    initializer, span: decl.span,
                })
            }
            Stmt::Typedef(decl) => {
                let ty = self.resolve(&decl.ty, decl.span)?;
                self.define_local(&decl.name, ty.clone(), SymbolKind::Typedef, Linkage::None,
                    false, decl.name_span.unwrap_or(decl.span))?;
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
            Stmt::Switch { control, body } => {
                let control_span = control.location().unwrap_or(span);
                let control = self.check_expr(control, control_span)?.rvalue();
                if !control.ty.is_integer() {
                    return Err(type_error(control_span,
                        "switch controlling expression requires an integer type"));
                }
                let promoted = self.target.promote(&control.ty);
                let control = self.convert(control, &promoted);
                self.switches.push(SwitchContext {
                    control_type: promoted,
                    seen_cases: HashSet::new(),
                    seen_default: false,
                });
                // Always pop the active switch even after a body error.
                let checked = self.check_stmt(body, span);
                self.switches.pop();
                CheckedStmt::Switch { control, body: Box::new(checked?) }
            }
            Stmt::Case { value, body } => {
                let case_span = value.location().unwrap_or(span);
                let promoted = self.switches.last().ok_or_else(||
                    type_error(case_span, "case label outside switch"))?
                    .control_type.clone();
                let value = self.check_expr(value, case_span)?.rvalue();
                if !value.ty.is_integer() {
                    return Err(type_error(case_span,
                        "case label requires an integer constant expression"));
                }
                // The case's integer constant expression is converted to the
                // promoted switch type before duplicate detection (C99 6.8.4.2).
                let value = const_eval::evaluate(&self.convert(value, &promoted), self.target)
                    .map_err(|why| type_error(case_span,
                        format!("case label requires an integer constant expression: {why}")))?;
                if !self.switches.last_mut().expect("active switch").seen_cases.insert(value) {
                    return Err(type_error(case_span, format!(
                        "duplicate case value {value} in switch")));
                }
                CheckedStmt::Case { value, body: Box::new(self.check_stmt(body, span)?) }
            }
            Stmt::Default(body) => {
                let context = self.switches.last_mut().ok_or_else(||
                    type_error(span, "default label outside switch"))?;
                if context.seen_default {
                    return Err(type_error(span, "duplicate default label in switch"));
                }
                context.seen_default = true;
                CheckedStmt::Default(Box::new(self.check_stmt(body, span)?))
            }
            Stmt::Label { name, body, .. } => {
                CheckedStmt::Label {
                    name: name.clone(),
                    body: Box::new(self.check_stmt(body, span)?),
                }
            }
            Stmt::Goto { target, span } => {
                if !self.function_labels.contains_key(target) {
                    return Err(type_error(*span, format!("goto to undefined label '{target}'")));
                }
                CheckedStmt::Goto { target: target.clone() }
            }
            Stmt::Break => {
                if self.loop_depth == 0 && self.switches.is_empty() {
                    return Err(type_error(span, "break outside loop or switch"));
                }
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

    /// Only file-scope compound literal objects can contribute static
    /// object addresses to another file-scope pointer initializer. A block
    /// compound literal is automatic even inside a block-scope static
    /// declaration; its address cannot be a static initializer.
    fn static_compound_lvalue(expr: &CheckedExpr) -> bool {
        match &expr.kind {
            CheckedExprKind::StaticCompoundLiteral { .. } => true,
            CheckedExprKind::Member(base, _) => Self::static_compound_lvalue(base),
            _ => false,
        }
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
                && matches!(element.unqualified(), CType::Integer { rank: IntRank::Char, .. })
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
            // A checked C null pointer initializer is commonly represented
            // as a conversion from an integer constant expression (including
            // explicit (void*)0 or nested pointer casts). Do not ask the
            // integer evaluator to interpret the pointer-typed conversion.
            let mut constant = expression;
            while let CheckedExprKind::Convert(inner) = &constant.kind {
                constant = inner;
            }
            let null = constant.ty.is_integer()
                && const_eval::evaluate(constant, self.target) == Ok(0);
            let address_of_global = match original {
                Expr::Unary(UnOp::Address, inner) => match inner.without_location() {
                    Expr::Name(name) => self.globals.get(name).is_some_and(|s| s.kind != SymbolKind::Typedef),
                    _ => false,
                },
                Expr::Name(name) => self.globals.get(name).is_some_and(|s|
                    matches!(&s.ty, CType::Array(..)) ||
                    (s.kind == SymbolKind::Function && matches!(ty, CType::Pointer(inner)
                        if matches!(inner.as_ref(), CType::Function { .. } | CType::VariadicFunction { .. })))),
                Expr::StringLiteral(_) => matches!(ty, CType::Pointer(element)
                    if matches!(element.as_ref(), CType::Integer { rank: IntRank::Char, .. } | CType::Void)),
                _ => false,
            };
            let static_compound_address = match &constant.kind {
                CheckedExprKind::Unary(UnOp::Address, inner) |
                CheckedExprKind::ArrayDecay(inner) =>
                    Self::static_compound_lvalue(inner),
                // Static array compound literals decay only after this
                // validator; the raw checked expression is still an lvalue.
                _ => Self::static_compound_lvalue(constant),
            };
            if !null && !address_of_global && !static_compound_address {
                return Err(type_error(span, "pointer initializer requires a null constant or address of global symbol"));
            }
        }
        Ok(())
    }

    /// Resolve the integer subset of C cast type-names without mutating tags
    /// or scopes; expression checking only holds an immutable checker borrow.
    fn cast_integer_type(&self, ty: &Type, span: Span) -> Result<CType, Diagnostic> {
        let resolved = match ty {
            Type::Bool => CType::Integer { rank: IntRank::Bool, unsigned: true },
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
        // A parameter declared using a tag reference can be completed by a
        // later definition of that tag. At the call site a by-value argument
        // must have the completed aggregate type, not the prototype's
        // earlier TagRef representation.
        let value_ty = match ty.unqualified() {
            CType::TagRef { .. } => self.complete_tag_type(ty.unqualified().clone(), span)?,
            other => other.clone(),
        };
        let compatible = match (&value_ty, &expr.ty) {
            (CType::Struct { .. }, CType::Struct { .. }) |
            (CType::Union { .. }, CType::Union { .. }) => value_ty.same_type(&expr.ty),
            (CType::Integer { .. }, CType::Integer { .. }) => true,
            // C99 6.3.1.2: any scalar pointer converts to _Bool by
            // comparing against null (rather than truncating an address).
            // Keep implicit pointer -> ordinary integer conversions invalid.
            (CType::Integer { rank: IntRank::Bool, .. }, CType::Pointer(_)) => true,
            // Preserve exact-rank floating values in checked code. Implicit
            // FP conversions and native emission remain separately gated.
            (CType::Floating(a), CType::Floating(b)) => a == b,
            (CType::Pointer(lhs), CType::Pointer(rhs)) =>
                CType::compatible_pointee_assignment(lhs, rhs),
            (CType::Pointer(_), CType::Integer { .. }) => const_eval::evaluate(&expr, self.target) == Ok(0),
            _ => false,
        };
        if !compatible {
            return Err(Diagnostic::coded(span, DiagnosticCode::SemanticIncompatibleAssignment, format!("incompatible assignment: {:?} to {:?}", expr.ty, ty)));
        }
        Ok(self.convert(expr, &value_ty))
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

    /// Resolve an inline aggregate definition in an expression type-name.
    /// C99 compound literals and sizeof(type-name) may introduce new struct
    /// or union tags. Use the current tag namespace, not a copied/structural
    /// type; each anonymous definition must have a fresh nominal identity.
    /// Declared member types are recursively resolved and *completed* before
    /// layout, while pointers to the tag under construction stay nominal.
    fn check_bit_width(
        &self, width: &Expr, ty: &CType, span: Span,
    ) -> Result<u8, Diagnostic> {
        if !matches!(ty.unqualified(),
            CType::Integer { rank: IntRank::Int, .. })
        {
            return Err(type_error(span,
                "bit-field requires signed or unsigned int in this milestone"));
        }
        let typed = self.check_expr(width, span)?.rvalue();
        if !typed.ty.is_integer() {
            return Err(type_error(span, "bit-field width must be an integer constant expression"));
        }
        let value = const_eval::evaluate(&typed, self.target)
            .map_err(|why| type_error(span, format!(
                "bit-field width requires integer constant expression: {why}")))?;
        if !(1..=32).contains(&value) {
            return Err(type_error(span,
                "named bit-field width must be between 1 and 32"));
        }
        u8::try_from(value).map_err(|_| type_error(span, "bit-field width overflow"))
    }

    fn resolve_typename_aggregate(
        &self, ty: &Type, span: Span,
    ) -> Result<CType, Diagnostic> {
        let (kind, tag, members) = match ty {
            Type::Struct { tag, members } => (TagKind::Struct, tag, members),
            Type::Union { tag, members } => (TagKind::Union, tag, members),
            _ => return Err(type_error(span,
                "inline aggregate type-name requires struct or union definition")),
        };
        let identity = if let Some(name) = tag {
            let existing = self.tags.borrow().last()
                .and_then(|scope| scope.get(name)).cloned();
            if let Some(entry) = existing {
                if entry.kind != kind {
                    return Err(type_error(span,
                        format!("conflicting tag kind for '{name}'")));
                }
                if entry.complete.is_some() {
                    return Err(type_error(span,
                        format!("redefinition of aggregate tag '{name}'")));
                }
                entry.identity
            } else {
                let id = self.new_tag_identity();
                self.tags.borrow_mut().last_mut().expect("tag scope")
                    .insert(name.clone(), TagEntry {
                        kind, identity: id, complete: None,
                    });
                id
            }
        } else {
            self.new_tag_identity()
        };
        let mut fields = Vec::with_capacity(members.len());
        for member in members {
            if member.init.is_some() || member.storage != StorageClass::Default {
                return Err(type_error(member.span,
                    "aggregate members cannot have initializers or storage classes"));
            }
            let (declared, bit_width) = match &member.ty {
                Type::BitField(base, width) => {
                    if member.name.is_empty() {
                        return Err(type_error(member.span,
                            "unnamed/zero-width bit-fields require separate layout support"));
                    }
                    (base.as_ref(), Some(width.as_ref()))
                }
                _ => (&member.ty, None),
            };
            let member_ty = self.resolve_sizeof_type(declared, member.span)?;
            let member_ty = self.try_complete_tag_type(member_ty)?;
            let width = bit_width.map(|width| {
                self.check_bit_width(width, &member_ty, member.span)
            }).transpose()?;
            fields.push((member.name.clone(), member_ty, width));
        }
        let mut complete = if kind == TagKind::Struct {
            types::layout_struct_with_bits(tag.clone(), fields, self.target)
        } else {
            types::layout_union_with_bits(tag.clone(), fields, self.target)
        }.map_err(|why| type_error(span, why))?;
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
            self.tag_definitions.borrow_mut()
                .insert(identity, complete.clone());
        }
        Ok(complete)
    }

    /// Type-name resolution in sizeof, casts and compound literals.
    /// References to existing tags are read-only, but an *explicit* inline
    /// struct/union definition introduces its tag in the current C scope.
    /// Enumerator definitions remain handled by declaration processing.
    fn resolve_sizeof_type(&self, ty: &Type, span: Span) -> Result<CType, Diagnostic> {
        Ok(match ty {
            Type::Void => CType::Void,
            Type::Bool => CType::Integer { rank: IntRank::Bool, unsigned: true },
            Type::Char => CType::Integer {
                rank: IntRank::Char, unsigned: !self.target.char_is_signed,
            },
            Type::Short => CType::Integer { rank: IntRank::Short, unsigned: false },
            Type::Int => CType::int(),
            Type::Long => CType::Integer { rank: IntRank::Long, unsigned: false },
            Type::LongLong => CType::Integer { rank: IntRank::LongLong, unsigned: false },
            Type::Floating(rank) => CType::Floating(*rank),
            Type::Struct { .. } | Type::Union { .. } =>
                self.resolve_typename_aggregate(ty, span)?,
            Type::TagRef(kind, name) => {
                // sizeof(struct Tag) needs the *visible* tag definition, not
                // an ordinary identifier lookup. This is deliberately
                // read-only: sizeof must not create or shadow a tag.
                // Keep incomplete references intact for legal
                // sizeof(struct Forward *), while rejecting the incomplete
                // object itself in the existing size_bytes check.
                let entry = self.lookup_tag(*kind, name, span)?.ok_or_else(|| {
                    type_error(span, format!("unknown aggregate tag '{name}' in sizeof"))
                })?;
                self.try_complete_tag_type(CType::TagRef {
                    kind: *kind, name: name.clone(), identity: entry.identity,
                })?
            }
            Type::Alias(name) => {
                let symbol = self.lookup(name).ok_or_else(|| type_error(span,
                    format!("unknown typedef '{name}' in sizeof")))?;
                if symbol.kind != SymbolKind::Typedef {
                    return Err(type_error(span, "sizeof type-name requires a typedef"));
                }
                symbol.ty.clone()
            }
            Type::Pointer(inner) => CType::Pointer(Box::new(
                self.resolve_sizeof_type(inner, span)?)),
            Type::Qualified(inner, qualifiers) => {
                let inner = self.resolve_sizeof_type(inner, span)?;
                if qualifiers.is_restrict && !matches!(inner.unqualified(), CType::Pointer(_)) {
                    return Err(type_error(span, "restrict requires pointer type"));
                }
                if qualifiers.is_const { inner.with_const() } else { inner }
            }
            Type::Signed(inner) | Type::Unsigned(inner) => {
                let unsigned = matches!(ty, Type::Unsigned(_));
                let CType::Integer { rank, .. } = self.resolve_sizeof_type(inner, span)? else {
                    return Err(type_error(span, "signed/unsigned requires integer type"));
                };
                CType::Integer { rank, unsigned }
            }
            Type::Array(inner, bound) => {
                let element = self.resolve_sizeof_type(inner, span)?;
                let count = bound.as_ref().map(|expr| {
                    let typed = self.check_expr(expr, span)?;
                    if !typed.ty.is_integer() {
                        return Err(type_error(span,
                            "array bound requires an integer constant expression"));
                    }
                    let value = const_eval::evaluate(&typed, self.target)
                        .map_err(|why| type_error(span, format!(
                            "array bound requires an integer constant expression: {why}")))?;
                    if value <= 0 {
                        return Err(type_error(span, "array bound must be positive"));
                    }
                    u64::try_from(value)
                        .map_err(|_| type_error(span, "array bound exceeds the supported range"))
                }).transpose()?;
                CType::Array(Box::new(element), count)
            }
            _ => return Err(type_error(span,
                "unsupported type-name in sizeof, cast or compound literal")),
        })
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
            Expr::Initializer(_) | Expr::Designated { .. } => Err(type_error(span,
                "brace initializer is only valid in an object declaration")),
            Expr::CompoundLiteral(type_name, initializer) => {
                if !matches!(initializer.without_location(), Expr::Initializer(_)) {
                    return Err(type_error(span,
                        "compound literal requires braced initializer list"));
                }
                // Resolve visible tags, typedefs and inline C99 struct/union
                // definitions, preserving a fresh nominal identity for each
                // anonymous type. Their member offsets use ordinary layout.
                let resolved = self.resolve_sizeof_type(type_name, span)?;
                let ty = self.complete_string_array(
                    self.try_complete_tag_type(resolved)?, Some(initializer), span)?;
                if !ty.is_object() || ty.size_bytes(self.target).is_none() {
                    return Err(type_error(span,
                        "compound literal requires a complete object type"));
                }
                let static_storage = self.result_ty.is_none();
                let value = self.check_object_initializer(initializer, &ty, span, static_storage)?;
                let kind = if static_storage {
                    // Checker::check_expr is read-only by design, so allocate
                    // per-occurrence IDs using the same Cell pattern as tag
                    // identities. The prefix contains '.' and cannot collide
                    // with a valid C source identifier.
                    let id = self.next_static_compound_identity.get();
                    self.next_static_compound_identity.set(id.checked_add(1)
                        .ok_or_else(|| type_error(span, "too many static compound literals"))?);
                    CheckedExprKind::StaticCompoundLiteral {
                        symbol: format!(".Lmini_c_compound_{id}"),
                        initializer: Box::new(value),
                    }
                } else {
                    CheckedExprKind::CompoundLiteral(Box::new(value))
                };
                Ok(CheckedExpr { ty, category: Category::LValue, kind })
            },
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
            Expr::SizeofExpr(operand) => {
                // C99 6.5.3.4: the operand is *not evaluated*. In particular
                // do not call .rvalue(), which would decay an array to pointer,
                // or evaluate side effects from function calls and ++/--.
                let typed = self.check_expr(operand, span)?;
                let ty = self.complete_tag_type(typed.ty, span)?;
                let size = ty.size_bytes(self.target).ok_or_else(|| type_error(
                    span, "sizeof requires a complete object type (not void or function)"
                ))?;
                let size_ty = CType::Integer { rank: IntRank::Long, unsigned: true };
                Ok(CheckedExpr::value(size_ty, CheckedExprKind::Literal(size)))
            }
            Expr::SizeofType(type_name) => {
                let operand_ty = self.resolve_sizeof_type(type_name, span)?;
                let operand_ty = self.complete_tag_type(operand_ty, span)?;
                let size = operand_ty.size_bytes(self.target).ok_or_else(|| type_error(
                    span, "sizeof requires a complete object type (not void or function)"
                ))?;
                // C sizeof has size_t type. The hosted x86-64 LP64 target
                // uses unsigned long, not the type of an ordinary decimal literal.
                let size_ty = CType::Integer { rank: IntRank::Long, unsigned: true };
                Ok(CheckedExpr::value(size_ty, CheckedExprKind::Literal(size)))
            }
            Expr::Int(number) => {
                let ty = self.target.decimal_literal(*number).ok_or_else(|| type_error(span, "decimal literal exceeds supported signed integer types"))?;
                Ok(CheckedExpr::value(ty, CheckedExprKind::Literal(*number)))
            }
            Expr::FloatLiteral(text, rank) => {
                // The frontend retains the exact token and its C rank without
                // rounding to the host f32/f64. Any native use is explicitly
                // rejected until target-format floating emission is supported.
                Ok(CheckedExpr::value(CType::Floating(*rank),
                    CheckedExprKind::FloatLiteral(text.clone())))
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
                let kind = match field.bit_field {
                    Some(info) => CheckedExprKind::BitFieldMember(
                        Box::new(object), offset, info.shift, info.width),
                    None => CheckedExprKind::Member(Box::new(object), offset),
                };
                Ok(CheckedExpr {
                    ty: field_ty,
                    category: Category::LValue,
                    kind,
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
                        // The constraint applies even if the backend would
                        // otherwise spill the local to memory.
                        if let Expr::Name(name) = inner.without_location() {
                            if self.lookup(name).is_some_and(|symbol| symbol.is_register) {
                                return Err(Diagnostic::coded(span,
                                    DiagnosticCode::SemanticInvalidOperatorOperands,
                                    "cannot take address of register object"));
                            }
                        }
                        if matches!(typed.kind, CheckedExprKind::BitFieldMember(..)) {
                            return Err(Diagnostic::coded(span, DiagnosticCode::SemanticInvalidOperatorOperands,
                                "cannot take the address of a bit-field"));
                        }
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
                        let category = if matches!(ty, CType::Function { .. } | CType::VariadicFunction { .. }) { Category::Function } else if ty == CType::Void {
                            return Err(type_error(span, "cannot dereference void pointer"));
                        } else { Category::LValue };
                        Ok(CheckedExpr { ty, category, kind: CheckedExprKind::Unary(*op, Box::new(typed)) })
                    }
                    UnOp::Not => {
                        if !typed.ty.is_scalar() { return Err(Diagnostic::coded(span, DiagnosticCode::SemanticInvalidOperatorOperands, "logical not requires scalar operand")); }
                        Ok(CheckedExpr::value(CType::int(), CheckedExprKind::Unary(*op, Box::new(typed.rvalue()))))
                    }
                    UnOp::Plus | UnOp::Neg | UnOp::BitNot => {
                        let floating = matches!(typed.ty.unqualified(), CType::Floating(_));
                        if !typed.ty.is_integer()
                            && !(floating && !matches!(op, UnOp::BitNot))
                        {
                            let reason = if floating {
                                "bitwise not requires integer"
                            } else {
                                "arithmetic unary operator requires integer"
                            };
                            return Err(Diagnostic::coded(span,
                                DiagnosticCode::SemanticInvalidOperatorOperands, reason));
                        }
                        let ty = if floating { typed.ty.unqualified().clone() }
                            else { self.target.promote(&typed.ty) };
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
                // Equality permits compatible pointers and object-pointer/void-pointer
                // pairs; ordering requires compatible object pointer types.
                let equality = matches!(op, BinOp::Eq | BinOp::Ne);
                if relation && (matches!((&left.ty, &right.ty), (CType::Pointer(a), CType::Pointer(b))
                        if (a.unqualified().same_type(b.unqualified())
                            && (equality || a.unqualified().is_object()))
                            || (equality && ((*a.unqualified() == CType::Void && b.unqualified().is_object())
                                || (*b.unqualified() == CType::Void && a.unqualified().is_object()))))
                    || (equality
                        && ((matches!(left.ty, CType::Pointer(_)) && const_eval::evaluate(&right, self.target) == Ok(0))
                            || (matches!(right.ty, CType::Pointer(_)) && const_eval::evaluate(&left, self.target) == Ok(0))))) {
                    return Ok(CheckedExpr::value(CType::int(), CheckedExprKind::Binary(*op, Box::new(left), Box::new(right))));
                }
                // C99 pointer subtraction yields ptrdiff_t, not a pointer.
                // Both pointers must refer to compatible complete object
                // types; only differences within the same array (or one past
                // it) are defined at runtime. The LP64 target uses signed long
                // for ptrdiff_t. Preserve pointers in checked IR so native
                // lowering can divide the byte delta by the element size.
                if *op == BinOp::Sub {
                    if let (CType::Pointer(lhs), CType::Pointer(rhs)) = (&left.ty, &right.ty) {
                        let left_element = self.complete_tag_type(lhs.as_ref().clone(), span)?;
                        let right_element = self.complete_tag_type(rhs.as_ref().clone(), span)?;
                        if !left_element.unqualified().same_type(right_element.unqualified()) {
                            return Err(Diagnostic::coded(span,
                                DiagnosticCode::SemanticInvalidOperatorOperands,
                                "pointer subtraction requires compatible pointed-to types"));
                        }
                        if left_element.size_bytes(self.target).is_none() {
                            return Err(Diagnostic::coded(span,
                                DiagnosticCode::SemanticInvalidOperatorOperands,
                                "pointer subtraction requires a complete object type"));
                        }
                        let left_ty = CType::Pointer(Box::new(left_element));
                        let right_ty = CType::Pointer(Box::new(right_element));
                        let left = self.convert(left, &left_ty);
                        let right = self.convert(right, &right_ty);
                        let ptrdiff = CType::Integer {
                            rank: IntRank::Long, unsigned: false,
                        };
                        return Ok(CheckedExpr::value(ptrdiff, CheckedExprKind::Binary(
                            BinOp::Sub, Box::new(left), Box::new(right),
                        )));
                    }
                }
                // Pointer + integer and pointer - integer retain pointer
                // type and scale offsets by the complete pointee's size.
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
                let common = if matches!(op, BinOp::Mod) || bitwise {
                    self.target.common_integer(&left.ty, &right.ty)
                } else {
                    self.target.common_arithmetic(&left.ty, &right.ty)
                }.ok_or_else(|| {
                    let has_float = matches!(left.ty.unqualified(), CType::Floating(_))
                        || matches!(right.ty.unqualified(), CType::Floating(_));
                    Diagnostic::coded(span, DiagnosticCode::SemanticInvalidOperatorOperands,
                        if has_float { "operator requires compatible arithmetic operands" }
                        else { "operator requires compatible integer operands" })
                })?;
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
                let ty = if let Some(arithmetic) =
                    self.target.common_arithmetic(&yes.ty, &no.ty)
                {
                    arithmetic
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
                    } else if (*base_a == CType::Void && base_b.is_object())
                        || (*base_b == CType::Void && base_a.is_object())
                    {
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
                // Preserve the existing integer cast rules, and accept the
                // ordinary C pointer type-name syntax without assuming that
                // all integer-to-pointer conversions have portable meaning.
                let dest = if matches!(ty, Type::Pointer(..) | Type::Alias(..)
                    | Type::Qualified(..))
                {
                    self.resolve_sizeof_type(ty, span)?
                } else {
                    self.cast_integer_type(ty, span)?
                };
                // C cast results are rvalues: top-level qualifiers on the
                // type name do not qualify the resulting scalar value.
                let dest = dest.unqualified().clone();
                let value = self.check_expr(operand, span)?.rvalue();
                match &dest {
                    CType::Integer { .. } if value.ty.is_integer() => {},
                    // C99 6.3.2.3 permits explicit pointer-to-integer casts.
                    // The resulting integer is implementation-defined; on
                    // our current LP64 target the checked IR preserves an
                    // explicit Convert, which Cranelift lowers as a same-width
                    // bit-pattern conversion (or integer truncation).
                    CType::Integer { .. } if matches!(value.ty.unqualified(), CType::Pointer(_)) => {},
                    CType::Pointer(_) if matches!(value.ty.unqualified(), CType::Pointer(_)) => {},
                    CType::Pointer(_) if value.ty.is_integer()
                        && const_eval::evaluate(&value, self.target) == Ok(0) => {},
                    CType::Pointer(_) => return Err(type_error(span,
                        "pointer cast requires a pointer or null integer constant")),
                    CType::Integer { .. } => return Err(type_error(span,
                        "integer cast requires integer operand")),
                    _ => return Err(type_error(span, "unsupported cast type-name")),
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
                // Recognize only the source-pinned GCC integer builtin required
                // by FreeBSD ffs. Do not invent prototypes for other builtins,
                // or steal a visible user declaration of the same name.
                if let Expr::Name(name) = callee.without_location() {
                    if name == "__builtin_ctz" && self.lookup(name).is_none() {
                        if args.len() != 1 {
                            return Err(Diagnostic::coded(span,
                                DiagnosticCode::SemanticWrongArgumentCount,
                                "__builtin_ctz expects exactly one argument"));
                        }
                        let argument = &args[0];
                        let argument_span = argument.location().unwrap_or(span);
                        let typed = self.check_expr(argument, argument_span)?.rvalue();
                        if !typed.ty.is_integer() {
                            return Err(Diagnostic::coded(argument_span,
                                DiagnosticCode::SemanticInvalidOperatorOperands,
                                "__builtin_ctz requires an integer argument"));
                        }
                        // GCC's signature is int __builtin_ctz(unsigned int).
                        // Conversion, including signed-negative inputs, happens
                        // before counting zero bits. The zero case is GCC UB;
                        // do not silently define a special return value here.
                        let uint = CType::Integer { rank: IntRank::Int, unsigned: true };
                        let argument = self.convert(typed, &uint);
                        return Ok(CheckedExpr::value(CType::int(),
                            CheckedExprKind::BuiltinCtz(Box::new(argument))));
                    }
                }
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
                    CType::Function { result, params } => Some((result.as_ref(), params.as_slice(), false)),
                    CType::VariadicFunction { result, params } => Some((result.as_ref(), params.as_slice(), true)),
                    CType::Pointer(inner) => match inner.as_ref() {
                        CType::Function { result, params } => Some((result.as_ref(), params.as_slice(), false)),
                        CType::VariadicFunction { result, params } => Some((result.as_ref(), params.as_slice(), true)),
                        _ => None,
                    },
                    _ => None,
                };
                let Some((result, params, variadic)) = signature else {
                    return Err(Diagnostic::coded(callee_span, DiagnosticCode::SemanticNotCallable,
                        "called expression is not a function"));
                };
                if (variadic && args.len() < params.len()) || (!variadic && args.len() != params.len()) {
                    return Err(Diagnostic::coded(span, DiagnosticCode::SemanticWrongArgumentCount,
                        if variadic {
                            format!("variadic function requires at least {} arguments, got {}", params.len(), args.len())
                        } else {
                            format!("function expects {} arguments, got {}", params.len(), args.len())
                        }));
                }
                let mut checked_args = Vec::with_capacity(args.len());
                for (arg, ty) in args.iter().zip(params) {
                    let typed = self.check_expr(arg, span)?;
                    checked_args.push(self.convert_for_assignment(typed, ty, arg,
                        arg.location().unwrap_or(span))?);
                }
                // C default argument promotions apply only to ellipsis arguments.
                for arg in args.iter().skip(params.len()) {
                    let typed = self.check_expr(arg, span)?.rvalue();
                    if !typed.ty.is_scalar() {
                        return Err(type_error(arg.location().unwrap_or(span),
                            "variadic argument requires a supported scalar type"));
                    }
                    let promoted = if matches!(typed.ty, CType::Floating(mini_c_syntax::ast::FloatRank::Float)) {
                        CType::Floating(mini_c_syntax::ast::FloatRank::Double)
                    } else {
                        self.target.promote(&typed.ty)
                    };
                    checked_args.push(if promoted == typed.ty {
                        typed
                    } else {
                        CheckedExpr::value(promoted, CheckedExprKind::Convert(Box::new(typed)))
                    });
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
