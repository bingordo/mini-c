//! Streaming, backend-neutral semantic analysis for the supported C subset.
//! A caller delivers one external item at a time; completed function ASTs are
//! never retained in the checker. No filesystem, global state or output side effects.
pub mod types;
pub mod const_eval;
pub mod builtins;
mod warnings;
#[allow(dead_code)] // generated target ISA tables remain test oracles, not source admission
mod asm_gp_rmw;
#[allow(dead_code)] // source checker does not decode assembler mnemonics
mod asm_gp_mov;
mod asm_names;
pub mod asm_output_roles;
#[cfg(test)]
mod asm_names_tests;
#[cfg(test)]
mod asm_gp_mov_tests;

use std::collections::{HashMap, HashSet};
use std::cell::{Cell, RefCell};
use mini_c_syntax::ast::{BinOp, Decl, Designator, Expr, Function, Span, Stmt, StorageClass, TagKind, Type, UnOp};
use mini_c_syntax::diag::{Diagnostic, DiagnosticCode, ReportedDiagnostic, WarningPolicy};
use mini_c_syntax::stream::{ExternalItem, ItemSource};

pub use types::{CType, IntRank, Target};
use types::type_error;
use warnings::UsageBinding;

/// Backend-private, per-function ELF object for C99's implicit
/// `static const char __func__[] = "function-name"`. Source identifiers
/// cannot contain '.', so this name never collides with a C declaration.
pub fn implicit_function_name_symbol(name: &str) -> String {
    format!(".Lmini_c_funcname_{name}")
}

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
    /// A later initializer of the same target displaces its earlier value,
    /// including the earlier expression's effects under the target ABI.
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
    /// Byte-reverse an exact unsigned width after one source conversion.
    BuiltinByteSwap(u8, Box<CheckedExpr>),
    /// Typed GNU atomic builtin. The operands are evaluated once, and the
    /// memory order/weak flag are checked source constants, not runtime
    /// arguments. Native TypeId lowering must be explicit; Cranelift rejects
    /// this until correct machine atomics exist.
    BuiltinAtomic(Box<CheckedAtomicBuiltin>),
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
pub struct CheckedAtomicBuiltin {
    pub builtin: builtins::BuiltinId,
    /// Only runtime value operands in original evaluation order. The order
    /// and weak arguments are constants and are not evaluated at runtime.
    pub operands: Vec<CheckedExpr>,
    pub bytes: u8,
    pub alignment: u8,
    /// GNU C11 memory-order constants 0..=5.
    pub success_order: u8,
    pub failure_order: Option<u8>,
    pub weak: bool,
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
    /// Carry the value-width property through C expressions which still
    /// denote the bit-field's value (including assignment and ++/--), but
    /// not through explicit casts which create an ordinary integer value.
    fn bit_field_width(&self) -> Option<u8> {
        match &self.kind {
            CheckedExprKind::BitFieldMember(_, _, _, width) => Some(*width),
            CheckedExprKind::Load(inner)
            | CheckedExprKind::Assign(inner, _)
            | CheckedExprKind::Comma(_, inner)
            | CheckedExprKind::Update { target: inner, .. }
            | CheckedExprKind::CompoundAssign(_, inner, _, _) =>
                inner.bit_field_width(),
            _ => None,
        }
    }

