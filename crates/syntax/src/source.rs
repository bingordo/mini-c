//! Byte-based source positions, with line/column conversion for diagnostics.
#[derive(Debug, Clone)]
pub struct SourceFile<'a> {
    pub name: &'a str,
    pub text: &'a str,
    line_starts: Vec<usize>,
}
impl<'a> SourceFile<'a> {
    pub fn new(name: &'a str, text: &'a str) -> Self {
        let mut line_starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self {
            name,
            text,
            line_starts,
        }
    }
    /// Returns 1-based line and UTF-8 character column for a byte offset.
    pub fn location(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.text.len());
        let mut offset = offset;
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        let line_index = self
            .line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);
        let start = self.line_starts[line_index];
        (line_index + 1, self.text[start..offset].chars().count() + 1)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lines_and_utf8() {
        let f = SourceFile::new("x.c", "a\nβz");
        assert_eq!(f.location(0), (1, 1));
        assert_eq!(f.location(2), (2, 1));
        assert_eq!(f.location(4), (2, 2));
    }
}
