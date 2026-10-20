//! Structured source diagnostics, independent of CLI presentation.
use crate::ast::Span;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic { pub span: Span, pub message: String }
impl Diagnostic {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self { span, message: message.into() }
    }
}
