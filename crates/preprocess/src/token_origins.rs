//! Side table mapping expanded byte offsets to physical token spellings.
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct TokenOrigin {
    pub output_start: usize,
    pub output_end: usize,
    pub source: Arc<str>,
    pub source_start: usize,
    pub source_end: usize,
    pub file: Arc<str>,
    pub line: usize,
}

impl TokenOrigin {
    /// Resolve a range within this token, never across unrelated spellings.
    pub fn excerpt(&self, start: usize, end: usize) -> Option<(&str, usize, usize)> {
        let lo = self.source_start + start.checked_sub(self.output_start)?;
        let hi = self.source_start + end.min(self.output_end).checked_sub(self.output_start)?;
        let before = self.source.get(..lo)?;
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let rest = self.source.get(line_start..)?;
        let line = rest.split('\n').next()?;
        let column = self.source.get(line_start..lo)?.chars().count() + 1;
        let end_column = self.source.get(lo..hi.min(line_start + line.len()))
            .map_or(column, |s| column + s.chars().count());
        Some((line, column, end_column))
    }
}

#[derive(Debug, Default)]
pub struct TokenOrigins(Vec<TokenOrigin>);

impl TokenOrigins {
    pub fn push(&mut self, origin: TokenOrigin) {
        self.0.push(origin);
    }

    pub fn lookup(&self, offset: usize) -> Option<&TokenOrigin> {
        let index = self.0.partition_point(|o| o.output_start <= offset).checked_sub(1)?;
        let origin = &self.0[index];
        (offset < origin.output_end).then_some(origin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_spelling_and_logical_line() {
        let mut origins = TokenOrigins::default();
        origins.push(TokenOrigin {
            output_start: 100, output_end: 103, source: "first\n\tbad;\n".into(),
            source_start: 7, source_end: 10, file: "virtual.c".into(), line: 42,
        });
        let origin = origins.lookup(100).unwrap();
        assert_eq!(origin.excerpt(100, 103), Some(("\tbad;", 2, 5)));
        assert_eq!(origin.line, 42);
        assert!(origins.lookup(103).is_none());
    }
}
