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
    SyntaxExpectedDeclarator,
    SyntaxInvalidDeclaration,
    SyntaxExpectedParameter,
    SemanticUnknownTypedef,
    SemanticDuplicateDeclaration,
    SemanticConflictingDeclaration,
    SemanticMultipleDefinitions,
    SemanticUndeclaredIdentifier,
    SemanticInvalidAssignmentTarget,
    SemanticWrongArgumentCount,
    SemanticIncompatibleAssignment,
    SemanticNotCallable,
    SemanticUnknownMember,
    SemanticInvalidMemberAccess,
    SemanticInvalidSubscript,
    SemanticInvalidOperatorOperands,
    SemanticInvalidCondition,
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
            Self::SyntaxExpectedDeclarator => "S0104",
            Self::SyntaxInvalidDeclaration => "S0105",
            Self::SyntaxExpectedParameter => "S0106",
            Self::SemanticUnknownTypedef => "T0101",
            Self::SemanticDuplicateDeclaration => "T0102",
            Self::SemanticConflictingDeclaration => "T0103",
            Self::SemanticMultipleDefinitions => "T0104",
            Self::SemanticUndeclaredIdentifier => "T0105",
            Self::SemanticInvalidAssignmentTarget => "T0106",
            Self::SemanticWrongArgumentCount => "T0107",
            Self::SemanticIncompatibleAssignment => "T0108",
            Self::SemanticNotCallable => "T0109",
            Self::SemanticUnknownMember => "T0110",
            Self::SemanticInvalidMemberAccess => "T0111",
            Self::SemanticInvalidSubscript => "T0112",
            Self::SemanticInvalidOperatorOperands => "T0113",
            Self::SemanticInvalidCondition => "T0114",
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


/// Severity of a frontend diagnostic. Errors remain the default for the
/// existing `Diagnostic` API; warnings are opt-in through `ReportedDiagnostic`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Note,
}

/// A diagnostic with presentation metadata, without changing the legacy
/// parser/semantic error type or requiring downstream users to update literals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportedDiagnostic {
    pub diagnostic: Diagnostic,
    pub severity: DiagnosticSeverity,
    /// GCC-style warning group, without the "-W" prefix.
    pub warning_group: Option<&'static str>,
}

impl ReportedDiagnostic {
    pub fn error(diagnostic: Diagnostic) -> Self {
        Self { diagnostic, severity: DiagnosticSeverity::Error, warning_group: None }
    }

    pub fn warning(diagnostic: Diagnostic, group: &'static str) -> Self {
        Self {
            diagnostic,
            severity: DiagnosticSeverity::Warning,
            warning_group: Some(group),
        }
    }
}

/// Small, intentionally explicit warning policy. Frontends should only emit
/// warnings they actually implement; recognizing a -W group is not a claim
/// that a corresponding analysis already exists.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WarningPolicy {
    pub wall: bool,
    pub wextra: bool,
    pub warnings_as_errors: bool,
    /// Ordered per-group overrides. The last matching flag wins.
    pub overrides: Vec<(String, bool)>,
    /// Per-warning overrides for severity promotion, last occurrence wins.
    pub error_overrides: Vec<(String, bool)>,
}

impl WarningPolicy {
    /// Handles the common GCC driver spellings. Unknown flags are rejected,
    /// rather than silently claiming that a warning is implemented.
    pub fn apply_flag(&mut self, flag: &str) -> Result<(), String> {
        match flag {
            "-Wall" => self.wall = true,
            "-Wno-all" => self.wall = false,
            "-Wextra" => self.wextra = true,
            "-Wno-extra" => self.wextra = false,
            "-Werror" => self.warnings_as_errors = true,
            "-Wno-error" => self.warnings_as_errors = false,
            _ if flag.starts_with("-Werror=") => {
                let group = &flag["-Werror=".len()..];
                if !Self::known_group(group) {
                    return Err(format!("unsupported warning option: {flag}"));
                }
                // GCC's -Werror=group also enables that warning group.
                self.overrides.push((group.to_owned(), true));
                self.error_overrides.push((group.to_owned(), true));
            }
            _ if flag.starts_with("-Wno-error=") => {
                let group = &flag["-Wno-error=".len()..];
                if !Self::known_group(group) {
                    return Err(format!("unsupported warning option: {flag}"));
                }
                // Suppress promotion, not the warning itself.
                self.error_overrides.push((group.to_owned(), false));
            }
            _ => {
                if let Some(name) = flag.strip_prefix("-Wno-") {
                    if !Self::known_group(name) {
                        return Err(format!("unsupported warning option: {flag}"));
                    }
                    self.overrides.push((name.to_owned(), false));
                } else if let Some(name) = flag.strip_prefix("-W") {
                    if !Self::known_group(name) {
                        return Err(format!("unsupported warning option: {flag}"));
                    }
                    self.overrides.push((name.to_owned(), true));
                } else {
                    return Err(format!("not a warning option: {flag}"));
                }
            }
        }
        Ok(())
    }

    pub fn known_group(group: &str) -> bool {
        // Only claim groups with working diagnostic producers. Other
        // groups must not silently turn into no-op compatibility flags.
        matches!(group, "shadow" | "unused-variable" | "unused-parameter" | "unused-but-set-variable" | "conversion" | "sign-conversion" | "sign-compare")
    }

    /// Default enablement is conservative until Mini-C has a tested
    /// implementation of each analysis.
    pub fn enabled(&self, group: &str) -> bool {
        // GCC keeps -Wshadow opt-in. Local-variable and parameter warnings
        // follow -Wall and -Wextra respectively.
        let default = match group {
            "unused-variable" | "unused-but-set-variable" | "sign-compare" => self.wall,
            "unused-parameter" => self.wextra,
            _ => false,
        };
        self.overrides.iter().rev().find(|(name, _)| name == group)
            .map_or(default, |(_, enabled)| *enabled)
    }

    pub fn resolve(&self, report: &ReportedDiagnostic) -> Option<DiagnosticSeverity> {
        match report.severity {
            DiagnosticSeverity::Warning => {
                let group = report.warning_group?;
                if !self.enabled(group) { return None; }
                let promoted = self.error_overrides.iter().rev()
                    .find(|(name, _)| name == group)
                    .map_or(self.warnings_as_errors, |(_, value)| *value);
                Some(if promoted {
                    DiagnosticSeverity::Error
                } else {
                    DiagnosticSeverity::Warning
                })
            }
            other => Some(other),
        }
    }
}

#[cfg(test)]
#[path = "warning_policy_tests.rs"]
mod warning_policy_tests;
