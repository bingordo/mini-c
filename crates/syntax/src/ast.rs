//! Unresolved C syntax AST. Source spans use UTF-8 byte offsets.
pub type Ident = String;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StorageClass {
    #[default]
    Default,
    Extern,
    Static,
    /// Block-scope or parameter automatic object; its address cannot be taken.
    Register,
    /// File-scope thread storage, kept distinct from ordinary static storage.
    ThreadLocal,
    ExternThreadLocal,
    StaticThreadLocal,
}

impl StorageClass {
    /// True for declarations whose object identity is per-thread.
    pub fn is_thread_local(self) -> bool {
        matches!(self, Self::ThreadLocal | Self::ExternThreadLocal | Self::StaticThreadLocal)
    }
    /// Linkage specifier independent of the thread storage duration.
    pub fn linkage_storage(self) -> Self {
        match self {
            Self::ThreadLocal => Self::Default,
            Self::ExternThreadLocal => Self::Extern,
            Self::StaticThreadLocal => Self::Static,
            other => other,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Span {
    pub lo: usize,
    pub hi: usize,
}
#[derive(Debug, Clone, PartialEq)]
pub struct TranslationUnit {
    pub items: Vec<Item>,
}
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Function(Function),
    Global(Decl),
}
#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub storage: StorageClass,
    pub is_inline: bool,
    pub name: Ident,
    /// Name token range in opt-in location-aware parser mode.
    pub name_span: Option<Span>,
    pub result: Type,
    /// An identifier-list or empty-list definition, rather than a prototype.
    pub old_style: bool,
    pub params: Vec<Decl>,
    pub body: Stmt,
    pub span: Span,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Decl {
    pub storage: StorageClass,
    pub is_inline: bool,
    pub name: Ident,
    /// Name token range; `span` continues to cover the full declaration.
    pub name_span: Option<Span>,
    pub ty: Type,
    /// GNU __asm__("link-name") annotation on a file-scope declaration.
    /// The source identifier is still used for C name lookup.
    pub asm_label: Option<String>,
    pub init: Option<Expr>,
    pub span: Span,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TagKind { Struct, Union, Enum }

/// C object-type qualifiers. Restrict is retained for placement checking;
/// const, volatile, and atomic must survive into checked types.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Qualifiers {
    pub is_const: bool,
    pub is_volatile: bool,
    pub is_restrict: bool,
    /// C11 atomic qualification changes access and alignment semantics.
    pub is_atomic: bool,
}
impl Qualifiers {
    pub fn any(self) -> bool { self.is_const || self.is_volatile || self.is_restrict || self.is_atomic }
}

/// IEEE floating formats in hosted C declarations. The native compiler
/// currently retains these types for checked prototypes, not FP codegen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatRank { Float, Double, LongDouble, Float128 }

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// GNU inferred declaration type; resolved only from its initializer.
    AutoType,
    /// GNU typeof preserves the unevaluated operand's type without decay.
    TypeOfExpr(Box<Expr>),
    /// Explicit type-name operand of GNU typeof.
    TypeOfType(Box<Type>),
    Void,
    /// ISO C99 _Bool, a distinct one-byte integer type.
    Bool,
    Char,
    Short,
    Int,
    Long,
    LongLong,
    /// GCC x86-64 builtin va_list (array of one ABI __va_list_tag).
    /// Its target-specific layout is resolved in semantic analysis.
    BuiltinVaList,
    Floating(FloatRank),
    Alias(Ident),
    /// Qualifiers bind to this type, not to the nearest pointer's pointee.
    Qualified(Box<Type>, Qualifiers),
    /// C11 `_Atomic(type-name)` is not interchangeable with ordinary storage.
    /// Preserve the spelling for checked lowering; never erase its semantics.
    Atomic(Box<Type>),
    Signed(Box<Type>),
    Unsigned(Box<Type>),
    Pointer(Box<Type>),
    Array(Box<Type>, Option<Box<Expr>>),
    /// C99 structure/union bit-field declaration. Valid only as a member.
    /// The width remains an expression until semantic constant evaluation.
    BitField(Box<Type>, Box<Expr>),
    /// GNU member alignment request, checked against target object layout.
    MemberAligned(Box<Type>, MemberAlignment),
    /// Complete structure definition, initially inlined into its declarator.
    /// Separate tag-only declarations and references are a follow-up.
    Struct { tag: Option<Ident>, members: Vec<Decl> },
    Union { tag: Option<Ident>, members: Vec<Decl> },
    /// Enumerators have optional explicit constant expressions.
    Enum { tag: Option<Ident>, enumerators: Vec<(Ident, Option<Expr>)> },
    /// Reference to a named (possibly incomplete) struct or union tag.
    TagRef(TagKind, Ident),
    Function {
        result: Box<Type>,
        params: Vec<Type>,
    },
    /// C99 empty parameter list in a declaration: no prototype, not (void).
    UnprototypedFunction { result: Box<Type> },
    VariadicFunction {
        result: Box<Type>,
        params: Vec<Type>,
    },
}
#[derive(Debug, Clone, PartialEq)]
pub enum MemberAlignment {
    Constant(Box<Expr>),
    AlignOf(Box<Type>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct AsmOperand {
    /// Optional GNU symbolic operand name, resolved by semantic checking.
    pub name: Option<String>,
    pub constraint: String,
    pub expr: Expr,
    pub span: Span,
}
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Block(Vec<Stmt>),
    Decl(Decl),
    Typedef(Decl),
    /// A standalone tag declaration or definition without an object declarator.
    Tag(Type),
    Expr(Option<Expr>),
    /// GNU basic asm statement. A separate declaration-asm-label parser handles
    /// ELF symbol aliases; this is executable code and must never be discarded.
    /// Outputs, inputs, clobbers and asm goto are deliberately NOT modeled yet.
    BasicAsm { template: String, span: Span },
    /// GCC extended asm with exactly zero outputs and inputs. The first
    /// supported clobbers are `memory` and `cc`, both explicitly tracked.
    /// Register constraints, operands and asm goto are not accepted yet.
    ExtendedAsm {
        template: String,
        outputs: Vec<AsmOperand>,
        inputs: Vec<AsmOperand>,
        clobbers: Vec<String>,
        span: Span,
    },
    Return(Option<Expr>),
    /// Explicit clang-compatible guaranteed sibling-tail return. Never
    /// silently rewrite this into an ordinary C return during lowering.
    MustTailReturn(Expr),
    If {
        cond: Expr,
        then: Box<Stmt>,
        otherwise: Option<Box<Stmt>>,
    },
    While {
        cond: Expr,
        body: Box<Stmt>,
    },
    /// Preserve C99 switch dispatch and fallthrough; case/default labels can
    /// occur under nested blocks and do not create a scope by themselves.
    Switch { control: Expr, body: Box<Stmt> },
    Case { value: Expr, body: Box<Stmt> },
    Default(Box<Stmt>),
    /// C post-test loop: execute the body at least once.
    DoWhile {
        body: Box<Stmt>,
        cond: Expr,
    },
    /// The initializer declaration has a scope spanning condition, step, and body.
    For {
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        step: Option<Expr>,
        body: Box<Stmt>,
    },
    Break,
    Continue,
    /// Labels have function scope and a separate namespace from ordinary names.
    Label { name: Ident, span: Span, body: Box<Stmt> },
    /// A forward reference is allowed; the checker resolves it in function scope.
    Goto { target: Ident, span: Span },
}
/// A C99 initializer designator; multiple entries form a nested chain.
#[derive(Debug, Clone, PartialEq)]
pub enum Designator {
    Field(Ident),
    Index(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Opt-in precise source span, only produced by Parser::new_with_spans.
    /// The ordinary parser retains its historical expression variants.
    Located(Box<Expr>, Span),
    Int(u64),
    /// C integer token with radix/suffix preserved for target type selection.
    IntegerConstant(crate::integer_literal::IntegerLiteral),
    /// Source-preserved C decimal floating constant, with its exact rank.
    /// Native floating-point arithmetic/codegen is separately gated.
    FloatLiteral(String, FloatRank),
    /// Decoded narrow string bytes, including the terminating NUL.
    StringLiteral(Vec<u8>),
    /// Braced initializer, valid only in an object declaration.
    Initializer(Vec<Expr>),
    /// One initializer-list entry with a C99 designation. The path is kept
    /// explicit; semantic analysis must not confuse it with an expression.
    Designated { path: Vec<Designator>, value: Box<Expr> },
    Name(Ident),
    /// C sizeof(type-name): operand is a type, never evaluated at runtime.
    SizeofType(Type),
    /// ISO C99 sizeof unary-expression: type-check but never evaluate operand.
    SizeofExpr(Box<Expr>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    /// Postfix subscript: `base[index]`, equivalent to `*(base + index)`.
    Index(Box<Expr>, Box<Expr>),
    /// Postfix member selection: true for `->`, false for `.`.
    Member(Box<Expr>, Ident, bool),
    Assign(Box<Expr>, Box<Expr>),
    /// GNU `({ ... })`: a scoped compound statement used as an expression.
    /// Preserve every statement, including the final expression statement.
    StatementExpression(Vec<Stmt>),
    /// C comma operator, evaluated left to right with the value of the right operand.
    Comma(Box<Expr>, Box<Expr>),
    /// C99 compound literal: an addressable object with the named type and
    /// initializer-list. Its lifetime is the enclosing block at block scope,
    /// unlike a cast, which yields a non-lvalue.
    CompoundLiteral(Type, Box<Expr>),
    /// Explicit C scalar cast; the type name is resolved during semantic analysis.
    Cast(Type, Box<Expr>),
    /// C conditional operator, evaluating only the selected arm.
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
    /// Single-evaluation compound assignment; op is the underlying binary operation.
    CompoundAssign(BinOp, Box<Expr>, Box<Expr>),
    /// C prefix/postfix ++/--. Bool is increment (true) or decrement (false).
    Update { target: Box<Expr>, increment: bool, postfix: bool },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Plus,
    Neg,
    Not,
    BitNot,
    Address,
    Deref,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

impl Expr {
    /// Byte range of a parser-annotated expression, if available.
    pub fn location(&self) -> Option<Span> {
        match self {
            Self::Located(_, span) => Some(*span),
            _ => None,
        }
    }

    /// Strip only the outer annotation; nested expressions retain theirs.
    pub fn without_location(&self) -> &Self {
        match self {
            Self::Located(inner, _) => inner.without_location(),
            other => other,
        }
    }
}
