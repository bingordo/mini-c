//! Shared source provenance for diagnostic producers and consumers.
//! Byte spans remain unchanged; producers can attach a compact context ID.
use std::collections::HashMap;
use std::fmt;

/// Original source coordinates, not coordinates in expanded text.
/// Columns are one-based UTF-8 character columns, or unknown.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Location {
    pub file: std::sync::Arc<str>,
    pub line: usize,
    pub column: Option<usize>,
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.file, self.line)?;
        if let Some(column) = self.column {
            write!(f, ":{column}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContextId(usize);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ContextKind {
    Macro { name: std::sync::Arc<str>, definition: Option<Location> },
    Inline { function: std::sync::Arc<str> },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Frame {
    pub kind: ContextKind,
    pub call_site: Location,
    pub parent: Option<ContextId>,
}

/// IDs belong to this arena. Identical frames share storage.
/// The parent must already exist, making the chain acyclic.
#[derive(Debug, Default)]
pub struct Contexts {
    frames: Vec<Frame>,
    ids: HashMap<Frame, ContextId>,
}

impl Contexts {
    pub fn intern(&mut self, frame: Frame) -> ContextId {
        if let Some(parent) = frame.parent {
            assert!(parent.0 < self.frames.len(), "invalid provenance parent");
        }
        if let Some(&id) = self.ids.get(&frame) {
            return id;
        }
        let id = ContextId(self.frames.len());
        self.frames.push(frame.clone());
        self.ids.insert(frame, id);
        id
    }

    /// Walk from the innermost context to the outermost.
    pub fn stack(&self, mut id: Option<ContextId>) -> impl Iterator<Item = &Frame> {
        std::iter::from_fn(move || {
            let frame = self.frames.get(id?.0)?;
            id = frame.parent;
            Some(frame)
        })
    }

    /// Append notes without changing diagnostic codes or severity.
    pub fn write_notes(&self, out: &mut impl fmt::Write, id: Option<ContextId>) -> fmt::Result {
        for frame in self.stack(id) {
            match &frame.kind {
                ContextKind::Macro { name, definition } => {
                    writeln!(out, "{}: note: expanded from macro '{}'", frame.call_site, name)?;
                    if let Some(definition) = definition {
                        writeln!(out, "{definition}: note: macro '{name}' defined here")?;
                    }
                }
                ContextKind::Inline { function } => {
                    writeln!(out, "{}: note: inlined from '{}'", frame.call_site, function)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn location(line: usize, column: Option<usize>) -> Location {
        Location { file: "test.c".into(), line, column }
    }

    #[test]
    fn nested_mixed_contexts_and_unknown_columns() {
        let mut contexts = Contexts::default();
        let outer = Frame {
            kind: ContextKind::Inline { function: "helper".into() },
            call_site: location(20, Some(4)),
            parent: None,
        };
        let id = contexts.intern(outer.clone());
        assert_eq!(contexts.intern(outer), id);
        let inner = contexts.intern(Frame {
            kind: ContextKind::Macro { name: "BAD".into(), definition: Some(location(1, Some(9))) },
            call_site: location(8, None),
            parent: Some(id),
        });
        let mut output = String::new();
        contexts.write_notes(&mut output, Some(inner)).unwrap();
        assert_eq!(output, "test.c:8: note: expanded from macro 'BAD'\ntest.c:1:9: note: macro 'BAD' defined here\ntest.c:20:4: note: inlined from 'helper'\n");
        assert_eq!(contexts.stack(None).count(), 0);
    }
}
