//! Warning-only lexical binding use tracking for Mini-C's existing semantic scopes.
//! A separate stack mirrors Checker::locals, so a name reference marks the
//! nearest binding rather than an unrelated declaration with the same spelling.
//! Tracking is opt-in and never changes the checked representation or codegen.

use super::{Checker, Diagnostic, ReportedDiagnostic, Span};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub(super) struct UsageBinding {
    pub span: Span,
    pub referenced: bool,
    pub parameter: bool,
    pub exempt: bool,
}

impl Checker {
    /// Add a use-tracking scope matching the semantic identifier scope.
    pub(super) fn push_warning_scope(&self) {
        if self.track_unused_warnings() {
            self.warning_scopes.borrow_mut().push(HashMap::new());
        }
    }

    /// Whether either currently implemented unused-name analysis is requested.
    fn track_unused_warnings(&self) -> bool {
        self.warning_policy.enabled("unused-variable")
            || self.warning_policy.enabled("unused-parameter")
    }

    /// Register only addressable ordinary declarations, not typedefs or extern aliases.
    pub(super) fn register_warning_binding(&self, name: &str, span: Span, parameter: bool, exempt: bool) {
        if !self.track_unused_warnings() || name.is_empty() { return; }
        if let Some(scope) = self.warning_scopes.borrow_mut().last_mut() {
            scope.insert(name.to_owned(), UsageBinding { span, referenced: false, parameter, exempt });
        }
    }

    /// Mark the innermost visible binding, including inside nested expressions.
    pub(super) fn reference_warning_binding(&self, name: &str) {
        if !self.track_unused_warnings() { return; }
        for scope in self.warning_scopes.borrow_mut().iter_mut().rev() {
            if let Some(binding) = scope.get_mut(name) {
                binding.referenced = true;
                break;
            }
        }
    }

    /// Retire scope-local use records and emit diagnostics for never-referenced bindings.
    pub(super) fn pop_warning_scope(&mut self) {
        if !self.track_unused_warnings() { return; }
        let Some(scope) = self.warning_scopes.get_mut().pop() else { return; };
        let mut bindings: Vec<_> = scope.into_iter().collect();
        bindings.sort_by(|(name_a, a), (name_b, b)| {
            a.span.lo.cmp(&b.span.lo).then_with(|| name_a.cmp(name_b))
        });
        for (name, binding) in bindings {
            if binding.referenced || binding.exempt { continue; }
            let group = if binding.parameter { "unused-parameter" } else { "unused-variable" };
            if !self.warning_policy.enabled(group) { continue; }
            let kind = if binding.parameter { "parameter" } else { "variable" };
            self.emitted_warnings.push(ReportedDiagnostic::warning(
                Diagnostic::semantic(binding.span, format!("unused {kind} '{name}'")),
                group,
            ));
        }
    }
}

#[cfg(test)]
#[path = "warnings_tests.rs"]
mod tests;
