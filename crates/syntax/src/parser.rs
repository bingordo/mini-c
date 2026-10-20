//! Incremental parser for the first supported subset of external declarations.
//! Function bodies are intentionally not accepted until statement parsing lands.
use crate::ast::{Decl, Function, Span, Stmt, Type};
use crate::diag::Diagnostic;
use crate::lexer::{Scanner, Token};
use crate::stream::{ExternalItem, ItemSource};

pub struct Parser<'a> {
    scanner: Scanner<'a>,
    lookahead: Option<Token<'a>>,
}

impl<'a> Parser<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            scanner: Scanner::new(source),
            lookahead: None,
        }
    }

    fn peek(&mut self) -> Result<Option<Token<'a>>, Diagnostic> {
        if self.lookahead.is_none() {
            self.lookahead = self.scanner.next_token(None)?;
        }
        Ok(self.lookahead)
    }

    fn take(&mut self) -> Result<Option<Token<'a>>, Diagnostic> {
        self.peek()?;
        Ok(self.lookahead.take())
    }

    fn expect(&mut self, expected: &str) -> Result<Token<'a>, Diagnostic> {
        match self.take()? {
            Some(token) if token.text == expected => Ok(token),
            Some(token) => Err(Diagnostic::new(
                token.span,
                format!("expected '{expected}', found '{}'", token.text),
            )),
            None => Err(Diagnostic::new(
                Span::default(),
                format!("expected '{expected}', found end of input"),
            )),
        }
    }

    fn parse_block(&mut self) -> Result<(Stmt, usize), Diagnostic> {
        self.expect("{")?;
        let mut statements = Vec::new();
        loop {
            let Some(token) = self.peek()? else {
                return Err(Diagnostic::new(Span::default(), "unterminated function body"));
            };
            match token.text {
                "}" => return Ok((Stmt::Block(statements), self.take()?.expect("peeked token").span.hi)),
                ";" => { self.take()?; statements.push(Stmt::Expr(None)); }
                "return" => {
                    self.take()?;
                    if self.peek()?.is_some_and(|t| t.text == ";") {
                        self.take()?;
                        statements.push(Stmt::Return(None));
                    } else {
                        return Err(Diagnostic::new(token.span, "return expressions are not implemented yet"));
                    }
                }
                _ => return Err(Diagnostic::new(token.span, "unsupported statement in function body")),
            }
        }
    }

    fn parse_type(&mut self) -> Result<Option<(Type, usize)>, Diagnostic> {
        let Some(token) = self.peek()? else {
            return Ok(None);
        };
        let ty = match token.text {
            "void" => Type::Void,
            "char" => Type::Char,
            "int" => Type::Int,
            "long" => Type::Long,
            _ => return Err(Diagnostic::new(token.span, "expected a type specifier")),
        };
        self.take()?;
        Ok(Some((ty, token.span.lo)))
    }
}

impl ItemSource for Parser<'_> {
    type Error = Diagnostic;

    fn next_item(&mut self) -> Result<Option<ExternalItem>, Self::Error> {
        let Some((mut ty, start)) = self.parse_type()? else {
            return Ok(None);
        };
        while self.peek()?.is_some_and(|token| token.text == "*") {
            self.take()?;
            ty = Type::Pointer(Box::new(ty));
        }
        let name = self
            .take()?
            .ok_or_else(|| Diagnostic::new(Span::default(), "expected identifier"))?;
        if name.kind != crate::lexer::TokenKind::Identifier {
            return Err(Diagnostic::new(name.span, "expected identifier"));
        }
        if self.peek()?.is_some_and(|token| token.text == "(") {
            return Err(Diagnostic::new(
                name.span,
                "function declarators are not implemented yet",
            ));
        }
        let end = self.expect(";")?.span.hi;
        Ok(Some(ExternalItem::Declaration(Decl {
            name: name.text.to_owned(),
            ty,
            init: None,
            span: Span { lo: start, hi: end },
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::{consume, ItemSink};

    #[derive(Default)]
    struct Collector(Vec<Decl>);
    impl ItemSink for Collector {
        type Error = ();
        fn declaration(&mut self, declaration: Decl) -> Result<(), Self::Error> {
            self.0.push(declaration);
            Ok(())
        }
        fn function(&mut self, _: crate::ast::Function) -> Result<(), Self::Error> {
            unreachable!()
        }
    }

    #[test]
    fn streams_global_declarations() {
        let mut parser = Parser::new("int a; char *b; long c;");
        let mut sink = Collector::default();
        consume(&mut parser, &mut sink).unwrap();
        assert_eq!(
            sink.0.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
        assert_eq!(sink.0[0].span, Span { lo: 0, hi: 6 });
        assert_eq!(sink.0[1].ty, Type::Pointer(Box::new(Type::Char)));
    }

    #[test]
    fn streams_function_definitions() {
        let mut parser = Parser::new("void first() { return; } int x; int second() { ; }");
        let first = parser.next_item().unwrap().unwrap();
        match first {
            ExternalItem::Function(f) => {
                assert_eq!(f.name, "first");
                assert_eq!(f.body, Stmt::Block(vec![Stmt::Return(None)]));
            }
            _ => panic!("expected function"),
        }
        assert!(matches!(parser.next_item().unwrap(), Some(ExternalItem::Declaration(_))));
        match parser.next_item().unwrap().unwrap() {
            ExternalItem::Function(f) => assert_eq!(f.name, "second"),
            _ => panic!("expected function"),
        }
        assert!(parser.next_item().unwrap().is_none());
    }

    #[test]
    fn rejects_unsupported_declarators() {
        let mut parser = Parser::new("int f();");
        assert!(parser
            .next_item()
            .unwrap_err()
            .message
            .contains("not implemented"));
    }
}
