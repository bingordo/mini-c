//! Incremental parser for the first supported subset of external declarations.
//! Each complete function body is returned separately; earlier bodies are not retained.
use crate::ast::{BinOp, Decl, Expr, Function, Span, Stmt, StorageClass, Type, UnOp};
use crate::diag::{Diagnostic, DiagnosticCode};
use crate::lexer::{Scanner, Token, TokenKind};
use crate::stream::{ExternalItem, ItemSource};
use crate::scopes::{Binding, Scopes};

/// Syntactic declarator operators, stored inside-out until the base type is known.
/// Applying them from the outside inward gives the C binding order:
/// `int *a[3]` is an array of pointers; `int (*a)[3]` is a pointer to an array.
/// This is parser-internal, so checked/frontend APIs still use `Type`.
enum Declarator {
    Name(String),
    Pointer(Box<Self>),
    Array(Box<Self>, Option<Expr>),
    Function(Box<Self>, Vec<Decl>),
}

impl Declarator {
    fn finish(self, base: Type) -> (String, Type, Option<Vec<Decl>>) {
        match self {
            Self::Name(name) => (name, base, None),
            Self::Pointer(inner) => inner.finish(Type::Pointer(Box::new(base))),
            Self::Array(inner, bound) => inner.finish(Type::Array(Box::new(base), bound.map(Box::new))),
            Self::Function(inner, params) => {
                let param_types = params.iter().map(|param| param.ty.clone()).collect();
                let (name, ty, inner_params) = inner.finish(Type::Function {
                    result: Box::new(base),
                    params: param_types,
                });
                // The parameter names for a definition belong to the resulting
                // function, not to a function nested inside a pointer type.
                let definition_params = if matches!(&ty, Type::Function { .. }) {
                    Some(params)
                } else {
                    inner_params
                };
                (name, ty, definition_params)
            }
        }
    }
}

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
        self.for_each_item_collecting(max_errors, |item, _remaining| {
            receive(item).err().into_iter().collect()
        })
    }

    /// Allow an external item to produce several diagnostics, for instance
    /// errors from independent statements in one function. The callback gets
    /// the remaining error budget and should never return more than that many
    /// diagnostics. Parser recovery and its end-of-file guarantees are shared
    /// with the simpler single-error API.
    pub fn for_each_item_collecting<F>(
        &mut self, max_errors: usize, mut receive: F
    ) -> Vec<Diagnostic>
    where
        F: FnMut(ExternalItem, usize) -> Vec<Diagnostic>,
    {
        let mut errors = Vec::new();
        if max_errors == 0 { return errors; }
        loop {
            match self.next_item() {
                Ok(Some(item)) => {
                    let new_errors = receive(item, max_errors - errors.len());
                    if !new_errors.is_empty() {
                        self.rollback_last_file_binding();
                        errors.extend(new_errors.into_iter().take(max_errors - errors.len()));
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
            Some(token) => Err(Diagnostic::coded(
                token.span,
                DiagnosticCode::SyntaxExpectedToken,
                format!("expected '{expected}', found '{}'", token.text),
            )),
            None => Err(Diagnostic::coded(
                self.eof_span(),
                DiagnosticCode::SyntaxExpectedToken,
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
            "|" => (BinOp::BitOr, 3),
            "^" => (BinOp::BitXor, 4),
            "&" => (BinOp::BitAnd, 5),
            "==" => (BinOp::Eq, 6),
            "!=" => (BinOp::Ne, 6),
            "<" => (BinOp::Lt, 7),
            "<=" => (BinOp::Le, 7),
            ">" => (BinOp::Gt, 7),
            ">=" => (BinOp::Ge, 7),
            "+" => (BinOp::Add, 8),
            "-" => (BinOp::Sub, 8),
            "*" => (BinOp::Mul, 9),
            "/" => (BinOp::Div, 9),
            "%" => (BinOp::Mod, 9),
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
            "~" => Some(UnOp::BitNot),
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
                return Err(Diagnostic::coded(token.span, DiagnosticCode::SyntaxExpectedExpression, "expected expression"));
            }
        }
        let mut expr = match self.take()? {
            Some(t) if t.kind == TokenKind::Character => Expr::Int(
                crate::char_literal::parse_ascii_character(t.text)
                    .map_err(|reason| Diagnostic::coded(
                        t.span,
                        DiagnosticCode::LexicalInvalidCharacter,
                        reason,
                    ))?,
            ),
            Some(t) if t.kind == TokenKind::Number => Expr::Int(
                t.text
                    .parse()
                    .map_err(|_| Diagnostic::coded(t.span, DiagnosticCode::SyntaxIntegerOverflow, "integer literal overflow"))?,
            ),
            Some(t) if matches!(t.kind, TokenKind::Identifier | TokenKind::TypedefName) => Expr::Name(t.text.to_owned()),
            Some(t) if t.text == "(" => {
                let e = self.parse_expr()?;
                self.expect(")")?;
                e
            }
            Some(t) => return Err(Diagnostic::coded(t.span, DiagnosticCode::SyntaxExpectedExpression, "expected expression")),
            None => return Err(Diagnostic::coded(self.eof_span(), DiagnosticCode::SyntaxExpectedExpression, "expected expression")),
        };
        loop {
            match self.peek()?.map(|t| t.text) {
                Some("(") => {
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
                Some("[") => {
                    self.take()?;
                    let index = self.parse_expr()?;
                    self.expect("]")?;
                    expr = Expr::Index(Box::new(expr), Box::new(index));
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    /// Parse C declarator binding order independently of the base type.
    /// Named declarators are required for ordinary definitions and declarations;
    /// parameter declarators may omit their name.
    fn parse_declarator_shape(&mut self, allow_abstract: bool) -> Result<Declarator, Diagnostic> {
        let mut stars = 0;
        while self.peek()?.is_some_and(|token| token.text == "*") {
            self.take()?;
            stars += 1;
        }
        let mut direct = if self.peek()?.is_some_and(|token| token.text == "(") {
            self.take()?;
            let inner = self.parse_declarator_shape(allow_abstract)?;
            self.expect(")")?;
            inner
        } else if let Some(token) = self.peek()?.filter(|token| {
            matches!(token.kind, TokenKind::Identifier | TokenKind::TypedefName)
                && !matches!(token.text, "int" | "char" | "long" | "void"
                    | "unsigned" | "signed" | "typedef" | "static" | "extern"
                    | "if" | "else" | "while" | "return" | "break" | "continue")
        }) {
            self.take()?;
            Declarator::Name(token.text.to_owned())
        } else if allow_abstract {
            Declarator::Name(String::new())
        } else {
            let span = self.peek()?.map_or(self.eof_span(), |token| token.span);
            return Err(Diagnostic::new(span, "expected identifier in declarator"));
        };

        loop {
            if self.peek()?.is_some_and(|token| token.text == "[") {
                self.take()?;
                let bound = if self.peek()?.is_some_and(|token| token.text == "]") {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                self.expect("]")?;
                direct = Declarator::Array(Box::new(direct), bound);
            } else if self.peek()?.is_some_and(|token| token.text == "(") {
                self.take()?;
                let params = self.parse_parameter_list()?;
                self.expect(")")?;
                direct = Declarator::Function(Box::new(direct), params);
            } else {
                break;
            }
        }
        for _ in 0..stars {
            direct = Declarator::Pointer(Box::new(direct));
        }
        Ok(direct)
    }

    fn parse_parameter_list(&mut self) -> Result<Vec<Decl>, Diagnostic> {
        let mut params = Vec::new();
        if self.peek()?.is_some_and(|token| token.text == ")") {
            return Ok(params);
        }
        loop {
            let (base, lo) = self.parse_type()?
                .ok_or_else(|| Diagnostic::new(self.eof_span(), "expected parameter type"))?;
            // The entire parameter list (void) denotes no parameters.
            if base == Type::Void && params.is_empty() {
                if self.peek()?.is_some_and(|token| token.text == ")") {
                    return Ok(params);
                }
                // An incomplete (void at EOF is a missing ')', not an
                // invalid void parameter with an absent declarator.
                if self.peek()?.is_none() {
                    return Err(Diagnostic::new(self.eof_span(), "expected ')'"));
                }
            }
            let (name, ty, _) = self.parse_declarator_shape(true)?.finish(base);
            if ty == Type::Void {
                return Err(Diagnostic::new(Span { lo, hi: lo }, "parameter cannot have void type"));
            }
            params.push(Decl {
                storage: StorageClass::Default,
                name,
                ty,
                init: None,
                span: Span { lo, hi: lo },
            });
            if !self.peek()?.is_some_and(|token| token.text == ",") {
                break;
            }
            self.take()?;
        }
        Ok(params)
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
        let (base, start) = self.parse_type()?
            .ok_or_else(|| Diagnostic::new(self.eof_span(), "expected type"))?;
        let (name, ty, _) = self.parse_declarator_shape(false)?.finish(base);
        self.scopes.declare(&name, if is_typedef { Binding::Typedef } else { Binding::Ordinary });
        let init = if self.peek()?.is_some_and(|t| t.text == "=") {
            self.take()?;
            Some(self.parse_expr()?)
        } else {
            None
        };
        if is_typedef && init.is_some() {
            return Err(Diagnostic::new(Span { lo: start, hi: start }, "typedef cannot have an initializer"));
        }
        let end = self.expect(";")?.span.hi;
        Ok(Decl { storage, name, ty, init, span: Span { lo: start, hi: end } })
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
        let Some((base, start)) = self.parse_type()? else {
            // EOF only succeeds when no declaration specifiers were consumed.
            if is_typedef || storage != StorageClass::Default {
                return Err(Diagnostic::new(self.eof_span(), "expected type specifier"));
            }
            return Ok(None);
        };
        let (name, ty, definition_params) = self.parse_declarator_shape(false)?.finish(base);
        self.pending_file_binding = Some((name.clone(), self.scopes.lookup(&name)));
        self.scopes.declare(&name, if is_typedef { Binding::Typedef } else { Binding::Ordinary });

        if self.peek()?.is_some_and(|token| token.text == "{") {
            let Type::Function { result, .. } = ty else {
                return Err(Diagnostic::new(Span { lo: start, hi: start }, "function body requires a function declarator"));
            };
            if is_typedef {
                return Err(Diagnostic::new(Span { lo: start, hi: start }, "typedef cannot define a function body"));
            }
            let params = definition_params.unwrap_or_default();
            if params.iter().any(|p| p.name.is_empty()) {
                return Err(Diagnostic::new(Span { lo: start, hi: start }, "function definition requires parameter names"));
            }
            self.scopes.enter();
            for param in &params { self.scopes.declare(&param.name, Binding::Ordinary); }
            let parsed = self.parse_block();
            self.scopes.leave();
            let (body, end) = parsed?;
            return Ok(Some(ExternalItem::Function(Function {
                storage,
                name,
                result: *result,
                params,
                body,
                span: Span { lo: start, hi: end },
            })));
        }
        let init = if self.peek()?.is_some_and(|t| t.text == "=") {
            self.take()?;
            Some(self.parse_expr()?)
        } else {
            None
        };
        if is_typedef && init.is_some() {
            return Err(Diagnostic::new(Span { lo: start, hi: start }, "typedef cannot have an initializer"));
        }
        let end = self.expect(";")?.span.hi;
        let decl = Decl { storage, name, ty, init, span: Span { lo: start, hi: end } };
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
    fn postfix_subscripts_bind_tighter_than_unary_and_arithmetic() {
        let mut parser = Parser::new(
            "int f(int *p, int i) { return *p + p[i + 1] * 2; }"
        );
        let ExternalItem::Function(f) = parser.next_item().unwrap().unwrap() else {
            panic!("expected function");
        };
        let Stmt::Block(body) = f.body else { panic!("expected block") };
        let Stmt::Return(Some(Expr::Binary(BinOp::Add, _, mul))) = &body[0] else {
            panic!("expected addition");
        };
        let Expr::Binary(BinOp::Mul, indexed, _) = mul.as_ref() else {
            panic!("expected multiplication");
        };
        assert!(matches!(indexed.as_ref(), Expr::Index(_, _)));
        let mut parser = Parser::new("int g(int *p) { return (p + 1)[0]; }");
        assert!(matches!(parser.next_item().unwrap(), Some(ExternalItem::Function(_))));
        for invalid in ["int f(int *p) { return p[]; }",
                        "int f(int *p) { return p[1; }"] {
            assert!(Parser::new(invalid).next_item().is_err(), "{invalid}");
        }
    }

    #[test]
    fn declarator_binding_order_and_function_pointer_typedefs() {
        let mut p = Parser::new(
            "int *a[3]; int (*b)[3]; typedef int (*Fn)(int); Fn callback; \
             int (*get(int n))(long); int *f(int n) { return 0; }"
        );
        let a = match p.next_item().unwrap().unwrap() {
            ExternalItem::Declaration(d) => d,
            other => panic!("unexpected item: {other:?}"),
        };
        assert_eq!(a.name, "a");
        assert_eq!(a.ty, Type::Array(Box::new(Type::Pointer(Box::new(Type::Int))),
            Some(Box::new(Expr::Int(3)))));
        let b = match p.next_item().unwrap().unwrap() {
            ExternalItem::Declaration(d) => d,
            other => panic!("unexpected item: {other:?}"),
        };
        assert_eq!(b.name, "b");
        assert_eq!(b.ty, Type::Pointer(Box::new(Type::Array(Box::new(Type::Int),
            Some(Box::new(Expr::Int(3)))))));
        let fptr = Type::Pointer(Box::new(Type::Function {
            result: Box::new(Type::Int), params: vec![Type::Int],
        }));
        match p.next_item().unwrap().unwrap() {
            ExternalItem::Typedef(d) => { assert_eq!(d.name, "Fn"); assert_eq!(d.ty, fptr); },
            other => panic!("unexpected item: {other:?}"),
        }
        match p.next_item().unwrap().unwrap() {
            ExternalItem::Declaration(d) => assert_eq!(d.ty, Type::Alias("Fn".into())),
            other => panic!("unexpected item: {other:?}"),
        }
        match p.next_item().unwrap().unwrap() {
            ExternalItem::Declaration(d) => {
                assert_eq!(d.name, "get");
                assert_eq!(d.ty, Type::Function {
                    result: Box::new(Type::Pointer(Box::new(Type::Function {
                        result: Box::new(Type::Int), params: vec![Type::Long],
                    }))),
                    params: vec![Type::Int],
                });
            }
            other => panic!("unexpected item: {other:?}"),
        }
        match p.next_item().unwrap().unwrap() {
            ExternalItem::Function(f) => {
                assert_eq!(f.result, Type::Pointer(Box::new(Type::Int)));
                assert_eq!(f.params[0].name, "n");
            },
            other => panic!("unexpected item: {other:?}"),
        }
        assert!(p.next_item().unwrap().is_none());
    }

    #[test]
    fn abstract_parameter_declarators_and_malformed_grouping() {
        let mut p = Parser::new("int h(int (*)(long), int [], char **);");
        let decl = match p.next_item().unwrap().unwrap() {
            ExternalItem::Declaration(d) => d,
            other => panic!("unexpected item: {other:?}"),
        };
        assert_eq!(decl.ty, Type::Function {
            result: Box::new(Type::Int),
            params: vec![
                Type::Pointer(Box::new(Type::Function {
                    result: Box::new(Type::Int), params: vec![Type::Long],
                })),
                Type::Array(Box::new(Type::Int), None),
                Type::Pointer(Box::new(Type::Pointer(Box::new(Type::Char)))),
            ],
        });
        for source in ["int (*bad)(int;", "int a[1;", "int *;", "int f(int,);"] {
            assert!(Parser::new(source).next_item().is_err(), "accepted {source:?}");
        }
    }

    #[test]
    fn rejects_malformed_prototypes() {
        let mut parser = Parser::new("int f(int a,);");
        assert!(parser.next_item().is_err());
    }
}
