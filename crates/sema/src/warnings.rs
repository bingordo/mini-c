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
    /// Number of ordinary references, excluding the LHS of a plain assignment.
    pub reads: usize,
    /// Number of successful plain assignments to this binding.
    pub writes: usize,
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
            || self.warning_policy.enabled("unused-but-set-variable")
    }

    /// Register only addressable ordinary declarations, not typedefs or extern aliases.
    pub(super) fn register_warning_binding(&self, name: &str, span: Span, parameter: bool, exempt: bool) {
        if !self.track_unused_warnings() || name.is_empty() { return; }
        if let Some(scope) = self.warning_scopes.borrow_mut().last_mut() {
            scope.insert(name.to_owned(), UsageBinding { span, referenced: false, reads: 0, writes: 0, parameter, exempt });
        }
    }

    /// Mark the innermost visible binding, including inside nested expressions.
    pub(super) fn reference_warning_binding(&self, name: &str) {
        if !self.track_unused_warnings() { return; }
        for scope in self.warning_scopes.borrow_mut().iter_mut().rev() {
            if let Some(binding) = scope.get_mut(name) {
                binding.referenced = true;
                binding.reads += 1;
                break;
            }
        }
    }

    /// Reclassify the checked name reference on a plain assignment's LHS
    /// as a write. The expression checker has already visited the LHS once.
    pub(super) fn write_warning_binding(&self, name: &str) {
        if !self.track_unused_warnings() { return; }
        for scope in self.warning_scopes.borrow_mut().iter_mut().rev() {
            if let Some(binding) = scope.get_mut(name) {
                binding.reads = binding.reads.saturating_sub(1);
                binding.writes += 1;
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
            if binding.exempt { continue; }
            let (group, message) = if !binding.referenced {
                if binding.parameter {
                    ("unused-parameter", format!("unused parameter '{name}'"))
                } else {
                    ("unused-variable", format!("unused variable '{name}'"))
                }
            } else if !binding.parameter && binding.writes > 0 && binding.reads == 0 {
                ("unused-but-set-variable", format!("variable '{name}' set but not used"))
            } else {
                continue;
            };
            if !self.warning_policy.enabled(group) { continue; }
            self.emitted_warnings.push(ReportedDiagnostic::warning(
                Diagnostic::semantic(binding.span, message), group,
            ));
        }
    }
}

#[cfg(test)]
#[path = "warnings_tests.rs"]
mod tests;
