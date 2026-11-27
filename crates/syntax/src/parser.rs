//! Incremental parser for the first supported subset of external declarations.
//! Each complete function body is returned separately; earlier bodies are not retained.
use crate::ast::{AsmOperand, BinOp, Decl, Designator, Expr, FloatRank, Function, MemberAlignment, Qualifiers, Span, Stmt, StorageClass, TagKind, Type, UnOp};
use crate::diag::{Diagnostic, DiagnosticCode};
use crate::lexer::{BorrowedParserTokenSource, ParserToken, ParserTokenSource, Scanner, TokenKind, TokenSource};
use crate::stream::{ExternalItem, ItemSource};
use crate::scopes::{Binding, Scopes};

/// Accept an unadorned GNU attribute identifier or its exactly double-
/// underscored spelling; arbitrary single/triple underscores are not aliases.
fn gnu_attribute_spelling(name: &str) -> &str {
    name.strip_prefix("__")
        .and_then(|inner| inner.strip_suffix("__"))
        .unwrap_or(name)
}

/// Syntactic declarator operators, stored inside-out until the base type is known.
/// Applying them from the outside inward gives the C binding order:
/// `int *a[3]` is an array of pointers; `int (*a)[3]` is a pointer to an array.
/// This is parser-internal, so checked/frontend APIs still use `Type`.
enum Declarator {
    Name(String, Option<Span>),
    Pointer(Box<Self>, Qualifiers),
    Array(Box<Self>, Option<Expr>),
    Function(Box<Self>, Vec<Decl>, bool, bool),
}

impl Declarator {
    fn finish(self, base: Type) -> (String, Type, Option<Vec<Decl>>, Option<Span>) {
        match self {
            Self::Name(name, span) => (name, base, None, span),
            Self::Pointer(inner, qualifiers) => {
                let pointer = Type::Pointer(Box::new(base));
                inner.finish(if qualifiers.any() {
                    Type::Qualified(Box::new(pointer), qualifiers)
                } else {
                    pointer
                })
            },
            Self::Array(inner, bound) => inner.finish(Type::Array(Box::new(base), bound.map(Box::new))),
            Self::Function(inner, params, variadic, unprototyped) => {
                let param_types = params.iter().map(|param| param.ty.clone()).collect();
                let function_ty = if unprototyped {
                    Type::UnprototypedFunction { result: Box::new(base) }
                } else if variadic {
                    Type::VariadicFunction { result: Box::new(base), params: param_types }
                } else {
                    Type::Function { result: Box::new(base), params: param_types }
                };
                let (name, ty, inner_params, name_span) = inner.finish(function_ty);
                // Preserve parameters from the function declarator bound
                // closest to the declared identifier. A nested outer
                // function suffix may instead describe the function type
                // RETURNED through a pointer:
                //
                //   int (*select(int flag, int (*cb)(int)))(int);
                //
                // Here `flag` and `cb` are the definition's parameters,
                // whereas the final unnamed `(int)` describes the returned
                // function pointer. Never overwrite the inner definition
                // parameters with those of the returned function.
                let definition_params = if matches!(&ty,
                    Type::Function { .. } | Type::VariadicFunction { .. } | Type::UnprototypedFunction { .. })
                {
                    inner_params.or(Some(params))
                } else {
                    inner_params
                };
                (name, ty, definition_params, name_span)
            }
        }
    }
}

pub struct Parser<'a> {
    scanner: Box<dyn ParserTokenSource<'a> + 'a>,
    input_len: usize,
    /// Furthest token seen so a true pull stream can diagnose truncated
    /// source without knowing the final translation-unit length up front.
    last_seen_end: usize,
    expression_spans: bool,
    lookahead: Option<ParserToken<'a>>,
    // Nonconsuming two-token lookahead replaces Scanner::clone, which
    // cannot work for one-shot preprocessing token iterators.
    second_lookahead: Option<Option<ParserToken<'a>>>,
    scopes: Scopes,
    brace_depth: usize,
    pending_file_binding: Option<(String, Option<Binding>)>,
    // Incremental external comma declarators. Keep only their common base
    // and storage class; each next_item parses/delivers one declaration,
    // without retaining a translation-unit AST or all initializers.
    pending_file_group: Option<(Type, usize, StorageClass, bool, bool)>,
}

impl<'a> Parser<'a> {
    pub fn new(source: &'a str) -> Self {
        Self::from_token_source(Scanner::new(source), source.len())
    }

    /// Parse tokens as they are requested from a producer rather than from
    /// one preprocessed source string. The caller owns the storage for
    /// borrowed token spellings until parsing finishes.
    ///
    /// `input_len` is the final byte offset for precise unexpected-EOF
    /// diagnostics. A streaming producer may use zero if it does not yet
    /// know the final size; the parser still tracks the furthest token
    /// span observed as a fallback.
    pub fn from_token_source(
        source: impl TokenSource<'a> + 'a,
        input_len: usize,
    ) -> Self {
        Self::from_parser_token_source(BorrowedParserTokenSource(source), input_len)
    }

    /// Parse from a producer of owned-or-borrowed token spellings. The
    /// preprocessor can hand over Arc-backed tokens one at a time; its
    /// output need not remain in an immutable translation-unit String.
    pub fn from_parser_token_source(
        source: impl ParserTokenSource<'a> + 'a,
        input_len: usize,
    ) -> Self {
        Self {
            scanner: Box::new(source),
            input_len,
            last_seen_end: 0,
            expression_spans: false,
            lookahead: None,
            second_lookahead: None,
            scopes: Scopes::new(),
            brace_depth: 0,
            pending_file_binding: None,
            pending_file_group: None,
        }
    }

    /// Enables byte-accurate expression locations for semantic diagnostics.
    /// Default Parser::new() preserves the original unannotated AST.
    pub fn new_with_spans(source: &'a str) -> Self {
        Self::new(source).with_expression_spans()
    }

    /// Preserve byte-accurate expressions in the streamed token path too.
    pub fn with_expression_spans(mut self) -> Self {
        self.expression_spans = true;
        self
    }

    fn located(&self, expr: Expr, span: Span) -> Expr {
        if self.expression_spans {
            Expr::Located(Box::new(expr), span)
        } else {
            expr
        }
    }

    /// Peek past the cached token without consuming it or changing typedef scopes.
    /// C labels use their own namespace even when spelled as typedef names.
    fn followed_by_colon(&mut self) -> Result<bool, Diagnostic> {
        self.peek()?;
        if self.second_lookahead.is_none() {
            self.second_lookahead = Some(self.scanner.next_parser_token(Some(&self.scopes))?);
        }
        Ok(self.second_lookahead.as_ref().and_then(Option::as_ref)
            .is_some_and(|token| token.text.as_ref() == ":"))
    }

    // A missing token belongs at the actual end of input, not byte zero.
    // In particular this must handle UTF-8 source preceding a truncated item.
    fn eof_span(&self) -> Span {
        let end = self.input_len.max(self.last_seen_end);
        Span { lo: end, hi: end }
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
                t.text.as_ref(), "int" | "_Bool" | "char" | "short" | "long" | "_Float128" | "float" | "double" | "void" | "signed" |
                    "unsigned" | "static" | "extern" | "register" | "typedef" | "struct" | "union" | "enum"
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
            if self.brace_depth == 0 && matches!(token.text.as_ref(), ";" | "}") {
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

    fn peek(&mut self) -> Result<Option<ParserToken<'a>>, Diagnostic> {
        if self.lookahead.is_none() {
            self.lookahead = self.scanner.next_parser_token(Some(&self.scopes))?;
            if let Some(token) = self.lookahead.as_ref() {
                self.last_seen_end = self.last_seen_end.max(token.span.hi);
            }
        }
        // Scope changes can occur while a token is held in lookahead.
        if let Some(token) = self.lookahead.as_mut() {
            if matches!(token.kind, TokenKind::Identifier | TokenKind::TypedefName) {
                token.kind = if self.scopes.is_typedef(token.text.as_ref()) {
                    TokenKind::TypedefName
                } else {
                    TokenKind::Identifier
                };
            }
        }
        Ok(self.lookahead.clone())
    }

    fn take(&mut self) -> Result<Option<ParserToken<'a>>, Diagnostic> {
        self.peek()?;
        let next = self.lookahead.take();
        if let Some(after) = self.second_lookahead.take() {
            self.lookahead = after;
        }
        if let Some(ref token) = next {
            match token.text.as_ref() {
                "{" => self.brace_depth += 1,
                "}" => self.brace_depth = self.brace_depth.saturating_sub(1),
                _ => {}
            }
        }
        Ok(next)
    }

    fn expect(&mut self, expected: &str) -> Result<ParserToken<'a>, Diagnostic> {
        // Do not consume a mismatching token: the recovery API needs to see
        // structural delimiters to resume at the next external declaration.
        match self.peek()? {
            Some(token) if token.text.as_ref() == expected =>
                Ok(self.take()?.expect("matching token was peeked")),
            Some(token) => Err(Diagnostic::coded(
                token.span,
                DiagnosticCode::SyntaxExpectedToken,
                format!("expected '{expected}', found '{}'", token.text.as_ref()),
            )),
            None => Err(Diagnostic::coded(
                self.eof_span(),
                DiagnosticCode::SyntaxExpectedToken,
                format!("expected '{expected}', found end of input"),
            )),
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, Diagnostic> {
        // The comma operator has the lowest precedence and associates to
        // the left. Assignment-expression contexts (e.g. function arguments
        // and initializer elements) call parse_assign directly.
        let mut lhs = self.parse_assign()?;
        while self.peek()?.is_some_and(|token| token.text.as_ref() == ",") {
            self.take()?;
            let rhs = self.parse_assign()?;
            let span = if self.expression_spans {
                Span {
                    lo: lhs.location().expect("located comma lhs").lo,
                    hi: rhs.location().expect("located comma rhs").hi,
                }
            } else { Span::default() };
            lhs = self.located(Expr::Comma(Box::new(lhs), Box::new(rhs)), span);
        }
        Ok(lhs)
    }

    fn parse_conditional(&mut self) -> Result<Expr, Diagnostic> {
        let condition = self.parse_binary(0)?;
        if !self.peek()?.is_some_and(|token| token.text.as_ref() == "?") {
            return Ok(condition);
        }
        self.take()?;
        // ISO C: the middle operand is a full expression (commas included);
        // the last operand is another conditional-expression, associating
        // ternaries to the right. Assignment binds less tightly.
        let when_true = self.parse_expr()?;
        self.expect(":")?;
        let when_false = self.parse_conditional()?;
        let span = if self.expression_spans {
            Span {
                lo: condition.location().expect("located conditional condition").lo,
                hi: when_false.location().expect("located conditional false arm").hi,
            }
        } else { Span::default() };
        Ok(self.located(Expr::Conditional(
            Box::new(condition), Box::new(when_true), Box::new(when_false),
        ), span))
    }

    fn parse_assign(&mut self) -> Result<Expr, Diagnostic> {
        let lhs = self.parse_conditional()?;
        if let Some(op) = self.peek()?.and_then(|t| match t.text.as_ref() {
            "=" => Some(None),
            "+=" => Some(Some(BinOp::Add)),
            "-=" => Some(Some(BinOp::Sub)),
            "*=" => Some(Some(BinOp::Mul)),
            "/=" => Some(Some(BinOp::Div)),
            "%=" => Some(Some(BinOp::Mod)),
            "&=" => Some(Some(BinOp::BitAnd)),
            "|=" => Some(Some(BinOp::BitOr)),
            "^=" => Some(Some(BinOp::BitXor)),
            "<<=" => Some(Some(BinOp::Shl)),
            ">>=" => Some(Some(BinOp::Shr)),
            _ => None,
        }) {
            self.take()?;
            let rhs = self.parse_assign()?;
            let span = if self.expression_spans {
                Span {
                    lo: lhs.location().expect("located assignment lhs").lo,
                    hi: rhs.location().expect("located assignment rhs").hi,
                }
            } else { Span::default() };
            let value = match op {
                Some(op) => Expr::CompoundAssign(op, Box::new(lhs), Box::new(rhs)),
                None => Expr::Assign(Box::new(lhs), Box::new(rhs)),
            };
            return Ok(self.located(value, span));
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
            "<<" => (BinOp::Shl, 8),
            ">>" => (BinOp::Shr, 8),
            "+" => (BinOp::Add, 9),
            "-" => (BinOp::Sub, 9),
            "*" => (BinOp::Mul, 10),
            "/" => (BinOp::Div, 10),
            "%" => (BinOp::Mod, 10),
            _ => return None,
        })
    }

