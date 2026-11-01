//! Maps output lines from external C preprocessors back to original files.
//! Columns are intentionally not reconstructed across macro expansion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct SourceMap {
    line_starts: Vec<usize>,
    origins: Vec<Origin>,
}

impl SourceMap {
    pub fn from_preprocessed(source: &str, default_file: &str) -> Self {
        let mut line_starts = Vec::new();
        let mut origins = Vec::new();
        let mut file = default_file.to_owned();
        let mut line = 1usize;
        let mut offset = 0;
        for segment in source.split_inclusive('\n') {
            line_starts.push(offset);
            if let Some((new_line, new_file)) = parse_marker(segment) {
                file = new_file;
                line = new_line;
                origins.push(Origin {
                    file: file.clone(),
                    line: 0,
                });
            } else {
                origins.push(Origin {
                    file: file.clone(),
                    line,
                });
                line = line.saturating_add(1);
            }
            offset += segment.len();
        }
        if source.is_empty() || source.ends_with('\n') {
            line_starts.push(offset);
            origins.push(Origin { file, line });
        }
        Self {
            line_starts,
            origins,
        }
    }

    /// Resolve a byte offset to the original file and line. No original column is claimed.
    pub fn origin(&self, byte_offset: usize) -> &Origin {
        let index = self
            .line_starts
            .partition_point(|&start| start <= byte_offset)
            .saturating_sub(1);
        &self.origins[index]
    }
}

/// Recognizes both '# 42 "file" flags' and '#line 42 "file"' directives.
/// Other directives remain ordinary lines; filenames may contain spaces.
fn parse_marker(segment: &str) -> Option<(usize, String)> {
    let mut s = segment.trim_start().strip_prefix('#')?.trim_start();
    if let Some(rest) = s.strip_prefix("line") {
        if !rest.starts_with(char::is_whitespace) {
            return None;
        }
        s = rest.trim_start();
    }
    let digit_count = s.bytes().take_while(u8::is_ascii_digit).count();
    if digit_count == 0 {
        return None;
    }
    let line: usize = s[..digit_count].parse().ok()?;
    s = s[digit_count..].trim_start();
    s = s.strip_prefix('"')?;
    let mut file = String::new();
    let mut chars = s.chars();
    loop {
        match chars.next()? {
            '"' => break,
            '\\' => match chars.next()? {
                '"' => file.push('"'),
                '\\' => file.push('\\'),
                // Unknown escapes are retained rather than silently
                // deleting a character from a diagnostic file path.
                other => {
                    file.push('\\');
                    file.push(other);
                }
            },
            '\n' | '\r' => return None,
            ch => file.push(ch),
        }
    }
    Some((line, file))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gcc_and_line_markers() {
        let src = "# 1 \"a.c\"\nint x;\n# 8 \"header with spaces.h\" 1\nint y;\n#line 20 \"a.c\"\nint z;\n";
        let map = SourceMap::from_preprocessed(src, "unknown");
        assert_eq!(
            map.origin(src.find("int x").unwrap()),
            &Origin {
                file: "a.c".into(),
                line: 1
            }
        );
        assert_eq!(
            map.origin(src.find("int y").unwrap()),
            &Origin {
                file: "header with spaces.h".into(),
                line: 8
            }
        );
        assert_eq!(
            map.origin(src.find("int z").unwrap()),
            &Origin {
                file: "a.c".into(),
                line: 20
            }
        );
    }
    #[test]
    fn escaped_marker_paths_and_line_directives() {
        let src = "# 5 \"dir/with \\\"quotes\\\" and \\\\slash.h\" 1\nint from_header;\n#line 70 \"main file.c\"\nint from_source;\n";
        let map = SourceMap::from_preprocessed(src, "fallback.c");
        assert_eq!(
            map.origin(src.find("int from_header").unwrap()),
            &Origin { file: "dir/with \"quotes\" and \\slash.h".into(), line: 5 },
        );
        assert_eq!(
            map.origin(src.find("int from_source").unwrap()),
            &Origin { file: "main file.c".into(), line: 70 },
        );
        // The diagnostic for a token *at* the end of a physical line
        // belongs to the next line, not the previous declaration.
        assert_eq!(map.origin(src.len()), &Origin {
            file: "main file.c".into(), line: 71,
        });
    }

    #[test]
    fn malformed_markers_do_not_redirect_diagnostics() {
        let src = "#line1 \"fake.c\"\nint a;\n# 3 \"unterminated\nint b;\n";
        let map = SourceMap::from_preprocessed(src, "original.c");
        assert_eq!(map.origin(src.find("int a").unwrap()).file, "original.c");
        assert_eq!(map.origin(src.find("int b").unwrap()).file, "original.c");
        assert_eq!(map.origin(src.find("int b").unwrap()).line, 4);
    }

    #[test]
    fn empty_source_and_no_markers() {
        let map = SourceMap::from_preprocessed("", "empty.c");
        assert_eq!(
            map.origin(0),
            &Origin {
                file: "empty.c".into(),
                line: 1
            }
        );
        let map = SourceMap::from_preprocessed("int a;\nint b;", "raw.c");
        assert_eq!(
            map.origin(7),
            &Origin {
                file: "raw.c".into(),
                line: 2
            }
        );
    }
}
