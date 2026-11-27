//! Track lexical regions introduced by variably modified declarations.
//! A goto may leave a region but may not enter it. Declaration IDs are
//! preorder indices; each region's descendants occupy a contiguous range.
//! This avoids copying the set of visible identifiers at every label or jump.
use std::collections::HashMap;
use mini_c_syntax::{ast::Span, diag::Diagnostic};
use crate::types::type_error;

#[derive(Default, Clone)]
pub(crate) struct VmScopes {
    current: usize,
    // Index zero is the root. Each nonroot region records its parent and
    // the exclusive end of the subtree once its lexical scope is closed.
    regions: Vec<(usize, usize)>,
    labels: HashMap<String, usize>,
    jumps: Vec<(String, usize, Span)>,
}

impl VmScopes {
    pub(crate) fn current(&self) -> usize { self.current }

    pub(crate) fn enter_declaration(&mut self) {
        let id = self.regions.len() + 1;
        self.regions.push((self.current, 0));
        self.current = id;
    }

    pub(crate) fn leave_scope(&mut self, saved: usize) {
        let end = self.regions.len() + 1;
        while self.current != saved {
            let region = &mut self.regions[self.current - 1];
            region.1 = end;
            self.current = region.0;
        }
    }

    pub(crate) fn label(&mut self, name: &str) {
        self.labels.insert(name.to_owned(), self.current);
    }

    pub(crate) fn jump(&mut self, target: &str, span: Span) {
        self.jumps.push((target.to_owned(), self.current, span));
    }

    /// Complete forward-goto validation after the entire function has been
    /// checked. A missing checked label can occur after diagnostic recovery;
    /// the original semantic error is reported by the caller.
    pub(crate) fn finish(mut self) -> Result<(), Diagnostic> {
        self.leave_scope(0);
        for (target, source, span) in &self.jumps {
            let Some(&destination) = self.labels.get(target) else { continue; };
            if destination != 0 {
                let end = self.regions[destination - 1].1;
                if !(destination..end).contains(source) {
                    return Err(type_error(*span, format!(
                        "goto to '{target}' enters the scope of a variably modified identifier")));
                }
            }
        }
        Ok(())
    }
}
