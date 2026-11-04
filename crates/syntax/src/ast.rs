//! Unresolved C syntax AST. Source spans use UTF-8 byte offsets.
pub type Ident = String;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StorageClass {
    #[default]
    Default,
    Extern,
    Static,
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
    pub name: Ident,
    /// Name token range in opt-in location-aware parser mode.
    pub name_span: Option<Span>,
    pub result: Type,
    pub params: Vec<Decl>,
    pub body: Stmt,
    pub span: Span,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Decl {
    pub storage: StorageClass,
    pub name: Ident,
    /// Name token range; `span` continues to cover the full declaration.
    pub name_span: Option<Span>,
    pub ty: Type,
    pub init: Option<Expr>,
    pub span: Span,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TagKind { Struct, Union, Enum }

/// C object-type qualifiers. Restrict is retained in the syntax tree to
/// check legal placement, but has no runtime representation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Qualifiers {
    pub is_const: bool,
    pub is_restrict: bool,
}
impl Qualifiers {
    pub fn any(self) -> bool { self.is_const || self.is_restrict }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Void,
    Char,
    Short,
    Int,
    Long,
    LongLong,
    Alias(Ident),
    /// Qualifiers bind to this type, not to the nearest pointer's pointee.
    Qualified(Box<Type>, Qualifiers),
    Signed(Box<Type>),
    Unsigned(Box<Type>),
    Pointer(Box<Type>),
    Array(Box<Type>, Option<Box<Expr>>),
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
    /// A prototype ending in C ellipsis; unlike Function, extra arguments
    /// undergo default argument promotions and the ABI is variadic.
    VariadicFunction {
        result: Box<Type>,
        params: Vec<Type>,
    },
}
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Block(Vec<Stmt>),
    Decl(Decl),
    Typedef(Decl),
    /// A standalone tag declaration or definition without an object declarator.
    Tag(Type),
    Expr(Option<Expr>),
    Return(Option<Expr>),
    If {
        cond: Expr,
        then: Box<Stmt>,
        otherwise: Option<Box<Stmt>>,
    },
    While {
        cond: Expr,
        body: Box<Stmt>,
    },
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
}
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Opt-in precise source span, only produced by Parser::new_with_spans.
    /// The ordinary parser retains its historical expression variants.
    Located(Box<Expr>, Span),
    Int(u64),
    /// C integer token with radix/suffix preserved for target type selection.
    IntegerConstant(crate::integer_literal::IntegerLiteral),
    /// Decoded narrow string bytes, including the terminating NUL.
    StringLiteral(Vec<u8>),
    /// Braced initializer, valid only in an object declaration.
    Initializer(Vec<Expr>),
    Name(Ident),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    /// Postfix subscript: `base[index]`, equivalent to `*(base + index)`.
    Index(Box<Expr>, Box<Expr>),
    /// Postfix member selection: true for `->`, false for `.`.
    Member(Box<Expr>, Ident, bool),
    Assign(Box<Expr>, Box<Expr>),
    /// C comma operator, evaluated left to right with the value of the right operand.
    Comma(Box<Expr>, Box<Expr>),
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
