//! C ordinary-identifier namespace used by typedef-aware lexing.
//! Tags and struct members have separate namespaces.
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Binding {
    Typedef,
    #[default]
    Ordinary,
}

#[derive(Debug, Default)]
pub struct Scopes {
    stack: Vec<HashMap<String, Binding>>,
}

impl Scopes {
    pub fn new() -> Self {
        Self {
            stack: vec![HashMap::new()],
        }
    }
    pub fn enter(&mut self) {
        self.stack.push(HashMap::new());
    }
    pub fn leave(&mut self) {
        assert!(self.stack.len() > 1, "cannot leave file scope");
        self.stack.pop();
    }
    pub fn declare(&mut self, name: impl Into<String>, binding: Binding) {
        self.stack
            .last_mut()
            .expect("file scope")
            .insert(name.into(), binding);
    }
    pub(crate) fn restore_file_binding(&mut self, name: &str, previous: Option<Binding>) {
        let file_scope = self.stack.first_mut().expect("file scope");
        match previous {
            Some(binding) => { file_scope.insert(name.to_owned(), binding); }
            None => { file_scope.remove(name); }
        }
    }

    pub fn lookup(&self, name: &str) -> Option<Binding> {
        self.stack
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }
    pub fn is_typedef(&self, name: &str) -> bool {
        self.lookup(name) == Some(Binding::Typedef)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typedef_shadowing() {
        let mut s = Scopes::new();
        s.declare("T", Binding::Typedef);
        assert!(s.is_typedef("T"));
        s.enter();
        s.declare("T", Binding::Ordinary);
        assert!(!s.is_typedef("T"));
        s.leave();
        assert!(s.is_typedef("T"));
    }
}
