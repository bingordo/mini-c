//! GCC-style source excerpts, independent of diagnostic severity and producer.
//! Columns are one-based Unicode scalar indices, matching SourceFile.
//! Tabs expand to eight-column stops; wide/combining Unicode display widths
//! are not yet modeled.
use std::fmt;

/// Render one original source line and its caret/range. The end column is
/// exclusive. Missing or invalid coordinates produce no misleading caret.
pub fn write_excerpt(
    out: &mut impl fmt::Write,
    source_line: &str,
    line: usize,
    column: Option<usize>,
    end_column: Option<usize>,
) -> fmt::Result {
    let source_line = source_line.trim_end_matches(['\r', '\n']);
    let mut expanded = String::new();
    let mut positions = vec![0usize];
    let mut width = 0;
    for ch in source_line.chars() {
        if ch == '\t' {
            let spaces = 8 - width % 8;
            expanded.extend((0..spaces).map(|_| ' '));
            width += spaces;
        } else {
            expanded.push(ch);
            width += 1;
        }
        positions.push(width);
    }
    let gutter = line.to_string().len().max(5);
    writeln!(out, "{line:>gutter$} | {expanded}")?;
    if let Some(start) = column.and_then(|c| c.checked_sub(1))
        .and_then(|c| positions.get(c)).copied()
    {
        let end = end_column.and_then(|c| c.checked_sub(1))
            .and_then(|c| positions.get(c)).copied().unwrap_or(start + 1);
        let tail = end.saturating_sub(start).saturating_sub(1);
        writeln!(out, "{:gutter$} | {}^{}", "", " ".repeat(start), "~".repeat(tail))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_with_tab_and_line_number() {
        let mut out = String::new();
        write_excerpt(&mut out, "\tbad();\r\n", 42, Some(2), Some(5)).unwrap();
        assert_eq!(out, "   42 |         bad();\n      |         ^~~\n");
    }

    #[test]
    fn missing_column_and_eof() {
        let mut out = String::new();
        write_excerpt(&mut out, "abc", 1, None, None).unwrap();
        write_excerpt(&mut out, "abc", 2, Some(4), None).unwrap();
        assert_eq!(out, "    1 | abc\n    2 | abc\n      |    ^\n");
    }
}
