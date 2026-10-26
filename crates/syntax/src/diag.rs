//! Structured source diagnostics, independent of CLI presentation.
use crate::ast::Span;

/// Stable broad diagnostic categories. Wording can improve without breaking
/// clients that distinguish lexical, syntactic and semantic failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticCode {
    Lexical,
    Syntax,
    Semantic,
}

impl DiagnosticCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lexical => "L0001",
            Self::Syntax => "S0001",
            Self::Semantic => "T0001",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub span: Span,
    pub message: String,
    pub code: DiagnosticCode,
}
impl Diagnostic {
    /// Default constructor for syntax errors; retained for source compatibility.
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self::syntax(span, message)
    }
    pub fn syntax(span: Span, message: impl Into<String>) -> Self {
        Self { span, message: message.into(), code: DiagnosticCode::Syntax }
    }
    pub fn lexical(span: Span, message: impl Into<String>) -> Self {
        Self { span, message: message.into(), code: DiagnosticCode::Lexical }
    }
    pub fn semantic(span: Span, message: impl Into<String>) -> Self {
        Self { span, message: message.into(), code: DiagnosticCode::Semantic }
    }
}