    /// C99 6.3.1.1: promotions of bit-fields depend on their *value width*,
    /// not just on the declared integer base type. Narrow unsigned int/long
    /// fields may promote to signed int. Keep the original member type/layout.
    fn integer_promotion_type(&self, target: Target) -> CType {
        let bit_width = self.bit_field_width();
        if let (Some(width), CType::Integer { unsigned, .. }) =
            (bit_width, self.ty.unqualified())
        {
            let width = u16::from(width);
            if width <= target.int_bits {
                // Signed width <= int_bits, or unsigned width < int_bits,
                // fits in signed int. A full-width unsigned field instead
                // promotes to unsigned int, even if declared unsigned long.
                return CType::Integer {
                    rank: IntRank::Int,
                    unsigned: *unsigned && width == target.int_bits,
                };
            }
        }
        target.promote(&self.ty)
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
    pub is_inline: bool,
    pub name: String,
    pub ty: CType,
    pub linkage: Linkage,
    /// External ELF linkage symbol, distinct from C source identifier.
    pub asm_label: Option<String>,
    pub initializer: Option<CheckedExpr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckedAsmOperand {
    pub constraint: String,
    pub expr: CheckedExpr,
}
#[derive(Debug, Clone, PartialEq)]
pub enum CheckedStmt {
    Block(Vec<CheckedStmt>),
    Decl(CheckedDecl),
    Typedef { name: String, ty: CType },
    Tag,
    Expr(Option<CheckedExpr>),
    /// Executable opaque basic asm: never treat it as a pure expression or
    /// discard its side effects. Native effectful lowering is tracked by ASM2.
    BasicAsm { template: String, span: Span },
    /// Checked, zero-operand GCC extended asm. Memory is a compiler
    /// read/write barrier; cc is allowed only because the admitted native
    /// instructions do not modify architectural arithmetic flags.
    ExtendedAsm {
        template: String,
        outputs: Vec<CheckedAsmOperand>,
        inputs: Vec<CheckedAsmOperand>,
        clobbers: Vec<String>,
        span: Span,
    },
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
    /// A plain inline definition with external linkage supplies an external
    /// symbol only if another file-scope declaration requires it.
    pub inline_only_candidate: bool,
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


// This walker is intentionally structural: checked lvalue-to-rvalue conversion
// strips top-level qualifiers from its result. Use one traversal for volatile
// and atomic memory so nested pointees, operands and declarations cannot
// silently escape the native backend's explicit capability check.
impl CheckedExpr {
    fn contains_memory_qualifier(&self, has:fn(&CType)->bool)->bool {
        if has(&self.ty) {return true;}
        use CheckedExprKind as K;
        match &self.kind {
            K::Initializer(values) => values.iter().any(|e|e.contains_memory_qualifier(has)),
            K::SparseInitializer(values) => values.iter()
                .any(|(_,e)|e.contains_memory_qualifier(has)),
            K::DeepInitializer(values) => values.iter()
                .any(|(_,e)|e.contains_memory_qualifier(has)),
            K::CompoundLiteral(inner) | K::StaticCompoundLiteral {initializer:inner,..} |
            K::Unary(_,inner) | K::BuiltinCtz(inner) | K::BuiltinByteSwap(_,inner) | K::Convert(inner) |
            K::Load(inner) | K::ArrayDecay(inner) | K::Member(inner,_) |
            K::BitFieldMember(inner,..) => inner.contains_memory_qualifier(has),
            K::BuiltinAtomic(checked) => checked.operands.iter()
                .any(|e|e.contains_memory_qualifier(has)),
            K::Binary(_,a,b) | K::Comma(a,b) | K::Assign(a,b) =>
                a.contains_memory_qualifier(has) || b.contains_memory_qualifier(has),
            K::Conditional(a,b,c) => a.contains_memory_qualifier(has)
                || b.contains_memory_qualifier(has) || c.contains_memory_qualifier(has),
            K::CompoundAssign(_,a,b,computation_ty) => has(computation_ty)
                || a.contains_memory_qualifier(has) || b.contains_memory_qualifier(has),
            K::Update {target,computation_ty,..} =>
                has(computation_ty) || target.contains_memory_qualifier(has),
            K::Call(callee,args) => callee.contains_memory_qualifier(has)
                || args.iter().any(|e|e.contains_memory_qualifier(has)),
            K::Literal(_) | K::FloatLiteral(_) | K::EnumConstant(_) |
            K::StringLiteral(_) | K::Name(_) => false,
        }
    }
}
impl CheckedDecl {
    fn contains_memory_qualifier(&self,has:fn(&CType)->bool)->bool {
        has(&self.ty) || self.initializer.as_ref()
            .is_some_and(|e|e.contains_memory_qualifier(has))
    }
}
impl CheckedStmt {
    fn contains_memory_qualifier(&self,has:fn(&CType)->bool)->bool {
        match self {
            Self::Block(stmts) => stmts.iter().any(|s|s.contains_memory_qualifier(has)),
            Self::Decl(decl) => decl.contains_memory_qualifier(has),
            Self::Typedef {ty,..} => has(ty),
            Self::Expr(expr) | Self::Return(expr) => expr.as_ref()
                .is_some_and(|e|e.contains_memory_qualifier(has)),
            Self::ExtendedAsm {outputs,inputs,..} => outputs.iter().chain(inputs)
                .any(|op|op.expr.contains_memory_qualifier(has)),
            Self::If {cond,then,otherwise} => cond.contains_memory_qualifier(has)
                || then.contains_memory_qualifier(has)
                || otherwise.as_ref().is_some_and(|s|s.contains_memory_qualifier(has)),
            Self::While {cond,body} | Self::Switch {control:cond,body} |
            Self::DoWhile {cond,body} => cond.contains_memory_qualifier(has)
                || body.contains_memory_qualifier(has),
            Self::Case {body,..} | Self::Default(body) | Self::Label {body,..} =>
                body.contains_memory_qualifier(has),
            Self::For(f) => f.init.as_ref().is_some_and(|s|s.contains_memory_qualifier(has))
                || f.cond.as_ref().is_some_and(|e|e.contains_memory_qualifier(has))
                || f.step.as_ref().is_some_and(|e|e.contains_memory_qualifier(has))
                || f.body.contains_memory_qualifier(has),
            Self::Tag | Self::BasicAsm {..} | Self::Break | Self::Continue |
            Self::Goto {..} => false,
        }
    }
}
impl CheckedItem {
    fn contains_memory_qualifier(&self,has:fn(&CType)->bool)->bool {
        match self {
            Self::Declaration(decl) => decl.contains_memory_qualifier(has),
            Self::Typedef {ty,..} => has(ty),
            Self::Function(f) => has(&f.signature)
                || f.params.iter().any(|p|p.contains_memory_qualifier(has))
                || f.body.contains_memory_qualifier(has),
            Self::Tag => false,
        }
    }
    /// Check all volatile types, even behind pointer expressions or in nested
    /// declarations; existing reference lowering is not volatile-safe.
    pub fn contains_volatile_type(&self)->bool {
        self.contains_memory_qualifier(CType::contains_volatile)
    }
    /// Check all C11 atomic qualified storage before code generation. The
    /// current Cranelift backend must refuse it rather than emit ordinary
    /// loads, stores or read-modify-writes.
    pub fn contains_atomic_type(&self)->bool {
        self.contains_memory_qualifier(CType::contains_atomic)
    }
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
    // For old-style definitions, retain the promoted ABI separately from the visible C type.
    old_style_signature: Option<CType>,
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
    saw_atomic_aggregate: Cell<bool>,
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
    warning_policy: WarningPolicy,
    emitted_warnings: Vec<ReportedDiagnostic>,
    warning_scopes: RefCell<Vec<HashMap<String, UsageBinding>>>,
}

impl Checker {
    pub fn new(target: Target) -> Result<Self, Diagnostic> {
        Ok(Self { target: target.validate()?, next_tag_identity: Cell::new(1), saw_atomic_aggregate: Cell::new(false), next_static_compound_identity: Cell::new(1), tags: RefCell::new(vec![HashMap::new()]), tag_definitions: RefCell::new(HashMap::new()), globals: HashMap::new(), locals: Vec::new(), result_ty: None, loop_depth: 0, switches: Vec::new(), function_labels: HashMap::new(), rollback_log: None, statement_undo: Vec::new(),
            recovery_errors: None, recovery_limit: 0,
            warning_policy: WarningPolicy::default(), emitted_warnings: Vec::new(),
            warning_scopes: RefCell::new(Vec::new()) })
    }

    /// Set policy before checking items. Warnings never change semantic acceptance.
    pub fn set_warning_policy(&mut self, policy: WarningPolicy) {
        self.warning_policy = policy;
    }

    /// Retrieve diagnostics emitted by successfully checked items.
    pub fn take_warnings(&mut self) -> Vec<ReportedDiagnostic> {
        std::mem::take(&mut self.emitted_warnings)
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
            CType::Volatile(inner) => Ok(self.complete_tag_type(*inner, span)?.with_volatile()),
            CType::Atomic(inner) => Ok(self.complete_tag_type(*inner, span)?.with_atomic()),
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
            CType::Volatile(inner) => Ok(self.try_complete_tag_type(*inner)?.with_volatile()),
            CType::Atomic(inner) => Ok(self.try_complete_tag_type(*inner)?.with_atomic()),
            CType::TagRef { kind, name, identity } => Ok(self.tag_definitions.borrow()
                .get(&identity).cloned()
                .unwrap_or(CType::TagRef { kind, name, identity })),
            CType::Array(element, count) => Ok(CType::Array(
                Box::new(self.try_complete_tag_type(*element)?), count)),
            CType::UnprototypedFunction { result } => Ok(CType::UnprototypedFunction {
                result: Box::new(self.try_complete_tag_type(*result)?),
            }),
            CType::Function { result, params } => Ok(CType::Function {
                result: Box::new(self.try_complete_tag_type(*result)?),
                params: params.into_iter()
                    .map(|ty| self.try_complete_tag_type(ty))
                    .collect::<Result<Vec<_>, _>>()?,
            }),
            CType::VariadicFunction { result, params } => Ok(CType::VariadicFunction {
                result: Box::new(self.try_complete_tag_type(*result)?),
                params: params.into_iter()
                    .map(|ty| self.try_complete_tag_type(ty))
                    .collect::<Result<Vec<_>, _>>()?,
            }),
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

    /// Preserve atomic qualification on integer storage and layout-checked
    /// one-byte aggregate declarations. Executable aggregate uses remain gated.
    /// Runtime access lowering is a separate capability gate.
    fn atomic_object_type(&self, base:CType, span:Span)->Result<CType,Diagnostic>{
        if base.is_atomic() {
            return Err(type_error(span,"duplicate C11 _Atomic qualification"));
        }
        if !matches!(base.unqualified(), CType::Integer { .. }) {
            // A complete one-byte aggregate has no extra alignment or
            // representation rounding in the current target data models.
            // Larger atomics need a target-specific atomic layout contract.
            if matches!(base.unqualified(), CType::Struct { .. } | CType::Union { .. })
                && base.size_bytes(self.target) == Some(1)
                && base.align_bytes(self.target) == Some(1)
                && !base.contains_atomic()
            {
                self.saw_atomic_aggregate.set(true);
                return Ok(base.with_atomic());
            }
            return Err(type_error(span,
                "C11 _Atomic requires supported integer or one-byte aggregate layout"));
        }
        let bytes=base.size_bytes(self.target)
            .ok_or_else(||type_error(span,"C11 _Atomic requires complete integer storage"))?;
        let align=base.align_bytes(self.target)
            .ok_or_else(||type_error(span,"C11 _Atomic requires naturally aligned storage"))?;
        if !matches!(bytes,1|2|4|8) || align<bytes || !align.is_power_of_two() {
            return Err(type_error(span,"unsupported C11 _Atomic width or alignment"));
        }
        Ok(base.with_atomic())
    }

    fn resolve(&mut self, ty: &Type, span: Span) -> Result<CType, Diagnostic> {
        Ok(match ty {
            Type::Atomic(inner) => {
                let base=self.resolve(inner,span)?;
                self.atomic_object_type(base,span)?
            }
            Type::Qualified(inner, qualifiers) => {
                let base = self.resolve(inner, span)?;
                if qualifiers.is_restrict && !matches!(base.unqualified(), CType::Pointer(pointee)
                    if !matches!(pointee.unqualified(), CType::Function { .. } | CType::VariadicFunction { .. } | CType::UnprototypedFunction { .. })) {
                    return Err(type_error(span, "restrict requires a pointer object type"));
                }
                if (qualifiers.is_const || qualifiers.is_volatile)
                    && matches!(base.unqualified(),
                        CType::Function { .. } | CType::VariadicFunction { .. } | CType::UnprototypedFunction { .. })
                {
                    return Err(type_error(span,
                        "const/volatile cannot qualify a function type"));
                }
                let base = if qualifiers.is_atomic {
                    self.atomic_object_type(base,span)?
                } else {base};
                let base = if qualifiers.is_const { base.with_const() } else { base };
                if qualifiers.is_volatile { base.with_volatile() } else { base }
            }
            Type::BuiltinVaList => {
                // x86-64 SysV GCC: typedef struct __va_list_tag {
                //   unsigned int gp_offset, fp_offset;
                //   void *overflow_arg_area, *reg_save_area;
                // } __builtin_va_list[1];
                // This must remain an array and decay to a tag pointer in
                // function parameters, not be modeled as an ordinary void*.
                if self.target.int_bits != 32 || self.target.long_bits != 64
                    || self.target.pointer_bits != 64 {
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
                    let (written, alignment) = match &member.ty {
                        Type::MemberAligned(base, alignment) => (base.as_ref(), Some(alignment)),
                        ty => (ty, None),
                    };
                    let (declared, bit_width) = match written {
                        Type::BitField(base, width) => {
                            (base.as_ref(), Some(width.as_ref()))
                        }
                        _ => (written, None),
                    };
                    let resolved = self.resolve(declared, member.span)?;
                    let resolved = self.try_complete_tag_type(resolved)?;
                    let width = bit_width.map(|width| {
                        self.check_bit_width(width, &resolved, member.name.is_empty(), member.span)
                    }).transpose()?;
                    self.check_member_alignment(alignment, &resolved, width.is_some(), member.span)?;
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
            Type::MemberAligned(..) => return Err(type_error(span,
                "member alignment is only valid inside a struct or union")),
            Type::BitField(..) => return Err(type_error(span,
                "bit-field declaration is only valid inside a struct or union")),
            Type::Pointer(inner) => CType::Pointer(Box::new(self.resolve(inner, span)?)),
            Type::UnprototypedFunction { result } => {
                let result = self.resolve(result, span)?;
                if matches!(result.unqualified(), CType::Array(..) | CType::Function { .. }
                    | CType::VariadicFunction { .. } | CType::UnprototypedFunction { .. }) {
                    return Err(type_error(span, "invalid function result type"));
                }
                CType::UnprototypedFunction { result: Box::new(result) }
            }
            Type::Function { result, params } | Type::VariadicFunction { result, params } => {
                let result = self.resolve(result, span)?;
                if matches!(result.unqualified(), CType::Function { .. } | CType::VariadicFunction { .. } | CType::UnprototypedFunction { .. }) {
                    return Err(type_error(span, "function cannot return a function"));
                }
                if matches!(result.unqualified(), CType::Array(..)) {
                    return Err(type_error(span, "function cannot return an array"));
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
                if self.try_complete_tag_type(element.clone())?.has_flexible_array_member() {
                    return Err(type_error(span, "array element cannot contain a flexible array member"));
                }
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
            CType::Function { .. } | CType::VariadicFunction { .. } | CType::UnprototypedFunction { .. } => CType::Pointer(Box::new(ty)),
            other => other,
        }
    }

    fn define_local(&mut self, name: &str, ty: CType, kind: SymbolKind, linkage: Linkage, is_register: bool, span: Span) -> Result<(), Diagnostic> {
        let scope = self.locals.last().expect("local declaration outside a function");
        if scope.contains_key(name) {
            return Err(Diagnostic::coded(span, DiagnosticCode::SemanticDuplicateDeclaration, format!("duplicate declaration of '{name}'")));
        }
        // Only a genuinely outer binding counts: the current scope is
        // checked above, and sibling scopes are not visible.
        let shadows = self.locals.iter().rev().skip(1).any(|s| s.contains_key(name))
            || self.globals.contains_key(name);
        if shadows && self.warning_policy.enabled("shadow") {
            self.emitted_warnings.push(ReportedDiagnostic::warning(
                Diagnostic::semantic(span, format!("declaration of '{name}' shadows a previous declaration")),
                "shadow",
            ));
        }
        self.locals.last_mut().expect("local scope").insert(name.to_owned(), Symbol {
            ty, kind, linkage, defined: true, asm_label: None, enum_value: None, is_register, old_style_signature: None,
        });
        if kind == SymbolKind::Object {
            // Extern aliases hide an outer automatic binding but are not
            // themselves candidates for unused-local diagnostics.
            self.register_warning_binding(name, span, false, linkage != Linkage::None);
        }
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
            if let Some(abi) = &prior.old_style_signature {
                if !matches!(&ty, CType::UnprototypedFunction { .. }) && abi.composite(&ty).is_none() {
                    return Err(Diagnostic::coded(span, DiagnosticCode::SemanticConflictingDeclaration,
                        format!("conflicting declaration of '{name}'")));
                }
            }
            let Some(composite) = prior.ty.composite(&ty).filter(|_| prior.kind == kind) else {
                return Err(Diagnostic::coded(span, DiagnosticCode::SemanticConflictingDeclaration, format!("conflicting declaration of '{name}'")));
            };
            // An extern redeclaration inherits a previous internal linkage.
            // Explicit static cannot follow a prior external declaration.
            if prior.linkage != requested && storage != StorageClass::Extern
                && !(kind == SymbolKind::Function && storage == StorageClass::Default) {
                return Err(type_error(span, format!("conflicting linkage for '{name}'")));
            }
            if defining && prior.defined {
                return Err(Diagnostic::coded(span, DiagnosticCode::SemanticMultipleDefinitions, format!("multiple definitions of '{name}'")));
            }
            prior.ty = composite;
            prior.defined |= defining;
        } else {
            self.globals.insert(name.to_owned(), Symbol {
                ty, kind, linkage: requested, defined: defining, asm_label: None, enum_value: None, is_register: false, old_style_signature: None,
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

    /// Check one top-level item and discard warnings emitted by a failed item.
    /// A rejected declaration must not leave misleading diagnostics in the
    /// stream consumed by subsequent successful items.
    pub fn check_item(&mut self, item: ExternalItem) -> Result<CheckedItem, Diagnostic> {
        let previous = self.emitted_warnings.len();
        let result = self.check_item_impl(item).and_then(|checked| {
            // No additional tree traversal on units without atomic aggregates.
            // Type-only declarations and unused prototypes may introduce the
            // checked type. Do not send unsupported storage/effects to either backend.
            if self.saw_atomic_aggregate.get() {
                let declaration_only = match &checked {
                    CheckedItem::Tag | CheckedItem::Typedef { .. } => true,
                    CheckedItem::Declaration(d) => matches!(d.ty,
                        CType::Function { .. } | CType::VariadicFunction { .. } |
                        CType::UnprototypedFunction { .. }),
                    _ => false,
                };
                if !declaration_only &&
                    checked.contains_memory_qualifier(CType::contains_atomic_aggregate)
                {
                    return Err(type_error(Span::default(),
                        "atomic aggregate storage or executable use requires backend support"));
                }
            }
            Ok(checked)
        });
        if result.is_err() {
            self.emitted_warnings.truncate(previous);
        }
        result
    }

    fn check_item_impl(&mut self, item: ExternalItem) -> Result<CheckedItem, Diagnostic> {
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

    /// Two paths overlap if they designate the same object, one contains
    /// the other, or they select different arms of the same union. Distinct
    /// struct/array members are independent, even if their bit-fields occupy
    /// the same underlying storage unit.
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

    /// An exact duplicate or selection of another union arm replaces the
    /// entire earlier initializer. Other parent/child overlaps are handled
    /// by descending into explicitly initialized leaves, so unrelated
    /// subobjects survive later designated writes.
    fn initializer_override_is_whole(ty: &CType, a: &[usize], b: &[usize]) -> bool {
        if a == b { return true; }
        let mut current = ty;
        for (left, right) in a.iter().zip(b) {
            if left != right {
                return matches!(current.unqualified(), CType::Union { .. });
            }
            current = match current.unqualified() {
                CType::Struct { members, .. } | CType::Union { members, .. } =>
                    match members.get(*left) {
                        Some(member) => &member.ty,
                        None => return false,
                    },
                CType::Array(element, _) => element.as_ref(),
                _ => return false,
            };
        }
        false
    }

    /// Expand a *braced* checked aggregate initializer into explicit
    /// leaf writes, with source-order subobject paths. Omitted members are
    /// intentionally absent: the enclosing object is zero-filled once by
    /// either static ELF or stack lowering, and a later parent replacement
    /// must discard all earlier children, including omitted ones.
    ///
    /// Non-braced aggregate values (such as a struct copied from a variable)
    /// remain whole-subobject writes; a later child designator can update
    /// part of that stored value without losing its other members.
    fn initializer_explicit_leaves(
        prefix: Vec<usize>, checked: CheckedExpr,
        out: &mut Vec<(Vec<usize>, CheckedExpr)>,
    ) {
        let CheckedExpr { ty, category, kind } = checked;
        match kind {
            CheckedExprKind::Initializer(values) => {
                for (index, value) in values.into_iter().enumerate() {
                    let mut path = prefix.clone();
                    path.push(index);
                    Self::initializer_explicit_leaves(path, value, out);
                }
            }
            CheckedExprKind::SparseInitializer(entries) => {
                for (index, value) in entries {
                    let mut path = prefix.clone();
                    path.push(index);
                    Self::initializer_explicit_leaves(path, value, out);
                }
            }
            CheckedExprKind::DeepInitializer(entries) => {
                for (suffix, value) in entries {
                    let mut path = prefix.clone();
                    path.extend(suffix);
                    Self::initializer_explicit_leaves(path, value, out);
                }
            }
            other => out.push((prefix, CheckedExpr { ty, category, kind: other })),
        }
    }

    /// C99 6.7.8: after a designated (or positional) initializer, the
    /// next undesignated expression initializes the following subobject.
    /// Walk outwards from the current path to find its next struct member
    /// or array element. In a union, an initializer cannot move to a
    /// different member positionally; only an explicit designator may
    /// select another union arm.
    fn initializer_next_subobject(ty: &CType, path: &[usize]) -> Option<Vec<usize>> {
        let mut parents = Vec::with_capacity(path.len());
        let mut current = ty;
        for &index in path {
            parents.push(current);
            current = match current.unqualified() {
                CType::Struct { members, .. } | CType::Union { members, .. } =>
                    &members.get(index)?.ty,
                CType::Array(element, Some(bound))
                    if u64::try_from(index).ok()? < *bound => element.as_ref(),
                _ => return None,
            };
        }
        for depth in (0..path.len()).rev() {
            let next = path[depth].checked_add(1)?;
            let valid = match parents[depth].unqualified() {
                CType::Struct { members, .. } => next < members.len(),
                CType::Array(_, Some(bound)) =>
                    u64::try_from(next).is_ok_and(|index| index < *bound),
                // C has one active union member, not a positional list.
                CType::Union { .. } => false,
                _ => false,
            };
            if valid {
                let mut successor = path[..depth].to_vec();
                successor.push(next);
                return Some(successor);
            }
        }
        None
    }

    /// Use flat source-order paths for nested designations so two writes like
    /// `[1].a=3,[1].b=4` do not each reset the other's struct storage.
    /// The current-object cursor tracks the next nested subobject across
    /// aggregate boundaries, not merely the next top-level member.
    fn check_deep_initializer(
        &self, values: &[Expr], ty: &CType, span: Span, is_static: bool,
    ) -> Result<CheckedExpr, Diagnostic> {
        let mut checked_paths: Vec<(Vec<usize>, CheckedExpr)> = Vec::new();
        // None means the initializer exhausted the enclosing object.
        // A later explicit designator may still select any valid path.
        let mut next_path: Option<Vec<usize>> = Some(vec![0]);
        for value in values {
            let located = value.location().unwrap_or(span);
            let (mut path, initializer) = match value.without_location() {
                Expr::Designated { path, value } => {
                    let (slots, _) = self.initializer_path(ty, path, located)?;
                    (slots, value.as_ref())
                }
                _ => {
                    let path = next_path.clone().ok_or_else(|| type_error(
                        located, "too many positional initializers after nested designator"))?;
                    (path, value)
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
            // C99 6.7.8 brace elision: a non-braced scalar initializer for
            // a selected aggregate initializes its first nested subobject.
            // Try normal aggregate assignment first: compatible struct
            // copies, string-initialized character arrays and braced
            // initializers must keep the selected parent as their target.
            // Both positional and one-component designated aggregate
            // initializers may elide braces (e.g. {.a=3,4} for int a[2]).
            let mut checked = self.check_object_initializer(
                initializer, current, span, is_static);
            while checked.is_err()
                && !matches!(initializer.without_location(), Expr::Initializer(_))
            {
                let first = match current.unqualified() {
                    CType::Struct { members, .. } | CType::Union { members, .. } =>
                        members.first().map(|member| &member.ty),
                    CType::Array(element, Some(bound)) if *bound > 0 =>
                        Some(element.as_ref()),
                    _ => None,
                };
                let Some(first) = first else { break };
                path.push(0);
                current = first;
                checked = self.check_object_initializer(
                    initializer, current, span, is_static);
            }
            let checked = checked?;
            // GCC and Clang differ if a non-braced *struct copy* is
            // followed by a nested child initializer: GCC can discard
            // that whole earlier copy, while Clang retains its siblings.
            // Braced parents are normalized to explicit leaves below;
            // non-braced aggregate copies remain fail-closed in this case.
            if checked_paths.iter().any(|(prior, previous)|
                path.starts_with(prior) && path.len() > prior.len()
                && matches!(previous.ty.unqualified(),
                    CType::Struct { .. } | CType::Union { .. } | CType::Array(..))
                && !matches!(previous.kind, CheckedExprKind::StringLiteral(_)))
            {
                return Err(type_error(located,
                    "partial override of non-braced aggregate initializer is unsupported"));
            }
            // A later initializer of an entire parent replaces all earlier
            // children, including those omitted by its braces. Selecting a
            // different union arm also replaces all prior writes to that
            // union. Earlier whole-object values are kept when a later
            // designator modifies only one child of the same object.
            checked_paths.retain(|(prior, _)|
                !Self::initializer_paths_overlap(ty, prior, &path)
                || !(prior.starts_with(&path)
                    || Self::initializer_override_is_whole(ty, prior, &path)));
            let mut leaves = Vec::new();
            Self::initializer_explicit_leaves(path.clone(), checked, &mut leaves);
            for (leaf_path, leaf) in leaves {
                // A later leaf overrides exactly that earlier leaf (or a
                // different active union member), never its siblings. The
                // displaced expression and its side effects leave checked IR.
                checked_paths.retain(|(prior, _)|
                    !Self::initializer_override_is_whole(ty, prior, &leaf_path));
                checked_paths.push((leaf_path, leaf));
            }
            next_path = Self::initializer_next_subobject(ty, &path);
        }
        Ok(CheckedExpr::value(ty.clone(),
            CheckedExprKind::DeepInitializer(checked_paths)))
    }

    fn check_object_initializer(
        &self, original: &Expr, ty: &CType, span: Span, is_static: bool
    ) -> Result<CheckedExpr, Diagnostic> {
        let span = original.location().unwrap_or(span);
        let original = original.without_location();
        // Top-level incomplete arrays are completed before reaching this
        // checker; a remaining incomplete array is not an initializer target.
        if matches!(ty.unqualified(), CType::Array(_, None)) {
            return Err(type_error(span, "incomplete array initializer"));
        }
        // Bit-fields use the ordinary typed integer initializer checker.
        // The checked aggregate retains member indices; native automatic and
        // static object emitters pack only each selected bit range and leave
        // neighboring fields intact.
        if let Expr::Initializer(values) = original {
            if values.iter().any(|value| matches!(
                value.without_location(), Expr::Designated { path, .. } if path.len() > 1
            )) {
                return self.check_deep_initializer(values, ty, span, is_static);
            }
            // Every entry has a source-order position. Designators can change
            // the current object; the next undesignated entry follows that
            // element, not the previous positional index.
            let mut entries: Vec<(usize, CheckedExpr)> =
                Vec::with_capacity(values.len());
            let mut cursor = 0usize;
            let mut has_designators = false;
            let array = matches!(ty.unqualified(), CType::Array(..));
            for value in values {
                let designated = matches!(value.without_location(), Expr::Designated { .. });
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
                let is_union = matches!(ty.unqualified(), CType::Union { .. });
                if !array && is_union && !entries.is_empty() && !designated {
                    // Undesignated union initialization still selects only
                    // its first member. A later explicit .member, however,
                    // can replace the earlier selected arm as a whole.
                    return Err(type_error(span, if has_designators {
                        "union positional continuation requires a designator"
                    } else { "union initializer selects only its first member" }));
                }
                let checked = match self.check_object_initializer(
                    input, element, span, is_static
                ) {
                    Ok(checked) => checked,
                    // The flat representation cannot carry positional
                    // subobject paths. On the first non-braced aggregate
                    // initializer that fails to initialize the aggregate
                    // as a whole, restart the entire brace group through
                    // the same path-aware checker used for nested
                    // designators. This preserves the source-order cursor,
                    // overrides and zero-fill behavior in one place.
                    Err(_) if matches!(element.unqualified(),
                        CType::Struct { .. } | CType::Union { .. } | CType::Array(..))
                        && !matches!(input.without_location(), Expr::Initializer(_)) =>
                    {
                        return self.check_deep_initializer(
                            values, ty, span, is_static);
                    }
                    Err(error) => return Err(error),
                };
                if is_union {
                    // Every new explicit union selection completely replaces
                    // the previously active member, including any side effects.
                    entries.clear();
                } else {
                    // Duplicate struct/array/bit-field leaf: retain only the
                    // last initializer, preserving final source order.
                    entries.retain(|(old, _)| *old != slot);
                }
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
        let kind = if matches!(ty, CType::Function { .. } | CType::VariadicFunction { .. } | CType::UnprototypedFunction { .. }) { SymbolKind::Function } else { SymbolKind::Object };
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
            storage: decl.storage, is_inline: decl.is_inline, name: decl.name, ty, linkage, asm_label,
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
        let result = self.complete_tag_type(result, func.span)?;
        if matches!(result.unqualified(), CType::Function { .. } | CType::VariadicFunction { .. } | CType::UnprototypedFunction { .. }) {
            return Err(type_error(func.span, "function cannot return a function"));
        }
        if matches!(result.unqualified(), CType::Array(..)) {
            return Err(type_error(func.span, "function cannot return an array"));
        }
        let mut params = Vec::new();
        for param in &func.params {
            if !matches!(param.storage, StorageClass::Default | StorageClass::Register) {
                return Err(type_error(param.span, "function parameter requires automatic or register storage"));
            }
            let resolved = self.resolve(&param.ty, param.span)?;
            let ty = self.adjust_parameter(resolved);
            let ty = self.complete_tag_type(ty, param.span)?;
            if !ty.is_object() {
                return Err(type_error(param.span, "function parameter cannot have void or function type"));
            }
            if param.storage == StorageClass::Register && !ty.is_scalar() {
                return Err(type_error(param.span, "register parameter requires a scalar type"));
            }
            params.push(CheckedDecl {
                storage: param.storage, is_inline: false, name: param.name.clone(), ty,
                linkage: Linkage::None, asm_label: None, initializer: None, span: param.span,
            });
        }
        let signature = CType::Function {
            result: Box::new(result.clone()),
            params: params.iter().map(|p| {
                let ty = p.ty.unqualified();
                if !func.old_style { return ty.clone(); }
                if matches!(ty, CType::Floating(mini_c_syntax::ast::FloatRank::Float)) {
                    CType::Floating(mini_c_syntax::ast::FloatRank::Double)
                } else { self.target.promote(ty) }
            }).collect(),
        };
        // Resolve labels independently of ordinary lexical identifier scopes.
        // A jump may target a label declared later in the same function.
        let mut function_labels = HashMap::new();
        Self::collect_function_labels(&func.body, &mut function_labels)?;
        // Register before checking the body: recursive calls are legal.
        let declaration_type = if func.old_style {
            if let Some(prior) = self.global(&func.name) {
                if !matches!(&prior.ty, CType::UnprototypedFunction { .. })
                    && signature.composite(&prior.ty).is_none()
                {
                    return Err(Diagnostic::coded(func.span, DiagnosticCode::SemanticConflictingDeclaration,
                        format!("conflicting declaration of '{}'", func.name)));
                }
            }
            CType::UnprototypedFunction { result: Box::new(result.clone()) }
        } else { signature.clone() };
        self.declare_global(&func.name, declaration_type, SymbolKind::Function, true, func.storage,
            func.name_span.unwrap_or(func.span))?;
        if func.old_style {
            self.globals.get_mut(&func.name).expect("registered function")
                .old_style_signature = Some(signature.clone());
        }
        self.locals.push(HashMap::new());
        self.push_warning_scope();
        self.tags.borrow_mut().push(HashMap::new());
        self.function_labels = function_labels;
        self.result_ty = Some(result);
        // C99 6.4.2.2: each function implicitly declares a static const
        // char array `__func__` immediately inside its outermost block.
        // Install it in the same scope as parameters, so nested blocks see
        // the array and sizeof does not decay it. A private name identifies
        // the separately stored object in native backends.
        let name_bytes = func.name.as_bytes();
        let name_size = u64::try_from(name_bytes.len())
            .ok().and_then(|n| n.checked_add(1))
            .ok_or_else(|| type_error(func.span, "__func__ name exceeds supported size"))?;
        let name_type = CType::Array(Box::new(
            CType::Integer {rank: IntRank::Char, unsigned: !self.target.char_is_signed }
                .with_const()), Some(name_size));
        self.locals.last_mut().expect("function scope").insert("__func__".into(), Symbol {
            ty: name_type, kind: SymbolKind::Object, linkage: Linkage::None,
            defined: true, asm_label: Some(implicit_function_name_symbol(&func.name)),
            enum_value: None, is_register: false, old_style_signature: None,
        });
        for (parsed_param, param) in func.params.iter().zip(&params) {
            if let Err(err) = self.define_local(&param.name, param.ty.clone(),
                SymbolKind::Object, Linkage::None,
                parsed_param.storage == StorageClass::Register,
                parsed_param.name_span.unwrap_or(param.span)) {
                self.pop_tag_scope();
                self.pop_warning_scope();
                self.locals.pop();
                self.result_ty = None;
                return Err(err);
            }
        }
        // Mark parameters distinctly for -Wunused-parameter after successful registration.
        for param in &func.params {
            if !param.name.is_empty() {
                self.register_warning_binding(&param.name,
                    param.name_span.unwrap_or(param.span), true, false);
            }
        }
        // Function parameters and its outermost compound statement share one scope.
        let checked = match &func.body {
            Stmt::Block(stmts) => self.check_statements(stmts, func.span).map(CheckedStmt::Block),
            other => self.check_stmt(other, func.span),
        };
        self.pop_tag_scope();
        self.pop_warning_scope();
        self.locals.pop();
        self.result_ty = None;
        self.function_labels.clear();
        let body = checked?;
        let linkage = self.global(&func.name).expect("registered function").linkage;
        Ok(CheckedFunction { linkage,
            inline_only_candidate: func.is_inline && func.storage == StorageClass::Default && linkage == Linkage::External,
            name: func.name, signature, params, body, span: func.span })
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
                self.push_warning_scope();
                self.tags.borrow_mut().push(HashMap::new());
                let inner = self.check_statements(stmts, span);
                self.pop_tag_scope();
                self.pop_warning_scope();
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
                    storage: decl.storage, is_inline: false, name: decl.name.clone(), ty,
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
            Stmt::BasicAsm { template, span } => CheckedStmt::BasicAsm {
                template: template.clone(), span: *span,
            },
            Stmt::ExtendedAsm { template, outputs, inputs, clobbers, span } => {
                // Resolve names only; physical instruction meanings stay opaque
                // to the source checker, TypeId, and register allocator.
                let names=outputs.iter().chain(inputs.iter())
                    .map(|operand|operand.name.as_deref()).collect::<Vec<_>>();
                let template=asm_names::positional_template(template,&names)
                    .map_err(|error|type_error(*span,error))?;
                // P11: one GP output and one genuine memory output can
                // share the same opaque assembler block, in either order.
                // Its single r input belongs to the memory output width,
                // and neither output's C lvalue may be implicitly scalarized.
                // Additional independent register results require a multi-
                // result machine contract, not speculative source admission.
                let dual_gp_plan=asm_output_roles::plan_dual_gp(
                    &outputs.iter().map(|o|o.constraint.as_str()).collect::<Vec<_>>(),
                    &inputs.iter().map(|i|i.constraint.as_str()).collect::<Vec<_>>(),
                );
                let dual_gp_results=dual_gp_plan.is_some();
                let group_gp_plan=asm_output_roles::plan_gp_output_group(
                    &outputs.iter().map(|o|o.constraint.as_str()).collect::<Vec<_>>(),
                    &inputs.iter().map(|i|i.constraint.as_str()).collect::<Vec<_>>(),
                );
                let group_gp_results=group_gp_plan.is_some();
                let mixed_gp_memory=outputs.len()==2 && inputs.len()==1
                    && inputs[0].constraint=="r"
                    && outputs.iter().filter(|o|
                        matches!(o.constraint.as_str(),"=r"|"=&r")).count()==1
                    && outputs.iter().filter(|o|
                        matches!(o.constraint.as_str(),"=m"|"+m")).count()==1;
                if (!mixed_gp_memory && !dual_gp_results
                    && !group_gp_results && outputs.len()>1)
                    || inputs.len()>2 {
                    return Err(type_error(*span,
                        "extended asm supports one output or a reviewed two-output GP/memory pair"));
                }
                let mut checked_outputs=Vec::new();
                for (output_index,output) in outputs.iter().enumerate() {
                    // GNU alternatives are selected once for the statement.
                    // Checked IR sees only that plan's precise GP role; all
                    // operand labels retain their original positional index.
                    let effective_constraint=dual_gp_plan
                        .map(|plan|plan.output_constraint(output_index))
                        .or_else(||group_gp_plan.as_ref().map(|plan|
                            plan.output_constraint(output_index)))
                        .unwrap_or(output.constraint.as_str());
                    if !matches!(effective_constraint,"=r"|"+r"|"=&r"|"=m"|"+m") {
                        return Err(type_error(output.span,"extended asm output requires reviewed GP, =m or +m memory constraint"));
                    }
                    // Register results still require a checked local name;
                    // "=m"/"+m" instead bind a genuine checked addressable C
                    // lvalue, such as *p, p[i], or a writable local scalar.
                    if !matches!(effective_constraint,"=m"|"+m")
                        && !matches!(output.expr.without_location(),Expr::Name(_)) {
                        return Err(type_error(output.span,"extended asm output currently requires local variable"));
                    }
                    let expr=self.check_expr(&output.expr,output.span)?;
                    if expr.category!=Category::LValue
                        || matches!(expr.ty,CType::Qualified(_))
                        || !matches!(expr.ty.unqualified(),CType::Integer { .. })
                        || !matches!(expr.ty.size_bytes(self.target),Some(4|8))
                    {
                        return Err(type_error(output.span,"extended asm output must be writable i32/i64 lvalue"));
                    }
                    checked_outputs.push(CheckedAsmOperand {
                        constraint:effective_constraint.to_owned(),expr,
                    });
                }
                // GNU extended asm is opaque to the C checker: the template
                // is NOT an instruction list or a semantic description.
                // Validate only C-side operand constraints and lvalues here.
                // Target-specific encoding/physical RA remains the compiler
                // backend's responsibility, with fail-closed diagnostics there.
                // In particular, "cc" is a *declaration*, not a condition
                // requiring the checker to recognize an arithmetic mnemonic.
                let gp_memory_read=inputs.len()==1 && inputs[0].constraint=="m";
                if gp_memory_read &&
                    (checked_outputs.len()!=1 || checked_outputs[0].constraint!="=r")
                {
                    return Err(type_error(*span,
                        "ASM3 memory input requires one =r output"));
                }
                let gp_memory_write=checked_outputs.len()==1
                    && checked_outputs[0].constraint=="=m";
                if gp_memory_write &&
                    (inputs.len()!=1 || inputs[0].constraint!="r")
                {
                    return Err(type_error(*span,
                        "ASM3 =m memory output requires exactly one r input"));
                }
                let gp_memory_rmw=checked_outputs.len()==1
                    && checked_outputs[0].constraint=="+m";
                if gp_memory_rmw &&
                    (inputs.len()!=1 || inputs[0].constraint!="r")
                {
                    return Err(type_error(*span,
                        "ASM3 +m memory output requires exactly one r input"));
                }
                // +r reads and writes operand %0 with no separate input. Numeric
                // 0 ties an input to the sole =r output. An =&r early-
                // clobber output instead needs one independent r input:
                // the machine allocator must keep its result distinct from
                // all input registers. Other constraints still fail closed.
                if !checked_outputs.is_empty() && !mixed_gp_memory
                    && !dual_gp_results && !group_gp_results {
                    let out=&checked_outputs[0];
                    if out.constraint=="+r" && !inputs.is_empty() {
                        return Err(type_error(*span,"+r output cannot have a separate input in the bounded ASM2 form"));
                    }
                    if out.constraint=="=&r" &&
                        (inputs.is_empty() || inputs.len()>2
                            || inputs.iter().any(|i|i.constraint!="r")) {
                        return Err(type_error(*span,"=&r requires one or two independent r inputs"));
                    }
                    if inputs.len()==2 && out.constraint!="=&r" {
                        return Err(type_error(*span,"two GP inputs require one =&r early-clobber output"));
                    }
                }
                let mut checked_inputs=Vec::new();
                for input in inputs {
                    let group_input=group_gp_plan.as_ref()
                        .map(|plan|plan.input_constraint());
                    let effective_constraint=if let Some(plan)=dual_gp_plan {
                        plan.input_constraint()
                    } else {group_input.as_deref().unwrap_or(input.constraint.as_str())};
                    if !matches!(effective_constraint,"r"|"0"|"1"|"m")
                        && !(group_gp_results
                            && effective_constraint.parse::<usize>().is_ok())
                    {
                        return Err(type_error(input.span,"extended asm input requires r, matching 0 or reviewed m"));
                    }
                    if input.constraint=="m" && !gp_memory_read {
                        return Err(type_error(input.span,"ASM3 memory input requires one reviewed mov"));
                    }
                    if matches!(effective_constraint,"0"|"1")
                        && !dual_gp_results && !group_gp_results
                        && (checked_outputs.len()!=1 || effective_constraint!="0"
                            || checked_outputs[0].constraint!="=r") {
                        return Err(type_error(input.span,
                            "matching input requires a non-early GP output"));
                    }
                    let expr=self.check_expr(&input.expr,input.span)?;
                    if !matches!(expr.ty.unqualified(),CType::Integer { .. })
                        || !matches!(expr.ty.size_bytes(self.target),Some(4|8))
                    {
                        return Err(type_error(input.span,"extended asm input must be i32/i64 scalar"));
                    }
                    if gp_memory_read && (expr.category!=Category::LValue
                        || expr.ty.size_bytes(self.target)
                            !=checked_outputs[0].expr.ty.size_bytes(self.target))
                    {
                        return Err(type_error(input.span,
                            "ASM3 memory input must be an addressable same-width i32/i64 C lvalue"));
                    }
                    if mixed_gp_memory && expr.ty.size_bytes(self.target)
                        !=checked_outputs.iter()
                            .find(|o|matches!(o.constraint.as_str(),"=m"|"+m"))
                            .and_then(|o|o.expr.ty.size_bytes(self.target))
                    {
                        return Err(type_error(input.span,
                            "ASM3 mixed GP/memory input must match the memory object width"));
                    }
                    if (gp_memory_write || gp_memory_rmw) && expr.ty.size_bytes(self.target)
                        !=checked_outputs[0].expr.ty.size_bytes(self.target)
                    {
                        return Err(type_error(input.span,
                            "ASM3 memory output/RMW GP input must match its object width"));
                    }
                    if let Some(index)=group_gp_plan.as_ref().and_then(|p|p.input_tie) {
                        if expr.ty.size_bytes(self.target)
                            !=checked_outputs[index].expr.ty.size_bytes(self.target)
                        {
                            return Err(type_error(input.span,
                                "ASM3 group matching input/output widths disagree"));
                        }
                    } else if let Some(tied_index)=dual_gp_plan.and_then(|p|p.input_tie) {
                        if expr.ty.size_bytes(self.target)
                            !=checked_outputs[tied_index as usize].expr.ty.size_bytes(self.target)
                        {
                            return Err(type_error(input.span,
                                "ASM3 tied input and selected GP output widths must agree"));
                        }
                    } else if effective_constraint=="0" &&
                        expr.ty.size_bytes(self.target)!=checked_outputs[0].expr.ty.size_bytes(self.target)
                    {
                        return Err(type_error(input.span,"matching 0 input/output GP widths must agree"));
                    }
                    if inputs.len()==2 &&
                        expr.ty.size_bytes(self.target)!=checked_outputs[0].expr.ty.size_bytes(self.target)
                    {
                        return Err(type_error(input.span,
                            "two independent ASM2 GP input widths must match the output"));
                    }
                    checked_inputs.push(CheckedAsmOperand{
                        constraint:effective_constraint.to_owned(),
                        expr:if gp_memory_read {expr} else {expr.rvalue()},
                    });
                }
                // P14: GCC clobber *names* are not the x86 operand register
                // spelling set. GCC accepts eax/rax but does not recognize
                // r8d/r9d/r10d/r11d as clobber names. Preserve architectural
                // numbers for alias interference; reject invalid source names.
                // RSP, RBP and preserved GP registers remain unsupported.
                // This does not authorize arbitrary source templates.
                let gp_clobber=|name:&str|->Option<u8> {Some(match name {
                    "eax"|"rax"=>0, "ecx"|"rcx"=>1, "edx"|"rdx"=>2,
                    "esi"|"rsi"=>6, "edi"|"rdi"=>7,
                    "r8"=>8, "r9"=>9,
                    "r10"=>10, "r11"=>11,
                    _=>return None,
                })};
                let mut clobbered_gp=0u16;
                for (index,clobber) in clobbers.iter().enumerate() {
                    if let Some(reg)=gp_clobber(clobber) {
                        if outputs.len()!=1 && !mixed_gp_memory
                            && !dual_gp_results && !group_gp_results {
                            return Err(type_error(*span,
                                "explicit GP clobbers require a checked GP asm output contract"));
                        }
                        let bit=1u16 << reg;
                        if clobbered_gp & bit != 0 {
                            return Err(type_error(*span,
                                "duplicate aliased extended asm GP clobber"));
                        }
                        clobbered_gp |= bit;
                    } else if !matches!(clobber.as_str(),"memory"|"cc") {
                        return Err(type_error(*span,
                            format!("unsupported extended asm clobber #{index}: {clobber}")));
                    }
                    if clobbers[..index].contains(clobber) {
                        return Err(type_error(*span,"duplicate extended asm clobber"));
                    }
                }
                CheckedStmt::ExtendedAsm {
                    template:template.clone(),
                    outputs:checked_outputs,inputs:checked_inputs,
                    clobbers:clobbers.clone(),span:*span,
                }
            },
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
                self.push_warning_scope();
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
                self.pop_warning_scope();
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
                let promoted = control.integer_promotion_type(self.target);
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
                        if matches!(inner.as_ref(), CType::Function { .. } | CType::VariadicFunction { .. } | CType::UnprototypedFunction { .. })))),
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
                if qualifiers.is_atomic {
                    return Err(type_error(span, "C11 _Atomic type is unsupported in casts"));
                }
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

    /// C99 6.3.2.3p3: an integer constant expression evaluating to zero,
    /// or that expression cast to exactly void*, is a null pointer constant.
    /// An ordinary void* expression or (void*)nonzero is NOT a null pointer
    /// constant; notably, neither may implicitly initialize a function ptr.
    fn is_null_pointer_constant(expr: &CheckedExpr, target: Target) -> bool {
        if expr.ty.is_integer() {
            return const_eval::evaluate(expr, target) == Ok(0);
        }
        match (&expr.ty, &expr.kind) {
            (CType::Pointer(pointee), CheckedExprKind::Convert(inner))
                if pointee.unqualified() == &CType::Void && inner.ty.is_integer() =>
                const_eval::evaluate(inner, target) == Ok(0),
            _ => false,
        }
    }

    fn convert_for_assignment(&self, expr: CheckedExpr, ty: &CType, _original: &Expr, span: Span) -> Result<CheckedExpr, Diagnostic> {
        // The source array undergoes the standard array-to-pointer conversion
        // before checking scalar assignment compatibility.
        let mut expr = expr.rvalue();
        // A function-pointer result can retain a nominal TagRef even after
        // its definition has become complete. Compare the completed aggregate
        // type without changing the checked call expression's kind.
        if matches!(expr.ty.unqualified(), CType::TagRef { .. }) {
            expr.ty = self.complete_tag_type(expr.ty.clone(), span)?;
        }
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
            (CType::Floating(_), CType::Floating(_)) |
            (CType::Floating(_), CType::Integer { .. }) |
            (CType::Integer { .. }, CType::Floating(_)) => true,
            (CType::Pointer(lhs), CType::Pointer(rhs)) =>
                Self::is_null_pointer_constant(&expr, self.target)
                    || CType::compatible_pointee_assignment(lhs, rhs),
            (CType::Pointer(_), CType::Integer { .. }) =>
                Self::is_null_pointer_constant(&expr, self.target),
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
        let integer_ty = integer.integer_promotion_type(self.target);
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
    /// A GNU aligned member request is a minimum alignment. Accept only
    /// requests proven not to change the natural layout for now; actual
    /// over-alignment also needs storage and ABI guarantees in both backends.
    fn check_member_alignment(
        &self, alignment: Option<&mini_c_syntax::ast::MemberAlignment>,
        ty: &CType, bit_field: bool, span: Span,
    ) -> Result<(), Diagnostic> {
        use mini_c_syntax::ast::MemberAlignment;
        let Some(alignment) = alignment else { return Ok(()); };
        if bit_field {
            return Err(type_error(span, "aligned bit-fields are unsupported"));
        }
        let requested = match alignment {
            MemberAlignment::AlignOf(source) => {
                let source = self.resolve_sizeof_type(source, span)?;
                let source = self.try_complete_tag_type(source)?;
                if !source.is_object() || source.size_bytes(self.target).is_none() {
                    return Err(type_error(span, "alignment requires a complete object type"));
                }
                source.align_bytes(self.target).ok_or_else(||
                    type_error(span, "alignment requires a complete object type"))?
            }
            MemberAlignment::Constant(expr) => {
                let typed = self.check_expr(expr, span)?.rvalue();
                if !typed.ty.is_integer() {
                    return Err(type_error(span, "member alignment requires an integer constant"));
                }
                let value = const_eval::evaluate(&typed, self.target)
                    .map_err(|_| type_error(span, "member alignment requires an integer constant"))?;
                u64::try_from(value).map_err(|_|
                    type_error(span, "member alignment must be positive"))?
            }
        };
        if !requested.is_power_of_two() {
            return Err(type_error(span, "member alignment must be a positive power of two"));
        }
        let natural = ty.align_bytes(self.target).ok_or_else(||
            type_error(span, "aligned member requires complete layout"))?;
        if requested > natural {
            return Err(type_error(span, "over-aligned member requires backend layout support"));
        }
        Ok(())
    }

    fn check_bit_width(
        &self, width: &Expr, ty: &CType, unnamed: bool, span: Span,
    ) -> Result<u8, Diagnostic> {
        let max_width = match ty.unqualified() {
            CType::Integer { rank: IntRank::Bool, .. } => 1i128,
            CType::Integer { rank: IntRank::Char | IntRank::Short
                | IntRank::Int | IntRank::Long, .. } =>
                i128::from(match ty.unqualified() {
                    CType::Integer { rank, .. } => self.target.bits(*rank),
                    _ => unreachable!(),
                }),
            _ => return Err(type_error(span,
                "bit-field requires _Bool, char, short, int or long")),
        };
        let typed = self.check_expr(width, span)?.rvalue();
        if !typed.ty.is_integer() {
            return Err(type_error(span, "bit-field width must be an integer constant expression"));
        }
        let value = const_eval::evaluate(&typed, self.target)
            .map_err(|why| type_error(span, format!(
                "bit-field width requires integer constant expression: {why}")))?;
        if !(if unnamed { 0..=max_width } else { 1..=max_width }).contains(&value) {
            let is_bool = max_width == 1;
            let reason = match (is_bool, unnamed) {
                (true, true) => "unnamed _Bool bit-field width must be 0 or 1".to_owned(),
                (true, false) => "named _Bool bit-field width must be 1".to_owned(),
                (false, true) => format!(
                    "unnamed bit-field width must be between 0 and {max_width}"),
                (false, false) => format!(
                    "named bit-field width must be between 1 and {max_width}"),
            };
            return Err(type_error(span, reason));
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
            let (written, alignment) = match &member.ty {
                Type::MemberAligned(base, alignment) => (base.as_ref(), Some(alignment)),
                ty => (ty, None),
            };
            let (declared, bit_width) = match written {
                Type::BitField(base, width) => {
                    (base.as_ref(), Some(width.as_ref()))
                }
                _ => (written, None),
            };
            let member_ty = self.resolve_sizeof_type(declared, member.span)?;
            let member_ty = self.try_complete_tag_type(member_ty)?;
            let width = bit_width.map(|width| {
                self.check_bit_width(width, &member_ty, member.name.is_empty(), member.span)
            }).transpose()?;
            self.check_member_alignment(alignment, &member_ty, width.is_some(), member.span)?;
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
            Type::Atomic(inner) => {
                let base=self.resolve_sizeof_type(inner,span)?;
                self.atomic_object_type(base,span)?
            }
            Type::Qualified(inner, qualifiers) => {
                let inner = self.resolve_sizeof_type(inner, span)?;
                if qualifiers.is_restrict && !matches!(inner.unqualified(), CType::Pointer(pointee)
                    if !matches!(pointee.unqualified(), CType::Function { .. } | CType::VariadicFunction { .. } | CType::UnprototypedFunction { .. })) {
                    return Err(type_error(span, "restrict requires pointer type"));
                }
                if qualifiers.is_const && matches!(inner.unqualified(),
                    CType::Function { .. } | CType::VariadicFunction { .. } | CType::UnprototypedFunction { .. }) {
                    return Err(type_error(span, "const cannot qualify a function type"));
                }
                if qualifiers.is_volatile && matches!(inner.unqualified(),
                    CType::Function { .. } | CType::VariadicFunction { .. } | CType::UnprototypedFunction { .. }) {
                    return Err(type_error(span, "volatile cannot qualify a function type"));
                }
                let inner = if qualifiers.is_atomic {
                    self.atomic_object_type(inner,span)?
                } else {inner};
                let inner = if qualifiers.is_const { inner.with_const() } else { inner };
                if qualifiers.is_volatile { inner.with_volatile() } else { inner }
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
                let element = self.try_complete_tag_type(element)?;
                if element.has_flexible_array_member() {
                    return Err(type_error(span, "array element cannot contain a flexible array member"));
                }
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

    /// A single checked source entry point for all GNU atomic builtin spellings.
    /// This produces a typed operation plan but never a generic C Call. Target
    /// code generation remains gated until true atomic lowering is available.
    fn check_atomic_builtin(
        &self, spec: &builtins::BuiltinSpec, args: &[Expr], span: Span,
    ) -> Result<CheckedExpr, Diagnostic> {
        use builtins::BuiltinId as A;
        let success = self.atomic_builtin_constant_arg(
            &args[spec.memory_order_argument.expect("atomic builtin has order")],
            span, "memory order")?;
        let failure = spec.failure_order_argument.map(|index|
            self.atomic_builtin_constant_arg(&args[index], span, "failure memory order")
        ).transpose()?;
        if !builtins::valid_atomic_orders(spec.id, success, failure) {
            return Err(type_error(span, format!(
                "{} has an invalid C11 success/failure memory-order combination", spec.name)));
        }
        let weak = spec.weak_argument.map(|index|
            self.atomic_builtin_constant_arg(&args[index], span, "weak compare-exchange flag")
        ).transpose()?;
        if weak.is_some_and(|flag| flag > 1) {
            return Err(type_error(span, "atomic compare-exchange weak flag must be 0 or 1"));
        }
        let weak = weak.unwrap_or(0) != 0;
        let mut operands = Vec::new();
        let (result_ty, bytes, alignment) =
            if matches!(spec.id, A::AtomicThreadFence | A::AtomicSignalFence) {
                (CType::Void, 0, 0)
            } else {
                let address = self.check_expr(&args[0],
                    args[0].location().unwrap_or(span))?.rvalue();
                let CType::Pointer(pointee) = &address.ty else {
                    return Err(type_error(span, format!(
                        "{} requires a pointer to an integer object", spec.name)));
                };
                let CType::Integer { rank, .. } = pointee.unqualified() else {
                    return Err(type_error(span, format!(
                        "{} requires an integer object (pointer and aggregate atomics are pending)",
                        spec.name)));
                };
                let width = self.target.bits(*rank);
                let bytes = u8::try_from(width / 8)
                    .map_err(|_| type_error(span, "unsupported atomic integer width"))?;
                let alignment = pointee.align_bytes(self.target)
                    .and_then(|align| u8::try_from(align).ok())
                    .ok_or_else(|| type_error(span, "unsupported atomic integer alignment"))?;
                if !matches!(width, 8 | 16 | 32 | 64) ||
                    alignment < bytes || !alignment.is_power_of_two()
                {
                    return Err(type_error(span, "atomic object requires a naturally aligned 8/16/32/64-bit integer"));
                }
                if spec.id != A::AtomicLoadN && pointee.is_const() {
                    return Err(type_error(span, format!(
                        "{} requires a writable integer object", spec.name)));
                }
                let value_ty = pointee.unqualified().clone();
                operands.push(address);
                let result_ty = match spec.id {
                    A::AtomicLoadN => value_ty,
                    A::AtomicStoreN | A::AtomicExchangeN |
                    A::AtomicFetchAdd | A::AtomicFetchSub |
                    A::AtomicFetchAnd | A::AtomicFetchOr | A::AtomicFetchXor => {
                        let argument = &args[1];
                        let checked = self.check_expr(argument,
                            argument.location().unwrap_or(span))?;
                        operands.push(self.convert_for_assignment(
                            checked, &value_ty, argument, argument.location().unwrap_or(span))?);
                        if spec.id == A::AtomicStoreN { CType::Void } else { value_ty }
                    }
                    A::AtomicCompareExchangeN => {
                        // GNU CAS passes expected by reference. The conditional
                        // failure writeback is observable even when Bool unused.
                        let expected = self.check_expr(&args[1],
                            args[1].location().unwrap_or(span))?.rvalue();
                        let CType::Pointer(expected_pointee) = &expected.ty else {
                            return Err(type_error(span,
                                "atomic compare-exchange expected argument must be a pointer"));
                        };
                        if expected_pointee.is_const() ||
                            !expected_pointee.unqualified().same_type(&value_ty)
                        {
                            return Err(type_error(span,
                                "atomic compare-exchange expected pointer requires matching writable integer type"));
                        }
                        operands.push(expected);
                        let argument = &args[2];
                        let checked = self.check_expr(argument,
                            argument.location().unwrap_or(span))?;
                        operands.push(self.convert_for_assignment(
                            checked, &value_ty, argument, argument.location().unwrap_or(span))?);
                        CType::Integer { rank: IntRank::Bool, unsigned: true }
                    }
                    _ => unreachable!("fences handled above; CTZ is separately checked"),
                };
                (result_ty, bytes, alignment)
            };
        Ok(CheckedExpr::value(result_ty,
            CheckedExprKind::BuiltinAtomic(Box::new(CheckedAtomicBuiltin {
                builtin: spec.id, operands, bytes, alignment,
                success_order: success, failure_order: failure, weak,
            }))))
    }

    fn atomic_builtin_constant_arg(
        &self, expr: &Expr, span: Span, label: &str,
    ) -> Result<u8, Diagnostic> {
        let actual_span = expr.location().unwrap_or(span);
        let checked = self.check_expr(expr, actual_span)?.rvalue();
        if !checked.ty.is_integer() {
            return Err(type_error(actual_span, format!(
                "atomic {label} requires an integer constant expression")));
        }
        let value = const_eval::evaluate(&checked, self.target)
            .map_err(|_| type_error(actual_span,
                format!("atomic {label} requires an integer constant expression")))?;
        u8::try_from(value).map_err(|_| type_error(actual_span,
            format!("atomic {label} is outside the supported constant range")))
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
                if matches!(typed.kind, CheckedExprKind::BitFieldMember(..)) {
                    return Err(type_error(span, "sizeof cannot be applied to a bit-field"));
                }
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
                // C sizeof has size_t type under the checked target model.
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
                self.reference_warning_binding(name);
                // The implicit object is internally named so that a
                // frontend user cannot collide with it and every use
                // within this function refers to the same static array.
                // An ordinary shadowing declaration (if accepted by an
                // implementation) has no private backing name.
                let resolved_name = if name == "__func__" {
                    symbol.asm_label.as_deref().unwrap_or(name)
                } else { name };
                Ok(CheckedExpr { ty: symbol.ty.clone(), category,
                    kind: CheckedExprKind::Name(resolved_name.to_owned()) })
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
                if object.ty.is_atomic() {
                    return Err(type_error(span, "member access of atomic aggregate is unsupported"));
                }
                let members = match object.ty.unqualified() {
                    CType::Struct { members, .. } | CType::Union { members, .. } => members,
                    _ => return Err(Diagnostic::coded(span, DiagnosticCode::SemanticInvalidMemberAccess, "member access requires a struct or union")),
                };
                let field = members.iter().find(|m| m.name == *name).ok_or_else(||
                    Diagnostic::coded(span, DiagnosticCode::SemanticUnknownMember, format!("unknown struct member '{name}'")))?;
                // A member of a const aggregate is itself nonmodifiable.
                // Array member qualification applies to its element type.
                // Qualification of an aggregate lvalue propagates to its
                // members, including the array element type (C99 6.5.2.3).
                let mut field_ty = field.ty.clone();
                if object.ty.is_const() { field_ty = field_ty.with_const(); }
                if object.ty.is_volatile() { field_ty = field_ty.with_volatile(); }
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
                        let category = if matches!(ty, CType::Function { .. } | CType::VariadicFunction { .. } | CType::UnprototypedFunction { .. }) { Category::Function } else if ty == CType::Void {
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
                            else { typed.integer_promotion_type(self.target) };
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
                        if (a.unqualified().composite(b.unqualified()).is_some()
                            && (equality || a.unqualified().is_object()))
                            || (equality && ((*a.unqualified() == CType::Void && b.unqualified().is_object())
                                || (*b.unqualified() == CType::Void && a.unqualified().is_object()))))
                    || (equality
                        && ((matches!(left.ty, CType::Pointer(_))
                             && Self::is_null_pointer_constant(&right, self.target))
                            || (matches!(right.ty, CType::Pointer(_))
                                && Self::is_null_pointer_constant(&left, self.target))))) {
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
                    let left_ty = left.integer_promotion_type(self.target);
                    let right_ty = right.integer_promotion_type(self.target);
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
                    self.target.common_integer(
                        &left.integer_promotion_type(self.target),
                        &right.integer_promotion_type(self.target))
                } else {
                    self.target.common_arithmetic(
                        &left.integer_promotion_type(self.target),
                        &right.integer_promotion_type(self.target))
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
                    self.target.common_arithmetic(
                        &yes.integer_promotion_type(self.target),
                        &no.integer_promotion_type(self.target))
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
                } else if Self::is_null_pointer_constant(&yes, self.target)
                    && matches!(no.ty, CType::Pointer(_))
                {
                    no.ty.clone()
                } else if Self::is_null_pointer_constant(&no, self.target)
                    && matches!(yes.ty, CType::Pointer(_))
                {
                    yes.ty.clone()
                } else if let (CType::Pointer(a), CType::Pointer(b)) =
                    (&yes.ty, &no.ty)
                {
                    let base_a = a.unqualified();
                    let base_b = b.unqualified();
                    let unified = if let Some(composite) = base_a.composite(base_b) {
                        composite
                    } else if (*base_a == CType::Void && base_b.is_object())
                        || (*base_b == CType::Void && base_a.is_object())
                    {
                        CType::Void
                    } else {
                        return Err(Diagnostic::coded(span,
                            DiagnosticCode::SemanticInvalidOperatorOperands,
                            "conditional operands require compatible types"));
                    };
                    // C99 conditional pointers produce the composite
                    // pointed-to type with the union of cv qualifiers.
                    let unified = if a.is_const() || b.is_const() {
                        unified.with_const()
                    } else { unified };
                    let unified = if a.is_volatile() || b.is_volatile() {
                        unified.with_volatile()
                    } else { unified };
                    CType::Pointer(Box::new(unified))
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
                let dest = if matches!(ty, Type::Void | Type::Pointer(..) | Type::Alias(..)
                    | Type::Qualified(..) | Type::Floating(..))
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
                    // ISO C99 6.5.4: (void)expr accepts any object/function
                    // expression and evaluates it once; only its value is
                    // discarded. Checked Convert retains observable effects.
                    CType::Void => {},
                    CType::Integer { .. } if value.ty.is_integer()
                        || matches!(value.ty.unqualified(), CType::Floating(_)) => {},
                    CType::Floating(_) if value.ty.is_integer()
                        || matches!(value.ty.unqualified(), CType::Floating(_)) => {},
                    // C99 6.3.2.3 permits explicit pointer-to-integer casts.
                    // The resulting integer is implementation-defined; on
                    // our current LP64 target the checked IR preserves an
                    // explicit Convert, which Cranelift lowers as a same-width
                    // bit-pattern conversion (or integer truncation).
                    CType::Integer { .. } if matches!(value.ty.unqualified(), CType::Pointer(_)) => {},
                    CType::Pointer(_) if matches!(value.ty.unqualified(), CType::Pointer(_)) => {},
                    // C99 6.3.2.3p5: explicit integer-to-pointer conversions
                    // are permitted, with implementation-defined result.
                    // Keep the checked Convert; the LP64 backend maps integer
                    // address bits into its pointer representation. Do NOT
                    // weaken implicit pointer assignment or promise that an
                    // arbitrary integer is a valid dereferenceable address.
                    CType::Pointer(_) if value.ty.is_integer() => {},
                    CType::Pointer(_) => return Err(type_error(span,
                        "pointer cast requires an integer or pointer operand")),
                    CType::Integer { .. } => return Err(type_error(span,
                        "integer cast requires integer operand")),
                    _ => return Err(type_error(span, "unsupported cast type-name")),
                }
                // An explicit cast ends bit-field value-width promotion
                // provenance, even when the destination is the *same*
                // declared base type. The usual implicit convert() helper
                // can elide an identity conversion, which would wrongly
                // make -(unsigned long)small_unsigned_bitfield promote to int.
                if value.bit_field_width().is_some() && value.ty.same_type(&dest) {
                    Ok(CheckedExpr::value(dest,
                        CheckedExprKind::Convert(Box::new(value))))
                } else {
                    Ok(self.convert(value, &dest))
                }
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
                // C assignment returns a value, not a qualified atomic
                // lvalue. Keep the lvalue itself qualified inside the AST
                // for the TypeId emitter to select ordered atomic storage.
                let result_ty=if lhs.ty.is_atomic() {
                    lhs.ty.unqualified().clone()
                } else {lhs.ty.clone()};
                Ok(CheckedExpr::value(result_ty, CheckedExprKind::Assign(
                    Box::new(lhs),Box::new(rhs_typed))))
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
                    || !(lhs.ty.is_integer()
                        || matches!(lhs.ty.unqualified(), CType::Floating(_))
                        || pointer_arithmetic)
                {
                    return Err(Diagnostic::coded(lhs_span,
                        DiagnosticCode::SemanticInvalidAssignmentTarget,
                        "compound assignment requires a modifiable arithmetic lvalue or pointer +=/-="));
                }
                // Bitwise, shifts and remainder require integer operands on
                // *both* sides, even if the right side happens to be integer.
                let integer_only = matches!(op, BinOp::Mod | BinOp::BitAnd
                    | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr);
                if integer_only && !lhs.ty.is_integer() {
                    return Err(Diagnostic::coded(lhs_span,
                        DiagnosticCode::SemanticInvalidOperatorOperands,
                        "integer compound operator requires integer lvalue"));
                }
                let rhs_checked = self.check_expr(rhs, span)?.rvalue();
                let rhs_float = matches!(rhs_checked.ty.unqualified(), CType::Floating(_));
                let needs_integer = pointer_arithmetic || matches!(op,
                    BinOp::Mod | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr);
                if !(rhs_checked.ty.is_integer() || (rhs_float && !needs_integer)) {
                    return Err(Diagnostic::coded(rhs_span,
                        DiagnosticCode::SemanticInvalidOperatorOperands,
                        if needs_integer { "compound assignment requires integer operands" }
                        else { "compound assignment requires arithmetic operands" }));
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
                } else if arithmetic && !matches!(op, BinOp::Mod) {
                    self.target.common_arithmetic(
                        &lhs.integer_promotion_type(self.target),
                        &rhs_checked.integer_promotion_type(self.target))
                        .ok_or_else(|| type_error(span, "incompatible compound assignment operands"))?
                } else if matches!(op, BinOp::Mod | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor) {
                    self.target.common_integer(
                        &lhs.integer_promotion_type(self.target),
                        &rhs_checked.integer_promotion_type(self.target))
                        .ok_or_else(|| type_error(span, "incompatible integer compound assignment operands"))?
                } else if matches!(op, BinOp::Shl | BinOp::Shr) {
                    lhs.integer_promotion_type(self.target)
                } else {
                    return Err(type_error(span, "unsupported compound assignment operator"));
                };
                // Shift and pointer offsets promote their right operand
                // independently; all other operations apply the usual
                // arithmetic conversions to both operands.
                let rhs_ty = if pointer_arithmetic ||
                    matches!(op, BinOp::Shl | BinOp::Shr)
                {
                    rhs_checked.integer_promotion_type(self.target)
                } else {
                    computation_ty.clone()
                };
                let rhs_checked = self.convert(rhs_checked, &rhs_ty);
                let result_ty = if lhs.ty.is_atomic() {
                    lhs.ty.unqualified().clone()
                } else {lhs.ty.clone()};
                Ok(CheckedExpr::value(result_ty, CheckedExprKind::CompoundAssign(
                    *op, Box::new(lhs), Box::new(rhs_checked), computation_ty)))
            }
            Expr::Update { target, increment, postfix } => {
                let target_span = target.location().unwrap_or(span);
                let checked = self.check_expr(target, target_span)?;
                if checked.category != Category::LValue || checked.ty.is_const()
                    || !(checked.ty.is_integer()
                        || matches!(checked.ty.unqualified(), CType::Floating(_) | CType::Pointer(_)))
                {
                    return Err(Diagnostic::coded(target_span,
                        DiagnosticCode::SemanticInvalidAssignmentTarget,
                        "increment/decrement requires a modifiable scalar lvalue"));
                }
                let computation_ty = if let CType::Pointer(pointee) = &checked.ty {
                    let complete = self.complete_tag_type(pointee.as_ref().clone(), target_span)?;
                    if complete.size_bytes(self.target).is_none() {
                        return Err(type_error(target_span,
                            "increment/decrement requires pointer to complete object type"));
                    }
                    CType::Pointer(Box::new(complete))
                } else {
                    checked.integer_promotion_type(self.target)
                };
                // Prefix/postfix C atomic updates produce ordinary value
                // types, although their target remains a qualified lvalue.
                let ty = if checked.ty.is_atomic() {
                    checked.ty.unqualified().clone()
                } else {checked.ty.clone()};
                Ok(CheckedExpr::value(ty, CheckedExprKind::Update {
                    target: Box::new(checked),
                    increment: *increment,
                    postfix: *postfix,
                    computation_ty,
                }))
            }
            Expr::Call(callee, args) => {
                // One typed builtin registry owns spelling, arity and target
                // lowering admission. Never turn an unimplemented GNU atomic
                // into a plain external call, regardless of a visible symbol.
                if let Expr::Name(name) = callee.without_location() {
                    if let Some(spec) = builtins::lookup(name) {
                        if spec.lowering == builtins::LoweringClass::AtomicIrRequired
                            || self.lookup(name).is_none()
                        {
                            if args.len() != spec.argument_count {
                                return Err(Diagnostic::coded(span,
                                    DiagnosticCode::SemanticWrongArgumentCount,
                                    if spec.argument_count == 1 {
                                        format!("{name} expects exactly one argument")
                                    } else {
                                        format!("{name} expects exactly {} arguments", spec.argument_count)
                                    }));
                            }
                            match spec.id {
                                builtins::BuiltinId::Ctz32 => {
                                    let argument = &args[0];
                                    let argument_span = argument.location().unwrap_or(span);
                                    let typed = self.check_expr(argument, argument_span)?.rvalue();
                                    if !typed.ty.is_integer() {
                                        return Err(Diagnostic::coded(argument_span,
                                            DiagnosticCode::SemanticInvalidOperatorOperands,
                                            "__builtin_ctz requires an integer argument"));
                                    }
                                    // The backend already implements this
                                    // single-evaluation unsigned-int builtin.
                                    let uint = CType::Integer {
                                        rank: IntRank::Int, unsigned: true,
                                    };
                                    let argument = self.convert(typed, &uint);
                                    return Ok(CheckedExpr::value(CType::int(),
                                        CheckedExprKind::BuiltinCtz(Box::new(argument))));
                                }
                                builtins::BuiltinId::ByteSwap(bits) => {
                                    let argument = &args[0];
                                    let at = argument.location().unwrap_or(span);
                                    let typed = self.check_expr(argument, at)?.rvalue();
                                    if !typed.ty.is_integer() {
                                        return Err(type_error(at, format!("{name} requires an integer argument")));
                                    }
                                    let rank = [IntRank::Short,IntRank::Int,IntRank::Long,IntRank::LongLong]
                                        .into_iter().find(|&rank| self.target.bits(rank)==u16::from(bits))
                                        .ok_or_else(||type_error(at,format!("{name} requires exact unsigned {bits}-bit type")))?;
                                    let ty=CType::Integer{rank,unsigned:true};
                                    let argument=self.convert(typed,&ty);
                                    return Ok(CheckedExpr::value(ty,CheckedExprKind::BuiltinByteSwap(bits,Box::new(argument))));
                                }
                                _ => return self.check_atomic_builtin(spec, args, span),
                            }
                        }
                    } else if builtins::reserved_name(name) {
                        return Err(Diagnostic::coded(span,
                            DiagnosticCode::SemanticInvalidOperatorOperands,
                            format!("unsupported compiler builtin {name}; no implicit external call")));
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
                    CType::UnprototypedFunction { result } => Some((result.as_ref(), &[][..], true)),
                    CType::Pointer(inner) => match inner.as_ref() {
                        CType::Function { result, params } => Some((result.as_ref(), params.as_slice(), false)),
                        CType::VariadicFunction { result, params } => Some((result.as_ref(), params.as_slice(), true)),
                    CType::UnprototypedFunction { result } => Some((result.as_ref(), &[][..], true)),
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
                // C default argument promotions apply to ellipsis and non-prototype arguments.
                for arg in args.iter().skip(params.len()) {
                    let typed = self.check_expr(arg, span)?.rvalue();
                    if !typed.ty.is_scalar() {
                        return Err(type_error(arg.location().unwrap_or(span),
                            "variadic argument requires a supported scalar type"));
                    }
                    let promoted = if matches!(typed.ty, CType::Floating(mini_c_syntax::ast::FloatRank::Float)) {
                        CType::Floating(mini_c_syntax::ast::FloatRank::Double)
                    } else {
                        typed.integer_promotion_type(self.target)
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

#[cfg(test)]
mod warning_tests;