    fn parse_binary(&mut self, min_prec: u8) -> Result<Expr, Diagnostic> {
        let mut lhs = self.parse_unary()?;
        while let Some((op, prec)) = self.peek()?.and_then(|t| Self::binary_op(t.text.as_ref())) {
            if prec < min_prec {
                break;
            }
            self.take()?;
            let rhs = self.parse_binary(prec + 1)?;
            let span = if self.expression_spans {
                Span {
                    lo: lhs.location().expect("located binary lhs").lo,
                    hi: rhs.location().expect("located binary rhs").hi,
                }
            } else { Span::default() };
            lhs = self.located(Expr::Binary(op, Box::new(lhs), Box::new(rhs)), span);
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, Diagnostic> {
        if self.peek()?.is_some_and(|token| token.text.as_ref() == "sizeof") {
            let keyword = self.take()?.expect("peeked sizeof");
            if self.peek()?.is_some_and(|t| t.text.as_ref() == "(") {
                self.take()?;
                // A typedef-aware type-name lookahead is essential:
                // sizeof(T) and sizeof(variable) have different AST forms.
                let type_name = self.peek()?.is_some_and(|next| {
                    matches!(next.text.as_ref(), "int" | "_Bool" | "char" | "short" | "long" | "_Float128" |
                        "float" | "double" | "void" | "signed" | "unsigned" |
                        "struct" | "union" | "enum" | "const" | "volatile" | "_Atomic" | "restrict" |
                        "__restrict" | "__restrict__")
                        || next.kind == TokenKind::TypedefName
                });
                if type_name {
                    let (base, _) = self.parse_type()?.expect("type-name lookahead");
                    let (name, ty, nested, _) = self.parse_declarator_shape(true)?.finish(base);
                    if !name.is_empty() || nested.is_some() {
                        return Err(Diagnostic::coded(keyword.span,
                            DiagnosticCode::SyntaxExpectedExpression,
                            "sizeof requires an abstract type-name declarator"));
                    }
                    let close = self.expect(")")?;
                    if self.peek()?.is_some_and(|token| token.text.as_ref() == "{") {
                        // sizeof (type){initializer} applies to the compound
                        // literal expression, not sizeof(type). The object
                        // itself remains unevaluated during sizeof.
                        let initializer = self.parse_initializer()?;
                        let hi = initializer.location().map_or(close.span.hi, |s| s.hi);
                        return Ok(self.located(Expr::SizeofExpr(Box::new(
                            self.located(Expr::CompoundLiteral(ty, Box::new(initializer)),
                                Span { lo: close.span.lo, hi }),
                        )), Span { lo: keyword.span.lo, hi }));
                    }
                    return Ok(self.located(Expr::SizeofType(ty),
                        Span { lo: keyword.span.lo, hi: close.span.hi }));
                }
                let operand = self.parse_expr()?;
                let close = self.expect(")")?;
                return Ok(self.located(Expr::SizeofExpr(Box::new(operand)),
                    Span { lo: keyword.span.lo, hi: close.span.hi }));
            }
            // sizeof unary-expression also accepts *p, ++x, string literals,
            // nested sizeof and other non-parenthesized unary expressions.
            let operand = self.parse_unary()?;
            let end = operand.location().map_or(keyword.span.hi, |s| s.hi);
            return Ok(self.located(Expr::SizeofExpr(Box::new(operand)),
                Span { lo: keyword.span.lo, hi: end }));
        }
        if let Some(increment) = self.peek()?.and_then(|t| match t.text.as_ref() {
            "++" => Some(true),
            "--" => Some(false),
            _ => None,
        }) {
            let prefix = self.take()?.expect("peeked update operator");
            let inner = self.parse_unary()?;
            let end = inner.location().map_or(prefix.span.hi, |loc| loc.hi);
            return Ok(self.located(Expr::Update {
                target: Box::new(inner), increment, postfix: false,
            }, Span { lo: prefix.span.lo, hi: end }));
        }
        if let Some(op) = self.peek()?.and_then(|t| match t.text.as_ref() {
            "+" => Some(UnOp::Plus),
            "-" => Some(UnOp::Neg),
            "!" => Some(UnOp::Not),
            "~" => Some(UnOp::BitNot),
            "&" => Some(UnOp::Address),
            "*" => Some(UnOp::Deref),
            _ => None,
        }) {
            let prefix = self.take()?.expect("peeked prefix");
            let inner = self.parse_unary()?;
            let end = inner.location().map_or(prefix.span.hi, |span| span.hi);
            return Ok(self.located(Expr::Unary(op, Box::new(inner)),
                Span { lo: prefix.span.lo, hi: end }));
        }
        // Keep synchronization tokens available to the recovery path.
        if let Some(token) = self.peek()? {
            if matches!(token.text.as_ref(), ";" | "}") {
                return Err(Diagnostic::coded(token.span, DiagnosticCode::SyntaxExpectedExpression, "expected expression"));
            }
        }
        let mut expr = match self.take()? {
            Some(t) if t.kind == TokenKind::String => {
                // C translation phase 6 concatenates adjacent string tokens
                // into one array with exactly one final implicit NUL. Decode
                // each token independently so escapes never cross boundaries.
                let mut bytes = crate::string_literal::decode_narrow_string(t.text.as_ref())
                    .map_err(|reason| Diagnostic::coded(
                        t.span, DiagnosticCode::LexicalInvalidCharacter, reason))?;
                let mut end = t.span.hi;
                while matches!(self.peek()?, Some(next) if next.kind == TokenKind::String) {
                    let next = self.take()?.expect("peeked string token");
                    let fragment = crate::string_literal::decode_narrow_string(next.text.as_ref())
                        .map_err(|reason| Diagnostic::coded(
                            next.span, DiagnosticCode::LexicalInvalidCharacter, reason))?;
                    bytes.pop(); // remove only the previous implicit terminator
                    bytes.extend(fragment);
                    end = next.span.hi;
                }
                self.located(Expr::StringLiteral(bytes), Span { lo: t.span.lo, hi: end })
            }
            Some(t) if t.kind == TokenKind::Character => {
                let value = crate::char_literal::parse_ascii_character(t.text.as_ref())
                    .map_err(|reason| Diagnostic::coded(
                        t.span, DiagnosticCode::LexicalInvalidCharacter, reason))?;
                self.located(Expr::Int(value), t.span)
            }
            Some(t) if t.kind == TokenKind::FloatingNumber => {
                let rank = crate::floating_literal::floating_rank(t.text.as_ref())
                    .map_err(|reason| Diagnostic::coded(
                        t.span, DiagnosticCode::LexicalNumericSuffix, reason))?;
                self.located(Expr::FloatLiteral(t.text.to_string(), rank), t.span)
            }
            Some(t) if t.kind == TokenKind::Number => {
                let expr = crate::integer_literal::parse_integer_expression(t.text.as_ref())
                    .map_err(|reason| Diagnostic::coded(
                        t.span, DiagnosticCode::SyntaxIntegerOverflow, reason))?;
                self.located(expr, t.span)
            }
            Some(t) if matches!(t.kind, TokenKind::Identifier | TokenKind::TypedefName) =>
                self.located(Expr::Name(t.text.to_string()), t.span),
            Some(t) if t.text.as_ref() == "(" => {
                // Typedef-aware type-name recognition distinguishes (T)x from
                // (x) without rewinding the incremental lexer. Parse the
                // full *abstract declarator* after the base type so a cast
                // like (void*)0 is not mistaken for (void) followed by *.
                let cast_type = self.peek()?.is_some_and(|next| {
                    matches!(next.text.as_ref(), "int" | "_Bool" | "char" | "short" | "long" | "_Float128" | "float" | "double" | "void" |
                        "signed" | "unsigned" | "struct" | "union" | "enum" |
                        "const" | "volatile" | "_Atomic" | "restrict" | "__restrict" | "__restrict__")
                        || next.kind == TokenKind::TypedefName
                });
                if cast_type {
                    let (base, _) = self.parse_type()?.expect("type-name lookahead");
                    let (name, ty, nested, _) = self.parse_declarator_shape(true)?.finish(base);
                    if !name.is_empty() || nested.is_some() {
                        return Err(Diagnostic::coded(t.span,
                            DiagnosticCode::SyntaxInvalidDeclaration,
                            "cast requires an abstract type-name declarator"));
                    }
                    let close = self.expect(")")?;
                    if self.peek()?.is_some_and(|token| token.text.as_ref() == "{") {
                        let initializer = self.parse_initializer()?;
                        let hi = initializer.location().map_or(close.span.hi, |span| span.hi);
                        self.located(Expr::CompoundLiteral(ty, Box::new(initializer)),
                            Span { lo: t.span.lo, hi })
                    } else {
                        let operand = self.parse_unary()?;
                        let hi = operand.location().map_or(close.span.hi, |span| span.hi);
                        self.located(Expr::Cast(ty, Box::new(operand)),
                            Span { lo: t.span.lo, hi })
                    }
                } else {
                    let e = self.parse_expr()?;
                    let close = self.expect(")")?;
                    self.located(e, Span { lo: t.span.lo, hi: close.span.hi })
                }
            }
            Some(t) => return Err(Diagnostic::coded(
                t.span, DiagnosticCode::SyntaxExpectedExpression, "expected expression")),
            None => return Err(Diagnostic::coded(
                self.eof_span(), DiagnosticCode::SyntaxExpectedExpression, "expected expression")),
        };
        loop {
            match self.peek()?.map(|t| t.text).as_deref() {
                Some("(") => {
                    self.take()?;
                    let mut args = Vec::new();
                    if !self.peek()?.is_some_and(|t| t.text.as_ref() == ")") {
                        loop {
                            args.push(self.parse_assign()?);
                            if !self.peek()?.is_some_and(|t| t.text.as_ref() == ",") {
                                break;
                            }
                            self.take()?;
                        }
                    }
                    let close = self.expect(")")?;
                    let start = expr.location().map_or(close.span.lo, |span| span.lo);
                    expr = self.located(Expr::Call(Box::new(expr), args),
                        Span { lo: start, hi: close.span.hi });
                }
                Some("[") => {
                    self.take()?;
                    let index = self.parse_expr()?;
                    let close = self.expect("]")?;
                    let start = expr.location().map_or(close.span.lo, |span| span.lo);
                    expr = self.located(Expr::Index(Box::new(expr), Box::new(index)),
                        Span { lo: start, hi: close.span.hi });
                }
                Some(".") | Some("->") => {
                    let arrow = self.take()?.expect("peeked operator").text.as_ref() == "->";
                    let token = self.take()?.ok_or_else(||
                        Diagnostic::new(self.eof_span(), "expected structure member name"))?;
                    if !matches!(token.kind, TokenKind::Identifier | TokenKind::TypedefName) {
                        return Err(Diagnostic::new(token.span, "expected structure member name"));
                    }
                    let start = expr.location().map_or(token.span.lo, |span| span.lo);
                    expr = self.located(Expr::Member(Box::new(expr), token.text.to_string(), arrow),
                        Span { lo: start, hi: token.span.hi });
                }
                Some("++") | Some("--") => {
                    let token = self.take()?.expect("peeked postfix update");
                    let start = expr.location().map_or(token.span.lo, |loc| loc.lo);
                    expr = self.located(Expr::Update {
                        target: Box::new(expr),
                        increment: token.text.as_ref() == "++",
                        postfix: true,
                    }, Span { lo: start, hi: token.span.hi });
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    /// Parse GNU declarator attributes as a bounded, structured list.
    ///
    /// Only annotations that are safe to *not* exploit for optimization or
    /// diagnostics are accepted. Layout, ABI, control-flow and unknown
    /// attributes fail closed; they must not disappear silently.
    fn parse_gnu_declarator_attributes(&mut self, function_params: Option<(usize, bool)>, mut decl_type: Option<&mut Type>)
        -> Result<(), Diagnostic>
    {
        let function = function_params.is_some();
        let start = self.take()?.expect("peeked GNU attribute").span;
        self.expect("(")?;
        self.expect("(")?;
        let mut count = 0usize;
        loop {
            let token = self.peek()?.ok_or_else(|| Diagnostic::coded(
                self.eof_span(), DiagnosticCode::SyntaxInvalidDeclaration,
                "unterminated GNU attribute list"
            ))?;
            if token.text.as_ref() == ")" {
                self.take()?;
                break;
            }
            if !matches!(token.kind, TokenKind::Identifier | TokenKind::TypedefName) {
                return Err(Diagnostic::coded(token.span,
                    DiagnosticCode::SyntaxInvalidDeclaration,
                    "expected GNU attribute identifier"));
            }
            self.take()?;
            count += 1;
            let name = gnu_attribute_spelling(token.text.as_ref());
            match name {
                // These are optional compiler optimization or diagnostic
                // promises. Ignoring them is conservative for generated code.
                "malloc" => {
                    // GNU malloc(deallocator, ptr-index) is a declaration-only
                    // allocation/deallocation annotation, not an ABI change.
                    // Accept only its documented, explicitly validated form.
                    if !function {
                        return Err(Diagnostic::coded(token.span,
                            DiagnosticCode::SyntaxInvalidDeclaration,
                            "malloc attribute requires a function declaration"));
                    }
                    if self.peek()?.is_some_and(|t| t.text.as_ref() == "(") {
                        self.take()?;
                        let deallocator = self.take()?.ok_or_else(|| Diagnostic::coded(
                            self.eof_span(), DiagnosticCode::SyntaxInvalidDeclaration,
                            "expected malloc deallocator function identifier"
                        ))?;
                        if !matches!(deallocator.kind, TokenKind::Identifier | TokenKind::TypedefName) {
                            return Err(Diagnostic::coded(deallocator.span,
                                DiagnosticCode::SyntaxInvalidDeclaration,
                                "malloc deallocator must be a function identifier"));
                        }
                        self.expect(",")?;
                        let arg = self.take()?.ok_or_else(|| Diagnostic::coded(
                            self.eof_span(), DiagnosticCode::SyntaxInvalidDeclaration,
                            "expected malloc deallocator argument position"
                        ))?;
                        if arg.kind != TokenKind::Number ||
                            !arg.text.bytes().all(|b| b.is_ascii_digit()) ||
                            arg.text.parse::<usize>().ok().filter(|p| *p > 0).is_none()
                        {
                            return Err(Diagnostic::coded(arg.span,
                                DiagnosticCode::SyntaxInvalidDeclaration,
                                "malloc deallocator position must be a positive decimal integer"));
                        }
                        self.expect(")")?;
                    }
                }
                // noreturn is a promise about control flow; cold only
                // suggests unlikely execution and size-oriented optimization.
                // Neither changes the calling convention or object layout.
                // The frontend does not exploit either annotation, keeping
                // ordinary call and return lowering conservative. Unlike
                // returns_twice, these need no special control-flow edges.
                "nothrow" | "leaf" | "pure" | "const" | "noreturn" | "cold"
                | "warn_unused_result" | "unused" | "deprecated"
                | "returns_nonnull" => {
                    if self.peek()?.is_some_and(|t| t.text.as_ref() == "(") {
                        return Err(Diagnostic::coded(token.span,
                            DiagnosticCode::SyntaxInvalidDeclaration,
                            format!("unsupported arguments to GNU attribute '{}'", token.text.as_ref())));
                    }
                }
                "format" => {
                    // GNU format(archetype, format-argument, first-to-check)
                    // describes compile-time format diagnostics, not the ABI.
                    // Validate the prototype and all operands before dropping
                    // this advisory annotation. Unknown kinds fail closed.
                    let Some((fixed, variadic)) = function_params else {
                        return Err(Diagnostic::coded(token.span,
                            DiagnosticCode::SyntaxInvalidDeclaration,
                            "format attribute requires a function prototype"));
                    };
                    self.expect("(")?;
                    let kind = self.take()?.ok_or_else(|| Diagnostic::coded(
                        self.eof_span(), DiagnosticCode::SyntaxInvalidDeclaration,
                        "expected GNU format archetype"
                    ))?;
                    if !matches!(gnu_attribute_spelling(kind.text.as_ref()),
                        "printf" | "gnu_printf" | "scanf" | "gnu_scanf")
                    {
                        return Err(Diagnostic::coded(kind.span,
                            DiagnosticCode::SyntaxInvalidDeclaration,
                            "unsupported GNU format archetype"));
                    }
                    self.expect(",")?;
                    let position = self.take()?.ok_or_else(|| Diagnostic::coded(
                        self.eof_span(), DiagnosticCode::SyntaxInvalidDeclaration,
                        "expected format string parameter index"
                    ))?;
                    let format_index = if position.kind == TokenKind::Number &&
                        position.text.bytes().all(|b| b.is_ascii_digit())
                    {
                        position.text.parse::<usize>().ok().filter(|n| *n > 0 && *n <= fixed)
                    } else { None };
                    let Some(format_index) = format_index else {
                        return Err(Diagnostic::coded(position.span,
                            DiagnosticCode::SyntaxInvalidDeclaration,
                            "format string index must name a fixed parameter"));
                    };
                    self.expect(",")?;
                    let argument = self.take()?.ok_or_else(|| Diagnostic::coded(
                        self.eof_span(), DiagnosticCode::SyntaxInvalidDeclaration,
                        "expected first format argument position"
                    ))?;
                    let first_index = if argument.kind == TokenKind::Number &&
                        argument.text.bytes().all(|b| b.is_ascii_digit())
                    {
                        argument.text.parse::<usize>().ok()
                    } else { None };
                    let valid = first_index.is_some_and(|n|
                        if n == 0 { true } else {
                            variadic && n == fixed.saturating_add(1) && n > format_index
                        }
                    );
                    if !valid {
                        return Err(Diagnostic::coded(argument.span,
                            DiagnosticCode::SyntaxInvalidDeclaration,
                            "format first-to-check index must be zero or the first ellipsis argument"));
                    }
                    self.expect(")")?;
                }
                "alloc_size" | "alloc_align" => {
                    // GNU alloc_size(i[,j]) and alloc_align(i) describe
                    // allocation sizes and alignment for diagnostics and
                    // optional optimization. Their omission is conservative
                    // and never changes calling convention or object layout.
                    // Check only documented parameter-index forms.
                    let Some((fixed, _)) = function_params else {
                        return Err(Diagnostic::coded(token.span,
                            DiagnosticCode::SyntaxInvalidDeclaration,
                            "allocation attribute requires a function prototype"));
                    };
                    self.expect("(")?;
                    let mut positions = Vec::new();
                    let max_positions = if name == "alloc_align" { 1 } else { 2 };
                    loop {
                        let index = self.take()?.ok_or_else(|| Diagnostic::coded(
                            self.eof_span(), DiagnosticCode::SyntaxInvalidDeclaration,
                            "expected allocation attribute parameter index"))?;
                        let n = if index.kind == TokenKind::Number &&
                            index.text.bytes().all(|b| b.is_ascii_digit())
                        {
                            index.text.parse::<usize>().ok()
                        } else { None };
                        if !n.is_some_and(|n| n > 0 && n <= fixed && !positions.contains(&n)) {
                            return Err(Diagnostic::coded(index.span,
                                DiagnosticCode::SyntaxInvalidDeclaration,
                                "allocation attribute index must name a distinct fixed parameter"));
                        }
                        positions.push(n.unwrap());
                        if positions.len() == max_positions ||
                            !self.peek()?.is_some_and(|t| t.text.as_ref() == ",") {
                            break;
                        }
                        self.take()?;
                    }
                    self.expect(")")?;
                }
                "nonnull" => {
                    if self.peek()?.is_some_and(|t| t.text.as_ref() == "(") {
                        self.take()?;
                        // No arguments denotes all pointer parameters.
                        if !self.peek()?.is_some_and(|t| t.text.as_ref() == ")") {
                            loop {
                                let argument = self.take()?.ok_or_else(|| Diagnostic::coded(
                                    self.eof_span(), DiagnosticCode::SyntaxInvalidDeclaration,
                                    "expected nonnull parameter position"
                                ))?;
                                let position = argument.text.parse::<usize>().ok()
                                    .filter(|position| *position != 0);
                                if argument.kind != TokenKind::Number || position.is_none() {
                                    return Err(Diagnostic::coded(argument.span,
                                        DiagnosticCode::SyntaxInvalidDeclaration,
                                        "nonnull attribute requires positive integer parameter positions"));
                                }
                                if !self.peek()?.is_some_and(|t| t.text.as_ref() == ",") { break; }
                                self.take()?;
                            }
                        }
                        self.expect(")")?;
                    }
                }
                "access" => {
                    // GCC access(mode, pointer-parameter[, size-parameter])
                    // annotates diagnostics and optional optimization; it
                    // does not change function calling convention, layout or
                    // runtime behavior. Ignore only this *validated* form,
                    // preserving the normal checked parameter types. Do not
                    // generalize this to arbitrary GNU attributes.
                    self.expect("(")?;
                    let mode = self.take()?.ok_or_else(|| Diagnostic::coded(
                        self.eof_span(), DiagnosticCode::SyntaxInvalidDeclaration,
                        "expected access mode"
                    ))?;
                    if !matches!(gnu_attribute_spelling(mode.text.as_ref()),
                        "read_only" | "write_only" | "read_write" | "none")
                    {
                        return Err(Diagnostic::coded(mode.span,
                            DiagnosticCode::SyntaxInvalidDeclaration,
                            "unsupported access attribute mode"));
                    }
                    self.expect(",")?;
                    for position in 0..2 {
                        let argument = self.take()?.ok_or_else(|| Diagnostic::coded(
                            self.eof_span(), DiagnosticCode::SyntaxInvalidDeclaration,
                            "expected access attribute parameter index"
                        ))?;
                        let valid = argument.kind == TokenKind::Number &&
                            argument.text.parse::<usize>().ok().is_some_and(|n| n != 0);
                        if !valid {
                            return Err(Diagnostic::coded(argument.span,
                                DiagnosticCode::SyntaxInvalidDeclaration,
                                "access attribute requires positive integer parameter indexes"));
                        }
                        if position == 1 || !self.peek()?.is_some_and(|t| t.text.as_ref() == ",") {
                            break;
                        }
                        self.take()?;
                    }
                    self.expect(")")?;
                }
                "mode" => {
                    // GCC's __mode__(__word__) on a signed/unsigned int
                    // declaration chooses the target's signed/unsigned word
                    // integer type. On supported LP64/ILP32 ABIs C long has
                    // pointer width. Preserve the resulting type in the AST:
                    // silently ignoring this would make glibc register_t
                    // only 32-bit on x86_64. Other modes remain unsupported.
                    self.expect("(")?;
                    let mode = self.take()?.ok_or_else(|| Diagnostic::coded(
                        self.eof_span(), DiagnosticCode::SyntaxInvalidDeclaration,
                        "expected GNU integer mode"))?;
                    if gnu_attribute_spelling(mode.text.as_ref()) != "word" {
                        return Err(Diagnostic::coded(mode.span,
                            DiagnosticCode::SyntaxInvalidDeclaration,
                            "unsupported GNU integer mode (possible ABI or semantic effect)"));
                    }
                    self.expect(")")?;
                    let Some(declared) = decl_type.as_deref_mut() else {
                        return Err(Diagnostic::coded(token.span,
                            DiagnosticCode::SyntaxInvalidDeclaration,
                            "GNU word mode requires a complete integer declarator"));
                    };
                    match declared {
                        Type::Int => *declared = Type::Long,
                        Type::Signed(inner) if matches!(inner.as_ref(), Type::Int) =>
                            *declared = Type::Signed(Box::new(Type::Long)),
                        Type::Unsigned(inner) if matches!(inner.as_ref(), Type::Int) =>
                            *declared = Type::Unsigned(Box::new(Type::Long)),
                        _ => return Err(Diagnostic::coded(token.span,
                            DiagnosticCode::SyntaxInvalidDeclaration,
                            "GNU word mode requires a signed or unsigned int base type")),
                    }
                }
                // Keep returns_twice, packed/aligned/vector_size, mode/section
                // and calling-convention annotations fail-closed: unlike
                // noreturn, these may change observable semantics or ABI.
                // 'packed', 'aligned', 'vector_size', 'mode', 'section',
                // 'ms_abi', 'sysv_abi' or unknown ABI-affecting extensions.
                _ => return Err(Diagnostic::coded(token.span,
                    DiagnosticCode::SyntaxInvalidDeclaration,
                    format!("unsupported GNU attribute '{}' (possible ABI or semantic effect)",
                        token.text.as_ref()))),
            }
            if !function && name != "mode" {
                return Err(Diagnostic::coded(token.span,
                    DiagnosticCode::SyntaxInvalidDeclaration,
                    "GNU function attribute requires a function declarator"));
            }
            if self.peek()?.is_some_and(|t| t.text.as_ref() == ",") {
                self.take()?;
            } else if !self.peek()?.is_some_and(|t| t.text.as_ref() == ")") {
                let span = self.peek()?.map_or(self.eof_span(), |t| t.span);
                return Err(Diagnostic::coded(span,
                    DiagnosticCode::SyntaxInvalidDeclaration,
                    "expected ',' or ')' in GNU attribute list"));
            }
        }
        if count == 0 {
            return Err(Diagnostic::coded(start, DiagnosticCode::SyntaxInvalidDeclaration,
                "empty GNU attribute list is unsupported"));
        }
        self.expect(")")?;
        Ok(())
    }

    /// Parse C declarator binding order independently of the base type.
    /// Named declarators are required for ordinary definitions and declarations;
    /// parameter declarators may omit their name.
    fn parse_declarator_shape(&mut self, allow_abstract: bool) -> Result<Declarator, Diagnostic> {
        self.parse_declarator_shape_impl(allow_abstract, false)
    }

    fn parse_declarator_shape_impl(&mut self, allow_abstract: bool, allow_old_style: bool) -> Result<Declarator, Diagnostic> {
        let mut stars = Vec::new();
        while self.peek()?.is_some_and(|token| token.text.as_ref() == "*") {
            self.take()?;
            stars.push(self.parse_qualifiers()?);
        }
        let mut direct = if self.peek()?.is_some_and(|token| token.text.as_ref() == "(") {
            self.take()?;
            // In an abstract declarator, '(' followed by a type specifier
            // begins a function parameter list, not a grouped declarator.
            // A typedef name in this ambiguous parameter context denotes
            // a type (C99 6.7.5.3). '*' or '[' still starts grouping.
            let parameter_list = allow_abstract && self.peek()?.is_some_and(|token|
                token.kind == TokenKind::TypedefName
                || matches!(token.text.as_ref(),
                    ")" | "void" | "_Bool" | "char" | "short" | "int" | "long"
                    | "signed" | "unsigned" | "float" | "double" | "_Float128"
                    | "const" | "volatile" | "restrict" | "__restrict" | "__restrict__"
                    | "_Atomic" | "struct" | "union" | "enum" | "register"
                    | "__builtin_va_list"));
            if parameter_list {
                let unprototyped = self.peek()?.is_some_and(|t| t.text.as_ref() == ")");
                let (params, variadic) = self.parse_parameter_list()?;
                self.expect(")")?;
                Declarator::Function(Box::new(Declarator::Name(String::new(), None)),
                    params, variadic, unprototyped)
            } else {
                let inner = self.parse_declarator_shape_impl(allow_abstract, allow_old_style)?;
                self.expect(")")?;
                inner
            }
        } else if let Some(token) = self.peek()?.filter(|token| {
            matches!(token.kind, TokenKind::Identifier | TokenKind::TypedefName)
                && !matches!(token.text.as_ref(), "int" | "_Bool" | "char" | "short" | "long" | "_Float128" | "float" | "double" | "void"
                    | "unsigned" | "signed" | "const" | "volatile" | "restrict" | "__restrict" | "__restrict__"
                    | "typedef" | "static" | "extern" | "register"
                    | "struct" | "union" | "enum" | "if" | "else" | "while" | "do" | "for" | "switch" | "case" | "default" | "return" | "break" | "continue")
        }) {
            self.take()?;
            Declarator::Name(token.text.to_string(), self.expression_spans.then_some(token.span))
        } else if allow_abstract {
            Declarator::Name(String::new(), None)
        } else {
            let span = self.peek()?.map_or(self.eof_span(), |token| token.span);
            return Err(Diagnostic::coded(span, DiagnosticCode::SyntaxExpectedDeclarator,
                "expected identifier in declarator"));
        };

        loop {
            if self.peek()?.is_some_and(|token| token.text.as_ref() == "[") {
                self.take()?;
                let bound = if self.peek()?.is_some_and(|token| token.text.as_ref() == "]") {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                self.expect("]")?;
                direct = Declarator::Array(Box::new(direct), bound);
            } else if self.peek()?.is_some_and(|token| token.text.as_ref() == "(") {
                self.take()?;
                let identifiers = allow_old_style && matches!(&direct, Declarator::Name(..))
                    && self.peek()?.is_some_and(|t| t.kind == TokenKind::Identifier
                        && !matches!(t.text.as_ref(), "void" | "_Bool" | "char" | "short" | "int" | "long"
                            | "signed" | "unsigned" | "float" | "double" | "_Float128"
                            | "const" | "volatile" | "restrict" | "__restrict" | "__restrict__"
                            | "_Atomic" | "struct" | "union" | "enum" | "register" | "__builtin_va_list"));
                let unprototyped = identifiers || self.peek()?.is_some_and(|t| t.text.as_ref() == ")");
                let (params, variadic) = if identifiers {
                    (self.parse_identifier_parameters()?, false)
                } else { self.parse_parameter_list()? };
                self.expect(")")?;
                direct = Declarator::Function(Box::new(direct), params, variadic, unprototyped);
            } else if matches!(&direct, Declarator::Function(_, _, _, _))
                && self.peek()?.is_some_and(|token|
                    token.text.as_ref() == "__attribute__" || token.text.as_ref() == "__attribute")
            {
                self.parse_gnu_declarator_attributes(
                    match &direct {
                        Declarator::Function(_, params, variadic, _) => Some((params.len(), *variadic)),
                        _ => None,
                    }, None
                )?;
            } else {
                break;
            }
        }
        // The first written '*' is the outermost pointer. Apply the
        // declarator operators in reverse so that 'int **const p' qualifies
        // the pointer variable, not its inner pointee pointer.
        for qualifiers in stars.into_iter().rev() {
            direct = Declarator::Pointer(Box::new(direct), qualifiers);
        }
        Ok(direct)
    }

    /// GNU declaration symbol alias. This changes the ELF symbol, so unlike
    /// advisory GNU attributes it must be preserved through native lowering.
    /// GCC accepts adjacent string literals, including an empty prefix.
    fn parse_asm_linkage_label(&mut self) -> Result<Option<String>, Diagnostic> {
        if !self.peek()?.is_some_and(|t| matches!(t.text.as_ref(), "__asm__" | "__asm")) {
            return Ok(None);
        }
        let keyword = self.take()?.expect("peeked asm");
        self.expect("(")?;
        let mut bytes = Vec::new();
        let mut fragments = 0usize;
        while self.peek()?.is_some_and(|t| t.kind == TokenKind::String) {
            let token = self.take()?.expect("peeked string");
            let decoded = crate::string_literal::decode_narrow_string(token.text.as_ref())
                .map_err(|reason| Diagnostic::coded(token.span,
                    DiagnosticCode::LexicalInvalidCharacter, reason))?;
            if decoded[..decoded.len().saturating_sub(1)].contains(&0) {
                return Err(Diagnostic::coded(token.span,
                    DiagnosticCode::SyntaxInvalidDeclaration,
                    "GNU asm linker symbol cannot contain NUL"));
            }
            bytes.extend_from_slice(&decoded[..decoded.len()-1]);
            fragments += 1;
        }
        if fragments == 0 || bytes.is_empty() || bytes.len() > 4096 {
            return Err(Diagnostic::coded(keyword.span,
                DiagnosticCode::SyntaxInvalidDeclaration,
                "GNU asm linker label requires a nonempty bounded string literal"));
        }
        self.expect(")")?;
        let name = String::from_utf8(bytes).map_err(|_| Diagnostic::coded(
            keyword.span, DiagnosticCode::SyntaxInvalidDeclaration,
            "GNU asm linker label must be UTF-8"))?;
        Ok(Some(name))
    }

    // Identifier-list definitions are parsed only on file-scope declarators.
    // Void placeholders cannot escape into the semantic checker: the following
    // declaration list must supply exactly one declaration for each name.
    fn parse_identifier_parameters(&mut self) -> Result<Vec<Decl>, Diagnostic> {
        let mut params: Vec<Decl> = Vec::new();
        loop {
            let token = self.take()?.ok_or_else(|| Diagnostic::new(self.eof_span(), "expected parameter identifier"))?;
            if token.kind != TokenKind::Identifier || matches!(token.text.as_ref(),
                "auto" | "break" | "case" | "char" | "const" | "continue" | "default" | "do"
                | "double" | "else" | "enum" | "extern" | "float" | "for" | "goto" | "if"
                | "inline" | "int" | "long" | "register" | "restrict" | "return" | "short"
                | "signed" | "sizeof" | "static" | "struct" | "switch" | "typedef" | "union"
                | "unsigned" | "void" | "volatile" | "while" | "_Bool" | "_Complex" | "_Imaginary"
                | "_Atomic" | "_Float128" | "__builtin_va_list" | "__restrict" | "__restrict__") {
                return Err(Diagnostic::new(token.span, "expected old-style parameter identifier"));
            }
            let name = token.text.to_string();
            if params.iter().any(|p| p.name == name) {
                return Err(Diagnostic::new(token.span, "duplicate old-style parameter identifier"));
            }
            params.push(Decl { storage: StorageClass::Default, is_inline: false, name,
                name_span: self.expression_spans.then_some(token.span), ty: Type::Void,
                asm_label: None, init: None, span: token.span });
            if !self.peek()?.is_some_and(|t| t.text.as_ref() == ",") { break; }
            self.take()?;
        }
        Ok(params)
    }

    fn parse_parameter_list(&mut self) -> Result<(Vec<Decl>, bool), Diagnostic> {
        let mut params = Vec::new();
        if self.peek()?.is_some_and(|token| token.text.as_ref() == ")") {
            return Ok((params, false));
        }
        loop {
            let token_span = self.peek()?.map_or(self.eof_span(), |token| token.span);
            if self.peek()?.is_some_and(|token| matches!(token.text.as_ref(), "," | ")")) {
                return Err(Diagnostic::coded(token_span, DiagnosticCode::SyntaxExpectedParameter,
                    "expected parameter type"));
            }
            let storage = if self.peek()?.is_some_and(|t| t.text.as_ref() == "register") {
                self.take()?;
                StorageClass::Register
            } else {
                StorageClass::Default
            };
            let (base, lo) = self.parse_type()?
                .ok_or_else(|| Diagnostic::coded(self.eof_span(),
                    DiagnosticCode::SyntaxExpectedParameter, "expected parameter type"))?;
            // The entire parameter list (void) denotes no parameters.
            if base == Type::Void && params.is_empty() && storage == StorageClass::Default {
                if self.peek()?.is_some_and(|token| token.text.as_ref() == ")") {
                    return Ok((params, false));
                }
                // An incomplete (void at EOF is a missing ')', not an
                // invalid void parameter with an absent declarator.
                if self.peek()?.is_none() {
                    return Err(Diagnostic::new(self.eof_span(), "expected ')'"));
                }
            }
            let (name, ty, _, name_span) = self.parse_declarator_shape(true)?.finish(base);
            if ty == Type::Void {
                return Err(Diagnostic::coded(token_span, DiagnosticCode::SyntaxExpectedParameter,
                    "parameter cannot have void type"));
            }
            params.push(Decl {
                is_inline: false,
                storage,
                name,
                name_span,
                ty,
                asm_label: None,
                init: None,
                span: Span { lo, hi: lo },
            });
            if !self.peek()?.is_some_and(|token| token.text.as_ref() == ",") {
                break;
            }
            self.take()?;
            if self.peek()?.is_some_and(|token| token.text.as_ref() == "...") {
                self.take()?;
                return Ok((params, true));
            }
        }
        Ok((params, false))
    }

    /// Parse C braced initializers recursively without enabling comma
    /// expressions in the ordinary expression grammar.
    fn parse_initializer(&mut self) -> Result<Expr, Diagnostic> {
        if !self.peek()?.is_some_and(|t| t.text.as_ref() == "{") {
            return self.parse_assign();
        }
        let open = self.take()?.expect("peeked opening brace");
        let mut values = Vec::new();
        if self.peek()?.is_some_and(|t| t.text.as_ref() == "}") {
            return Err(Diagnostic::new(self.peek()?.expect("peeked").span,
                "empty aggregate initializers are unsupported"));
        }
        loop {
            let mut path = Vec::new();
            let start = self.peek()?.map_or(self.eof_span(), |t| t.span);
            while let Some(token) = self.peek()? {
                match token.text.as_ref() {
                    "." => {
                        self.take()?;
                        let field = self.take()?.ok_or_else(||
                            Diagnostic::new(self.eof_span(), "expected field designator name"))?;
                        if !matches!(field.kind, TokenKind::Identifier | TokenKind::TypedefName) {
                            return Err(Diagnostic::new(field.span, "expected field designator name"));
                        }
                        path.push(Designator::Field(field.text.to_string()));
                    }
                    "[" => {
                        self.take()?;
                        let expr = self.parse_conditional()?;
                        self.expect("]")?;
                        path.push(Designator::Index(Box::new(expr)));
                    }
                    _ => break,
                }
            }
            let value = if path.is_empty() {
                self.parse_initializer()?
            } else {
                self.expect("=")?;
                let value = self.parse_initializer()?;
                self.located(Expr::Designated { path, value: Box::new(value) },
                    Span { lo: start.lo, hi: start.hi })
            };
            values.push(value);
            if !self.peek()?.is_some_and(|t| t.text.as_ref() == ",") { break; }
            self.take()?;
            if self.peek()?.is_some_and(|t| t.text.as_ref() == "}") { break; }
        }
        let close = self.expect("}")?;
        Ok(self.located(Expr::Initializer(values),
            Span { lo: open.span.lo, hi: close.span.hi }))
    }

    fn parse_decl(&mut self, is_typedef: bool) -> Result<Decl, Diagnostic> {
        let mut declarations = self.parse_decl_group(is_typedef)?;
        if declarations.len() != 1 {
            return Err(Diagnostic::coded(
                declarations[1].name_span.unwrap_or(declarations[1].span),
                DiagnosticCode::SyntaxInvalidDeclaration,
                "multiple declarators are supported only inside a block",
            ));
        }
        Ok(declarations.pop().expect("nonempty declaration group"))
    }

    /// ISO C declarations share one type specifier and storage class, but
    /// each comma-separated declarator has its *own* pointer/array shape and
    /// initializer. Register every name before its own initializer; this
    /// preserves typedef shadowing and ordinary C declaration scope.
    /// Callers inside blocks flatten these into statements in the same scope.
    fn parse_decl_group(&mut self, is_typedef: bool) -> Result<Vec<Decl>, Diagnostic> {
        let storage_span = self.peek()?.map_or(self.eof_span(), |t| t.span);
        let storage = match self.peek()?.map(|t| t.text).as_deref() {
            Some("static") => { self.take()?; StorageClass::Static },
            Some("extern") => { self.take()?; StorageClass::Extern },
            Some("register") => { self.take()?; StorageClass::Register },
            _ => StorageClass::Default,
        };
        if is_typedef && storage != StorageClass::Default {
            return Err(Diagnostic::coded(storage_span, DiagnosticCode::SyntaxInvalidDeclaration,
                "typedef cannot combine with storage class"));
        }
        let (base, start) = self.parse_type()?
            .ok_or_else(|| Diagnostic::new(self.eof_span(), "expected type"))?;
        let mut declarations = Vec::new();
        loop {
            let (name, ty, _, name_span) =
                self.parse_declarator_shape(false)?.finish(base.clone());
            self.scopes.declare(&name,
                if is_typedef { Binding::Typedef } else { Binding::Ordinary });
            let initializer_operator = self.peek()?.filter(|t| t.text.as_ref() == "=").map(|t| t.span);
            let init = if initializer_operator.is_some() {
                self.take()?;
                Some(self.parse_initializer()?)
            } else {
                None
            };
            if is_typedef {
                if let Some(span) = initializer_operator {
                    return Err(Diagnostic::coded(span, DiagnosticCode::SyntaxInvalidDeclaration,
                        "typedef cannot have an initializer"));
                }
            }
            declarations.push(Decl {
                is_inline: false,
                storage, name, name_span, ty, asm_label: None, init,
                span: Span { lo: start, hi: start },
            });
            if self.peek()?.is_some_and(|t| t.text.as_ref() == ",") {
                self.take()?;
                continue;
            }
            let end = self.expect(";")?.span.hi;
            for declaration in &mut declarations {
                declaration.span.hi = end;
            }
            return Ok(declarations);
        }
    }

    fn parse_tag_or_decl(&mut self) -> Result<Stmt, Diagnostic> {
        let (base, start) = self.parse_type()?
            .ok_or_else(|| Diagnostic::new(self.eof_span(), "expected tag type"))?;
        if self.peek()?.is_some_and(|t| t.text.as_ref() == ";") {
            self.take()?;
            return Ok(Stmt::Tag(base));
        }
        let (name, ty, _, name_span) = self.parse_declarator_shape(false)?.finish(base.clone());
        self.scopes.declare(&name, Binding::Ordinary);
        let init = if self.peek()?.is_some_and(|t| t.text.as_ref() == "=") {
            self.take()?;
            Some(self.parse_initializer()?)
        } else { None };
        let end = self.expect(";")?.span.hi;
        Ok(Stmt::Decl(Decl {
            is_inline: false,
            storage: StorageClass::Default, name, name_span, ty, asm_label: None, init,
            span: Span { lo: start, hi: end },
        }))
    }

    /// GNU basic asm is a *statement*, not a declaration-level symbol alias.
    /// Preserve the decoded template exactly, including adjacent C strings.
    /// Extended-asm operands and asm goto are not silently accepted.
    /// Parse a positional or [named] GNU operand without decoding its asm body.
    fn parse_asm_operand(&mut self) -> Result<AsmOperand, Diagnostic> {
        let operand_name=if self.peek()?.is_some_and(|t| t.text.as_ref()=="[") {
            self.expect("[")?;
            let token=self.take()?.ok_or_else(||Diagnostic::new(
                self.eof_span(),"expected GNU asm operand name"))?;
            if token.kind!=TokenKind::Identifier {
                return Err(Diagnostic::new(token.span,"GNU asm operand name must be an identifier"));
            }
            let name=token.text.to_string();
            self.expect("]")?;
            Some(name)
        } else { None };
        let token=self.take()?.ok_or_else(||Diagnostic::new(
            self.eof_span(),"expected asm constraint string"))?;
        if token.kind!=TokenKind::String {
            return Err(Diagnostic::new(token.span,"asm constraint must be a string literal"));
        }
        let bytes=crate::string_literal::decode_narrow_string(token.text.as_ref())
            .map_err(|reason|Diagnostic::new(token.span,reason))?;
        let name=String::from_utf8(bytes[..bytes.len()-1].to_vec())
            .map_err(|_|Diagnostic::new(token.span,"asm constraint must be UTF-8"))?;
        if name.contains('\0') {
            return Err(Diagnostic::new(token.span,"asm constraint contains NUL"));
        }
        self.expect("(")?;
        let expr=self.parse_assign()?;
        let end=self.expect(")")?;
        Ok(AsmOperand {name:operand_name,constraint:name,expr,span:Span{
            lo:token.span.lo,hi:end.span.hi,
        }})
    }

    fn parse_basic_asm_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let keyword = self.take()?.expect("peeked basic asm keyword");
        // GNU basic asm is implicitly volatile. Accept the explicit synonym
        // while retaining just the template until the effect IR is implemented.
        if self.peek()?.is_some_and(|token| matches!(
            token.text.as_ref(), "volatile" | "__volatile" | "__volatile__"
        )) {
            self.take()?;
        }
        self.expect("(")?;
        let mut bytes = Vec::new();
        let mut fragments = 0usize;
        while self.peek()?.is_some_and(|token| token.kind == TokenKind::String) {
            let token = self.take()?.expect("peeked basic asm string");
            let decoded = crate::string_literal::decode_narrow_string(token.text.as_ref())
                .map_err(|reason| Diagnostic::new(token.span, reason))?;
            bytes.extend_from_slice(&decoded[..decoded.len()-1]);
            if bytes.len() > 16384 {
                return Err(Diagnostic::new(token.span, "basic asm template exceeds 16KiB"));
            }
            fragments += 1;
        }
        if fragments == 0 {
            return Err(Diagnostic::new(keyword.span,
                "basic asm requires a literal string template"));
        }
        // A NUL inside source text would truncate assembler input.
        if bytes.contains(&0) {
            return Err(Diagnostic::new(keyword.span, "basic asm template contains NUL"));
        }
        let template = String::from_utf8(bytes).map_err(|_| Diagnostic::new(
            keyword.span, "basic asm template must be UTF-8"))?;
        // GCC extended asm has three colon-separated operand/clobber
        // sections. Only the no-output/no-input subset is admitted here.
        // Each empty section is still explicitly required; the syntax
        // cannot silently reinterpret an operand expression as a clobber.
        let extended=self.peek()?.is_some_and(|t|t.text.as_ref()==":");
        let mut outputs=Vec::new();
        let mut inputs=Vec::new();
        let mut clobbers=Vec::new();
        if extended {
            self.expect(":")?;
            if self.peek()?.is_some_and(|t|t.kind==TokenKind::String || t.text.as_ref()=="[") {
                // Parse all syntactic output operands; the semantic checker
                // (not this parser) establishes whether the target supports
                // the resulting GP/memory multi-output constraint contract.
                loop {
                    outputs.push(self.parse_asm_operand()?);
                    if !self.peek()?.is_some_and(|t|t.text.as_ref()==",") {
                        break;
                    }
                    self.take()?;
                    if !self.peek()?.is_some_and(|t|
                        t.kind==TokenKind::String || t.text.as_ref()=="[")
                    {
                        return Err(Diagnostic::new(self.eof_span(),
                            "GNU asm output list requires operand after comma"));
                    }
                }
            }
            self.expect(":")?;
            if self.peek()?.is_some_and(|t|t.kind==TokenKind::String || t.text.as_ref()=="[") {
                loop {
                    inputs.push(self.parse_asm_operand()?);
                    if inputs.len()>2 {
                        return Err(Diagnostic::new(self.eof_span(),
                            "ASM2 extended asm permits at most two input operands"));
                    }
                    if !self.peek()?.is_some_and(|t|t.text.as_ref()==",") {
                        break;
                    }
                    self.take()?;
                    if !self.peek()?.is_some_and(|t|t.kind==TokenKind::String || t.text.as_ref()=="[") {
                        return Err(Diagnostic::new(self.eof_span(),
                            "ASM2 asm input list requires a constraint string after comma"));
                    }
                }
            }
            if self.peek()?.is_some_and(|t|t.text.as_ref()==":") {
                self.expect(":")?;
            } else if !self.peek()?.is_some_and(|t|t.text.as_ref()==")") {
                return Err(Diagnostic::new(self.eof_span(),"expected asm clobber ':' or ')'"));
            }
            if self.peek()?.is_some_and(|t|t.kind==TokenKind::String) {
                loop {
                    let token=self.take()?.expect("peeked asm clobber string");
                    let bytes=crate::string_literal::decode_narrow_string(token.text.as_ref())
                        .map_err(|reason|Diagnostic::new(token.span,reason))?;
                    if bytes[..bytes.len()-1].contains(&0) {
                        return Err(Diagnostic::new(token.span,"asm clobber contains NUL"));
                    }
                    let name=String::from_utf8(bytes[..bytes.len()-1].to_vec())
                        .map_err(|_|Diagnostic::new(token.span,"asm clobber must be UTF-8"))?;
                    clobbers.push(name);
                    if clobbers.len()>32 {
                        return Err(Diagnostic::new(token.span,"too many asm clobbers"));
                    }
                    if !self.peek()?.is_some_and(|t|t.text.as_ref()==",") {
                        break;
                    }
                    self.take()?;
                    if !self.peek()?.is_some_and(|t|t.kind==TokenKind::String) {
                        return Err(Diagnostic::new(token.span,"asm clobber list expects string after comma"));
                    }
                }
            }
        }
        self.expect(")")?;
        let semi = self.expect(";")?;
        let span=Span {lo:keyword.span.lo,hi:semi.span.hi};
        if extended {
            Ok(Stmt::ExtendedAsm {template,outputs,inputs,clobbers,span})
        } else {
            Ok(Stmt::BasicAsm {template,span})
        }
    }

    fn parse_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let token = self.peek()?.ok_or_else(|| Diagnostic::new(self.eof_span(), "expected statement"))?;
        match token.text.as_ref() {
            "{" => Ok(self.parse_block()?.0),
            "asm" | "__asm" | "__asm__" => self.parse_basic_asm_stmt(),
            ";" => { self.take()?; Ok(Stmt::Expr(None)) },
            "goto" => {
                self.take()?;
                let target = self.take()?.ok_or_else(||
                    Diagnostic::new(self.eof_span(), "goto requires a label identifier"))?;
                if !matches!(target.kind, TokenKind::Identifier | TokenKind::TypedefName) {
                    return Err(Diagnostic::new(target.span, "goto requires a label identifier"));
                }
                self.expect(";")?;
                Ok(Stmt::Goto { target: target.text.to_string(), span: target.span })
            }
            // Clang-compatible C/C++ statement attribute. C99 has no
            // standard musttail keyword; parse the explicit qualified
            // spelling and retain it as an AST obligation.
            "[" => {
                let start=self.take()?.expect("attribute bracket was peeked").span;
                self.expect("[")?;
                let namespace=self.take()?.ok_or_else(||
                    Diagnostic::new(start,"musttail attribute requires clang namespace"))?;
                if namespace.text.as_ref()!="clang" {
                    return Err(Diagnostic::new(start,
                        "unsupported statement attribute namespace (expected clang::musttail)"));
                }
                self.expect(":")?;
                self.expect(":")?;
                self.expect("musttail")?;
                self.expect("]")?;
                self.expect("]")?;
                self.expect("return")?;
                if self.peek()?.is_some_and(|t|t.text.as_ref()==";") {
                    return Err(Diagnostic::new(start,
                        "clang::musttail return requires a direct call expression"));
                }
                let value=self.parse_expr()?;
                self.expect(";")?;
                Ok(Stmt::MustTailReturn(value))
            }
            "return" => {
                self.take()?;
                let value = if self.peek()?.is_some_and(|t| t.text.as_ref() == ";") { None } else { Some(self.parse_expr()?) };
                self.expect(";")?;
                Ok(Stmt::Return(value))
            }
            "if" => {
                self.take()?; self.expect("(")?;
                let cond = self.parse_expr()?;
                self.expect(")")?;
                let then = Box::new(self.parse_stmt()?);
                let otherwise = if self.peek()?.is_some_and(|t| t.text.as_ref() == "else") {
                    self.take()?; Some(Box::new(self.parse_stmt()?))
                } else { None };
                Ok(Stmt::If { cond, then, otherwise })
            }
            "switch" => {
                self.take()?; self.expect("(")?;
                let control = self.parse_expr()?;
                self.expect(")")?;
                let body = Box::new(self.parse_stmt()?);
                Ok(Stmt::Switch { control, body })
            }
            "case" => {
                self.take()?;
                let value = self.parse_conditional()?;
                self.expect(":")?;
                Ok(Stmt::Case { value, body: Box::new(self.parse_stmt()?) })
            }
            "default" => {
                self.take()?;
                self.expect(":")?;
                Ok(Stmt::Default(Box::new(self.parse_stmt()?)))
            }
            "while" => {
                self.take()?; self.expect("(")?;
                let cond = self.parse_expr()?;
                self.expect(")")?;
                let body = Box::new(self.parse_stmt()?);
                Ok(Stmt::While { cond, body })
            }
            "for" => {
                self.take()?;
                self.expect("(")?;
                // A for-initializer declaration has its own scope, including
                // typedef classification. Restore it even on syntax errors.
                self.scopes.enter();
                let parsed = (|| {
                    let init = if self.peek()?.is_some_and(|t| t.text.as_ref() == ";") {
                        self.take()?;
                        None
                    } else {
                        let token = self.peek()?.expect("for initializer token");
                        let stmt = match token.text.as_ref() {
                            "typedef" => {
                                self.take()?;
                                Stmt::Typedef(self.parse_decl(true)?)
                            }
                            "struct" | "union" | "enum" => self.parse_tag_or_decl()?,
                            "static" | "extern" | "register" | "int" | "_Bool" | "char" | "long"
                                | "void" | "unsigned" | "signed" | "const" | "volatile" | "restrict" | "__restrict" | "__restrict__" =>
                                Stmt::Decl(self.parse_decl(false)?),
                            _ if token.kind == TokenKind::TypedefName =>
                                Stmt::Decl(self.parse_decl(false)?),
                            _ => {
                                let expr = self.parse_expr()?;
                                self.expect(";")?;
                                Stmt::Expr(Some(expr))
                            }
                        };
                        Some(Box::new(stmt))
                    };
                    let cond = if self.peek()?.is_some_and(|t| t.text.as_ref() == ";") {
                        None
                    } else {
                        Some(self.parse_expr()?)
                    };
                    self.expect(";")?;
                    let step = if self.peek()?.is_some_and(|t| t.text.as_ref() == ")") {
                        None
                    } else {
                        Some(self.parse_expr()?)
                    };
                    self.expect(")")?;
                    let body = Box::new(self.parse_stmt()?);
                    Ok(Stmt::For { init, cond, step, body })
                })();
                self.scopes.leave();
                parsed
            }
            "do" => {
                self.take()?;
                let body = Box::new(self.parse_stmt()?);
                self.expect("while")?;
                self.expect("(")?;
                let cond = self.parse_expr()?;
                self.expect(")")?;
                self.expect(";")?;
                Ok(Stmt::DoWhile { body, cond })
            }
            "break" => { self.take()?; self.expect(";")?; Ok(Stmt::Break) }
            "continue" => { self.take()?; self.expect(";")?; Ok(Stmt::Continue) }
            "typedef" => { self.take()?; Ok(Stmt::Typedef(self.parse_decl(true)?)) },
            "static" | "extern" | "register" => Ok(Stmt::Decl(self.parse_decl(false)?)),
            "struct" | "union" | "enum" => self.parse_tag_or_decl(),
            "int" | "_Bool" | "char" | "short" | "long" | "_Float128" | "float" | "double" | "void" | "unsigned" | "signed"
                | "const" | "volatile" | "restrict" | "__restrict" | "__restrict__" => Ok(Stmt::Decl(self.parse_decl(false)?)),
            _ if matches!(token.kind, TokenKind::Identifier | TokenKind::TypedefName)
                && self.followed_by_colon()? => {
                let name = self.take()?.expect("peeked label");
                self.expect(":")?;
                // ISO C99 permits labels on statements, not declarations.
                let next = self.peek()?;
                let is_decl_keyword = next.as_ref().is_some_and(|token| matches!(token.text.as_ref(),
                    "typedef" | "static" | "extern" | "register" | "struct" | "union" | "enum" |
                    "int" | "_Bool" | "char" | "short" | "long" | "_Float128" |
                    "float" | "double" | "void" | "unsigned" | "signed" |
                    "const" | "volatile" | "restrict" | "__restrict" | "__restrict__"));
                let is_typedef_decl = next.is_some_and(|token| token.kind == TokenKind::TypedefName)
                    && !self.followed_by_colon()?;
                if is_decl_keyword || is_typedef_decl {
                    return Err(Diagnostic::new(name.span, "C99 label must precede a statement, not a declaration"));
                }
                Ok(Stmt::Label { name: name.text.to_string(), span: name.span,
                    body: Box::new(self.parse_stmt()?) })
            }
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
                if token.text.as_ref() == "}" {
                    let end = self.take()?.expect("peeked token").span.hi;
                    return Ok((Stmt::Block(statements), end));
                }
                // A declaration-list is one C block item, not a nested
                // scope. Flatten its individual checked declarations into
                // this same block so later statements can use every name.
                let declaration = match token.text.as_ref() {
                    "typedef" => {
                        self.take()?;
                        Some(true)
                    }
                    "static" | "extern" | "register" | "int" | "_Bool" | "char" | "short"
                    | "long" | "_Float128" | "float" | "double" | "void"
                    | "unsigned" | "signed" | "const" | "volatile" | "restrict" | "_Atomic"
                    | "__restrict" | "__restrict__" => Some(false),
                    _ if token.kind == TokenKind::TypedefName
                        && !self.followed_by_colon()? => Some(false),
                    _ => None,
                };
                if let Some(is_typedef) = declaration {
                    for decl in self.parse_decl_group(is_typedef)? {
                        statements.push(if is_typedef { Stmt::Typedef(decl) }
                            else { Stmt::Decl(decl) });
                    }
                } else {
                    statements.push(self.parse_stmt()?);
                }
            }
        })();
        self.scopes.leave();
        parsed
    }

    fn parse_type(&mut self) -> Result<Option<(Type, usize)>, Diagnostic> {
        let Some(first) = self.peek()? else {
            return Ok(None);
        };
        let mut qualifiers = self.parse_qualifiers()?;
        let token = self.peek()?.ok_or_else(|| Diagnostic::new(
            first.span, "type qualifier requires a type specifier"
        ))?;
        let ty = match token.text.as_ref() {
            "_Atomic" => {
                // C11 _Atomic(type-name) is a type specifier, not a qualifier.
                // Retain it in the AST so semantic analysis cannot mistake
                // these accesses for ordinary memory operations.
                self.take()?;
                self.expect("(")?;
                let (base, _) = self.parse_type()?.ok_or_else(|| {
                    Diagnostic::coded(self.eof_span(),
                        DiagnosticCode::SyntaxInvalidDeclaration,
                        "expected type name in _Atomic(...)")
                })?;
                let (name, inner, nested, _) =
                    self.parse_declarator_shape(true)?.finish(base);
                if !name.is_empty() || nested.is_some() {
                    return Err(Diagnostic::coded(token.span,
                        DiagnosticCode::SyntaxInvalidDeclaration,
                        "_Atomic(...) requires a type name"));
                }
                self.expect(")")?;
                Type::Atomic(Box::new(inner))
            }
            "enum" => {
                self.take()?;
                let tag = self.peek()?.filter(|t| matches!(t.kind,
                    TokenKind::Identifier | TokenKind::TypedefName))
                    .map(|t| t.text.to_string());
                if tag.is_some() { self.take()?; }
                if !self.peek()?.is_some_and(|t| t.text.as_ref() == "{") {
                    let tag = tag.ok_or_else(|| Diagnostic::new(token.span,
                        "anonymous enum requires enumerators"))?;
                    let mut ty = Type::TagRef(TagKind::Enum, tag);
                    let trailing = self.parse_qualifiers()?;
                    qualifiers.is_const |= trailing.is_const;
                    qualifiers.is_volatile |= trailing.is_volatile;
                    qualifiers.is_restrict |= trailing.is_restrict;
                    qualifiers.is_atomic |= trailing.is_atomic;
                    if qualifiers.any() { ty = Type::Qualified(Box::new(ty), qualifiers); }
                    return Ok(Some((ty, first.span.lo)));
                }
                self.take()?;
                if self.peek()?.is_some_and(|t| t.text.as_ref() == "}") {
                    return Err(Diagnostic::new(token.span, "empty enums are unsupported"));
                }
                let mut enumerators = Vec::new();
                loop {
                    let name = self.take()?.ok_or_else(||
                        Diagnostic::new(self.eof_span(), "unterminated enum definition"))?;
                    if !matches!(name.kind, TokenKind::Identifier | TokenKind::TypedefName) {
                        return Err(Diagnostic::new(name.span, "expected enumerator name"));
                    }
                    let value = if self.peek()?.is_some_and(|t| t.text.as_ref() == "=") {
                        self.take()?;
                        Some(self.parse_assign()?)
                    } else { None };
                    enumerators.push((name.text.to_string(), value));
                    if !self.peek()?.is_some_and(|t| t.text.as_ref() == ",") { break; }
                    self.take()?;
                    if self.peek()?.is_some_and(|t| t.text.as_ref() == "}") { break; }
                }
                self.expect("}")?;
                Type::Enum { tag, enumerators }
            }
            "struct" | "union" => {
                let is_union = token.text.as_ref() == "union";
                self.take()?;
                let tag = self.peek()?.filter(|t| matches!(t.kind,
                    TokenKind::Identifier | TokenKind::TypedefName))
                    .map(|t| t.text.to_string());
                if tag.is_some() { self.take()?; }
                if !self.peek()?.is_some_and(|t| t.text.as_ref() == "{") {
                    let tag = tag.ok_or_else(|| Diagnostic::new(token.span,
                        "anonymous struct or union requires a member definition"))?;
                    let kind = if is_union { TagKind::Union } else { TagKind::Struct };
                    let mut ty = Type::TagRef(kind, tag);
                    let trailing = self.parse_qualifiers()?;
                    qualifiers.is_const |= trailing.is_const;
                    qualifiers.is_volatile |= trailing.is_volatile;
                    qualifiers.is_restrict |= trailing.is_restrict;
                    qualifiers.is_atomic |= trailing.is_atomic;
                    if qualifiers.any() { ty = Type::Qualified(Box::new(ty), qualifiers); }
                    return Ok(Some((ty, first.span.lo)));
                }
                self.take()?;
                let mut members = Vec::new();
                while !self.peek()?.is_some_and(|t| t.text.as_ref() == "}") {
                    // glibc uses __extension__ on 64-bit union members to
                    // suppress pedantic diagnostics. It does not alter member
                    // type, alignment, offsets or ABI and is not a qualifier.
                    if self.peek()?.is_some_and(|t| t.text.as_ref() == "__extension__") {
                        self.take()?;
                    }
                    let (base, start) = self.parse_type()?.ok_or_else(||
                        Diagnostic::new(self.eof_span(), "unterminated struct definition"))?;
                    // A C struct/union member declaration has a *shared* base
                    // specifier but independently bound pointer/array declarators:
                    //   intmax_t quot, rem;   int *p, a[3];
                    // Preserve their source order for checked layout. Each
                    // member is parsed and checked; no implicit initializer,
                    // named function member or unsupported bit-field admitted.
                    let first = members.len();
                    loop {
                        let unnamed_bitfield = self.peek()?.is_some_and(|t| t.text.as_ref() == ":");
                        let (name, ty, params, name_span) =
                            if unnamed_bitfield {
                                (String::new(), base.clone(), None, None)
                            } else {
                                self.parse_declarator_shape(false)?.finish(base.clone())
                            };
                        if params.is_some() || matches!(ty, Type::Function { .. } | Type::VariadicFunction { .. } | Type::UnprototypedFunction { .. }) {
                            return Err(Diagnostic::new(Span { lo: start, hi: start },
                                "struct member cannot have function type"));
                        }
                        let ty = if self.peek()?.is_some_and(|t| t.text.as_ref() == ":") {
                            self.take()?;
                            let width = self.parse_assign()?;
                            Type::BitField(Box::new(ty), Box::new(width))
                        } else {
                            ty
                        };
                        let ty = if self.peek()?.is_some_and(|t|
                            matches!(t.text.as_ref(), "__attribute__" | "__attribute")) {
                            // Field alignment affects layout, so retain the
                            // request until target-aware semantic checking.
                            self.take()?;
                            self.expect("(")?;
                            self.expect("(")?;
                            let attr = self.take()?.ok_or_else(||
                                Diagnostic::new(self.eof_span(), "expected member attribute"))?;
                            if gnu_attribute_spelling(attr.text.as_ref()) != "aligned" {
                                return Err(Diagnostic::new(attr.span,
                                    "unsupported GNU member attribute"));
                            }
                            self.expect("(")?;
                            let alignment = if self.peek()?.is_some_and(|t|
                                matches!(t.text.as_ref(), "__alignof__" | "__alignof" | "_Alignof")) {
                                self.take()?;
                                self.expect("(")?;
                                let (base, _) = self.parse_type()?.ok_or_else(||
                                    Diagnostic::new(self.eof_span(), "alignment requires a type name"))?;
                                let (name, ty, params, _) = self.parse_declarator_shape(true)?.finish(base);
                                if !name.is_empty() || params.is_some() {
                                    return Err(Diagnostic::new(attr.span, "alignment requires an abstract type name"));
                                }
                                self.expect(")")?;
                                MemberAlignment::AlignOf(Box::new(ty))
                            } else {
                                MemberAlignment::Constant(Box::new(self.parse_assign()?))
                            };
                            self.expect(")")?;
                            self.expect(")")?;
                            self.expect(")")?;
                            Type::MemberAligned(Box::new(ty), alignment)
                        } else { ty };
                        members.push(Decl {
                            is_inline: false,
                            storage: StorageClass::Default, name, name_span, ty,
                            asm_label: None, init: None,
                            span: Span { lo: start, hi: start },
                        });
                        if self.peek()?.is_some_and(|t| t.text.as_ref() == ",") {
                            self.take()?;
                            continue;
                        }
                        let end = self.expect(";")?.span.hi;
                        for member in &mut members[first..] {
                            member.span.hi = end;
                        }
                        break;
                    }
                }
                self.expect("}")?;
                if members.is_empty() {
                    return Err(Diagnostic::new(token.span, "empty structs are unsupported"));
                }
                if is_union { Type::Union { tag, members } }
                else { Type::Struct { tag, members } }
            }
            "void" => { self.take()?; Type::Void }
            "_Bool" => { self.take()?; Type::Bool }
            "float" => { self.take()?; Type::Floating(FloatRank::Float) }
            "__builtin_va_list" => { self.take()?; Type::BuiltinVaList }
            "_Float128" => { self.take()?; Type::Floating(FloatRank::Float128) }
            "double" => {
                self.take()?;
                if self.peek()?.is_some_and(|t| t.text.as_ref() == "long") {
                    self.take()?;
                    Type::Floating(FloatRank::LongDouble)
                } else { Type::Floating(FloatRank::Double) }
            }
            "char" | "short" | "int" | "long" | "unsigned" | "signed" => {
                // C integer declaration specifiers are order-independent:
                // glibc's standard typedef 'long unsigned int size_t' must
                // mean exactly the same type as 'unsigned long int'.
                // Fail closed on duplicates and unsupported long long rather
                // than quietly treating a second specifier as a declarator.
                let mut is_char = false;
                let mut is_short = false;
                let mut is_int = false;
                let mut is_long_double = false;
                let mut long_count = 0u8;
                let mut sign: Option<bool> = None;
                while let Some(next) = self.peek()? {
                    match next.text.as_ref() {
                        "char" if !is_char && !is_int && !is_short && long_count == 0 => is_char = true,
                        "short" if !is_short && !is_char && long_count == 0 => is_short = true,
                        "int" if !is_int && !is_char => is_int = true,
                        "long" if long_count < 2 && !is_char && !is_short => long_count += 1,
                        // C17 permits either 'long double' or 'double long'.
                        "double" if long_count == 1 && !is_int && sign.is_none() => {
                            is_long_double = true;
                            self.take()?;
                            break;
                        }
                        "unsigned" if sign.is_none() => sign = Some(true),
                        "signed" if sign.is_none() => sign = Some(false),
                        "const" => qualifiers.is_const = true,
                        "volatile" => qualifiers.is_volatile = true,
                        "_Atomic" => qualifiers.is_atomic = true,
                        "restrict" | "__restrict" | "__restrict__" =>
                            qualifiers.is_restrict = true,
                        "char" | "short" | "int" | "long" | "unsigned" | "signed" => {
                            return Err(Diagnostic::coded(next.span,
                                DiagnosticCode::SyntaxInvalidDeclaration,
                                "invalid or unsupported integer type-specifier combination"));
                        }
                        _ => break,
                    }
                    self.take()?;
                }
                let base = if is_long_double { Type::Floating(FloatRank::LongDouble) }
                    else if is_char { Type::Char }
                    else if is_short { Type::Short }
                    else if long_count == 2 { Type::LongLong }
                    else if long_count == 1 { Type::Long }
                    else { Type::Int };
                match sign {
                    Some(true) => Type::Unsigned(Box::new(base)),
                    Some(false) => Type::Signed(Box::new(base)),
                    None => base,
                }
            }
            _ if token.kind == TokenKind::TypedefName => {
                self.take()?;
                Type::Alias(token.text.to_string())
            }
            _ => return Err(Diagnostic::new(token.span, "expected a type specifier")),
        };
        let trailing = self.parse_qualifiers()?;
        qualifiers.is_const |= trailing.is_const;
        qualifiers.is_volatile |= trailing.is_volatile;
        qualifiers.is_restrict |= trailing.is_restrict;
        qualifiers.is_atomic |= trailing.is_atomic;
        let ty = if qualifiers.any() {
            Type::Qualified(Box::new(ty), qualifiers)
        } else { ty };
        Ok(Some((ty, first.span.lo)))
    }

    /// Qualifiers are permitted before/after a specifier and on each '*'.
    /// GNU headers spell restrict as __restrict or __restrict__.
    fn parse_qualifiers(&mut self) -> Result<Qualifiers, Diagnostic> {
        let mut result = Qualifiers::default();
        while let Some(token) = self.peek()? {
            match token.text.as_ref() {
                "const" => result.is_const = true,
                "volatile" => result.is_volatile = true,
                "restrict" | "__restrict" | "__restrict__" => result.is_restrict = true,
                "_Atomic" => {
                    // Parenthesized _Atomic(type-name) is a type specifier,
                    // not the type qualifier. Preserve both forms.
                    self.peek()?;
                    if self.second_lookahead.is_none() {
                        self.second_lookahead = Some(
                            self.scanner.next_parser_token(Some(&self.scopes))?
                        );
                    }
                    if self.second_lookahead.as_ref().and_then(Option::as_ref)
                        .is_some_and(|next| next.text.as_ref() == "(") {
                        break;
                    }
                    if result.is_atomic {
                        return Err(Diagnostic::coded(
                            token.span, DiagnosticCode::SyntaxInvalidDeclaration,
                            "duplicate C11 _Atomic qualifier"));
                    }
                    result.is_atomic = true;
                },
                _ => break,
            }
            self.take()?;
        }
        Ok(result)
    }
}

impl ItemSource for Parser<'_> {
    type Error = Diagnostic;

    fn next_item(&mut self) -> Result<Option<ExternalItem>, Self::Error> {
        self.pending_file_binding = None;
        // A file-scope comma list has a common base/storage but individual
        // declarators, bindings and optional initializers. Consume one on
        // demand so the parser stays incremental even for long lists.
        if let Some((base, start, storage, is_typedef, is_inline)) =
            self.pending_file_group.take()
        {
            let (name, mut ty, _, name_span) =
                self.parse_declarator_shape(false)?.finish(base.clone());
            let asm_label = self.parse_asm_linkage_label()?;
            if is_inline &&
                !matches!(&ty, Type::Function { .. } | Type::VariadicFunction { .. } | Type::UnprototypedFunction { .. })
            {
                return Err(Diagnostic::coded(name_span.unwrap_or(Span { lo: start, hi: start }),
                    DiagnosticCode::SyntaxInvalidDeclaration,
                    "inline requires a function declarator"));
            }
            while self.peek()?.is_some_and(|t|
                matches!(t.text.as_ref(), "__attribute__" | "__attribute"))
            {
                let signature = match &ty {
                    Type::Function { params, .. } => Some((params.len(), false)),
                    Type::VariadicFunction { params, .. } => Some((params.len(), true)),
                    _ => None,
                };
                self.parse_gnu_declarator_attributes(signature, Some(&mut ty))?;
            }
            if asm_label.is_some() &&
                (!matches!(ty, Type::Function { .. } | Type::VariadicFunction { .. } | Type::UnprototypedFunction { .. }) || is_typedef)
            {
                return Err(Diagnostic::coded(name_span.unwrap_or(Span { lo: start, hi: start }),
                    DiagnosticCode::SyntaxInvalidDeclaration,
                    "GNU asm linker label requires a non-typedef function declaration"));
            }
            // The name enters file scope before parsing its own initializer.
            self.pending_file_binding = Some((name.clone(), self.scopes.lookup(&name)));
            self.scopes.declare(&name,
                if is_typedef { Binding::Typedef } else { Binding::Ordinary });
            let init_op = self.peek()?.filter(|t| t.text.as_ref() == "=").map(|t| t.span);
            let init = if init_op.is_some() {
                self.take()?;
                Some(self.parse_initializer()?)
            } else { None };
            if is_typedef {
                if let Some(span) = init_op {
                    return Err(Diagnostic::coded(span, DiagnosticCode::SyntaxInvalidDeclaration,
                        "typedef cannot have an initializer"));
                }
            }
            let end = if self.peek()?.is_some_and(|t| t.text.as_ref() == ",") {
                let comma = self.take()?.expect("peeked comma");
                self.pending_file_group = Some((base, start, storage, is_typedef, is_inline));
                comma.span.hi
            } else {
                self.expect(";")?.span.hi
            };
            let decl = Decl { storage, is_inline, name, name_span, ty, asm_label, init,
                span: Span { lo: start, hi: end } };
            return Ok(Some(if is_typedef {
                ExternalItem::Typedef(decl)
            } else { ExternalItem::Declaration(decl) }));
        }
        // GNU __extension__ only suppresses pedantic diagnostics for the
        // following declaration; it changes neither type nor ABI. Admit
        // precisely this file-scope prefix, without dropping the declaration
        // or interpreting arbitrary GNU extensions as harmless.
        if self.peek()?.is_some_and(|t| t.text.as_ref() == "__extension__") {
            let marker = self.take()?.expect("peeked __extension__");
            if self.peek()?.is_none() {
                return Err(Diagnostic::coded(marker.span,
                    DiagnosticCode::SyntaxInvalidDeclaration,
                    "GNU __extension__ requires a following declaration"));
            }
        }
        let is_typedef = self.peek()?.is_some_and(|t| t.text.as_ref() == "typedef");
        if is_typedef { self.take()?; }
        let storage_span = self.peek()?.map_or(self.eof_span(), |t| t.span);
        // Inline and storage-class specifiers may appear in either order.
        let leading_inline = self.peek()?.is_some_and(|t|
            matches!(t.text.as_ref(), "inline" | "__inline" | "__inline__"));
        if leading_inline { self.take()?; }
        let leading_tls = self.peek()?.is_some_and(|t|
            matches!(t.text.as_ref(), "__thread" | "_Thread_local"));
        if leading_tls { self.take()?; }
        let mut storage = match self.peek()?.map(|t| t.text).as_deref() {
            Some("static") => { self.take()?; StorageClass::Static },
            Some("extern") => { self.take()?; StorageClass::Extern },
            Some("register") => { self.take()?; StorageClass::Register },
            _ => StorageClass::Default,
        };
        let trailing_tls = self.peek()?.is_some_and(|t|
            matches!(t.text.as_ref(), "__thread" | "_Thread_local"));
        if trailing_tls { self.take()?; }
        if leading_tls && trailing_tls {
            return Err(Diagnostic::coded(storage_span, DiagnosticCode::SyntaxInvalidDeclaration,
                "duplicate thread storage specifier"));
        }
        if leading_tls || trailing_tls {
            storage = match storage {
                StorageClass::Default => StorageClass::ThreadLocal,
                StorageClass::Extern => StorageClass::ExternThreadLocal,
                StorageClass::Static => StorageClass::StaticThreadLocal,
                _ => return Err(Diagnostic::coded(storage_span, DiagnosticCode::SyntaxInvalidDeclaration,
                    "thread storage cannot combine with register")),
            };
        }
        if is_typedef && storage != StorageClass::Default {
            return Err(Diagnostic::coded(storage_span, DiagnosticCode::SyntaxInvalidDeclaration,
                "typedef cannot combine with storage class"));
        }
        if storage == StorageClass::Register {
            return Err(Diagnostic::coded(storage_span, DiagnosticCode::SyntaxInvalidDeclaration,
                "register storage class is not allowed at file scope"));
        }
        // Keep the inline specifier for translation-unit linkage selection.
        let mut is_inline = leading_inline;
        while self.peek()?.is_some_and(|t|
            matches!(t.text.as_ref(), "inline" | "__inline" | "__inline__"))
        {
            self.take()?;
            is_inline = true;
        }
        if is_inline && is_typedef {
            return Err(Diagnostic::coded(storage_span, DiagnosticCode::SyntaxInvalidDeclaration,
                "inline cannot be combined with typedef"));
        }
        let Some((base, start)) = self.parse_type()? else {
            // EOF only succeeds when no declaration specifiers were consumed.
            if is_typedef || is_inline || storage != StorageClass::Default {
                return Err(Diagnostic::new(self.eof_span(), "expected type specifier"));
            }
            return Ok(None);
        };
        // Also accept inline after the complete return type, before the declarator.
        while self.peek()?.is_some_and(|t|
            matches!(t.text.as_ref(), "inline" | "__inline" | "__inline__"))
        {
            self.take()?;
            is_inline = true;
        }
        if is_inline && is_typedef {
            return Err(Diagnostic::coded(storage_span, DiagnosticCode::SyntaxInvalidDeclaration,
                "inline cannot be combined with typedef"));
        }
        if self.peek()?.is_some_and(|t| t.text.as_ref() == ";") {
            if storage.is_thread_local() {
                return Err(Diagnostic::coded(storage_span, DiagnosticCode::SyntaxInvalidDeclaration,
                    "thread storage requires an object declarator"));
            }
            if is_inline {
                return Err(Diagnostic::coded(storage_span, DiagnosticCode::SyntaxInvalidDeclaration,
                    "inline requires a function declarator"));
            }
            if !matches!(&base, Type::Struct { .. } | Type::Union { .. } | Type::Enum { .. } | Type::TagRef(..)) {
                return Err(Diagnostic::coded(self.peek()?.expect("peeked").span,
                    DiagnosticCode::SyntaxExpectedDeclarator,
                    "declaration requires an identifier"));
            }
            if is_typedef {
                return Err(Diagnostic::coded(self.peek()?.expect("peeked").span,
                    DiagnosticCode::SyntaxExpectedDeclarator,
                    "typedef requires a declarator"));
            }
            self.take()?;
            return Ok(Some(ExternalItem::Tag(base)));
        }
        let (name, mut ty, definition_params, name_span) = self.parse_declarator_shape_impl(false, true)?.finish(base.clone());
        let asm_label = self.parse_asm_linkage_label()?;
        if is_inline && !matches!(&ty, Type::Function { .. } | Type::VariadicFunction { .. } | Type::UnprototypedFunction { .. }) {
            return Err(Diagnostic::coded(name_span.unwrap_or(storage_span),
                DiagnosticCode::SyntaxInvalidDeclaration,
                "inline requires a function declarator"));
        }
        // GNU permits advisory function attributes following the asm label.
        while self.peek()?.is_some_and(|t|
            matches!(t.text.as_ref(), "__attribute__" | "__attribute"))
        {
            let signature = match &ty {
                Type::Function { params, .. } => Some((params.len(), false)),
                Type::VariadicFunction { params, .. } => Some((params.len(), true)),
                _ => None,
            };
            self.parse_gnu_declarator_attributes(signature, Some(&mut ty))?;
        }
        if asm_label.is_some() &&
            (!matches!(ty, Type::Function { .. } | Type::VariadicFunction { .. } | Type::UnprototypedFunction { .. }) || is_typedef)
        {
            return Err(Diagnostic::coded(name_span.unwrap_or(Span { lo: start, hi: start }),
                DiagnosticCode::SyntaxInvalidDeclaration,
                "GNU asm linker label requires a non-typedef function declaration"));
        }
        self.pending_file_binding = Some((name.clone(), self.scopes.lookup(&name)));
        self.scopes.declare(&name, if is_typedef { Binding::Typedef } else { Binding::Ordinary });

        if storage.is_thread_local() && matches!(&ty, Type::Function { .. } | Type::VariadicFunction { .. } | Type::UnprototypedFunction { .. }) {
            return Err(Diagnostic::coded(storage_span, DiagnosticCode::SyntaxInvalidDeclaration,
                "thread storage requires an object declarator"));
        }
        let old_style = matches!(&ty, Type::UnprototypedFunction { .. });
        let identifier_list = old_style && definition_params.as_ref().is_some_and(|p| !p.is_empty());
        if identifier_list || self.peek()?.is_some_and(|token| token.text.as_ref() == "{") {
            let (Type::Function { result, .. } | Type::UnprototypedFunction { result }) = ty else {
                return Err(Diagnostic::coded(self.peek()?.map_or(self.eof_span(), |t| t.span),
                    DiagnosticCode::SyntaxInvalidDeclaration, "function body requires a function declarator"));
            };
            if is_typedef {
                return Err(Diagnostic::new(Span { lo: start, hi: start }, "typedef cannot define a function body"));
            }
            let mut params = definition_params.unwrap_or_default();
            if params.iter().any(|p| p.name.is_empty()) {
                return Err(Diagnostic::new(Span { lo: start, hi: start }, "function definition requires parameter names"));
            }
            self.scopes.enter();
            for param in &params { self.scopes.declare(&param.name, Binding::Ordinary); }
            let parsed = (|| {
                if identifier_list {
                    let mut declared = std::collections::HashMap::new();
                    while !self.peek()?.is_some_and(|t| t.text.as_ref() == "{") {
                        for decl in self.parse_decl_group(false)? {
                            if !params.iter().any(|p| p.name == decl.name) {
                                return Err(Diagnostic::new(decl.span, "declaration does not name an old-style parameter"));
                            }
                            if decl.init.is_some() || !matches!(decl.storage, StorageClass::Default | StorageClass::Register) {
                                return Err(Diagnostic::new(decl.span, "invalid old-style parameter declaration"));
                            }
                            let span = decl.span;
                            if declared.insert(decl.name.clone(), decl).is_some() {
                                return Err(Diagnostic::new(span, "duplicate old-style parameter declaration"));
                            }
                        }
                    }
                    for param in &mut params {
                        let mut decl = declared.remove(&param.name).ok_or_else(||
                            Diagnostic::new(param.span, "old-style parameter requires a declaration"))?;
                        decl.name_span = param.name_span;
                        *param = decl;
                    }
                }
                self.parse_block()
            })();
            self.scopes.leave();
            let (body, end) = parsed?;
            return Ok(Some(ExternalItem::Function(Function {
                storage,
                is_inline,
                name,
                name_span,
                result: *result,
                old_style,
                params,
                body,
                span: Span { lo: start, hi: end },
            })));
        }
        let initializer_operator = self.peek()?.filter(|t| t.text.as_ref() == "=").map(|t| t.span);
        let init = if initializer_operator.is_some() {
            self.take()?;
            Some(self.parse_initializer()?)
        } else {
            None
        };
        if is_typedef {
            if let Some(span) = initializer_operator {
                return Err(Diagnostic::coded(span, DiagnosticCode::SyntaxInvalidDeclaration,
                    "typedef cannot have an initializer"));
            }
        }
        let end = if self.peek()?.is_some_and(|t| t.text.as_ref() == ",") {
            let comma = self.take()?.expect("peeked comma");
            self.pending_file_group = Some((base, start, storage, is_typedef, is_inline));
            comma.span.hi
        } else {
            self.expect(";")?.span.hi
        };
        let decl = Decl { storage, is_inline, name, name_span, ty, asm_label, init, span: Span { lo: start, hi: end } };
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

    /// Exercise the parser against an actual one-shot scanner iterator,
    /// not a collected token Vec or a cloned scanner.
    #[test]
    fn parser_owned_token_source_is_lazy_and_matches_borrowed_path() {
        use std::{cell::Cell, rc::Rc, sync::Arc};
        use crate::lexer::{ParserToken, ParserTokenSource, TokenText};

        struct SharedScanner<'a> {
            scanner: Scanner<'a>,
            pulls: Rc<Cell<usize>>,
        }
        impl<'a> ParserTokenSource<'a> for SharedScanner<'a> {
            fn next_parser_token(
                &mut self, _scopes: Option<&Scopes>,
            ) -> Result<Option<ParserToken<'a>>, Diagnostic> {
                self.pulls.set(self.pulls.get() + 1);
                Ok(self.scanner.next_token(None)?.map(|token| ParserToken {
                    kind: token.kind,
                    text: TokenText::Shared(Arc::from(token.text)),
                    span: token.span,
                }))
            }
        }
        let source = "typedef int T; T value; int f(void) { T: goto T; } int g(void) { int T = 3; return T; }";
        let mut ordinary = Parser::new(source);
        let mut expected = Vec::new();
        while let Some(item) = ordinary.next_item().unwrap() {
            expected.push(item);
        }
        let pulls = Rc::new(Cell::new(0));
        let shared = SharedScanner {
            scanner: Scanner::new(source), pulls: Rc::clone(&pulls),
        };
        let mut parsed = Parser::from_parser_token_source(shared, source.len());
        let first = parsed.next_item().unwrap().unwrap();
        assert!(pulls.get() < source.len() / 2,
            "parser should not pull entire source before its first item");
        let mut actual = vec![first];
        while let Some(item) = parsed.next_item().unwrap() {
            actual.push(item);
        }
        assert_eq!(actual, expected);
    }

