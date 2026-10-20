//! Unresolved C syntax AST. Source spans use UTF-8 byte offsets.
pub type Ident = String;
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
    pub name: Ident,
    pub result: Type,
    pub params: Vec<Decl>,
    pub body: Stmt,
    pub span: Span,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Decl {
    pub name: Ident,
    pub ty: Type,
    pub init: Option<Expr>,
    pub span: Span,
}
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Void,
    Char,
    Int,
    Long,
    Unsigned(Box<Type>),
    Pointer(Box<Type>),
    Array(Box<Type>, Option<Box<Expr>>),
    Function {
        result: Box<Type>,
        params: Vec<Type>,
    },
}
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Block(Vec<Stmt>),
    Decl(Decl),
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
    Name(Ident),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
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
