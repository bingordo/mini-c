//! Shared driver diagnostic presentation.
use crate::source_map::SourceMap;
use mini_c_syntax::source::SourceFile;

/// Render original source only. Preprocessor line markers do not establish
/// original columns, so never use expanded-text offsets for their carets.
pub fn render_diagnostic(
    file: &SourceFile<'_>, map: Option<&SourceMap>, origins: &crate::token_origins::TokenOrigins,
    diagnostic: &mini_c_syntax::diag::Diagnostic, level: &str, suffix: &str,
) -> String {
    use mini_c_syntax::diagnostic_excerpt::write_excerpt;
    let (name, line, column, end_column, excerpt) = if let Some((origin, (excerpt, column, end))) = origins.lookup(diagnostic.span.lo)
        .and_then(|origin| origin.excerpt(diagnostic.span.lo, diagnostic.span.hi).map(|excerpt| (origin, excerpt))) {
        (origin.file.as_ref(), origin.line, Some(column), Some(end), Some(excerpt))
    } else if let Some(map) = map {
        let origin = map.origin(diagnostic.span.lo);
        // A logical #line filename may not correspond to a physical file.
        // Do not read arbitrary paths from preprocessor markers.
        (origin.file.as_str(), origin.line, None, None, None)
    } else {
        let (line, column) = file.location(diagnostic.span.lo);
        let (end_line, end_column) = file.location(diagnostic.span.hi);
        (file.name, line, Some(column),
            (line == end_line).then_some(end_column),
            file.line_text(line))
    };
    let location = match column {
        Some(column) => format!("{name}:{line}:{column}"),
        None => format!("{name}:{line}"),
    };
    let mut output = format!("{location}: {level}: {}{suffix}", diagnostic.message);
    if let Some(excerpt) = excerpt {
        output.push('\n');
        write_excerpt(&mut output, excerpt, line, column, end_column)
            .expect("writing to String cannot fail");
        output.pop(); // The caller supplies the final newline.
    }
    let mut frame = origins.lookup(diagnostic.span.lo).and_then(|origin| origin.expansion.as_deref());
    while let Some(expansion) = frame {
        use std::fmt::Write;
        let site = &expansion.invocation;
        if let Some((excerpt, column, end)) = site.excerpt() {
            write!(output, "\n{}:{}:{}: note: in expansion of macro '{}'", site.file, site.line, column, expansion.name).unwrap();
            output.push('\n');
            write_excerpt(&mut output, excerpt, site.line, Some(column), Some(end)).unwrap();
            output.pop();
        } else {
            write!(output, "\n{}:{}: note: in expansion of macro '{}'", site.file, site.line, expansion.name).unwrap();
        }
        frame = expansion.parent.as_deref();
    }
    output
}