    #[test]
    fn parser_releases_shared_spellings_after_consumption() {
        use std::{collections::VecDeque, sync::Arc};
        use crate::lexer::{ParserToken, ParserTokenSource, TokenText};
        struct Queue(VecDeque<ParserToken<'static>>);
        impl ParserTokenSource<'static> for Queue {
            fn next_parser_token(
                &mut self, _scopes: Option<&Scopes>,
            ) -> Result<Option<ParserToken<'static>>, Diagnostic> {
                Ok(self.0.pop_front())
            }
        }
        let name: Arc<str> = Arc::from("first");
        let second: Arc<str> = Arc::from("second");
        let make = |kind, text: Arc<str>, lo, hi| ParserToken {
            kind, text: TokenText::Shared(text), span: Span { lo, hi },
        };
        let mut source = Queue(VecDeque::from([
            make(TokenKind::Identifier, Arc::from("int"), 0, 3),
            make(TokenKind::Identifier, Arc::clone(&name), 4, 9),
            make(TokenKind::Punctuation, Arc::from(";"), 9, 10),
            make(TokenKind::Identifier, Arc::from("int"), 11, 14),
            make(TokenKind::Identifier, Arc::clone(&second), 15, 21),
            make(TokenKind::Punctuation, Arc::from(";"), 21, 22),
        ]));
        let mut parser = Parser::from_parser_token_source(
            Queue(std::mem::take(&mut source.0)), 22,
        );
        assert!(matches!(parser.next_item().unwrap(),
            Some(ExternalItem::Declaration(ref decl)) if decl.name == "first"));
        assert_eq!(Arc::strong_count(&name), 1,
            "consumed identifier spelling must not remain pinned by the parser");
        assert!(matches!(parser.next_item().unwrap(),
            Some(ExternalItem::Declaration(ref decl)) if decl.name == "second"));
        assert_eq!(Arc::strong_count(&second), 1);
    }

    #[test]
    fn pull_token_source_delivers_items_only_when_requested() {
        use std::{cell::Cell, rc::Rc};
        use crate::lexer::PullTokenSource;

        let source = "int first; int second; int third; int fourth;";
        let mut lexer = Scanner::new(source);
        let calls = Rc::new(Cell::new(0));
        let observed = Rc::clone(&calls);
        let tokens = std::iter::from_fn(move || {
            observed.set(observed.get() + 1);
            lexer.next_token(None).transpose()
        });
        let mut parser = Parser::from_token_source(
            PullTokenSource::new(tokens), source.len(),
        );

        let first = parser.next_item().unwrap().unwrap();
        assert!(matches!(first, ExternalItem::Declaration(ref d) if d.name == "first"));
        assert!(calls.get() < 13, "parser should not pull the whole translation unit");
        let mut names = Vec::new();
        while let Some(item) = parser.next_item().unwrap() {
            let ExternalItem::Declaration(decl) = item else { panic!("declaration expected") };
            names.push(decl.name);
        }
        assert_eq!(names, ["second", "third", "fourth"]);
    }

    #[test]
    fn pull_token_source_matches_scanner_for_typedefs_labels_and_shadowing() {
        use crate::lexer::PullTokenSource;
        let source = "typedef int T; T value; int f(void) { T: goto T; } int g(void) { int T = 3; return T; }";
        let mut regular = Parser::new(source);
        let mut expected = Vec::new();
        while let Some(item) = regular.next_item().unwrap() { expected.push(item); }

        let mut lexer = Scanner::new(source);
        let tokens = std::iter::from_fn(move || lexer.next_token(None).transpose());
        let mut streaming = Parser::from_token_source(
            PullTokenSource::new(tokens), source.len(),
        );
        let mut actual = Vec::new();
        while let Some(item) = streaming.next_item().unwrap() { actual.push(item); }
        assert_eq!(actual, expected, "token-provider parser must match plain scanner");
    }

    #[test]
    fn pull_token_source_keeps_error_recovery_and_eof_locations() {
        use crate::lexer::PullTokenSource;
        let source = "int first; int broken = ; int last;";
        let mut lexer = Scanner::new(source);
        let tokens = std::iter::from_fn(move || lexer.next_token(None).transpose());
        let mut parser = Parser::from_token_source(
            PullTokenSource::new(tokens), source.len(),
        );
        let mut names = Vec::new();
        let errors = parser.for_each_item_recovering(4, |item| {
            if let ExternalItem::Declaration(decl) = item { names.push(decl.name); }
        });
        assert_eq!(names, ["first", "last"]);
        assert_eq!(errors.len(), 1, "one malformed item should not lose the following declaration");
        let incomplete = "int unfinished = ";
        let mut lexer = Scanner::new(incomplete);
        let tokens = std::iter::from_fn(move || lexer.next_token(None).transpose());
        let mut parser = Parser::from_token_source(
            PullTokenSource::new(tokens), incomplete.len(),
        );
        let error = parser.next_item().unwrap_err();
        assert_eq!(error.span, Span { lo: incomplete.len(), hi: incomplete.len() });
    }

    #[test]
    fn c99_goto_and_labels_preserve_forward_references() {
        let item = Parser::new("int f(void) { start: ; goto done; done: return 0; }")
            .next_item().unwrap().unwrap();
        let ExternalItem::Function(function) = item else { panic!("function expected") };
        let Stmt::Block(stmts) = function.body else { panic!("block expected") };
        assert!(matches!(&stmts[0], Stmt::Label { name, .. } if name == "start"));
        assert!(matches!(&stmts[1], Stmt::Goto { target, .. } if target == "done"));
        assert!(matches!(&stmts[2], Stmt::Label { name, .. } if name == "done"));

        let mut parser = Parser::new("typedef int T; int f(void) { T: goto T; }");
        parser.next_item().unwrap().unwrap();
        let item = parser.next_item().unwrap().unwrap();
        let ExternalItem::Function(function) = item else { panic!("function expected") };
        let Stmt::Block(stmts) = function.body else { panic!("block expected") };
        assert!(matches!(&stmts[0], Stmt::Label { name, .. } if name == "T"));
    }

    #[test]
    fn c99_switch_preserves_fallthrough_and_nested_labels() {
        let source = "int f(int x) { switch(x) { case 1: x=2; case 2: { x=3; break; } default: return x; } return 0; }";
        let item = Parser::new(source).next_item().expect("parse").expect("function");
        let ExternalItem::Function(function) = item else { panic!("expected function"); };
        let Stmt::Block(statements) = function.body else { panic!("expected function body"); };
        let Stmt::Switch { control, body } = &statements[0] else { panic!("expected switch"); };
        assert!(matches!(control.without_location(), Expr::Name(name) if name == "x"));
        let Stmt::Block(arms) = body.as_ref() else { panic!("expected switch block"); };
        assert!(matches!(&arms[0], Stmt::Case { .. }));
        assert!(matches!(&arms[1], Stmt::Case { body, .. }
            if matches!(body.as_ref(), Stmt::Block(_))));
        assert!(matches!(&arms[2], Stmt::Default(_)));
    }

    #[test]
    fn c99_switch_parser_rejects_missing_case_colon() {
        let source = "int f(int x) { switch(x) { case 1 return x; } }";
        assert!(Parser::new(source).next_item().is_err());
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
    fn m10_short_specifiers_parse_in_any_standard_order() {
        for source in [
            "typedef unsigned short int __u_short;",
            "typedef signed short int __int16_t;",
            "typedef short unsigned int U;",
            "typedef int short signed S;",
            "typedef short T;",
            "typedef short int T;",
            "typedef int short T;",
            "short f(unsigned short x) { short y = x; return y; }",
            "int main(void) { return (unsigned short)65537 == 1; }",
        ] {
            let mut parser = Parser::new(source);
            assert!(parser.next_item().is_ok(), "failed to parse: {source}");
        }
        for source in [
            "typedef short long Bad;",
            "typedef long short Bad;",
            "typedef char short Bad;",
            "typedef short short Bad;",
            "typedef unsigned signed short Bad;",
        ] {
            assert!(Parser::new(source).next_item().is_err(),
                "incorrectly accepted {source}");
        }
    }

    #[test]
    fn atomic_types_fail_closed_with_specific_diagnostics() {
        // These spellings are legal C11 but cannot be interpreted as
        // ordinary types or silently desugared into non-atomic accesses.
        // The existing GCC bootstrap remains the supported path.
        for source in [
            "_Atomic int counter;",
            "int _Atomic counter;",
            "typedef _Atomic int Counter;",
            "struct S { _Atomic int state; };",
            "int f(void) { _Atomic int value; return 0; }",
            "int f(_Atomic int *value);",
        ] {
            // These types parse but may never lower as ordinary memory.
            assert!(Parser::new(source).next_item().is_ok(), "{source}");
        }
        let duplicate = Parser::new("_Atomic _Atomic int x;")
            .next_item().expect_err("duplicate qualifier");
        assert!(duplicate.message.contains("duplicate C11 _Atomic qualifier"));
        // Non-atomic input and the ordinary const type qualifier must
        // retain their existing parser behavior.
        assert!(Parser::new("int plain;").next_item().is_ok());
        assert!(Parser::new("const int plain;").next_item().is_ok());
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
            ExternalItem::Declaration(d) => assert_eq!(d.ty, Type::UnprototypedFunction {
                result: Box::new(Type::Int),
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
        match parser.next_item().unwrap().unwrap() {
            ExternalItem::Declaration(d) => assert_eq!(d.ty, Type::Function {
                result: Box::new(Type::Int), params: vec![],
            }),
            _ => panic!("expected explicit void prototype"),
        }
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
    fn c99_abstract_function_parameter_declarators() {
        let mut p = Parser::new("typedef long T; int h(int (int), int (T), int (void), int (int,...));");
        p.next_item().unwrap().unwrap();
        let Some(ExternalItem::Declaration(decl)) = p.next_item().unwrap()
            else { panic!("expected prototype") };
        assert_eq!(decl.ty, Type::Function {
            result: Box::new(Type::Int),
            params: vec![
                Type::Function { result: Box::new(Type::Int), params: vec![Type::Int] },
                Type::Function { result: Box::new(Type::Int), params: vec![Type::Alias("T".into())] },
                Type::Function { result: Box::new(Type::Int), params: vec![] },
                Type::VariadicFunction { result: Box::new(Type::Int), params: vec![Type::Int] },
            ],
        });
        assert!(Parser::new("int h(int (int,));").next_item().is_err());
    }

    #[test]
    fn rejects_malformed_prototypes() {
        let mut parser = Parser::new("int f(int a,);");
        assert!(parser.next_item().is_err());
    }
    #[test]
    fn narrow_string_literal_parses_as_bytes() {
        let mut parser = Parser::new(r#"int main(void) { "hi\n"; return 0; }"#);
        let item = parser.next_item().unwrap().unwrap();
        let ExternalItem::Function(function) = item else { panic!("expected function"); };
        let Stmt::Block(stmts) = function.body else { panic!("expected block"); };
        assert_eq!(stmts[0], Stmt::Expr(Some(Expr::StringLiteral(b"hi\n\0".to_vec()))));
    }
    #[test]
    fn c99_bitfield_member_declarators_preserve_integer_width_expressions() {
        let mut p = Parser::new(concat!(
            "struct Flags {unsigned int a:3, b:5; signed int c:7; int tail;};",
            "union Choice {unsigned int x:4;long y;};",
            "struct Pending {unsigned int:0;unsigned int x:2;};"
        ));
        let Some(ExternalItem::Tag(Type::Struct { members, .. })) =
            p.next_item().unwrap() else { panic!("struct definition expected") };
        assert_eq!(members.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            ["a","b","c","tail"]);
        for (member, width) in members[..3].iter().zip([3,5,7]) {
            let Type::BitField(_, size) = &member.ty else {
                panic!("bit-field syntax must retain its width expression");
            };
            assert!(matches!(size.as_ref(), Expr::Int(v) if *v == width));
        }
        assert!(matches!(&members[3].ty, Type::Int));
        let Some(ExternalItem::Tag(Type::Union { members, .. })) =
            p.next_item().unwrap() else { panic!("union expected") };
        assert!(matches!(&members[0].ty, Type::BitField(_, width)
            if matches!(width.as_ref(), Expr::Int(4))));
        let Some(ExternalItem::Tag(Type::Struct { members, .. })) =
            p.next_item().unwrap() else { panic!("unnamed field parsed") };
        assert_eq!(members[0].name, "");
        assert!(matches!(&members[0].ty, Type::BitField(_, width)
            if matches!(width.as_ref(), Expr::Int(0))));
    }

    #[test]
    fn inline_structs_and_members_parse() {
        let mut p = Parser::new("struct Pair { char a; int b; long c; } item; int f(void) { struct { int x; char y; } obj; obj.x=7; return obj.x; }");
        let item = match p.next_item().unwrap().unwrap() {
            ExternalItem::Declaration(d) => d,
            other => panic!("expected structure: {other:?}"),
        };
        let Type::Struct { tag, members } = item.ty else { panic!("struct expected") };
        assert_eq!(tag.as_deref(), Some("Pair"));
        assert_eq!(members.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["a","b","c"]);
        let fun = match p.next_item().unwrap().unwrap() {
            ExternalItem::Function(f) => f,
            other => panic!("expected function: {other:?}"),
        };
        let Stmt::Block(body) = fun.body else { panic!("block expected") };
        assert_eq!(body[1], Stmt::Expr(Some(Expr::Assign(
            Box::new(Expr::Member(Box::new(Expr::Name("obj".into())), "x".into(), false)),
            Box::new(Expr::Int(7))
        ))));
        let mut p = Parser::new("int read(struct { long a; } *p) { return p->a; }");
        assert!(matches!(p.next_item(), Ok(Some(ExternalItem::Function(_)))));
        assert!(matches!(Parser::new("struct Unknown value;").next_item(),
            Ok(Some(ExternalItem::Declaration(Decl { ty: Type::TagRef(TagKind::Struct, _), .. })))));
    }

    #[test]
    fn nested_aggregate_initializers_parse() {
        let mut parser = Parser::new("int a[3] = {1, 2, 3,}; int f(void) { int b[2][2] = {{1,2},{3,4}}; return b[1][0]; }");
        let ExternalItem::Declaration(decl) = parser.next_item().unwrap().unwrap() else { panic!("declaration expected") };
        assert_eq!(decl.init, Some(Expr::Initializer(vec![Expr::Int(1), Expr::Int(2), Expr::Int(3)])));
        let ExternalItem::Function(fun) = parser.next_item().unwrap().unwrap() else { panic!("function expected") };
        let Stmt::Block(stmts) = fun.body else { panic!("block expected") };
        assert!(matches!(&stmts[0], Stmt::Decl(Decl { init: Some(Expr::Initializer(items)), .. }) if items.len() == 2));
        assert!(Parser::new("int a[2]={};").next_item().is_err());
    }

    #[test]
    fn tagged_enums_and_constant_expressions_parse() {
        let mut p = Parser::new("enum Color { RED, GREEN=7, BLUE=GREEN+2, }; enum Color shade=BLUE; int f(void) { enum Color value=RED; return BLUE; }");
        let ExternalItem::Tag(Type::Enum { tag, enumerators }) = p.next_item().unwrap().unwrap() else { panic!("enum expected") };
        assert_eq!(tag.as_deref(), Some("Color"));
        assert_eq!(enumerators.len(), 3);
        assert_eq!(enumerators[1].1, Some(Expr::Int(7)));
        let ExternalItem::Declaration(shade) = p.next_item().unwrap().unwrap() else { panic!("enum object expected") };
        assert!(matches!(shade.ty, Type::TagRef(TagKind::Enum, _)));
        assert!(matches!(p.next_item(), Ok(Some(ExternalItem::Function(_)))));
    }
    #[test]
    fn tag_declarations_and_recursive_pointer_syntax() {
        let mut p = Parser::new("struct Node; struct Node { int value; struct Node *next; }; struct Node *root; int f(void) { struct Node *local; return 0; }");
        assert!(matches!(p.next_item(), Ok(Some(ExternalItem::Tag(Type::TagRef(TagKind::Struct, _))))));
        assert!(matches!(p.next_item(), Ok(Some(ExternalItem::Tag(Type::Struct { .. })))));
        let ExternalItem::Declaration(root) = p.next_item().unwrap().unwrap() else { panic!("root expected") };
        assert!(matches!(root.ty, Type::Pointer(inner) if matches!(*inner, Type::TagRef(TagKind::Struct, _))));
        assert!(matches!(p.next_item(), Ok(Some(ExternalItem::Function(_)))));
        let mut p = Parser::new("int f(void) { struct Inner { int a; }; struct Inner *p; return 0; }");
        assert!(matches!(p.next_item(), Ok(Some(ExternalItem::Function(_)))));
    }
    #[test]
    fn union_declaration_and_member_parse() {
        let mut p = Parser::new("union Word { int i; long l; } word; int f(void) { union { int i; long l; } u; u.i=3; return u.i; }");
        let ExternalItem::Declaration(word) = p.next_item().unwrap().unwrap() else { panic!("union expected") };
        let Type::Union { tag, members } = word.ty else { panic!("union expected") };
        assert_eq!(tag.as_deref(), Some("Word"));
        assert_eq!(members.len(), 2);
        let ExternalItem::Function(fun) = p.next_item().unwrap().unwrap() else { panic!("function expected") };
        let Stmt::Block(body) = fun.body else { panic!("block expected") };
        assert!(matches!(&body[1], Stmt::Expr(Some(Expr::Assign(lhs, _)))
            if matches!(lhs.as_ref(), Expr::Member(_, field, false) if field == "i")));
    }

    #[test]
    fn m10_gnu_extension_prefix_only_on_valid_external_declarations() {
        let mut p = Parser::new(concat!(
            "__extension__\n",
            "extern long long int llrint (double x) ",
            "__attribute__((__nothrow__,__leaf__));",
            "__extension__ typedef unsigned long long wide_t;",
            "int main(void){return 0;}"
        ));
        assert!(matches!(p.next_item(), Ok(Some(ExternalItem::Declaration(_)))));
        assert!(matches!(p.next_item(), Ok(Some(ExternalItem::Typedef(_)))));
        assert!(matches!(p.next_item(), Ok(Some(ExternalItem::Function(_)))));
        assert!(p.next_item().unwrap().is_none());

        for invalid in [
            "__extension__",
            "__extension__ ;",
            "__extension__ bogus;",
            "__extension__ int x = ;",
        ] {
            assert!(Parser::new(invalid).next_item().is_err(),
                "must not erase malformed declarations: {invalid}");
        }
        // This slice explicitly does not enable statement or expression
        // extensions, nor suppress semantic checks on declared C types.
        assert!(Parser::new(
            "int main(void){__extension__ int x=2;return x;}"
        ).next_item().is_err());
    }

    #[test]
    fn m10_float128_glibc_prototype_is_not_long_double() {
        let src = "extern int __fpclassifyf128 (_Float128 __value) __attribute__ ((__nothrow__,__leaf__));";
        let mut parser = Parser::new(src);
        let ExternalItem::Declaration(prototype) = parser.next_item().unwrap().unwrap()
            else { panic!("expected declaration") };
        let Type::Function { result, params } = prototype.ty else {
            panic!("expected function prototype");
        };
        assert_eq!(*result, Type::Int);
        assert_eq!(params, vec![Type::Floating(FloatRank::Float128)]);
        assert!(parser.next_item().unwrap().is_none());
        for invalid in ["typedef unsigned _Float128 T;", "typedef _Float128 long T;",
                        "typedef double _Float128 T;"] {
            assert!(Parser::new(invalid).next_item().is_err(),
                "invalid mixed floating specifiers must fail: {invalid}");
        }
    }

    #[test]
    fn m10_glibc_noreturn_advisory_is_not_an_abi_or_control_flow_assumption() {
        // The exact GNU spelling in the Csmith-preprocessed glibc assert.h.
        // We accept the attribute as advisory and deliberately do not rewrite
        // function signatures or infer that callsites are unreachable.
        for source in [
            "extern void __assert_fail (const char *__assertion) __attribute__ ((__nothrow__,__leaf__)) __attribute__ ((__noreturn__));",
            "extern void fatal(void) __attribute__((noreturn));",
            "extern int x(int n) __attribute__((__noreturn__,__unused__));",
        ] {
            let mut parser = Parser::new(source);
            assert!(parser.next_item().is_ok(), "valid GNU declaration: {source}");
        }
        // No arguments, never on a data object; unsafe call/ABI-changing
        // attributes remain unsupported rather than silently discarded.
        for source in [
            "extern void bad(void) __attribute__((__noreturn__(1)));",
            "int object __attribute__((__noreturn__));",
            "extern void bad(void) __attribute__((__returns_twice__));",
            "extern void bad(void) __attribute__((__ms_abi__));",
        ] {
            let mut parser = Parser::new(source);
            assert!(parser.next_item().is_err(), "invalid GNU declaration: {source}");
        }
    }

    #[test]
    fn m10_float_declarations_preserve_three_distinct_types() {
        let src = concat!(
            "typedef float float_t; typedef double double_t;",
            "extern int __fpclassify(double __value) __attribute__((__nothrow__,__leaf__));",
            "extern double nexttoward(double x, long double y);",
            "extern long double sqrtl(long double x);",
            "extern float nexttowardf(float x, double long y);",
        );
        let mut parser = Parser::new(src);
        let ExternalItem::Typedef(f) = parser.next_item().unwrap().unwrap()
            else { panic!("float typedef required") };
        assert_eq!(f.ty, Type::Floating(FloatRank::Float));
        let ExternalItem::Typedef(d) = parser.next_item().unwrap().unwrap()
            else { panic!("double typedef required") };
        assert_eq!(d.ty, Type::Floating(FloatRank::Double));
        for _ in 0..4 {
            assert!(matches!(parser.next_item(), Ok(Some(ExternalItem::Declaration(_)))));
        }
        assert!(parser.next_item().unwrap().is_none());
    }

    #[test]
    fn m10_float_specifier_mixes_fail_closed() {
        for src in [
            "typedef unsigned float Invalid;",
            "typedef signed double Invalid;",
            "typedef long long double Invalid;",
            "typedef short double Invalid;",
            "typedef double int Invalid;",
            "typedef float long Invalid;",
        ] {
            assert!(Parser::new(src).next_item().is_err(),
                "incorrectly accepted {src}");
        }
    }

    #[test]
    fn integer_casts_preserve_type_names_and_precedence() {
        let mut parser = Parser::new("typedef unsigned int U; int f(int c) { return (U)c + (unsigned char)'a'; }");
        assert!(matches!(parser.next_item(), Ok(Some(ExternalItem::Typedef(..)))));
        let ExternalItem::Function(fun) = parser.next_item().unwrap().unwrap() else { panic!("function expected"); };
        let Stmt::Block(body) = fun.body else { panic!("body expected"); };
        let Stmt::Return(Some(Expr::Binary(BinOp::Add, lhs, rhs))) = &body[0] else { panic!("sum expected"); };
        assert!(matches!(lhs.as_ref(), Expr::Cast(Type::Alias(name), inner)
            if name == "U" && matches!(inner.as_ref(), Expr::Name(name) if name == "c")));
        assert!(matches!(rhs.as_ref(), Expr::Cast(Type::Unsigned(inner), _)
            if **inner == Type::Char));
        let mut parser = Parser::new("int f(int x) { return (x) + 1; }");
        assert!(matches!(parser.next_item(), Ok(Some(ExternalItem::Function(_)))));
        assert!(matches!(Parser::new("int f(int x) { return (const int)x; }")
            .next_item(), Ok(Some(ExternalItem::Function(_)))));
        let ExternalItem::Function(fun) = Parser::new(
            "int f(void) { return (void*)0 == (unsigned *)0; }"
        ).next_item().unwrap().unwrap() else { panic!("function expected"); };
        let Stmt::Block(stmts) = fun.body else { panic!("block expected"); };
        let Stmt::Return(Some(Expr::Binary(BinOp::Eq, left, right))) = &stmts[0]
            else { panic!("cast comparison expected"); };
        assert!(matches!(left.as_ref(), Expr::Cast(Type::Pointer(inner), _)
            if matches!(inner.as_ref(), Type::Void)));
        assert!(matches!(right.as_ref(), Expr::Cast(Type::Pointer(inner), _)
            if matches!(inner.as_ref(), Type::Unsigned(_))));
        assert!(Parser::new("int f(int x) { return (unsigned *name)x; }")
            .next_item().is_err());
    }


    #[test]
    fn c99_inline_aggregate_type_names_in_compound_literals_and_sizeof_parse() {
        let source = concat!(
            "int first(void){return ((struct {int a;long b;})",
            "{.a=3,.b=5}).a;}",
            "int second(void){return ((union NewTag {int x;long y;})",
            "{.y=9}).y;}",
            "int third(void){return sizeof(struct {int x;long y;});}",
        );
        let mut parser = Parser::new(source);
        let Some(ExternalItem::Function(first)) = parser.next_item().unwrap() else {
            panic!("first function");
        };
        let Stmt::Block(stmts) = first.body else { panic!("body") };
        let Stmt::Return(Some(Expr::Member(base, name, false))) = &stmts[0] else {
            panic!("member from inline aggregate literal");
        };
        assert_eq!(name, "a");
        assert!(matches!(base.as_ref(), Expr::CompoundLiteral(
            Type::Struct { tag: None, members }, _
        ) if members.len() == 2));
        let Some(ExternalItem::Function(second)) = parser.next_item().unwrap() else {
            panic!("second function");
        };
        let Stmt::Block(stmts) = second.body else { panic!("body") };
        assert!(matches!(&stmts[0], Stmt::Return(Some(Expr::Member(base, name, false)))
            if name == "y" && matches!(base.as_ref(), Expr::CompoundLiteral(
                Type::Union { tag: Some(name), members }, _
            ) if name == "NewTag" && members.len() == 2)));
        let Some(ExternalItem::Function(third)) = parser.next_item().unwrap() else {
            panic!("third function");
        };
        let Stmt::Block(stmts) = third.body else { panic!("body") };
        assert!(matches!(&stmts[0], Stmt::Return(Some(Expr::SizeofType(
            Type::Struct { tag: None, members }
        ))) if members.len() == 2));
        assert!(parser.next_item().unwrap().is_none());
    }

    #[test]
    fn c99_compound_literals_parse_as_postfix_lvalues_and_sizeof_expressions() {
        let src = concat!(
            "typedef int Word;",
            "struct Pair {int left; int right;};",
            "int f(void) {",
            "  int *p=(int[]){1,2,3};",
            "  int x=(struct Pair){.right=7,.left=2}.right;",
            "  int y=sizeof (int[]){1,2,3};",
            "  return ((Word){5}) + p[1] + x + y;",
            "}",
        );
        let mut parser = Parser::new(src);
        assert!(matches!(parser.next_item().unwrap(), Some(ExternalItem::Typedef(_))));
        assert!(matches!(parser.next_item().unwrap(), Some(ExternalItem::Tag(_))));
        let Some(ExternalItem::Function(function)) = parser.next_item().unwrap() else {
            panic!("expected function");
        };
        let Stmt::Block(statements) = function.body else { panic!("body"); };
        let Stmt::Decl(pointer) = &statements[0] else { panic!("pointer declaration"); };
        assert!(matches!(pointer.init.as_ref(), Some(Expr::CompoundLiteral(
            Type::Array(_, None), initializer
        )) if matches!(initializer.as_ref(), Expr::Initializer(values) if values.len()==3)));
        let Stmt::Decl(member) = &statements[1] else { panic!("member declaration"); };
        assert!(matches!(member.init.as_ref(), Some(Expr::Member(base, field, false))
            if field == "right" &&
            matches!(base.as_ref(), Expr::CompoundLiteral(Type::TagRef(..), _))));
        let Stmt::Decl(size) = &statements[2] else { panic!("sizeof declaration"); };
        assert!(matches!(size.init.as_ref(), Some(Expr::SizeofExpr(inner))
            if matches!(inner.as_ref(), Expr::CompoundLiteral(..))));
        assert!(parser.next_item().unwrap().is_none());
    }

    #[test]
    fn m10_decimal_float_literals_keep_type_rank_and_full_lexeme() {
        for (literal, rank) in [
            ("0.5f", FloatRank::Float),
            ("1e+3", FloatRank::Double),
            ("1.25L", FloatRank::LongDouble),
            (".125F", FloatRank::Float),
            ("0x1.0p-100f", FloatRank::Float),
        ] {
            let src = format!("double value(void) {{ return {literal}; }}");
            let mut parser = Parser::new(&src);
            let item = parser.next_item().unwrap().unwrap();
            let ExternalItem::Function(fun) = item else { panic!("function"); };
            let Stmt::Block(stmts) = fun.body else { panic!("body"); };
            let Stmt::Return(Some(Expr::FloatLiteral(text, actual))) = &stmts[0] else {
                panic!("float literal expected");
            };
            assert_eq!(text, literal);
            assert_eq!(*actual, rank);
        }
        for malformed in ["1e+", "0.5ff", "0x1p+"] {
            let src = format!("double value(void) {{ return {malformed}; }}");
            assert!(Parser::new(&src).next_item().is_err(), "{malformed}");
        }
    }


    #[test]
    fn m10_block_declaration_lists_share_one_scope_and_keep_shapes() {
        let source = "int use(void) { typedef int Word, Other; \
            Word i=3, j=i+4; int *p=&i, k=2; return *p+j+k; }";
        let mut parser = Parser::new(source);
        let Some(ExternalItem::Function(fun)) = parser.next_item().unwrap() else {
            panic!("function required");
        };
        let Stmt::Block(stmts) = fun.body else { panic!("block required") };
        assert_eq!(stmts.len(), 7);
        let names = stmts.iter().filter_map(|stmt| match stmt {
            Stmt::Decl(d) => Some(d.name.as_str()),
            _ => None,
        }).collect::<Vec<_>>();
        assert_eq!(names, ["i", "j", "p", "k"]);
        assert!(matches!(stmts[0], Stmt::Typedef(_)));
        assert!(matches!(stmts[1], Stmt::Typedef(_)));
        assert!(matches!(stmts[6], Stmt::Return(_)));
        assert!(parser.next_item().unwrap().is_none());
    }

    #[test]
    fn m10_block_list_rejects_malformed_declarators_and_initializers() {
        for source in [
            "int f(void){ int a,; return 0; }",
            "int f(void){ int a=1,b=; return a; }",
            "int f(void){ typedef int A,B=1; return 0; }",
            "int f(void){ int a=1,,b=2; return a; }",
            "int f(void){ for(int i=0,j=1;i<2;i++) return j; return 0; }",
        ] {
            assert!(Parser::new(source).next_item().is_err(), "{source}");
        }
    }


    #[test]
    fn asm2_basic_asm_is_a_function_body_statement_not_linkage_alias() {
        let source = r#"int f(void) { __asm__ volatile ("nop" ";" "mfence"); asm(""); return 1; }"#;
        let Some(ExternalItem::Function(function)) =
            Parser::new(source).next_item().unwrap() else { panic!("function expected"); };
        let Stmt::Block(statements) = function.body else { panic!("body expected"); };
        assert!(matches!(&statements[0],
            Stmt::BasicAsm { template, .. } if template == "nop;mfence"));
        assert!(matches!(&statements[1],
            Stmt::BasicAsm { template, .. } if template.is_empty()));
        assert!(matches!(&statements[2], Stmt::Return(_)));
    }

    #[test]
    fn asm2_basic_asm_rejects_extended_asm_and_nonliteral_templates() {
        for source in [
            r#"int f(void) { asm("nop" : "=r"(x) : : "memory" : label); return 0; }"#,
            r#"int f(void) { asm goto ("jmp %l0" : : : : label); label: return 0; }"#,
            r#"int f(void) { asm(0); return 0; }"#,
            r#"int f(void) { asm("nop" "\0" "ret"); return 0; }"#,
            r#"int f(void) { asm("nop" { return 0; }"#,
        ] {
            assert!(Parser::new(source).next_item().is_err(),
                "accepted unsupported basic asm spelling: {source}");
        }
    }

    #[test]
    fn asm2_extended_clobber_only_syntax_is_separate_from_basic_asm() {
        let src=r#"int f(void) {
            __asm__ volatile ("mfence" : : : "memory");
            asm ("" : : : "cc", "memory");
            asm ("nop" : : :);
            return 1;
        }"#;
        let Some(ExternalItem::Function(f)) =
            Parser::new(src).next_item().unwrap() else {panic!("expected function")};
        let Stmt::Block(stmts)=f.body else {panic!("expected compound body")};
        assert!(matches!(&stmts[0],Stmt::ExtendedAsm{template,clobbers,..}
            if template=="mfence" && clobbers==&["memory"]));
        assert!(matches!(&stmts[1],Stmt::ExtendedAsm{template,clobbers,..}
            if template.is_empty() && clobbers==&["cc","memory"]));
        assert!(matches!(&stmts[2],Stmt::ExtendedAsm{template,clobbers,..}
            if template=="nop" && clobbers.is_empty()));
    }

    #[test]
    fn asm2_extended_rejects_unsupported_operands_and_malformed_clobber_groups() {
        for src in [


            r#"int f(void) { asm("nop" ::: "memory",); }"#,
            r#"int f(void) { asm("nop" :: "memory"); }"#,
            r#"int f(void) { asm("nop" : : : "memory" : label); }"#,
            r#"int f(void) { asm("nop" : : : 3); }"#,
        ] {
            assert!(Parser::new(src).next_item().is_err(),
                "unexpectedly accepted unsupported extended asm: {src}");
        }
    }
    #[test]
    fn asm2_p5_single_gp_input_and_output_are_preserved_separately() {
        let source=r#"int f(int x) {
            int y=0;
            __asm__ volatile("movl %1, %0" : "=r"(y) : "r"(x) : "cc");
            return y;
        }"#;
        let Some(ExternalItem::Function(f))=Parser::new(source).next_item().unwrap()
        else {panic!("function expected")};
        let Stmt::Block(stmts)=f.body else {panic!("block expected")};
        assert!(matches!(&stmts[1],Stmt::ExtendedAsm{template,outputs,inputs,clobbers,..}
            if template=="movl %1, %0"
                && outputs.len()==1 && outputs[0].constraint=="=r"
                && inputs.len()==1 && inputs[0].constraint=="r"
                && clobbers==&["cc"]));
    }
    #[test]
    fn asm2_p5_operand_syntax_rejects_multiple_or_malformed_constraints() {
        for source in [
            r#"int f(void){ asm("nop":"=r"(x),:"r"(z)); }"#,
            r#"int f(void){ asm("nop":"=r"x:"r"(z)); }"#,
            r#"int f(void){ asm("nop":"=r"(x):"r"(z),"r"(y),"r"(w)); }"#,
            r#"int f(void){ asm("nop":"=r"(x):"r"(z):"memory",); }"#,
        ] {
            assert!(Parser::new(source).next_item().is_err(),"{source}");
        }
    }
    #[test]
    fn asm3_named_operands_preserve_labels_and_source_template() {
        let source=r#"int f(int x) {
            int y=0;
            __asm__ volatile("movl %[input], %[output]"
                : [output] "=r"(y) : [input] "r"(x) : "cc");
            return y;
        }"#;
        let Some(ExternalItem::Function(f))=Parser::new(source).next_item().unwrap()
        else {panic!("function expected")};
        let Stmt::Block(stmts)=f.body else {panic!("block expected")};
        let Stmt::ExtendedAsm {template,outputs,inputs,..}=&stmts[1]
        else {panic!("asm expected")};
        assert_eq!(template,"movl %[input], %[output]");
        assert_eq!(outputs[0].name.as_deref(),Some("output"));
        assert_eq!(outputs[0].constraint,"=r");
        assert_eq!(inputs[0].name.as_deref(),Some("input"));
        assert_eq!(inputs[0].constraint,"r");
    }

    #[test]
    fn asm3_malformed_operand_label_rejects_before_sema() {
        for src in [
            r#"int f(int x) { int y; asm("nop" : [x "=r"(y) : "r"(x)); }"#,
            r#"int f(int x) { int y; asm("nop" : [] "=r"(y) : "r"(x)); }"#,
            r#"int f(int x) { int y; asm("nop" : [42] "=r"(y) : "r"(x)); }"#,
            r#"int f(int x) { int y; asm("nop" : [output] "=r"(y) : [x] "r"(x), [bad "r"(x)); }"#,
        ] {
            assert!(Parser::new(src).next_item().is_err(),"invalid source: {src}");
        }
    }

    #[test]
    fn gnu_word_mode_preserves_glibc_register_t_abi_width() {
        let source = "typedef int register_t __attribute__ ((__mode__ (__word__)));";
        let Some(ExternalItem::Typedef(decl)) =
            Parser::new(source).next_item().unwrap() else {panic!("typedef expected");};
        assert_eq!(decl.name, "register_t");
        assert_eq!(decl.ty, Type::Long);
        let source = "typedef unsigned int uword_t __attribute__((mode(word)));";
        let Some(ExternalItem::Typedef(decl)) =
            Parser::new(source).next_item().unwrap() else {panic!("typedef expected");};
        assert_eq!(decl.ty, Type::Unsigned(Box::new(Type::Long)));
    }

    #[test]
    fn gnu_word_mode_rejects_unrelated_layout_and_abi_modes() {
        for src in [
            "typedef int short_t __attribute__((__mode__(__QI__)));",
            "typedef int other_t __attribute__((mode(__DI__)));",
            "typedef long long scalar_t __attribute__((mode(word)));",
            "typedef int *ptr_t __attribute__((mode(word)));",
            "int f(void) __attribute__((mode(word)));",
            "typedef int malformed_t __attribute__((mode));",
        ] {
            assert!(Parser::new(src).next_item().is_err(), "{src}");
        }
    }

    #[test]
    fn glibc_static_inline_helpers_keep_internal_linkage() {
        for keyword in ["inline", "__inline", "__inline__"] {
            let src = format!("static {keyword} int helper(int x) {{ return x + 1; }}");
            let Some(ExternalItem::Function(func)) =
                Parser::new(&src).next_item().unwrap() else {panic!("static inline function expected");};
            assert_eq!(func.name, "helper");
            assert_eq!(func.storage, StorageClass::Static);
        }
        for src in [
            "inline static int helper(int x) { return x; }",
            "static int inline helper(int x) { return x; }",
            "static inline inline int helper(int x) { return x; }",
        ] {
            let Some(ExternalItem::Function(func)) = Parser::new(src).next_item().unwrap()
                else { panic!("expected static inline function: {src}") };
            assert_eq!(func.storage, StorageClass::Static);
        }
        for src in [
            "static inline struct S { int x; };",
            "inline static int global;",
            "static inline int global;",
        ] {
            assert!(Parser::new(src).next_item().is_err(), "{src}");
        }
    }

    #[test]
    fn c99_external_inline_specifiers_are_preserved() {
        let mut parser = Parser::new("inline int f(void), g(void); extern inline int f(void){return 1;} int g(void){return 2;}");
        for _ in 0..2 {
            let Some(ExternalItem::Declaration(d)) = parser.next_item().unwrap() else {panic!("declaration")};
            assert!(d.is_inline);
            assert_eq!(d.storage, StorageClass::Default);
        }
        let Some(ExternalItem::Function(f)) = parser.next_item().unwrap() else {panic!("function")};
        assert!(f.is_inline);
        assert_eq!(f.storage, StorageClass::Extern);
        let Some(ExternalItem::Function(g)) = parser.next_item().unwrap() else {panic!("function")};
        assert!(!g.is_inline);
    }

    #[test]
    fn glibc_union_extension_member_retains_64_bit_layout_type() {
        let src = "typedef union { __extension__ unsigned long long int a; unsigned int b; } counter_t;";
        let Some(ExternalItem::Typedef(decl)) = Parser::new(src).next_item().unwrap()
        else { panic!("typedef expected"); };
        let Type::Union { members, .. } = decl.ty else {panic!("union expected")};
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].name, "a");
        assert_eq!(members[0].ty, Type::Unsigned(Box::new(Type::LongLong)));
        assert_eq!(members[1].ty, Type::Unsigned(Box::new(Type::Int)));
        for malformed in [
            "typedef union { __extension__ ; } bad_t;",
            "typedef union { __extension__ } bad_t;",
        ] {
            assert!(Parser::new(malformed).next_item().is_err(), "{malformed}");
        }
    }

    #[test]
    fn glibc_alloc_size_is_checked_without_abi_rewrite() {
        for source in [
            "void *allocate(unsigned long n) __attribute__((alloc_size(1)));",
            "void *allocate(unsigned long m, unsigned long n) __attribute__((__alloc_size__(1, 2)));",
        ] {
            let Some(ExternalItem::Declaration(decl)) =
                Parser::new(source).next_item().unwrap() else {panic!("function declaration expected")};
            assert!(matches!(decl.ty, Type::Function { .. }));
        }
        for source in [
            "int x __attribute__((alloc_size(1)));",
            "void *allocate(unsigned long n) __attribute__((alloc_size(0)));",
            "void *allocate(unsigned long n) __attribute__((alloc_size(2)));",
            "void *allocate(unsigned long n) __attribute__((alloc_size(1, 1)));",
            "void *allocate(unsigned long n) __attribute__((alloc_size(x)));",
            "void *allocate(unsigned long n) __attribute__((alloc_size(1,)));",
        ] {
            assert!(Parser::new(source).next_item().is_err(), "{source}");
        }
    }

    #[test]
    fn glibc_alloc_align_validates_one_existing_fixed_argument() {
        let source = "void *aligned_alloc(unsigned long align, unsigned long bytes) __attribute__((__alloc_align__(1)));";
        let Some(ExternalItem::Declaration(decl)) =
            Parser::new(source).next_item().unwrap() else {panic!("function expected")};
        assert_eq!(decl.name, "aligned_alloc");
        for source in [
            "void *f(unsigned long n) __attribute__((alloc_align(0)));",
            "void *f(unsigned long n) __attribute__((alloc_align(2)));",
            "void *f(unsigned long n) __attribute__((alloc_align(x)));",
            "void *f(unsigned long n) __attribute__((alloc_align(1, 1)));",
            "void *f(unsigned long n) __attribute__((alloc_align(1,)));",
        ] {
            assert!(Parser::new(source).next_item().is_err(), "{source}");
        }
    }

    #[test]
    fn file_scope_declarator_lists_stream_in_source_order() {
        let src = "float value, fixed, floated; int *p, a[3]; int x=1, y=2;";
        let mut parser = Parser::new(src);
        let mut names = Vec::new();
        let mut entries = Vec::new();
        while let Some(item) = parser.next_item().unwrap() {
            let ExternalItem::Declaration(decl) = item else {panic!("declaration expected");};
            names.push(decl.name.clone());
            entries.push(decl);
        }
        assert_eq!(names, ["value", "fixed", "floated", "p", "a", "x", "y"]);
        assert!(matches!(&entries[3].ty, Type::Pointer(_)));
        assert!(matches!(&entries[4].ty, Type::Array(_, _)));
        assert!(entries[5].init.is_some() && entries[6].init.is_some());
        assert!(entries[0].init.is_none());
    }

    #[test]
    fn file_scope_comma_typedefs_are_immediately_visible() {
        let src = "typedef int first_t, second_t; first_t x; second_t y;";
        let mut parser = Parser::new(src);
        let mut names = Vec::new();
        while let Some(item) = parser.next_item().unwrap() {
            match item {
                ExternalItem::Typedef(decl) => names.push(("typedef", decl.name)),
                ExternalItem::Declaration(decl) => names.push(("declaration", decl.name)),
                _ => panic!("unexpected item"),
            }
        }
        assert_eq!(names, [("typedef", "first_t".into()),
            ("typedef", "second_t".into()),
            ("declaration", "x".into()), ("declaration", "y".into())]);
    }

    #[test]
    fn file_scope_comma_storage_and_function_declarators() {
        let mut parser = Parser::new(
            "static int left, right; extern int a, b; int f(void), g(void);"
        );
        let mut entries = Vec::new();
        while let Some(item) = parser.next_item().unwrap() {
            let ExternalItem::Declaration(decl) = item else {panic!("declaration expected")};
            entries.push(decl);
        }
        assert_eq!(entries.len(), 6);
        assert_eq!(entries[0].storage, StorageClass::Static);
        assert_eq!(entries[1].storage, StorageClass::Static);
        assert_eq!(entries[2].storage, StorageClass::Extern);
        assert_eq!(entries[3].storage, StorageClass::Extern);
        assert!(matches!(&entries[4].ty, Type::Function { .. }));
        assert!(matches!(&entries[5].ty, Type::Function { .. }));
    }

    #[test]
    fn file_scope_malformed_comma_declarators_fail_closed() {
        for source in [
            "float a,;",
            "int a,,b;",
            "typedef int A, B = 3;",
            "int a = 1, b = ;",
            "int a, 42;",
        ] {
            let mut parser = Parser::new(source);
            let mut failed = false;
            for _ in 0..8 {
                match parser.next_item() {
                    Err(_) => { failed = true; break; },
                    Ok(None) => break,
                    Ok(Some(_)) => {},
                }
            }
            assert!(failed, "invalid comma group unexpectedly accepted: {source}");
        }
    }

    #[test]
    fn comma_file_items_do_not_consume_following_function_body() {
        let source = "int a, b, c; int foo(void) { return b; } int d;";
        let mut parser = Parser::new(source);
        for expected in ["a", "b", "c"] {
            let Some(ExternalItem::Declaration(decl)) = parser.next_item().unwrap()
            else {panic!("expected declaration");};
            assert_eq!(decl.name, expected);
        }
        let Some(ExternalItem::Function(function)) = parser.next_item().unwrap()
        else {panic!("function expected");};
        assert_eq!(function.name, "foo");
        let Some(ExternalItem::Declaration(decl)) = parser.next_item().unwrap()
        else {panic!("final declaration expected");};
        assert_eq!(decl.name, "d");
        assert!(parser.next_item().unwrap().is_none());
    }

}
