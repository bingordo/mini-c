//! Opt-in direct internal-CPP -> Mini-C parser/checker integration.
//!
//! This bypasses the complete expanded-C String. The preprocessor's bounded
//! worker produces tokens with Arc-backed spellings; the parser asks for only
//! the next token, keeping typedef classification in the syntax frontend.
//!
//! This is still experimental. SourceMap loads each input/header fully and
//! macro-bearing stretches are buffered by the existing Prosser expander.
use mini_c_preprocess::{self as preprocess, internal_rcc::stream::{self, TokenStream}};
use mini_c_sema::{Checker, Target};
use mini_c_syntax::{
    ast::Span,
    diag::{Diagnostic, DiagnosticCode},
    lexer::{ParserToken, ParserTokenSource, Scanner, TokenText},
    parser::Parser,
    scopes::Scopes,
    stream::ItemSource,
};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone)]
struct Origin {
    start: usize,
    file: Arc<str>,
    line: u32,
}

#[derive(Default)]
struct OriginLines(Vec<Origin>);

impl OriginLines {
    fn record(&mut self, start: usize, file: &Arc<str>, line: u32) {
        if !self.0.last().is_some_and(|previous|
            previous.line == line && previous.file.as_ref() == file.as_ref())
        {
            self.0.push(Origin {
                start, file: Arc::clone(file), line,
            });
        }
    }

    fn display(&self, offset: usize, fallback: &str) -> String {
        let n = self.0.partition_point(|entry| entry.start <= offset);
        match n.checked_sub(1).and_then(|i| self.0.get(i)) {
            Some(origin) => format!("{}:{}", origin.file, origin.line),
            None => format!("{fallback}:1"),
        }
    }
}

struct CppTokens {
    stream: TokenStream,
    pending: VecDeque<ParserToken<'static>>,
    offset: usize,
    origins: Rc<RefCell<OriginLines>>,
}

impl ParserTokenSource<'static> for CppTokens {
    fn next_parser_token(
        &mut self, _scopes: Option<&Scopes>,
    ) -> Result<Option<ParserToken<'static>>, Diagnostic> {
        loop {
            if let Some(token) = self.pending.pop_front() {
                return Ok(Some(token));
            }
            let pp = match self.stream.next() {
                Some(Ok(token)) => token,
                Some(Err(message)) => {
                    return Err(Diagnostic::coded(
                        Span { lo: self.offset, hi: self.offset },
                        DiagnosticCode::Lexical,
                        format!("internal CPP: {message}"),
                    ));
                }
                None => return Ok(None),
            };
            let start = self.offset;
            // One artificial separator between expanded pp-tokens prevents
            // lexing 'a + + b' as 'a++b'. This is a virtual offset only:
            // no complete text buffer is materialized.
            self.offset = self.offset.saturating_add(pp.spelling.len().saturating_add(1));
            let mut lexer = Scanner::new(&pp.spelling);
            let mut offset_in_spelling = 0usize;
            loop {
                let token = lexer.next_token(None).map_err(|mut error| {
                    error.span.lo += start;
                    error.span.hi += start;
                    error
                })?;
                let Some(token) = token else { break; };
                if token.span.lo < offset_in_spelling {
                    return Err(Diagnostic::lexical(
                        Span { lo: start, hi: self.offset },
                        "internal CPP token lexer moved backwards",
                    ));
                }
                offset_in_spelling = token.span.hi;
                let begin = start.saturating_add(token.span.lo);
                let end = start.saturating_add(token.span.hi);
                self.origins.borrow_mut().record(begin, &pp.file, pp.line);
                self.pending.push_back(ParserToken {
                    kind: token.kind,
                    text: TokenText::Shared(Arc::from(token.text)),
                    span: Span { lo: begin, hi: end },
                });
            }
            // Any CPP spelling which is whitespace/comment-only is skipped.
        }
    }
}

pub(super) fn check(
    path: &str, config: &preprocess::Config, max_errors: Option<usize>,
) -> Result<(), String> {
    let stream = stream::start(Path::new(path), &config.args)?;
    let origins = Rc::new(RefCell::new(OriginLines::default()));
    let source = CppTokens {
        stream,
        pending: VecDeque::new(),
        offset: 0,
        origins: Rc::clone(&origins),
    };
    // Unknown end offset is updated from the actually pulled tokens.
    let mut parser = Parser::from_parser_token_source(source, 0)
        .with_expression_spans();
    let mut checker = Checker::new(Target::default())
        .expect("valid default target");
    if let Some(limit) = max_errors {
        let errors = mini_c_sema::diagnose_stream_recovering(
            &mut parser, &mut checker, limit,
        );
        if errors.is_empty() {
            return Ok(());
        }
        return Err(errors.into_iter().map(|error| {
            let location = origins.borrow().display(error.span.lo, path);
            format!("{location}: error[{}]: {}", error.code.as_str(), error.message)
        }).collect::<Vec<_>>().join("\n"));
    }
    loop {
        let checked = parser.next_item().and_then(|item| {
            item.map(|item| checker.check_item(item)).transpose()
        });
        match checked {
            Ok(Some(_)) => {}
            Ok(None) => return Ok(()),
            Err(error) => {
                let location = origins.borrow().display(error.span.lo, path);
                return Err(format!("{location}: error: {}", error.message));
            }
        }
    }
}
