//! Structured source diagnostics, independent of CLI presentation.
use crate::ast::Span;

/// Stable diagnostic codes. General categories are retained for compatibility;
/// new kinds identify actionable errors independently of English messages.
/// Codes are never derived by matching diagnostic text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticCode {
    Lexical,
    Syntax,
    Semantic,
    LexicalUnterminatedComment,
    LexicalNumericSuffix,
    LexicalInvalidCharacter,
    SyntaxExpectedToken,
    SyntaxExpectedExpression,
    SyntaxIntegerOverflow,
    SemanticUnknownTypedef,
    SemanticDuplicateDeclaration,
    SemanticConflictingDeclaration,
    SemanticMultipleDefinitions,
    SemanticUndeclaredIdentifier,
    SemanticInvalidAssignmentTarget,
    SemanticWrongArgumentCount,
    SemanticIncompatibleAssignment,
}

impl DiagnosticCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lexical => "L0001",
            Self::Syntax => "S0001",
            Self::Semantic => "T0001",
            Self::LexicalUnterminatedComment => "L0101",
            Self::LexicalNumericSuffix => "L0102",
            Self::LexicalInvalidCharacter => "L0103",
            Self::SyntaxExpectedToken => "S0101",
            Self::SyntaxExpectedExpression => "S0102",
            Self::SyntaxIntegerOverflow => "S0103",
            Self::SemanticUnknownTypedef => "T0101",
            Self::SemanticDuplicateDeclaration => "T0102",
            Self::SemanticConflictingDeclaration => "T0103",
            Self::SemanticMultipleDefinitions => "T0104",
            Self::SemanticUndeclaredIdentifier => "T0105",
            Self::SemanticInvalidAssignmentTarget => "T0106",
            Self::SemanticWrongArgumentCount => "T0107",
            Self::SemanticIncompatibleAssignment => "T0108",
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
    pub fn coded(span: Span, code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self { span, message: message.into(), code }
    }
}
