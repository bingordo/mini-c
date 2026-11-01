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
    pub result: Type,
    pub params: Vec<Decl>,
    pub body: Stmt,
    pub span: Span,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Decl {
    pub storage: StorageClass,
    pub name: Ident,
    pub ty: Type,
    pub init: Option<Expr>,
    pub span: Span,
}
#[derive(Debug, Clone, PartialEq)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TagKind { Struct, Union }

pub enum Type {
    Void,
    Char,
    Int,
    Long,
    Alias(Ident),
    Signed(Box<Type>),
    Unsigned(Box<Type>),
    Pointer(Box<Type>),
    Array(Box<Type>, Option<Box<Expr>>),
    /// Complete structure definition, initially inlined into its declarator.
    /// Separate tag-only declarations and references are a follow-up.
    Struct { tag: Option<Ident>, members: Vec<Decl> },
    Union { tag: Option<Ident>, members: Vec<Decl> },
    /// Reference to a named (possibly incomplete) struct or union tag.
    TagRef(TagKind, Ident),
    Function {
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
    Break,
    Continue,
}
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(u64),
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
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Plus,
    Neg,
    Not,
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
}
