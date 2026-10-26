//! Incremental parser for the first supported subset of external declarations.
//! Each complete function body is returned separately; earlier bodies are not retained.
use crate::ast::{BinOp, Decl, Expr, Function, Span, Stmt, StorageClass, Type, UnOp};
use crate::diag::Diagnostic;
use crate::lexer::{Scanner, Token, TokenKind};
use crate::stream::{ExternalItem, ItemSource};
use crate::scopes::{Binding, Scopes};

pub struct Parser<'a> {
    scanner: Scanner<'a>,
    input_len: usize,
    lookahead: Option<Token<'a>>,
    scopes: Scopes,
    brace_depth: usize,
    pending_file_binding: Option<(String, Option<Binding>)>,
}

impl<'a> Parser<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            scanner: Scanner::new(source),
            input_len: source.len(),
            lookahead: None,
            scopes: Scopes::new(),
            brace_depth: 0,
            pending_file_binding: None,
        }
    }

    // A missing token belongs at the actual end of input, not byte zero.
    // In particular this must handle UTF-8 source preceding a truncated item.
    fn eof_span(&self) -> Span {
        Span { lo: self.input_len, hi: self.input_len }
    }

    /// Skip a damaged external item after `next_item` has reported an error.
    /// This is opt-in: ordinary users of `ItemSource` remain fail-fast.
    ///
    /// Resynchronize at a top-level semicolon or a function's closing brace.
    /// A missing semicolon followed by a new type specifier may be repaired
    /// without consuming the following declaration.
    fn rollback_last_file_binding(&mut self) {
        if let Some((name, previous)) = self.pending_file_binding.take() {
            self.scopes.restore_file_binding(&name, previous);
        }
    }

    fn recover_to_next_item(&mut self, error: &Diagnostic) -> bool {
        // A malformed typedef must not poison future typedef classification.
        self.rollback_last_file_binding();
        if self.brace_depth == 0
            && error.message.starts_with("expected ';'")
            && self.peek().ok().flatten().is_some_and(|t| matches!(
                t.text, "int" | "char" | "long" | "void" | "signed" |
                    "unsigned" | "static" | "extern" | "typedef"
            ) || t.kind == TokenKind::TypedefName)
        {
            return true;
        }
        loop {
            // Lexical errors advance the underlying scanner; discard stray
            // invalid characters while synchronizing to avoid cascades.
            let token = match self.take() {
                Ok(Some(token)) => token,
                Ok(None) => return false,
                Err(_) => continue,
            };
            if self.brace_depth == 0 && matches!(token.text, ";" | "}") {
                return true;
            }
        }
    }

    /// Deliver valid external declarations while collecting up to `max_errors`
    /// syntax/lexer errors, without retaining function ASTs.
    pub fn for_each_item_recovering<F>(
        &mut self, max_errors: usize, mut receive: F
    ) -> Vec<Diagnostic>
    where
        F: FnMut(ExternalItem),
    {
        self.for_each_item_with_diagnostics(max_errors, |item| {
            receive(item);
            Ok(())
        })
    }

    /// Streaming syntax and consumer-error recovery, bounded by `max_errors`.
    ///
    /// A failing consumer must roll back its own state (e.g., by using the
    /// semantic checker's transactional entry point). This method restores
    /// parser typedef bindings for failed consumer items.
    pub fn for_each_item_with_diagnostics<F>(
        &mut self, max_errors: usize, mut receive: F
    ) -> Vec<Diagnostic>
    where
        F: FnMut(ExternalItem) -> Result<(), Diagnostic>,
    {
        let mut errors = Vec::new();
        if max_errors == 0 { return errors; }
        loop {
            match self.next_item() {
                Ok(Some(item)) => {
                    if let Err(error) = receive(item) {
                        self.rollback_last_file_binding();
                        errors.push(error);
                        if errors.len() >= max_errors { break; }
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let can_resume = if errors.len() + 1 < max_errors {
                        self.recover_to_next_item(&error)
                    } else {
                        false
                    };
                    errors.push(error);
                    if !can_resume { break; }
                }
            }
        }
        errors
    }

    fn peek(&mut self) -> Result<Option<Token<'a>>, Diagnostic> {
        if self.lookahead.is_none() {
            self.lookahead = self.scanner.next_token(Some(&self.scopes))?;
        }
        // Scope changes can occur while a token is held in lookahead.
        if let Some(token) = self.lookahead.as_mut() {
            if matches!(token.kind, TokenKind::Identifier | TokenKind::TypedefName) {
                token.kind = if self.scopes.is_typedef(token.text) {
                    TokenKind::TypedefName
                } else {
                    TokenKind::Identifier
                };
            }
        }
        Ok(self.lookahead)
    }

    fn take(&mut self) -> Result<Option<Token<'a>>, Diagnostic> {
        self.peek()?;
        let next = self.lookahead.take();
        if let Some(token) = next {
            match token.text {
                "{" => self.brace_depth += 1,
                "}" => self.brace_depth = self.brace_depth.saturating_sub(1),
                _ => {}
            }
        }
        Ok(next)
    }

    fn expect(&mut self, expected: &str) -> Result<Token<'a>, Diagnostic> {
        // Do not consume a mismatching token: the recovery API needs to see
        // structural delimiters to resume at the next external declaration.
        match self.peek()? {
            Some(token) if token.text == expected =>
                Ok(self.take()?.expect("matching token was peeked")),
            Some(token) => Err(Diagnostic::new(
                token.span,
                format!("expected '{expected}', found '{}'", token.text),
            )),
            None => Err(Diagnostic::new(
                self.eof_span(),
                format!("expected '{expected}', found end of input"),
            )),
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, Diagnostic> {
        self.parse_assign()
    }

    fn parse_assign(&mut self) -> Result<Expr, Diagnostic> {
        let lhs = self.parse_binary(0)?;
        if self.peek()?.is_some_and(|t| t.text == "=") {
            self.take()?;
            return Ok(Expr::Assign(Box::new(lhs), Box::new(self.parse_assign()?)));
        }
        Ok(lhs)
    }

    fn binary_op(text: &str) -> Option<(BinOp, u8)> {
        Some(match text {
            "||" => (BinOp::Or, 1),
            "&&" => (BinOp::And, 2),
            "==" => (BinOp::Eq, 3),
            "!=" => (BinOp::Ne, 3),
            "<" => (BinOp::Lt, 4),
            "<=" => (BinOp::Le, 4),
            ">" => (BinOp::Gt, 4),
            ">=" => (BinOp::Ge, 4),
            "+" => (BinOp::Add, 5),
            "-" => (BinOp::Sub, 5),
            "*" => (BinOp::Mul, 6),
            "/" => (BinOp::Div, 6),
            "%" => (BinOp::Mod, 6),
            _ => return None,
        })
    }

    fn parse_binary(&mut self, min_prec: u8) -> Result<Expr, Diagnostic> {
        let mut lhs = self.parse_unary()?;
        while let Some((op, prec)) = self.peek()?.and_then(|t| Self::binary_op(t.text)) {
            if prec < min_prec {
                break;
            }
            self.take()?;
            let rhs = self.parse_binary(prec + 1)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, Diagnostic> {
        if let Some(op) = self.peek()?.and_then(|t| match t.text {
            "+" => Some(UnOp::Plus),
            "-" => Some(UnOp::Neg),
            "!" => Some(UnOp::Not),
            "&" => Some(UnOp::Address),
            "*" => Some(UnOp::Deref),
            _ => None,
        }) {
            self.take()?;
            return Ok(Expr::Unary(op, Box::new(self.parse_unary()?)));
        }
        // Keep synchronization tokens available to the recovery path.
        if let Some(token) = self.peek()? {
            if matches!(token.text, ";" | "}") {
                return Err(Diagnostic::new(token.span, "expected expression"));
            }
        }
        let mut expr = match self.take()? {
            Some(t) if t.kind == TokenKind::Number => Expr::Int(
                t.text
                    .parse()
                    .map_err(|_| Diagnostic::new(t.span, "integer literal overflow"))?,
            ),
            Some(t) if matches!(t.kind, TokenKind::Identifier | TokenKind::TypedefName) => Expr::Name(t.text.to_owned()),
            Some(t) if t.text == "(" => {
                let e = self.parse_expr()?;
                self.expect(")")?;
                e
            }
            Some(t) => return Err(Diagnostic::new(t.span, "expected expression")),
            None => return Err(Diagnostic::new(self.eof_span(), "expected expression")),
        };
        while self.peek()?.is_some_and(|t| t.text == "(") {
            self.take()?;
            let mut args = Vec::new();
            if !self.peek()?.is_some_and(|t| t.text == ")") {
                loop {
                    args.push(self.parse_expr()?);
                    if !self.peek()?.is_some_and(|t| t.text == ",") {
                        break;
                    }
                    self.take()?;
                }
            }
            self.expect(")")?;
            expr = Expr::Call(Box::new(expr), args);
        }
        Ok(expr)
    }

    fn parse_decl(&mut self, is_typedef: bool) -> Result<Decl, Diagnostic> {
        let storage = match self.peek()?.map(|t| t.text) {
            Some("static") => { self.take()?; StorageClass::Static },
            Some("extern") => { self.take()?; StorageClass::Extern },
            _ => StorageClass::Default,
        };
        if is_typedef && storage != StorageClass::Default {
            return Err(Diagnostic::new(self.eof_span(), "typedef cannot combine with storage class"));
        }
        let (mut ty, start) = self
            .parse_type()?
            .ok_or_else(|| Diagnostic::new(self.eof_span(), "expected type"))?;
        while self.peek()?.is_some_and(|t| t.text == "*") {
            self.take()?;
            ty = Type::Pointer(Box::new(ty));
        }
        let name = self
            .take()?
            .ok_or_else(|| Diagnostic::new(self.eof_span(), "expected identifier"))?;
        if !matches!(name.kind, TokenKind::Identifier | TokenKind::TypedefName) {
            return Err(Diagnostic::new(name.span, "expected identifier"));
        }
        self.scopes.declare(name.text, if is_typedef { Binding::Typedef } else { Binding::Ordinary });
        let init = if self.peek()?.is_some_and(|t| t.text == "=") {
            self.take()?;
            Some(self.parse_expr()?)
        } else {
            None
        };
        if is_typedef && init.is_some() {
            return Err(Diagnostic::new(name.span, "typedef cannot have an initializer"));
        }
        let end = self.expect(";")?.span.hi;
        Ok(Decl {
            storage,
            name: name.text.to_owned(),
            ty,
            init,
            span: Span { lo: start, hi: end },
        })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let token = self.peek()?.ok_or_else(|| Diagnostic::new(self.eof_span(), "expected statement"))?;
        match token.text {
            "{" => Ok(self.parse_block()?.0),
            ";" => { self.take()?; Ok(Stmt::Expr(None)) },
            "return" => {
                self.take()?;
                let value = if self.peek()?.is_some_and(|t| t.text == ";") { None } else { Some(self.parse_expr()?) };
                self.expect(";")?;
                Ok(Stmt::Return(value))
            }
            "if" => {
                self.take()?; self.expect("(")?;
                let cond = self.parse_expr()?;
                self.expect(")")?;
                let then = Box::new(self.parse_stmt()?);
                let otherwise = if self.peek()?.is_some_and(|t| t.text == "else") {
                    self.take()?; Some(Box::new(self.parse_stmt()?))
                } else { None };
                Ok(Stmt::If { cond, then, otherwise })
            }
            "while" => {
                self.take()?; self.expect("(")?;
                let cond = self.parse_expr()?;
                self.expect(")")?;
                let body = Box::new(self.parse_stmt()?);
                Ok(Stmt::While { cond, body })
            }
            "break" => { self.take()?; self.expect(";")?; Ok(Stmt::Break) }
            "continue" => { self.take()?; self.expect(";")?; Ok(Stmt::Continue) }
            "typedef" => { self.take()?; Ok(Stmt::Typedef(self.parse_decl(true)?)) },
            "static" | "extern" => Ok(Stmt::Decl(self.parse_decl(false)?)),
            "int" | "char" | "long" | "void" | "unsigned" | "signed" => Ok(Stmt::Decl(self.parse_decl(false)?)),
            _ if token.kind == TokenKind::TypedefName => Ok(Stmt::Decl(self.parse_decl(false)?)),
            _ => { let expr = self.parse_expr()?; self.expect(";")?; Ok(Stmt::Expr(Some(expr))) }
        }
    }

    fn parse_block(&mut self) -> Result<(Stmt, usize), Diagnostic> {
        self.expect("{")?;
        self.scopes.enter();
        // Restore the scope even when parsing fails. This keeps typedef
        // classification from leaking from an incomplete nested block.
        let parsed = (|| {
            let mut statements = Vec::new();
            loop {
                let Some(token) = self.peek()? else {
                    return Err(Diagnostic::new(
                        self.eof_span(),
                        "unterminated function body",
                    ));
                };
                if token.text == "}" {
                    let end = self.take()?.expect("peeked token").span.hi;
                    return Ok((Stmt::Block(statements), end));
                }
                statements.push(self.parse_stmt()?);
            }
        })();
        self.scopes.leave();
        parsed
    }

    fn parse_type(&mut self) -> Result<Option<(Type, usize)>, Diagnostic> {
        let Some(token) = self.peek()? else {
            return Ok(None);
        };
        let ty = match token.text {
            "void" => { self.take()?; Type::Void }
            "char" => { self.take()?; Type::Char }
            "int" => { self.take()?; Type::Int }
            "long" => {
                self.take()?;
                if self.peek()?.is_some_and(|t| t.text == "int") { self.take()?; }
                Type::Long
            }
            "unsigned" | "signed" => {
                let unsigned = token.text == "unsigned";
                self.take()?;
                let base = match self.peek()?.map(|t| t.text) {
                    Some("char") => { self.take()?; Type::Char },
                    Some("int") => { self.take()?; Type::Int },
                    Some("long") => {
                        self.take()?;
                        if self.peek()?.is_some_and(|t| t.text == "int") { self.take()?; }
                        Type::Long
                    }
                    _ => Type::Int,
                };
                if unsigned { Type::Unsigned(Box::new(base)) } else { Type::Signed(Box::new(base)) }
            }
            _ if token.kind == TokenKind::TypedefName => {
                self.take()?;
                Type::Alias(token.text.to_owned())
            }
            _ => return Err(Diagnostic::new(token.span, "expected a type specifier")),
        };
        Ok(Some((ty, token.span.lo)))
    }
}

impl ItemSource for Parser<'_> {
    type Error = Diagnostic;

    fn next_item(&mut self) -> Result<Option<ExternalItem>, Self::Error> {
        self.pending_file_binding = None;
        let is_typedef = self.peek()?.is_some_and(|t| t.text == "typedef");
        if is_typedef { self.take()?; }
        let storage = match self.peek()?.map(|t| t.text) {
            Some("static") => { self.take()?; StorageClass::Static },
            Some("extern") => { self.take()?; StorageClass::Extern },
            _ => StorageClass::Default,
        };
        if is_typedef && storage != StorageClass::Default {
            return Err(Diagnostic::new(self.eof_span(), "typedef cannot combine with storage class"));
        }
        let Some((mut ty, start)) = self.parse_type()? else {
            // EOF is only a successful end of the translation unit before any
            // declaration specifier has been consumed.
            if is_typedef || storage != StorageClass::Default {
                return Err(Diagnostic::new(self.eof_span(), "expected type specifier"));
            }
            return Ok(None);
        };
        while self.peek()?.is_some_and(|token| token.text == "*") {
            self.take()?;
            ty = Type::Pointer(Box::new(ty));
        }
        let name = self
            .take()?
            .ok_or_else(|| Diagnostic::new(self.eof_span(), "expected identifier"))?;
        if !matches!(name.kind, TokenKind::Identifier | TokenKind::TypedefName) {
            return Err(Diagnostic::new(name.span, "expected identifier"));
        }
        self.pending_file_binding = Some((name.text.to_owned(), self.scopes.lookup(name.text)));
        self.scopes.declare(name.text, if is_typedef { Binding::Typedef } else { Binding::Ordinary });
        if self.peek()?.is_some_and(|token| token.text == "(") {
            self.take()?;
            let mut params = Vec::new();
            if !self.peek()?.is_some_and(|t| t.text == ")") {
                loop {
                    let (mut param_ty, lo) = self
                        .parse_type()?
                        .ok_or_else(|| Diagnostic::new(self.eof_span(), "expected parameter type"))?;
                    while self.peek()?.is_some_and(|t| t.text == "*") {
                        self.take()?;
                        param_ty = Type::Pointer(Box::new(param_ty));
                    }
                    if param_ty == Type::Void && params.is_empty()
                        && self.peek()?.is_some_and(|t| t.text == ")")
                    {
                        break; // The single (void) parameter list means no parameters.
                    }
                    if param_ty == Type::Void && params.is_empty() && self.peek()?.is_none() {
                        return Err(Diagnostic::new(self.eof_span(), "expected ')'"));
                    }
                    if param_ty == Type::Void {
                        return Err(Diagnostic::new(name.span, "parameter cannot have void type"));
                    }
                    // Names are optional in declarations, required in definitions.
                    let param_name = if self.peek()?.is_some_and(|t|
                        matches!(t.kind, TokenKind::Identifier | TokenKind::TypedefName))
                    {
                        Some(self.take()?.expect("looked ahead"))
                    } else {
                        None
                    };
                    let (param_name, hi) = match param_name {
                        Some(token) => (token.text.to_owned(), token.span.hi),
                        None => (String::new(), lo),
                    };
                    params.push(Decl {
                        storage: StorageClass::Default,
                        name: param_name,
                        ty: param_ty,
                        init: None,
                        span: Span { lo, hi },
                    });
                    if !self.peek()?.is_some_and(|t| t.text == ",") {
                        break;
                    }
                    self.take()?;
                }
            }
            self.expect(")")?;
            if self.peek()?.is_some_and(|token| token.text == "{") {
                if is_typedef {
                    return Err(Diagnostic::new(name.span, "typedef cannot define a function body"));
                }
                if params.iter().any(|p| p.name.is_empty()) {
                    return Err(Diagnostic::new(name.span, "function definition requires parameter names"));
                }
                self.scopes.enter();
                for param in &params { self.scopes.declare(&param.name, Binding::Ordinary); }
                let parsed = self.parse_block();
                self.scopes.leave();
                let (body, end) = parsed?;
                return Ok(Some(ExternalItem::Function(Function {
                    storage,
                    name: name.text.to_owned(),
                    result: ty,
                    params,
                    body,
                    span: Span { lo: start, hi: end },
                })));
            }
            let end = self.expect(";")?.span.hi;
            let decl = Decl {
                storage,
                name: name.text.to_owned(),
                ty: Type::Function {
                    result: Box::new(ty),
                    params: params.into_iter().map(|p| p.ty).collect(),
                },
                init: None,
                span: Span { lo: start, hi: end },
            };
            return Ok(Some(if is_typedef {
                ExternalItem::Typedef(decl)
            } else {
                ExternalItem::Declaration(decl)
            }));
        }
        let init = if self.peek()?.is_some_and(|t| t.text == "=") { self.take()?; Some(self.parse_expr()?) } else { None };
        if is_typedef && init.is_some() { return Err(Diagnostic::new(name.span, "typedef cannot have an initializer")); }
        let end = self.expect(";")?.span.hi;
        let decl = Decl { storage, name: name.text.to_owned(), ty, init, span: Span { lo: start, hi: end } };
        Ok(Some(if is_typedef { ExternalItem::Typedef(decl) } else { ExternalItem::Declaration(decl) }))
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
        assert!(matches!(
            parser.next_item().unwrap(),
            Some(ExternalItem::Declaration(_))
        ));
        match parser.next_item().unwrap().unwrap() {
            ExternalItem::Function(f) => assert_eq!(f.name, "second"),
            _ => panic!("expected function"),
        }
        assert!(parser.next_item().unwrap().is_none());
    }

    #[test]
    fn nested_blocks_and_unterminated_body() {
        let mut parser = Parser::new("void f() { { return; } ; }");
        match parser.next_item().unwrap().unwrap() {
            ExternalItem::Function(f) => assert_eq!(
                f.body,
                Stmt::Block(vec![
                    Stmt::Block(vec![Stmt::Return(None)]),
                    Stmt::Expr(None),
                ])
            ),
            _ => panic!("expected function"),
        }
        let mut parser = Parser::new("void f() { { return; }");
        assert!(parser
            .next_item()
            .unwrap_err()
            .message
            .contains("unterminated"));
    }

    #[test]
    fn parses_parameters_locals_and_expressions() {
        let mut parser =
            Parser::new("int add(int a, int b) { int sum = a + b * 2; return sum; } int z;");
        match parser.next_item().unwrap().unwrap() {
            ExternalItem::Function(f) => {
                assert_eq!(f.params.len(), 2);
                assert_eq!(f.params[0].name, "a");
                match f.body {
                    Stmt::Block(stmts) => {
                        assert_eq!(stmts.len(), 2);
                        assert!(matches!(
                            &stmts[0],
                            Stmt::Decl(Decl {
                                init: Some(Expr::Binary(BinOp::Add, _, _)),
                                ..
                            })
                        ));
                        assert_eq!(stmts[1], Stmt::Return(Some(Expr::Name("sum".into()))));
                    }
                    _ => panic!("expected block"),
                }
            }
            _ => panic!("expected function"),
        }
        assert!(matches!(
            parser.next_item().unwrap(),
            Some(ExternalItem::Declaration(_))
        ));
    }

    #[test]
    fn parses_control_flow_and_global_initializer() {
        let mut parser = Parser::new("int seed = 7; int f(int n) { while (n) { if (n == 1) break; else n = n - 1; continue; } return n; }");
        match parser.next_item().unwrap().unwrap() {
            ExternalItem::Declaration(d) => assert_eq!(d.init, Some(Expr::Int(7))),
            _ => panic!("expected declaration"),
        }
        match parser.next_item().unwrap().unwrap() {
            ExternalItem::Function(f) => match f.body {
                Stmt::Block(stmts) => {
                    assert!(matches!(&stmts[0], Stmt::While { .. }));
                    assert!(matches!(&stmts[1], Stmt::Return(Some(_))));
                }
                _ => panic!("expected block"),
            },
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn registers_identifiers_before_initializers() {
        let mut parser = Parser::new("int x = x + 1; int f(void) { int y = y + 1; return y; }");
        assert!(matches!(parser.next_item().unwrap(), Some(ExternalItem::Declaration(_))));
        assert_eq!(parser.scopes.lookup("x"), Some(Binding::Ordinary));
        assert!(matches!(parser.next_item().unwrap(), Some(ExternalItem::Function(_))));
        assert_eq!(parser.scopes.lookup("y"), None);
        assert_eq!(parser.scopes.lookup("f"), Some(Binding::Ordinary));
    }

    #[test]
    fn parses_typedefs_and_shadowing() {
        let mut parser = Parser::new("typedef int T; T global; int f(void) { T x; { int T; T = 2; } T y; return 0; }");
        assert!(matches!(parser.next_item().unwrap(), Some(ExternalItem::Typedef(_))));
        match parser.next_item().unwrap().unwrap() {
            ExternalItem::Declaration(d) => assert_eq!(d.ty, Type::Alias("T".into())),
            _ => panic!("expected typedef-typed declaration"),
        }
        assert!(matches!(parser.next_item().unwrap(), Some(ExternalItem::Function(_))));
        assert!(parser.next_item().unwrap().is_none());
    }

    #[test]
    fn tracks_ordinary_identifier_scopes() {
        let mut parser = Parser::new("int f(int a) { int b = a; { int a = 3; } return a; } int a;");
        assert!(matches!(parser.next_item().unwrap(), Some(ExternalItem::Function(_))));
        assert!(matches!(parser.next_item().unwrap(), Some(ExternalItem::Declaration(_))));
        assert_eq!(parser.scopes.lookup("a"), Some(Binding::Ordinary));
        assert_eq!(parser.scopes.lookup("b"), None);
    }

    #[test]
    fn parses_function_prototypes() {
        let mut parser = Parser::new("int f(); long g(int a, char *p); int h(void);");
        match parser.next_item().unwrap().unwrap() {
            ExternalItem::Declaration(d) => assert_eq!(d.ty, Type::Function {
                result: Box::new(Type::Int), params: vec![],
            }),
            _ => panic!("expected prototype"),
        }
        match parser.next_item().unwrap().unwrap() {
            ExternalItem::Declaration(d) => assert_eq!(d.ty, Type::Function {
                result: Box::new(Type::Long),
                params: vec![Type::Int, Type::Pointer(Box::new(Type::Char))],
            }),
            _ => panic!("expected prototype"),
        }
        assert!(matches!(parser.next_item().unwrap(), Some(ExternalItem::Declaration(_))));
        assert!(parser.next_item().unwrap().is_none());
    }

    #[test]
    fn rejects_malformed_prototypes() {
        let mut parser = Parser::new("int f(int a,);");
        assert!(parser.next_item().is_err());
    }
}
