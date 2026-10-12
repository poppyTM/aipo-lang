//! Recursive descent parser with Pratt expression parsing and error recovery.

use crate::shift::shift_expr;
use aipo_ast::*;
use aipo_diagnostics::{Diagnostic, DiagnosticCode, PrimarySpan};

/// Largest integer the runtime treats as safe (`2^53 - 1`).
///
/// Used as the implicit end bound of an open-ended slice, where the runtime clamps every
/// slice bound into `0..=len` and any value at or above `len` therefore means "to the end".
const SAFE_MAX_INT: i64 = 9_007_199_254_740_991;
use aipo_lexer::{Lexer, StringPrefix as LexerStringPrefix, Token, TokenKind};
use aipo_source::{Source, SourceId, SourceSpan};

/// Moves a sub-parse diagnostic back to original-file coordinates.
///
/// Sub-parses run on the unpadded slice, so every offset is relative to the
/// slice start; adding `delta` restores absolute positions, with line/column
/// re-derived from the outer source. Offsets are clamped to the file length so
/// a span touching the synthetic trailing newline can never escape the file.
fn shift_diagnostic(source: &Source, mut diagnostic: Diagnostic, delta: usize) -> Diagnostic {
    let outer_len = source.len();
    if let Some(span) = diagnostic.primary_span.take() {
        let start = (span.start + delta).min(outer_len);
        let end = (span.end + delta).min(outer_len).max(start);
        diagnostic.primary_span = Some(PrimarySpan::from_source(
            source,
            SourceSpan::new(start, end),
        ));
    }
    for suggestion in &mut diagnostic.suggestions {
        suggestion.start = (suggestion.start + delta).min(outer_len);
        suggestion.end = (suggestion.end + delta)
            .min(outer_len)
            .max(suggestion.start);
    }
    diagnostic
}

/// Finds the `}` that closes the placeholder body starting at `body_start`.
///
/// Braces may nest inside a placeholder (a dict literal or a nested format string), so
/// the scan is depth aware.
fn find_placeholder_end(bytes: &[u8], body_start: usize) -> Option<usize> {
    let mut depth = 1usize;
    let mut index = body_start;
    let mut in_string = false;
    let mut string_escape = false;
    while index < bytes.len() {
        if in_string {
            if string_escape {
                string_escape = false;
            } else if bytes[index] == b'\\' {
                string_escape = true;
            } else if bytes[index] == b'"' {
                in_string = false;
            }
        } else {
            match bytes[index] {
                b'"' => in_string = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(index);
                    }
                }
                _ => {}
            }
        }
        index += 1;
    }
    None
}

/// Resolves the `{{`/`}}` literal-brace escapes of a format-string segment.
fn unescape_braces(text: &str) -> String {
    text.replace("{{", "{").replace("}}", "}")
}

/// Parser state maintaining token stream, cursor position, and diagnostic collection.
pub struct Parser<'a> {
    source: &'a Source,
    tokens: Vec<Token>,
    cursor: usize,
    diagnostics: Vec<Diagnostic>,
    /// Directives `#!...` already lexed but not yet attached to an item.
    ///
    /// A directive applies to the item that follows it. Collecting them here keeps the
    /// attachment at the item constructors instead of threading a parameter through every
    /// `parse_*_decl` signature.
    pending_directives: Vec<Directive>,
    /// Current expression nesting depth (parentheses, calls, binary chains).
    expr_depth: usize,
    /// Current block nesting depth (`if`/`fn`/loops/`match` bodies).
    block_depth: usize,
    /// Set once a nesting overflow is reported: the rest of the file is
    /// skipped quietly instead of cascading one error per remaining token.
    depth_aborted: bool,
    /// Whether multiple comma-separated subjects before `is` (`a, b, c is T`) are permitted in the current context.
    allow_comma_is: bool,
    /// Whether the expression being parsed is the type operand of an `is` test.
    ///
    /// There, a trailing `?` is the nullable marker of `is Int?`, not the failure
    /// propagation `expr?`. The flag suspends the postfix so the caller can consume it.
    in_is_type: bool,
    /// Whether the expression being parsed is a block header (`if` condition,
    /// `while` condition, `repeat` count, `each` iterable, `match` target).
    ///
    /// There, a trailing `{` on the same line is the body opener, never struct
    /// construction (`board {` is `board` + body, while `Board{` with no gap
    /// stays construction). The flag lets `parse_primary` leave that brace
    /// for the caller instead of consuming it as `Type{...}`.
    in_block_header: bool,
}

/// Maximum expression nesting before the parser bails out with
/// `AIPO_PARSE_NESTING_TOO_DEEP`.
///
/// Rationale (see ADP-005): the deepest corpus nesting is single digits, so 128
/// is orders of magnitude beyond legitimate code, while measured per-level
/// frame cost (~10–16 KiB) keeps 128 levels under 2 MiB — inside the smallest
/// supported host stack (test threads). This is a robustness bound, not syntax:
/// every program below it parses exactly as before.
const MAX_EXPR_DEPTH: usize = 128;

/// Maximum block nesting, same rationale as [`MAX_EXPR_DEPTH`]. Block frames
/// measured larger (~13–27 KiB), hence the lower bound.
const MAX_BLOCK_DEPTH: usize = 64;

impl<'a> Parser<'a> {
    /// Constructs a new parser from source and token list.
    #[must_use]
    pub fn new(source: &'a Source, tokens: Vec<Token>) -> Self {
        // Filter out non-significant comments; keep significant newlines
        let tokens: Vec<Token> = tokens
            .into_iter()
            .filter(|t| !matches!(t.kind, TokenKind::Comment(_)))
            .collect();

        Self {
            source,
            tokens,
            cursor: 0,
            diagnostics: Vec::new(),
            pending_directives: Vec::new(),
            expr_depth: 0,
            block_depth: 0,
            depth_aborted: false,
            allow_comma_is: true,
            in_is_type: false,
            in_block_header: false,
        }
    }

    /// Reports host-stack exhaustion protection: nesting past the documented
    /// bound is a controlled diagnostic, never a process abort.
    fn too_deep(&mut self, what: &str) {
        self.depth_aborted = true;
        let span = self
            .peek_token()
            .map(|t| t.span)
            .unwrap_or_else(|| SourceSpan::empty(self.source.len()));
        self.diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::AIPO_PARSE_NESTING_TOO_DEEP,
                format!("{what} nesting is too deep (limit documented in ADP-005)"),
            )
            .with_primary_span(self.source, span),
        );
    }

    /// Parses a single expression from the token stream.
    ///
    /// Exposed for format-string interpolation, which parses the expression embedded in
    /// a `{...}` placeholder out of the same source buffer.
    pub fn parse_expression(&mut self) -> Option<Expr> {
        self.parse_expr()
    }

    /// Desugars a format string into a concatenation of text segments and `String(...)`.
    ///
    /// `f"total: {n}!"` becomes `"total: " + String(n) + "!"` so interpolation and the
    /// explicit `String(value)` conversion share one definition of textual rendering
    /// (`Aipo V1 — Language Reference`: interpolation is the preferred composition form).
    ///
    /// Both the text segments and the placeholder expressions are re-lexed from the
    /// original buffer, so literal escapes are processed by the single lexer
    /// implementation and every produced span points at a real file position.
    /// `{{` and `}}` are the literal-brace escapes.
    fn parse_format_string(&mut self, span: SourceSpan, prefix: StringPrefix) -> Option<Expr> {
        let text = self.source.text();
        let raw = &text[span.start..span.end];
        let prefix_len = raw
            .chars()
            .take_while(|ch| *ch == 'f' || *ch == 'r')
            .count();
        let quote_len = if raw[prefix_len..].starts_with("\"\"\"") {
            3
        } else {
            1
        };

        let inner_start = span.start + prefix_len + quote_len;
        let inner_end = span.end.saturating_sub(quote_len);
        if inner_end < inner_start {
            return None;
        }

        let inner = &text[inner_start..inner_end];
        let is_raw = matches!(prefix, StringPrefix::FormatRaw);
        let bytes = inner.as_bytes();
        let mut parts: Vec<Expr> = Vec::new();
        let mut segment_start = 0usize;
        let mut index = 0usize;

        while index < bytes.len() {
            match bytes[index] {
                b'{' if bytes.get(index + 1) == Some(&b'{') => index += 2,
                b'}' if bytes.get(index + 1) == Some(&b'}') => index += 2,
                b'{' => {
                    self.push_format_segment(segment_start, index, inner_start, is_raw, &mut parts);

                    let body_start = index + 1;
                    let Some(body_end) = find_placeholder_end(bytes, body_start) else {
                        self.diagnostics.push(
                            Diagnostic::error(
                                DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN,
                                "unclosed interpolation placeholder in format string",
                            )
                            .with_primary_span(self.source, span),
                        );
                        return None;
                    };

                    let expression = self
                        .parse_slice_expression(inner_start + body_start, inner_start + body_end)?;
                    let call_span =
                        SourceSpan::new(inner_start + index, inner_start + body_end + 1);
                    let callee = Expr::Identifier(Ident::new("String".to_string(), call_span));
                    let argument = CallArg {
                        name: None,
                        value: expression,
                        span: call_span,
                    };
                    parts.push(Expr::Call(CallExpr {
                        callee: Box::new(callee),
                        args: vec![argument],
                        trailing_block: None,
                        span: call_span,
                    }));

                    index = body_end + 1;
                    segment_start = index;
                }
                b'}' => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN,
                            "unmatched '}' in format string: use '}}' for a literal brace",
                        )
                        .with_primary_span(self.source, span),
                    );
                    return None;
                }
                _ => index += 1,
            }
        }

        self.push_format_segment(segment_start, bytes.len(), inner_start, is_raw, &mut parts);

        let mut parts = parts.into_iter();
        let Some(first) = parts.next() else {
            return Some(Expr::Literal(
                Literal::String(String::new(), StringPrefix::Normal),
                span,
            ));
        };

        let folded = parts.fold(first, |left, right| {
            Expr::Binary(BinaryOp::Add, Box::new(left), Box::new(right), span)
        });
        Some(folded)
    }

    /// Appends the format-string text between two offsets as a `String` literal.
    fn push_format_segment(
        &mut self,
        from: usize,
        to: usize,
        inner_start: usize,
        is_raw: bool,
        parts: &mut Vec<Expr>,
    ) {
        if from >= to {
            return;
        }

        let segment_span = SourceSpan::new(inner_start + from, inner_start + to);
        let Some(content) = self.lex_slice_string(inner_start + from, inner_start + to, is_raw)
        else {
            return;
        };
        if content.is_empty() {
            return;
        }

        parts.push(Expr::Literal(
            Literal::String(unescape_braces(&content), StringPrefix::Normal),
            segment_span,
        ));
    }

    /// Re-lexes a slice of the original text as a string literal and returns its content.
    ///
    /// The slice is lexed unpadded: only the literal content is needed here, so
    /// no absolute-offset padding is built (it made placeholder-heavy files
    /// quadratic — see ADP-005).
    fn lex_slice_string(&self, start: usize, end: usize, is_raw: bool) -> Option<String> {
        let mut text = String::with_capacity(end - start + 3);
        text.push_str(if is_raw { "r\"" } else { "\"" });
        text.push_str(&self.source.text()[start..end]);
        text.push('"');

        let sub_source = Source::new(SourceId::next(), self.source.name(), &text);
        let (tokens, diagnostics) = Lexer::new(&sub_source).tokenize();
        if !diagnostics.is_empty() {
            return None;
        }

        match tokens.first().map(|token| &token.kind) {
            Some(TokenKind::StringLiteral { content, .. }) => Some(content.clone()),
            _ => None,
        }
    }

    /// Parses a slice of the original text as a single expression.
    ///
    /// The slice is parsed unpadded and every produced span is shifted back by
    /// `start` (see `shift.rs`), so observable spans are identical to the old
    /// padded implementation while the work stays proportional to the slice.
    fn parse_slice_expression(&mut self, start: usize, end: usize) -> Option<Expr> {
        if start > end || end > self.source.text().len() {
            return None;
        }

        let mut text = String::with_capacity(end - start + 1);
        text.push_str(&self.source.text()[start..end]);
        text.push('\n');

        let sub_source = Source::new(SourceId::next(), self.source.name(), &text);
        let (tokens, diagnostics) = Lexer::new(&sub_source).tokenize();
        if let Some(first) = diagnostics.first() {
            self.diagnostics
                .push(shift_diagnostic(self.source, first.clone(), start));
            return None;
        }

        let mut sub = Parser::new(&sub_source, tokens);
        let expression = sub.parse_expression().map(|expr| shift_expr(&expr, start));
        for diagnostic in sub.diagnostics {
            self.diagnostics
                .push(shift_diagnostic(self.source, diagnostic, start));
        }

        if expression.is_none() {
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN,
                    "format string placeholder must contain a single expression",
                )
                .with_primary_span(self.source, SourceSpan::new(start, end)),
            );
        }
        expression
    }

    /// Parses the entire token stream into a `Program`.
    pub fn parse(mut self) -> (Program, Vec<Diagnostic>) {
        let mut items = Vec::new();
        let mut statements = Vec::new();

        self.skip_newlines();

        while !self.is_at_end() && !self.depth_aborted {
            if let TokenKind::Directive(raw) = self.peek().clone() {
                let token = self.advance();
                let dir = self.parse_directive_payload(&raw, token.span);
                self.pending_directives.push(dir);
                self.skip_newlines();
                continue;
            }

            if self.is_item_start() {
                if let Some(item) = self.parse_item() {
                    items.push(item);
                }
            } else if let Some(stmt) = self.parse_stmt() {
                if !self.pending_directives.is_empty() {
                    let first = &self.pending_directives[0];
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN,
                            "directives can only be attached to items, not statements",
                        )
                        .with_primary_span(self.source, first.span),
                    );
                    self.pending_directives.clear();
                }
                statements.push(stmt);
            } else {
                self.synchronize();
            }
            self.skip_newlines();
        }

        if !self.pending_directives.is_empty() {
            let first = &self.pending_directives[0];
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN,
                    "trailing directive has no item to attach to",
                )
                .with_primary_span(self.source, first.span),
            );
            self.pending_directives.clear();
        }

        let span = if let (Some(first), Some(last)) = (self.tokens.first(), self.tokens.last()) {
            first.span.merge(last.span)
        } else {
            SourceSpan::empty(0)
        };

        (
            Program {
                items,
                statements,
                span,
            },
            self.diagnostics,
        )
    }

    // --- Helpers ---

    fn peek(&self) -> &TokenKind {
        self.tokens
            .get(self.cursor)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    fn peek_token(&self) -> Option<&Token> {
        self.tokens.get(self.cursor)
    }

    fn peek_ahead(&self, offset: usize) -> &TokenKind {
        self.tokens
            .get(self.cursor + offset)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    fn advance(&mut self) -> Token {
        if !self.is_at_end() {
            let tok = self.tokens[self.cursor].clone();
            self.cursor += 1;
            tok
        } else {
            self.tokens
                .last()
                .cloned()
                .unwrap_or_else(|| Token::new(TokenKind::Eof, SourceSpan::empty(self.source.len())))
        }
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek(), TokenKind::Eof)
    }

    /// Looks ahead through matching parentheses to check if a `=>` follows,
    /// identifying a short lambda `(...) => expr`.
    fn is_lambda_ahead(&self) -> bool {
        let mut depth = 0;
        let mut i = self.cursor;
        while i < self.tokens.len() {
            match &self.tokens[i].kind {
                TokenKind::LParen => depth += 1,
                TokenKind::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        let mut next_i = i + 1;
                        while next_i < self.tokens.len()
                            && self.tokens[next_i].kind == TokenKind::Newline
                        {
                            next_i += 1;
                        }
                        return self.tokens.get(next_i).map(|t| &t.kind)
                            == Some(&TokenKind::FatArrow);
                    }
                }
                TokenKind::Eof => {
                    return false;
                }
                _ => {}
            }
            i += 1;
        }
        false
    }

    fn check(&self, expected: &TokenKind) -> bool {
        if self.is_at_end() {
            false
        } else {
            std::mem::discriminant(self.peek()) == std::mem::discriminant(expected)
        }
    }

    fn match_token(&mut self, expected: &TokenKind) -> bool {
        if self.check(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), TokenKind::Newline) {
            self.advance();
        }
    }

    fn expect(&mut self, expected: &TokenKind, msg: &str) -> Option<Token> {
        if self.check(expected) {
            Some(self.advance())
        } else {
            let span = self
                .peek_token()
                .map(|t| t.span)
                .unwrap_or_else(|| SourceSpan::empty(self.source.len()));
            self.diagnostics.push(
                Diagnostic::error(DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN, msg)
                    .with_primary_span(self.source, span),
            );
            None
        }
    }

    fn synchronize(&mut self) {
        while !self.is_at_end() {
            if matches!(
                self.peek(),
                TokenKind::Let
                    | TokenKind::Var
                    | TokenKind::Fn
                    | TokenKind::Async
                    | TokenKind::Await
                    | TokenKind::Struct
                    | TokenKind::Interface
                    | TokenKind::Import
                    | TokenKind::Export
                    | TokenKind::If
                    | TokenKind::While
                    | TokenKind::Loop
                    | TokenKind::Repeat
                    | TokenKind::Each
                    | TokenKind::Return
            ) {
                return;
            }
            let tok = self.advance();
            if matches!(tok.kind, TokenKind::Newline) {
                return;
            }
        }
    }

    /// Parses `#!nome ...` payload text into a [`Directive`].
    ///
    /// Form: bare name, `nome[tag]`, or `nome("texto")`. Anything else after the name is
    /// kept as raw text; validation of which names and arguments are legal happens later,
    /// when the directive is attached to an item.
    fn parse_directive_payload(&mut self, raw: &str, span: SourceSpan) -> Directive {
        let trimmed = raw.trim();
        let end_of_name = trimmed
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(trimmed.len());
        let name_text = &trimmed[..end_of_name];
        let rest = trimmed[end_of_name..].trim();
        Directive {
            name: Ident::new(name_text.to_string(), span),
            argument: if rest.is_empty() {
                None
            } else {
                Some(rest.to_string())
            },
            span,
        }
    }

    fn take_directives(&mut self) -> Vec<Directive> {
        std::mem::take(&mut self.pending_directives)
    }

    fn is_item_start(&self) -> bool {
        // `Tipo:nome` and `Tipo::[...]` start with an identifier, so the same two-token
        // lookahead `parse_item` uses is required here to route them to the item parser.
        if (matches!(self.peek(), TokenKind::Identifier(_))
            && matches!(self.peek_ahead(1), TokenKind::Colon | TokenKind::ColonColon))
            || (matches!(self.peek(), TokenKind::Async)
                && matches!(self.peek_ahead(1), TokenKind::Identifier(_))
                && matches!(self.peek_ahead(2), TokenKind::Colon))
        {
            return true;
        }
        matches!(
            self.peek(),
            TokenKind::Fn
                | TokenKind::Async
                | TokenKind::Struct
                | TokenKind::Enum
                | TokenKind::Interface
                | TokenKind::Import
                | TokenKind::Export
        )
    }

    // --- Items ---

    fn parse_item(&mut self) -> Option<Item> {
        // `Tipo:nome(...)` and `Tipo::[...]` are type-associated items. The lookahead is
        // what separates them from a bare identifier, which is never an item start.
        if (matches!(self.peek(), TokenKind::Identifier(_))
            && matches!(self.peek_ahead(1), TokenKind::Colon | TokenKind::ColonColon))
            || (matches!(self.peek(), TokenKind::Async)
                && matches!(self.peek_ahead(1), TokenKind::Identifier(_))
                && matches!(self.peek_ahead(2), TokenKind::Colon))
        {
            return self.parse_associated_item();
        }
        match self.peek() {
            TokenKind::Fn => self.parse_function_decl().map(Item::Fn),
            TokenKind::Async => self.parse_async_item(),
            TokenKind::Struct => self.parse_struct_decl().map(Item::Struct),
            TokenKind::Enum => self.parse_enum_decl().map(Item::Enum),
            TokenKind::Interface => self.parse_interface_decl().map(Item::Interface),
            TokenKind::Import => self.parse_import_decl().map(Item::Import),
            TokenKind::Export => self.parse_export_decl().map(Item::Export),
            _ => None,
        }
    }

    fn parse_struct_decl(&mut self) -> Option<StructDecl> {
        let start = self.advance().span; // 'struct'
        let name = self.parse_ident()?;
        self.skip_newlines();

        self.expect(
            &TokenKind::LBrace,
            "expected '{' to open struct declaration",
        )?;
        self.skip_newlines();

        let mut fields = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let is_var = self.match_token(&TokenKind::Var);
            let is_legacy_fixed = self.match_token(&TokenKind::Fixed);
            let is_fixed = !is_var || is_legacy_fixed;
            let field_name = self.parse_ident()?;
            let type_annotation = if self.match_token(&TokenKind::Colon) {
                Some(self.parse_type_annotation()?)
            } else {
                None
            };
            let default = if self.match_token(&TokenKind::Equal) {
                Some(self.parse_expr()?)
            } else {
                None
            };
            let span = if let Some(def) = &default {
                field_name.span.merge(def.span())
            } else {
                field_name.span
            };
            fields.push(StructField {
                name: field_name,
                is_fixed,
                type_annotation,
                default,
                span,
            });
            self.skip_newlines();
            let _ = self.match_token(&TokenKind::Comma);
            self.skip_newlines();
        }

        let end_span = self
            .expect(
                &TokenKind::RBrace,
                "expected '}' to close struct declaration",
            )?
            .span;
        Some(StructDecl {
            directives: self.take_directives(),
            name,
            fields,
            span: start.merge(end_span),
        })
    }

    fn parse_type_annotation(&mut self) -> Option<TypeAnnotation> {
        let type_ident = self.parse_ident()?;
        let is_nullable = self.match_token(&TokenKind::Question);
        self.check_no_parametric_contract();
        let span = if is_nullable {
            type_ident.span.merge(self.tokens[self.cursor - 1].span)
        } else {
            type_ident.span
        };
        Some(TypeAnnotation {
            name: type_ident.name,
            is_nullable,
            span,
        })
    }

    fn parse_enum_decl(&mut self) -> Option<EnumDecl> {
        let start = self.advance().span; // 'enum'
        let name = self.parse_ident()?;
        self.skip_newlines();

        self.expect(&TokenKind::LBrace, "expected '{' to open enum declaration")?;
        self.skip_newlines();

        let mut variants = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let var_start = self.tokens[self.cursor].span;
            let var_name = self.parse_ident()?;
            self.skip_newlines();

            let payload = if self.match_token(&TokenKind::LParen) {
                // Tuple variant: `Desligado(motivo: String)` or `Desligado(String)`
                self.skip_newlines();
                let mut fields = Vec::new();
                while !self.check(&TokenKind::RParen) && !self.is_at_end() {
                    let field_start = self.tokens[self.cursor].span;
                    let (param_name, type_annotation) = if self.peek_ahead(1) == &TokenKind::Colon {
                        let id = self.parse_ident()?;
                        self.expect(&TokenKind::Colon, "expected ':' after parameter name")?;
                        let ty = self.parse_type_annotation()?;
                        (Some(id), ty)
                    } else {
                        let ty = self.parse_type_annotation()?;
                        (None, ty)
                    };
                    let span = field_start.merge(type_annotation.span);
                    fields.push(EnumTupleField {
                        name: param_name,
                        type_annotation,
                        span,
                    });
                    self.skip_newlines();
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                    self.skip_newlines();
                }
                self.expect(
                    &TokenKind::RParen,
                    "expected ')' to close enum tuple variant",
                )?;
                EnumVariantPayload::Tuple(fields)
            } else if self.match_token(&TokenKind::LBrace) {
                // Struct variant: `Ativo { desde: Int }`
                self.skip_newlines();
                let mut fields = Vec::new();
                while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
                    let field_start = self.tokens[self.cursor].span;
                    let field_name = self.parse_ident()?;
                    let type_annotation = if self.match_token(&TokenKind::Colon) {
                        Some(self.parse_type_annotation()?)
                    } else {
                        None
                    };
                    let span = if let Some(ty) = &type_annotation {
                        field_start.merge(ty.span)
                    } else {
                        field_name.span
                    };
                    fields.push(EnumStructField {
                        name: field_name,
                        type_annotation,
                        span,
                    });
                    self.skip_newlines();
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                    self.skip_newlines();
                }
                self.expect(
                    &TokenKind::RBrace,
                    "expected '}' to close enum struct variant",
                )?;
                EnumVariantPayload::Struct(fields)
            } else {
                EnumVariantPayload::Unit
            };

            let var_end = self.tokens[self.cursor.saturating_sub(1)].span;
            variants.push(EnumVariant {
                name: var_name,
                payload,
                span: var_start.merge(var_end),
            });

            self.skip_newlines();
            let _ = self.match_token(&TokenKind::Comma);
            self.skip_newlines();
        }

        let end_span = self
            .expect(&TokenKind::RBrace, "expected '}' to close enum declaration")?
            .span;

        Some(EnumDecl {
            directives: self.take_directives(),
            name,
            variants,
            span: start.merge(end_span),
        })
    }

    /// Parses the type-associated forms that replace `impl Tipo { ... }`.
    ///
    /// Three shapes, distinguished by the token after the type name:
    ///
    /// - `Tipo:nome(params) { }` — a method, or the `init` hook when `nome` is `init`
    /// - `Tipo:invariant { }` — the structural invariant
    /// - `Tipo::[fn1, fn2]` — batch association of free functions
    ///
    /// The association lives in the declaration, so no wrapper block appears and the
    /// receiver is implicit unless the author writes `self` or `var self`.
    fn parse_associated_item(&mut self) -> Option<Item> {
        let is_async_prefix = self.match_token(&TokenKind::Async);
        let start = self.tokens[self.cursor].span;
        let target = self.parse_ident()?;
        self.advance(); // ':' or '::'

        if matches!(
            self.tokens[self.cursor.saturating_sub(1)].kind,
            TokenKind::ColonColon
        ) {
            return self.parse_batch_bind(start, target).map(Item::Batch);
        }

        let is_async = is_async_prefix || self.match_token(&TokenKind::Async);
        let name = if self.match_token(&TokenKind::Init) {
            let span = self.tokens[self.cursor - 1].span;
            Ident::new("init".into(), span)
        } else if self.match_token(&TokenKind::Invariant) {
            let span = self.tokens[self.cursor - 1].span;
            Ident::new("invariant".into(), span)
        } else {
            self.parse_ident()?
        };

        if name.name == "invariant" {
            let hook = self.parse_invariant_hook_body(name.span)?;
            let span = start.merge(hook.span);
            return Some(Item::Invariant(TargetInvariant { target, hook, span }));
        }

        let is_init = name.name == "init";
        let function = self.parse_method_body(start, name, is_init, is_async)?;
        let span = function.span;
        Some(Item::Method(MethodDecl {
            target,
            function,
            span,
        }))
    }

    /// Parses `Tipo::[fn1, fn2]`, the batch association form.
    fn parse_batch_bind(&mut self, start: SourceSpan, target: Ident) -> Option<BatchBind> {
        if !self.check(&TokenKind::LBracket) {
            let span = self
                .peek_token()
                .map(|t| t.span)
                .unwrap_or_else(|| SourceSpan::empty(self.source.len()));
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::AIPO_PARSE_LOTE_INVALIDO,
                    "'::' espera uma lista entre colchetes; use ':' para declarar um método individual",
                )
                .with_primary_span(self.source, span)
                .with_note(format!(
                    "`{}:...()` declara um método; `{}::[f1, f2]` agrupa funções",
                    target.name, target.name
                )),
            );
            return None;
        }
        self.advance(); // '['
        self.skip_newlines();

        let mut functions = Vec::new();
        while !self.check(&TokenKind::RBracket) && !self.is_at_end() {
            functions.push(self.parse_ident()?);
            self.skip_newlines();
            if !self.match_token(&TokenKind::Comma) {
                break;
            }
            self.skip_newlines();
        }
        let end = self.expect(
            &TokenKind::RBracket,
            "expected ']' to close batch association",
        )?;
        Some(BatchBind {
            target,
            functions,
            span: start.merge(end.span),
        })
    }

    /// Parses the `invariant { ... }` body after `Tipo:invariant` was consumed.
    fn parse_invariant_hook_body(&mut self, name_span: SourceSpan) -> Option<InvariantHook> {
        if self.match_token(&TokenKind::LParen) {
            self.expect(&TokenKind::RParen, "expected ')' after invariant")?;
        }
        self.skip_newlines();

        self.expect(&TokenKind::LBrace, "expected '{' to open invariant")?;
        self.skip_newlines();

        let mut conditions = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if let Some(expr) = self.parse_expr() {
                conditions.push(expr);
            }
            self.skip_newlines();
        }
        let end_span = self
            .expect(&TokenKind::RBrace, "expected '}' to close invariant")?
            .span;

        Some(InvariantHook {
            conditions,
            span: name_span.merge(end_span),
        })
    }

    /// Parses `(params) -> T { body }` for a method already named by `Tipo:nome`.
    ///
    /// `fn` is absent by design: the `Tipo:` prefix already says this is a function.
    /// The receiver is injected exactly like `init` does, so every later stage sees the
    /// same shape whether the author wrote `self`, `var self` or nothing.
    fn parse_method_body(
        &mut self,
        start: SourceSpan,
        name: Ident,
        is_init: bool,
        is_async: bool,
    ) -> Option<FunctionDecl> {
        self.expect(&TokenKind::LParen, "expected '(' after method name")?;
        let mut params = self.parse_params()?;
        self.expect(&TokenKind::RParen, "expected ')' after method parameters")?;
        let return_type = self.parse_optional_return_type();
        self.skip_newlines();

        // `init` builds the instance, so its implicit receiver is mutable; every other
        // method keeps an immutable implicit receiver and must write `var self` to mutate.
        let implicit_mutable = is_init;
        if params.first().is_none_or(|param| param.name.name != "self") {
            params.insert(
                0,
                Param {
                    name: Ident::new("self".into(), start),
                    is_mutable: implicit_mutable,
                    type_annotation: None,
                    default: None,
                    span: start,
                },
            );
        }

        self.expect(&TokenKind::LBrace, "expected '{' to open method body")?;
        self.skip_newlines();

        let mut body = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if let Some(stmt) = self.parse_stmt() {
                body.push(stmt);
            }
            self.skip_newlines();
        }
        let end_span = self
            .expect(&TokenKind::RBrace, "expected '}' to close method")?
            .span;

        Some(FunctionDecl {
            directives: Vec::new(),
            name,
            is_async,
            params,
            return_type,
            body,
            span: start.merge(end_span),
        })
    }

    fn parse_interface_decl(&mut self) -> Option<InterfaceDecl> {
        let start = self.advance().span; // 'interface'
        let name = self.parse_ident()?;
        self.skip_newlines();

        self.expect(
            &TokenKind::LBrace,
            "expected '{' to open interface declaration",
        )?;
        self.skip_newlines();

        let mut methods = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if let Some(method) = self.parse_function_signature() {
                methods.push(method);
            } else {
                self.advance();
            }
            self.skip_newlines();
        }

        let end_span = self
            .expect(
                &TokenKind::RBrace,
                "expected '}' to close interface declaration",
            )?
            .span;
        Some(InterfaceDecl {
            name,
            methods,
            span: start.merge(end_span),
        })
    }

    fn parse_import_decl(&mut self) -> Option<ImportDecl> {
        let start = self.advance().span; // 'import'
        let mut module_name = vec![self.parse_ident()?];
        while self.match_token(&TokenKind::Dot) {
            module_name.push(self.parse_ident()?);
        }

        let alias = if self.match_token(&TokenKind::As) {
            Some(self.parse_ident()?)
        } else {
            None
        };

        let mut names = Vec::new();
        if self.match_token(&TokenKind::Colon) {
            loop {
                names.push(self.parse_ident()?);
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
        }

        let end_span = names
            .last()
            .map(|i| i.span)
            .or_else(|| alias.as_ref().map(|a| a.span))
            .or_else(|| module_name.last().map(|segment| segment.span))
            .unwrap_or(start);

        Some(ImportDecl {
            module_name,
            alias,
            names,
            span: start.merge(end_span),
        })
    }

    fn parse_export_decl(&mut self) -> Option<ExportDecl> {
        let start = self.advance().span; // 'export'
        let mut names = Vec::new();
        loop {
            names.push(self.parse_ident_or_contextual()?);
            if !self.match_token(&TokenKind::Comma) {
                break;
            }
        }
        let end_span = names.last().map(|i| i.span).unwrap_or(start);
        Some(ExportDecl {
            names,
            span: start.merge(end_span),
        })
    }

    // --- Statements ---

    fn parse_stmt(&mut self) -> Option<Stmt> {
        // After the first overflow the file is skipped quietly: no new
        // diagnostics, but the shared progress guarantee below still applies,
        // otherwise body loops would spin on the stalled cursor forever.
        let over_limit = self.depth_aborted || self.block_depth >= MAX_BLOCK_DEPTH;
        if over_limit && !self.depth_aborted {
            self.too_deep("block");
        }
        let start = self.cursor;
        let result = if over_limit {
            None
        } else {
            self.block_depth += 1;
            let parsed = self.parse_stmt_inner();
            self.block_depth -= 1;
            parsed
        };
        // Progress guarantee: body loops spin until a terminator, so a failed
        // parse that consumed nothing would hang them forever. Skipping one
        // token only ever fires on inputs that previously hung (terminating
        // inputs always make progress), hence observable behavior there is
        // unchanged by construction.
        if result.is_none() {
            self.synchronize();
            if self.cursor == start && !self.is_at_end() {
                self.advance();
            }
        }
        result
    }

    fn parse_stmt_inner(&mut self) -> Option<Stmt> {
        match self.peek() {
            TokenKind::Let => self.parse_let_stmt(),
            TokenKind::Var => self.parse_var_stmt(),
            TokenKind::If => self.parse_if_stmt(),
            TokenKind::Match => self.parse_match_stmt(),
            TokenKind::Loop => self.parse_loop_stmt(),
            TokenKind::While => self.parse_while_stmt(),
            TokenKind::Repeat => self.parse_repeat_stmt(),
            TokenKind::Each => self.parse_each_stmt(),
            TokenKind::Break => {
                let span = self.advance().span;
                Some(Stmt::Break(span))
            }
            TokenKind::Continue => {
                let span = self.advance().span;
                Some(Stmt::Continue(span))
            }
            TokenKind::Return => self.parse_return_stmt(),
            TokenKind::Fail => self.parse_fail_stmt(),
            TokenKind::Attempt => self.parse_attempt_stmt(),
            TokenKind::Async => self.parse_async_stmt(),
            TokenKind::Await => self.parse_await_stmt(),
            TokenKind::Fn => {
                // A named `fn name(params) ... end` inside a statement position is a local
                // function declaration: canon makes its binding exist when execution reaches
                // the declaration, visible inside its own body for recursion. The name must
                // survive lowering, so it is kept instead of being collapsed into an
                // anonymous closure expression.
                self.parse_function_decl().map(Stmt::Fn)
            }
            TokenKind::SlashSlash => {
                let span = self.advance().span;
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN,
                        "'//' is integer division in Aipo, not a comment; use '#' for line comments",
                    )
                    .with_primary_span(self.source, span),
                );
                while !self.is_at_end() && !matches!(self.peek(), TokenKind::Newline) {
                    self.advance();
                }
                None
            }
            _ => self.parse_expr_or_assign_stmt(),
        }
    }

    fn parse_let_stmt(&mut self) -> Option<Stmt> {
        let start = self.advance().span; // 'let'
        let pattern = self.parse_binding_pattern()?;
        self.expect(&TokenKind::Equal, "expected '=' in let binding")?;
        let expr = self.parse_expr()?;
        let span = start.merge(expr.span());
        Some(Stmt::Let(pattern, expr, span))
    }

    fn parse_var_stmt(&mut self) -> Option<Stmt> {
        let start = self.advance().span; // 'var'
        let pattern = self.parse_binding_pattern()?;
        self.expect(&TokenKind::Equal, "expected '=' in var binding")?;
        let expr = self.parse_expr()?;
        let span = start.merge(expr.span());
        Some(Stmt::Var(pattern, expr, span))
    }

    fn parse_binding_pattern(&mut self) -> Option<BindingPattern> {
        if self.match_token(&TokenKind::Discard) {
            let span = self.tokens[self.cursor - 1].span;
            Some(BindingPattern::Discard(span))
        } else if self.match_token(&TokenKind::LBracket) {
            let start = self.tokens[self.cursor - 1].span;
            let mut list = Vec::new();
            while !self.check(&TokenKind::RBracket) && !self.is_at_end() {
                list.push(self.parse_binding_pattern()?);
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
            let end = self
                .expect(&TokenKind::RBracket, "expected ']' after binding list")?
                .span;
            Some(BindingPattern::List(list, start.merge(end)))
        } else if self.match_token(&TokenKind::LBrace) {
            let start = self.tokens[self.cursor - 1].span;
            let mut fields = Vec::new();
            while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
                fields.push(self.parse_ident()?);
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
            let end = self
                .expect(&TokenKind::RBrace, "expected '}' after binding struct")?
                .span;
            Some(BindingPattern::Struct(fields, start.merge(end)))
        } else {
            let ident = self.parse_ident()?;
            Some(BindingPattern::Ident(ident))
        }
    }

    fn parse_expr_or_assign_stmt(&mut self) -> Option<Stmt> {
        let expr = self.parse_expr()?;

        // Check for simple assignment '='
        if self.match_token(&TokenKind::Equal) {
            let value = self.parse_expr()?;
            let span = expr.span().merge(value.span());
            return Some(Stmt::Assign(expr, value, span));
        }

        // Check for compound assignments
        let compound_op = if self.match_token(&TokenKind::PlusEq) {
            Some(BinaryOp::Add)
        } else if self.match_token(&TokenKind::MinusEq) {
            Some(BinaryOp::Sub)
        } else if self.match_token(&TokenKind::StarEq) {
            Some(BinaryOp::Mul)
        } else if self.match_token(&TokenKind::SlashEq) {
            Some(BinaryOp::Div)
        } else if self.match_token(&TokenKind::SlashSlashEq) {
            Some(BinaryOp::IntDiv)
        } else if self.match_token(&TokenKind::PercentEq) {
            Some(BinaryOp::Mod)
        } else {
            None
        };

        if let Some(op) = compound_op {
            let value = self.parse_expr()?;
            let span = expr.span().merge(value.span());
            return Some(Stmt::CompoundAssign(op, expr, value, span));
        }

        Some(Stmt::Expr(expr))
    }

    fn parse_if_stmt(&mut self) -> Option<Stmt> {
        let start = self.advance().span; // 'if'
        let condition = self.parse_block_header_expr()?;

        // Inline if: `if c then stmt [else stmt]` (`then` never takes `{}`).
        if self.match_token(&TokenKind::Then) {
            if self.check(&TokenKind::Newline) || self.check(&TokenKind::LBrace) {
                let span = self
                    .peek_token()
                    .map(|t| t.span)
                    .unwrap_or_else(|| SourceSpan::empty(self.source.len()));
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN,
                        "`then` is only for inline conditionals (`if c then a else b`); remove `then` before `{`",
                    )
                    .with_primary_span(self.source, span),
                );
                return None;
            }
            let then_stmt = self.parse_stmt()?;
            let else_branch = if self.match_token(&TokenKind::Else) {
                Some(vec![self.parse_stmt()?])
            } else {
                None
            };
            let end_span = else_branch
                .as_ref()
                .and_then(|b| b.last())
                .map(stmt_span)
                .unwrap_or_else(|| stmt_span(&then_stmt));

            return Some(Stmt::If(IfStmt {
                condition,
                then_branch: vec![then_stmt],
                elif_branches: Vec::new(),
                else_branch,
                span: start.merge(end_span),
            }));
        }

        self.skip_newlines();
        self.expect(&TokenKind::LBrace, "expected '{' to open if body")?;
        self.skip_newlines();

        let mut then_branch = Vec::new();
        while !self.check(&TokenKind::RBrace)
            && !self.check(&TokenKind::Elif)
            && !self.check(&TokenKind::Else)
            && !self.is_at_end()
        {
            if let Some(s) = self.parse_stmt() {
                then_branch.push(s);
            }
            self.skip_newlines();
        }
        self.expect(&TokenKind::RBrace, "expected '}' after if body")?;
        self.skip_newlines();

        let mut elif_branches = Vec::new();
        while self.match_token(&TokenKind::Elif) {
            let elif_cond = self.parse_block_header_expr()?;
            if self.match_token(&TokenKind::Then) {
                let span = self
                    .peek_token()
                    .map(|t| t.span)
                    .unwrap_or_else(|| SourceSpan::empty(self.source.len()));
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN,
                        "`then` is only for inline conditionals (`if c then a else b`); remove `then` before `{`",
                    )
                    .with_primary_span(self.source, span),
                );
                return None;
            }
            self.skip_newlines();
            self.expect(&TokenKind::LBrace, "expected '{' to open elif body")?;
            self.skip_newlines();
            let mut elif_body = Vec::new();
            while !self.check(&TokenKind::RBrace)
                && !self.check(&TokenKind::Elif)
                && !self.check(&TokenKind::Else)
                && !self.is_at_end()
            {
                if let Some(s) = self.parse_stmt() {
                    elif_body.push(s);
                }
                self.skip_newlines();
            }
            self.expect(&TokenKind::RBrace, "expected '}' after elif body")?;
            self.skip_newlines();
            elif_branches.push((elif_cond, elif_body));
        }

        let else_branch = if self.match_token(&TokenKind::Else) {
            self.skip_newlines();
            if self.check(&TokenKind::If) {
                let span = self
                    .peek_token()
                    .map(|t| t.span)
                    .unwrap_or_else(|| SourceSpan::empty(self.source.len()));
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN,
                        "`else if` is not supported; use `elif` instead",
                    )
                    .with_primary_span(self.source, span),
                );
                return None;
            }
            self.expect(&TokenKind::LBrace, "expected '{' to open else body")?;
            self.skip_newlines();
            let mut else_body = Vec::new();
            while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
                if let Some(s) = self.parse_stmt() {
                    else_body.push(s);
                }
                self.skip_newlines();
            }
            self.expect(&TokenKind::RBrace, "expected '}' after else body")?;
            Some(else_body)
        } else {
            None
        };

        let end_span = self.tokens[self.cursor.saturating_sub(1)].span;

        Some(Stmt::If(IfStmt {
            condition,
            then_branch,
            elif_branches,
            else_branch,
            span: start.merge(end_span),
        }))
    }

    fn parse_match_stmt(&mut self) -> Option<Stmt> {
        let start = self.advance().span; // 'match'
        let target = self.parse_block_header_expr()?;
        self.skip_newlines();

        self.expect(&TokenKind::LBrace, "expected '{' to open match arms")?;
        self.skip_newlines();

        let mut when_arms = Vec::new();
        while self.match_token(&TokenKind::When) {
            let arm_start = self.tokens[self.cursor - 1].span;
            let mut patterns = Vec::new();
            loop {
                // Struct destructuring: `when { name, age }`
                if self.check(&TokenKind::LBrace) {
                    let _ = self.advance(); // consume '{'
                    self.skip_newlines();
                    let mut fields = Vec::new();
                    while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
                        fields.push(self.parse_ident()?);
                        self.skip_newlines();
                        if !self.match_token(&TokenKind::Comma) {
                            break;
                        }
                        self.skip_newlines();
                    }
                    self.expect(
                        &TokenKind::RBrace,
                        "expected '}' to close destructuring pattern",
                    )?;
                    patterns.push(MatchPattern::Destructure(fields));
                } else if self.is_variant_pattern_start() {
                    patterns.push(self.parse_variant_pattern()?);
                } else {
                    let old = self.allow_comma_is;
                    self.allow_comma_is = false;
                    let pat = self.parse_expr()?;
                    self.allow_comma_is = old;
                    patterns.push(MatchPattern::Value(pat));
                }
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }

            // Optional guard: `when pattern if guard then`
            let guard = if self.match_token(&TokenKind::If) {
                let cond = self.parse_expr()?;
                Some(cond)
            } else {
                None
            };

            let _ = self.match_token(&TokenKind::Then);
            self.skip_newlines();
            let arm_brace = self.match_token(&TokenKind::LBrace);
            self.skip_newlines();

            let mut arm_body = Vec::new();
            while !self.check(&TokenKind::RBrace)
                && !self.check(&TokenKind::When)
                && !self.check(&TokenKind::Else)
                && !self.is_at_end()
            {
                if let Some(s) = self.parse_stmt() {
                    arm_body.push(s);
                }
                self.skip_newlines();
            }
            if arm_brace {
                self.expect(&TokenKind::RBrace, "expected '}' after when arm")?;
                self.skip_newlines();
            }
            let arm_end = self.tokens[self.cursor - 1].span;
            when_arms.push(MatchArm {
                patterns,
                guard,
                body: arm_body,
                span: arm_start.merge(arm_end),
            });
        }

        let else_arm = if self.match_token(&TokenKind::Else) {
            self.skip_newlines();
            let else_brace = self.match_token(&TokenKind::LBrace);
            self.skip_newlines();
            let mut else_body = Vec::new();
            while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
                if let Some(s) = self.parse_stmt() {
                    else_body.push(s);
                }
                self.skip_newlines();
            }
            if else_brace {
                self.expect(&TokenKind::RBrace, "expected '}' after else arm")?;
                self.skip_newlines();
            }
            Some(else_body)
        } else {
            None
        };

        let end_span = self
            .expect(&TokenKind::RBrace, "expected '}' to close match statement")?
            .span;

        Some(Stmt::Match(MatchStmt {
            target,
            when_arms,
            else_arm,
            span: start.merge(end_span),
        }))
    }

    fn is_variant_pattern_start(&self) -> bool {
        if let TokenKind::Identifier(first) = self.peek()
            && first.chars().next().is_some_and(|c| c.is_uppercase())
            && self.peek_ahead(1) == &TokenKind::Dot
            && let TokenKind::Identifier(second) = self.peek_ahead(2)
        {
            return second.chars().next().is_some_and(|c| c.is_uppercase());
        }
        false
    }

    fn is_variant_struct_payload(&self) -> bool {
        let mut idx = self.cursor + 1;
        while idx < self.tokens.len() {
            match &self.tokens[idx].kind {
                TokenKind::Newline => {
                    idx += 1;
                }
                TokenKind::Identifier(_) => {
                    idx += 1;
                    while idx < self.tokens.len() && self.tokens[idx].kind == TokenKind::Newline {
                        idx += 1;
                    }
                    if idx < self.tokens.len() && self.tokens[idx].kind == TokenKind::Comma {
                        idx += 1;
                    }
                }
                TokenKind::RBrace => {
                    let mut after = idx + 1;
                    while after < self.tokens.len() && self.tokens[after].kind == TokenKind::Newline
                    {
                        after += 1;
                    }
                    if after < self.tokens.len() {
                        return matches!(
                            self.tokens[after].kind,
                            TokenKind::LBrace | TokenKind::If | TokenKind::Then | TokenKind::Comma
                        );
                    }
                    return false;
                }
                _ => return false,
            }
        }
        false
    }

    fn parse_variant_pattern(&mut self) -> Option<MatchPattern> {
        let start = self.tokens[self.cursor].span;
        let enum_name = self.parse_ident()?;
        self.expect(&TokenKind::Dot, "expected '.' in enum variant pattern")?;
        let variant_name = self.parse_ident()?;

        let (payload, end_span) = if self.match_token(&TokenKind::LParen) {
            self.skip_newlines();
            let mut ids = Vec::new();
            while !self.check(&TokenKind::RParen) && !self.is_at_end() {
                ids.push(self.parse_ident()?);
                self.skip_newlines();
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
                self.skip_newlines();
            }
            let rparen = self.expect(
                &TokenKind::RParen,
                "expected ')' to close tuple variant pattern",
            )?;
            (VariantPatternPayload::Tuple(ids), rparen.span)
        } else if self.check(&TokenKind::LBrace) && self.is_variant_struct_payload() {
            self.advance(); // consume '{'
            self.skip_newlines();
            let mut ids = Vec::new();
            while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
                ids.push(self.parse_ident()?);
                self.skip_newlines();
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
                self.skip_newlines();
            }
            let rbrace = self.expect(
                &TokenKind::RBrace,
                "expected '}' to close struct variant pattern",
            )?;
            (VariantPatternPayload::Struct(ids), rbrace.span)
        } else {
            (VariantPatternPayload::Unit, variant_name.span)
        };

        Some(MatchPattern::Variant {
            enum_name: Some(enum_name),
            variant_name,
            payload,
            span: start.merge(end_span),
        })
    }

    fn parse_loop_stmt(&mut self) -> Option<Stmt> {
        let start = self.advance().span; // 'loop'
        self.skip_newlines();

        self.expect(&TokenKind::LBrace, "expected '{' to open loop body")?;
        self.skip_newlines();

        let mut body = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if let Some(s) = self.parse_stmt() {
                body.push(s);
            }
            self.skip_newlines();
        }
        let end_span = self
            .expect(&TokenKind::RBrace, "expected '}' to close loop")?
            .span;
        Some(Stmt::Loop(body, start.merge(end_span)))
    }

    fn parse_while_stmt(&mut self) -> Option<Stmt> {
        let start = self.advance().span; // 'while'
        let condition = self.parse_block_header_expr()?;
        self.skip_newlines();

        self.expect(&TokenKind::LBrace, "expected '{' to open while body")?;
        self.skip_newlines();

        let mut body = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if let Some(s) = self.parse_stmt() {
                body.push(s);
            }
            self.skip_newlines();
        }
        let end_span = self
            .expect(&TokenKind::RBrace, "expected '}' to close while")?
            .span;
        Some(Stmt::While(condition, body, start.merge(end_span)))
    }

    fn parse_repeat_stmt(&mut self) -> Option<Stmt> {
        let start = self.advance().span; // 'repeat'
        let count = self.parse_block_header_expr()?;
        let index_var = if self.match_token(&TokenKind::As) {
            Some(self.parse_ident()?)
        } else {
            None
        };
        self.skip_newlines();

        self.expect(&TokenKind::LBrace, "expected '{' to open repeat body")?;
        self.skip_newlines();

        let mut body = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if let Some(s) = self.parse_stmt() {
                body.push(s);
            }
            self.skip_newlines();
        }
        let end_span = self
            .expect(&TokenKind::RBrace, "expected '}' to close repeat")?
            .span;
        Some(Stmt::Repeat(count, index_var, body, start.merge(end_span)))
    }

    fn parse_each_stmt(&mut self) -> Option<Stmt> {
        let start = self.advance().span; // 'each'
        let mut bindings = Vec::new();
        loop {
            bindings.push(self.parse_ident()?);
            if !self.match_token(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::In, "expected 'in' in each loop")?;
        let iterable = self.parse_block_header_expr()?;
        self.skip_newlines();

        self.expect(&TokenKind::LBrace, "expected '{' to open each body")?;
        self.skip_newlines();

        let mut body = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if let Some(s) = self.parse_stmt() {
                body.push(s);
            }
            self.skip_newlines();
        }
        let end_span = self
            .expect(&TokenKind::RBrace, "expected '}' to close each loop")?
            .span;
        Some(Stmt::Each(bindings, iterable, body, start.merge(end_span)))
    }

    fn parse_return_stmt(&mut self) -> Option<Stmt> {
        let start = self.advance().span; // 'return'
        let value = if matches!(self.peek(), TokenKind::Newline | TokenKind::Eof) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        let span = if let Some(val) = &value {
            start.merge(val.span())
        } else {
            start
        };
        Some(Stmt::Return(value, span))
    }

    /// Parses the expression between `[` and `]`, normalizing omitted slice bounds.
    ///
    /// Canon defines slicing as half-open and explicitly allows omitted limits. The runtime
    /// already clamps both ends of a slice into `0..=len`, so an omitted bound is exactly
    /// equivalent to the corresponding extreme value and needs no new runtime kind: an
    /// omitted start means index `0`, and an omitted end means "up to the last element",
    /// spelled with the largest safe Aipo integer.
    fn parse_index_expr(&mut self) -> Option<Expr> {
        if self.check(&TokenKind::DotDot) {
            let dot = self.advance().span;
            let start = Expr::Literal(Literal::Int("0".to_string()), dot);
            let end = if self.check(&TokenKind::RBracket) {
                Expr::Literal(Literal::Int(SAFE_MAX_INT.to_string()), dot)
            } else {
                self.parse_pratt_expr(Precedence::Range)?
            };
            let span = dot.merge(end.span());
            return Some(Expr::Binary(
                BinaryOp::Range,
                Box::new(start),
                Box::new(end),
                span,
            ));
        }

        // Stop *before* `..` so the slice bound is not swallowed by the range operator.
        let start = self.parse_pratt_expr(Precedence::Range)?;
        if self.check(&TokenKind::DotDot) {
            let dot = self.advance().span;
            let end = if self.check(&TokenKind::RBracket) {
                Expr::Literal(Literal::Int(SAFE_MAX_INT.to_string()), dot)
            } else {
                self.parse_pratt_expr(Precedence::Range)?
            };
            let span = start.span().merge(end.span());
            return Some(Expr::Binary(
                BinaryOp::Range,
                Box::new(start),
                Box::new(end),
                span,
            ));
        }
        // Nothing to slice: finish any looser operators (`xs[a == b]`).
        self.continue_pratt(start, Precedence::Lowest)
    }

    fn parse_fail_stmt(&mut self) -> Option<Stmt> {
        let start = self.advance().span; // 'fail'
        let value = self.parse_expr()?;
        let span = start.merge(value.span());
        Some(Stmt::Fail(value, span))
    }

    fn parse_attempt_stmt(&mut self) -> Option<Stmt> {
        let start = self.advance().span; // 'attempt'
        self.skip_newlines();

        self.expect(&TokenKind::LBrace, "expected '{' to open attempt body")?;
        self.skip_newlines();

        let mut body = Vec::new();
        while !self.check(&TokenKind::RBrace)
            && !self.check(&TokenKind::Failed)
            && !self.is_at_end()
        {
            if let Some(s) = self.parse_stmt() {
                body.push(s);
            }
            self.skip_newlines();
        }
        self.expect(&TokenKind::RBrace, "expected '}' after attempt body")?;
        self.skip_newlines();

        self.expect(&TokenKind::Failed, "expected 'failed' in attempt statement")?;
        let err_binding = if matches!(self.peek(), TokenKind::Newline | TokenKind::LBrace)
            || self.match_token(&TokenKind::Discard)
        {
            None
        } else {
            Some(self.parse_ident()?)
        };
        self.skip_newlines();

        self.expect(&TokenKind::LBrace, "expected '{' to open failed body")?;
        self.skip_newlines();

        let mut failed_body = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if let Some(s) = self.parse_stmt() {
                failed_body.push(s);
            }
            self.skip_newlines();
        }
        let end_span = self
            .expect(
                &TokenKind::RBrace,
                "expected '}' to close attempt statement",
            )?
            .span;

        Some(Stmt::Attempt(AttemptStmt {
            body,
            err_binding,
            failed_body,
            span: start.merge(end_span),
        }))
    }

    fn parse_function_decl(&mut self) -> Option<FunctionDecl> {
        let start = self.advance().span; // 'fn'
        self.parse_function_decl_rest(start, false)
    }

    /// Parses `async fn ...`: the modifier is contextual (only meaningful before
    /// `fn`); a bare `async` elsewhere is a dedicated diagnostic, never a name.
    fn parse_async_function_decl(&mut self) -> Option<FunctionDecl> {
        let start = self.advance().span; // 'async'
        if !self.check(&TokenKind::Fn) {
            let span = self
                .peek_token()
                .map(|t| t.span)
                .unwrap_or_else(|| SourceSpan::empty(self.source.len()));
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN,
                    "expected 'fn' after 'async'",
                )
                .with_primary_span(self.source, span),
            );
            return None;
        }
        self.advance(); // 'fn'
        self.parse_function_decl_rest(start, true)
    }

    /// Shared body of `fn` / `async fn` declarations.
    fn parse_function_decl_rest(
        &mut self,
        start: SourceSpan,
        is_async: bool,
    ) -> Option<FunctionDecl> {
        let name = self.parse_ident_or_contextual()?;
        self.expect(
            &TokenKind::LParen,
            "expected '(' in function parameter list",
        )?;
        let params = self.parse_params()?;
        self.expect(&TokenKind::RParen, "expected ')' after parameters")?;

        let return_type = self.parse_optional_return_type();
        self.skip_newlines();

        self.expect(&TokenKind::LBrace, "expected '{' to open function body")?;
        self.skip_newlines();

        let mut body = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if let Some(s) = self.parse_stmt() {
                body.push(s);
            }
            self.skip_newlines();
        }
        let end_span = self
            .expect(
                &TokenKind::RBrace,
                "expected '}' to close function declaration",
            )?
            .span;

        Some(FunctionDecl {
            directives: self.take_directives(),
            name,
            is_async,
            params,
            return_type,
            body,
            span: start.merge(end_span),
        })
    }

    fn parse_function_signature(&mut self) -> Option<FunctionDecl> {
        let (start, is_async) = if self.match_token(&TokenKind::Async) {
            let async_span = self.tokens[self.cursor - 1].span;
            let _ = self.match_token(&TokenKind::Fn);
            (async_span, true)
        } else if self.match_token(&TokenKind::Fn) {
            let fn_span = self.tokens[self.cursor - 1].span;
            (fn_span, false)
        } else {
            let start = self.tokens[self.cursor].span;
            (start, false)
        };
        let name = self.parse_ident_or_contextual()?;
        self.expect(&TokenKind::LParen, "expected '(' in method signature")?;
        let params = self.parse_params()?;
        self.expect(&TokenKind::RParen, "expected ')' after parameters")?;
        let return_type = self.parse_optional_return_type();
        let end_span = return_type.as_ref().map(|r| r.span).unwrap_or(name.span);

        Some(FunctionDecl {
            directives: Vec::new(),
            name,
            is_async,
            params,
            return_type,
            body: Vec::new(),
            span: start.merge(end_span),
        })
    }

    /// Parses `async ...` in statement position: only `async fn` is meaningful.
    fn parse_async_stmt(&mut self) -> Option<Stmt> {
        self.parse_async_function_decl().map(Stmt::Fn)
    }

    /// Parses `async ...` as a top-level item: only `async fn` is meaningful.
    fn parse_async_item(&mut self) -> Option<Item> {
        self.parse_async_function_decl().map(Item::Fn)
    }

    /// Parses `await ...` in statement position: `await do ... end` becomes an
    /// await-block, anything else an awaited expression statement.
    fn parse_await_stmt(&mut self) -> Option<Stmt> {
        let start = self.advance().span; // 'await'
        if self.check(&TokenKind::Do) {
            return self.parse_await_do_block(start);
        }
        let inner = self.parse_expr()?;
        let span = start.merge(inner.span());
        Some(Stmt::Expr(Expr::Await(Box::new(inner), span)))
    }

    /// Parses `await expr` in value position. Placement is validated later:
    /// explicit `await` belongs to statements, initializers and returns, never
    /// to arbitrary subexpressions (`AIPO_SEM_AWAIT_IN_SUBEXPRESSION`).
    fn parse_await_expr(&mut self) -> Option<Expr> {
        let start = self.advance().span; // 'await'
        let inner = self.parse_pratt_expr(Precedence::Prefix)?;
        let span = start.merge(inner.span());
        Some(Expr::Await(Box::new(inner), span))
    }

    /// Parses `await do ... end`: sugar for sequential awaits, kept as a block
    /// so later stages can apply the known-Task rule (canon: sugar, never a
    /// second async machine).
    fn parse_await_do_block(&mut self, start: SourceSpan) -> Option<Stmt> {
        self.advance(); // 'do'
        self.skip_newlines();

        self.expect(&TokenKind::LBrace, "expected '{' to open await do block")?;
        self.skip_newlines();

        let mut body = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if let Some(stmt) = self.parse_stmt() {
                body.push(stmt);
            }
            self.skip_newlines();
        }
        let end_span = self
            .expect(&TokenKind::RBrace, "expected '}' to close await do block")?
            .span;
        Some(Stmt::AwaitDo(body, start.merge(end_span)))
    }

    /// Parses `async fn(...) ... end` as an anonymous function value.
    fn parse_async_fn_expr(&mut self) -> Option<Expr> {
        let async_start = self.advance().span; // 'async'
        if !self.check(&TokenKind::Fn) {
            let span = self
                .peek_token()
                .map(|t| t.span)
                .unwrap_or_else(|| SourceSpan::empty(self.source.len()));
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN,
                    "expected 'fn' after 'async'",
                )
                .with_primary_span(self.source, span),
            );
            return None;
        }
        self.advance(); // 'fn'
        // The expression spans the whole `async fn ... end` form, so it starts at `async`.
        self.parse_fn_expr_rest(async_start, true)
    }

    /// Rejects parametric `Name[Args]` contracts: the language has no generics
    /// yet (see ADP-006), so `Task[T]` and friends are a dedicated diagnostic,
    /// never silent acceptance. Call after parsing a contract type name.
    fn check_no_parametric_contract(&mut self) {
        if !self.check(&TokenKind::LBracket) {
            return;
        }
        let span = self
            .peek_token()
            .map(|t| t.span)
            .unwrap_or_else(|| SourceSpan::empty(self.source.len()));
        self.diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::AIPO_SEM_PARAMETRIC_CONTRACT,
                "parametric contracts like `Task[T]` are not in V1 (see ADP-006); write the bare contract name",
            )
            .with_primary_span(self.source, span),
        );
        // Skip the bracketed section so recovery continues after it.
        let mut depth = 0usize;
        while !self.is_at_end() {
            match self.peek() {
                TokenKind::LBracket => {
                    depth += 1;
                    self.advance();
                }
                TokenKind::RBracket => {
                    self.advance();
                    // The bracket that opened the skipped section closes it, so
                    // recovery stops before the caller's own `)` / `]`.
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                _ => {
                    self.advance();
                }
            }
        }
    }

    fn parse_params(&mut self) -> Option<Vec<Param>> {
        let mut params = Vec::new();
        self.skip_newlines();
        while !self.check(&TokenKind::RParen) && !self.is_at_end() {
            let is_var = self.match_token(&TokenKind::Var);
            let (param_name, is_mut) = if is_var {
                if self.match_token(&TokenKind::SelfVal) {
                    let span = self.tokens[self.cursor - 1].span;
                    (Ident::new("self".into(), span), true)
                } else {
                    let id = self.parse_ident()?;
                    (id, true)
                }
            } else if self.match_token(&TokenKind::SelfVal) {
                let span = self.tokens[self.cursor - 1].span;
                (Ident::new("self".into(), span), false)
            } else if self.match_token(&TokenKind::Discard) {
                let span = self.tokens[self.cursor - 1].span;
                (Ident::new("_".into(), span), false)
            } else {
                let id = self.parse_ident()?;
                (id, false)
            };

            let type_annotation = if self.match_token(&TokenKind::Colon) {
                Some(self.parse_type_annotation()?)
            } else {
                None
            };

            let default = if self.match_token(&TokenKind::Equal) {
                Some(self.parse_expr()?)
            } else {
                None
            };

            let span = param_name.span;
            params.push(Param {
                name: param_name,
                is_mutable: is_mut,
                type_annotation,
                default,
                span,
            });

            self.skip_newlines();
            if !self.match_token(&TokenKind::Comma) {
                break;
            }
            self.skip_newlines();
        }
        Some(params)
    }

    fn parse_optional_return_type(&mut self) -> Option<TypeAnnotation> {
        // Look for `-> Type[?]`
        if self.match_token(&TokenKind::Minus) && self.match_token(&TokenKind::Greater) {
            self.parse_type_annotation()
        } else {
            None
        }
    }

    // --- Pratt Expression Parsing ---

    fn parse_expr(&mut self) -> Option<Expr> {
        let left = self.parse_pratt_expr(Precedence::Lowest)?;
        if self.allow_comma_is && self.check(&TokenKind::Comma) && self.has_is_after_commas() {
            let multiple = self.parse_multiple_is(left)?;
            self.continue_pratt(multiple, Precedence::Lowest)
        } else {
            Some(left)
        }
    }

    /// Parses a block header expression (`if`/`elif` condition, `while` condition,
    /// `repeat` count, `each` iterable, `match` target).
    ///
    /// A trailing `{` with a gap in the source is the body opener, never struct
    /// construction, so `parse_primary` leaves it for the caller when this flag
    /// is set (`board {` stays `board` + body; `Board{` with no gap stays
    /// construction for the inner case).
    fn parse_block_header_expr(&mut self) -> Option<Expr> {
        let prev = std::mem::replace(&mut self.in_block_header, true);
        let res = self.parse_expr();
        self.in_block_header = prev;
        res
    }

    /// Checks whether comma-separated subjects are followed by `is` at current delimiter depth.
    fn has_is_after_commas(&self) -> bool {
        let mut i = self.cursor;
        let mut paren_depth: usize = 0;
        let mut bracket_depth: usize = 0;
        let mut brace_depth: usize = 0;
        while i < self.tokens.len() {
            match &self.tokens[i].kind {
                TokenKind::LParen => paren_depth += 1,
                TokenKind::RParen => {
                    if paren_depth == 0 {
                        return false;
                    }
                    paren_depth -= 1;
                }
                TokenKind::LBracket => bracket_depth += 1,
                TokenKind::RBracket => {
                    if bracket_depth == 0 {
                        return false;
                    }
                    bracket_depth -= 1;
                }
                TokenKind::LBrace => brace_depth += 1,
                TokenKind::RBrace => {
                    if brace_depth == 0 {
                        return false;
                    }
                    brace_depth -= 1;
                }
                TokenKind::Is if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                    return true;
                }
                TokenKind::Newline
                | TokenKind::Eof
                | TokenKind::Then
                | TokenKind::Do
                | TokenKind::Else
                | TokenKind::When
                | TokenKind::Equal
                | TokenKind::Colon
                    if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 =>
                {
                    return false;
                }
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// Parses `a, b, c is Type[?]` syntactic sugar into `a is Type and b is Type and c is Type`.
    fn parse_multiple_is(&mut self, first: Expr) -> Option<Expr> {
        let mut subjects = vec![first];
        while self.match_token(&TokenKind::Comma) {
            let next_subject = self.parse_pratt_expr(Precedence::Comparison)?;
            subjects.push(next_subject);
            if self.check(&TokenKind::Is) {
                break;
            }
        }
        let is_tok = self.expect(&TokenKind::Is, "expected 'is' after subject list")?;
        if self.check(&TokenKind::Not) {
            let not_span = self.advance().span;
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN,
                    "'is not' is not supported; use canonical negation 'not value is Type' instead",
                )
                .with_primary_span(self.source, is_tok.span.merge(not_span)),
            );
            return None;
        }
        let was_in_is_type = self.in_is_type;
        self.in_is_type = true;
        let type_expr = self.parse_pratt_expr(Precedence::Comparison);
        self.in_is_type = was_in_is_type;
        let type_expr = type_expr?;
        let is_nullable = self.match_token(&TokenKind::Question);
        let comp_op = if is_nullable {
            BinaryOp::IsNullable
        } else {
            BinaryOp::Is
        };
        let end_span = if is_nullable {
            self.tokens[self.cursor - 1].span
        } else {
            type_expr.span()
        };

        let mut result = None;
        for s in subjects {
            let s_span = s.span();
            let full_span = s_span.merge(end_span);
            let check = Expr::Binary(comp_op, Box::new(s), Box::new(type_expr.clone()), full_span);
            result = match result {
                None => Some(check),
                Some(prev) => {
                    let prev_span = prev.span();
                    let combo_span = prev_span.merge(check.span());
                    Some(Expr::Binary(
                        BinaryOp::And,
                        Box::new(prev),
                        Box::new(check),
                        combo_span,
                    ))
                }
            };
        }
        result
    }

    fn parse_pratt_expr(&mut self, precedence: Precedence) -> Option<Expr> {
        let over_limit = self.depth_aborted || self.expr_depth >= MAX_EXPR_DEPTH;
        if over_limit && !self.depth_aborted {
            self.too_deep("expression");
            return None;
        }
        if over_limit {
            return None;
        }
        self.expr_depth += 1;
        let result = self.parse_pratt_expr_inner(precedence);
        self.expr_depth -= 1;
        result
    }

    fn parse_pratt_expr_inner(&mut self, precedence: Precedence) -> Option<Expr> {
        let left = self.parse_prefix_expr()?;
        self.continue_pratt(left, precedence)
    }

    /// Keeps consuming infix operators above `precedence`, continuing from `left`.
    fn continue_pratt(&mut self, mut left: Expr, precedence: Precedence) -> Option<Expr> {
        while !self.is_at_end() && precedence < self.peek_precedence() {
            left = self.parse_infix_expr(left)?;
        }

        Some(left)
    }

    fn parse_prefix_expr(&mut self) -> Option<Expr> {
        match self.peek().clone() {
            TokenKind::IntLiteral(digits) => {
                let span = self.advance().span;
                Some(Expr::Literal(Literal::Int(digits), span))
            }
            TokenKind::FloatLiteral(digits) => {
                let span = self.advance().span;
                Some(Expr::Literal(Literal::Float(digits), span))
            }
            TokenKind::True => {
                let span = self.advance().span;
                Some(Expr::Literal(Literal::Bool(true), span))
            }
            TokenKind::False => {
                let span = self.advance().span;
                Some(Expr::Literal(Literal::Bool(false), span))
            }
            TokenKind::None => {
                let span = self.advance().span;
                Some(Expr::Literal(Literal::None, span))
            }
            TokenKind::StringLiteral {
                content,
                prefix,
                is_multiline: _,
            } => {
                let span = self.advance().span;
                let ast_prefix = match prefix {
                    LexerStringPrefix::Normal => StringPrefix::Normal,
                    LexerStringPrefix::Format => StringPrefix::Format,
                    LexerStringPrefix::Raw => StringPrefix::Raw,
                    LexerStringPrefix::FormatRaw => StringPrefix::FormatRaw,
                };

                // Format strings are desugared: `f"a{x}b"` becomes `"a" + String(x) + "b"`,
                // so interpolation and the explicit `String(value)` conversion share one
                // definition of textual rendering.
                if matches!(ast_prefix, StringPrefix::Format | StringPrefix::FormatRaw) {
                    return self.parse_format_string(span, ast_prefix);
                }

                Some(Expr::Literal(Literal::String(content, ast_prefix), span))
            }
            TokenKind::SelfVal => {
                let span = self.advance().span;
                Some(Expr::Identifier(Ident::new("self".into(), span)))
            }
            TokenKind::Discard => {
                let span = self.advance().span;
                if self.check(&TokenKind::FatArrow) {
                    return self.parse_short_lambda_discard(span);
                }
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN,
                        "unexpected token 'Discard' in expression",
                    )
                    .with_primary_span(self.source, span),
                );
                None
            }
            TokenKind::Identifier(name) => {
                let span = self.advance().span;
                let ident = Ident::new(name, span);

                // Check for single-parameter short lambda `x => expr`
                if self.check(&TokenKind::FatArrow) {
                    return self.parse_short_lambda_single(ident);
                }

                // Check for struct construction `Type{...}`
                if self.check(&TokenKind::LBrace) {
                    // A type operand of `is` is never construction: `if v is Int {`
                    // opens the body even without a gap before the brace.
                    if self.in_is_type {
                        return Some(Expr::Identifier(ident));
                    }
                    // In a block header (`if flag {`, `each x in xs {`, `match x {`),
                    // a `{` separated by whitespace is the body opener, not
                    // construction. `Board{` (no gap) stays construction so an
                    // inner `Point{x = 1}` in the header still parses.
                    if self.in_block_header
                        && let Some(brace_tok) = self.peek_token()
                    {
                        let gap = self
                            .source
                            .text()
                            .get(span.end..brace_tok.span.start)
                            .unwrap_or("");
                        if !gap.is_empty() {
                            return Some(Expr::Identifier(ident));
                        }
                    }
                    if ident.name.chars().next().is_some_and(|c| c.is_uppercase()) {
                        return self.parse_construct_expr(ident);
                    }
                    let mut chars = ident.name.chars();
                    let suggestion: String = match chars.next() {
                        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                        None => ident.name.clone(),
                    };
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN,
                            format!(
                                "struct names start with an uppercase letter: did you mean `{suggestion}`?"
                            ),
                        )
                        .with_primary_span(self.source, span),
                    );
                    return None;
                }

                Some(Expr::Identifier(ident))
            }
            TokenKind::LParen => {
                if self.is_lambda_ahead() {
                    return self.parse_short_lambda_tuple();
                }
                self.parse_paren_expr()
            }
            TokenKind::LBracket => self.parse_list_expr(),
            TokenKind::LBrace => self.parse_dict_expr(),
            TokenKind::Minus => {
                let span = self.advance().span;
                let operand = self.parse_pratt_expr(Precedence::Prefix)?;
                let full_span = span.merge(operand.span());
                Some(Expr::Unary(UnaryOp::Neg, Box::new(operand), full_span))
            }
            TokenKind::Plus => {
                let span = self.advance().span;
                let operand = self.parse_pratt_expr(Precedence::Prefix)?;
                let full_span = span.merge(operand.span());
                Some(Expr::Unary(UnaryOp::Pos, Box::new(operand), full_span))
            }
            TokenKind::Not => {
                let span = self.advance().span;
                let operand = self.parse_pratt_expr(Precedence::Not)?;
                let full_span = span.merge(operand.span());
                Some(Expr::Unary(UnaryOp::Not, Box::new(operand), full_span))
            }
            TokenKind::Fn => self.parse_fn_expr(),
            TokenKind::Async => self.parse_async_fn_expr(),
            TokenKind::Await => self.parse_await_expr(),
            TokenKind::Fail => {
                // Canon: `fail("message")` creates a recoverable Failure and `fail(err)`
                // repropagates one. The form also terminates a path inside an expression
                // (`return fail(...)`), so it must parse wherever a value is expected.
                let start = self.advance().span;
                let value = if self.check(&TokenKind::LParen) {
                    self.advance();
                    let inner = self.parse_expr()?;
                    self.expect(&TokenKind::RParen, "expected ')' after 'fail' argument")?;
                    inner
                } else {
                    // Bare form `fail err` is the statement spelling and stays accepted.
                    self.parse_expr()?
                };
                let span = start.merge(value.span());
                Some(Expr::Call(CallExpr {
                    callee: Box::new(Expr::Identifier(Ident::new("fail".into(), start))),
                    args: vec![CallArg {
                        name: None,
                        value,
                        span,
                    }],
                    trailing_block: None,
                    span,
                }))
            }
            TokenKind::If => {
                // Inline conditional value form: `if c then a else b`
                let start = self.advance().span;
                let condition = self.parse_expr()?;
                self.expect(
                    &TokenKind::Then,
                    "expected 'then' in conditional value expression",
                )?;
                let then_expr = self.parse_expr()?;
                self.expect(
                    &TokenKind::Else,
                    "expected 'else' in conditional value expression",
                )?;
                let else_expr = self.parse_expr()?;
                let full_span = start.merge(else_expr.span());
                Some(Expr::If(
                    Box::new(condition),
                    Box::new(then_expr),
                    Box::new(else_expr),
                    full_span,
                ))
            }
            TokenKind::Newline | TokenKind::Eof => {
                let span = self
                    .peek_token()
                    .map(|t| t.span)
                    .unwrap_or_else(|| SourceSpan::empty(self.source.len()));
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN,
                        "expected expression, found newline or end of input",
                    )
                    .with_primary_span(self.source, span),
                );
                None
            }
            TokenKind::SlashSlash => {
                let tok = self.advance();
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN,
                        "'//' is integer division in Aipo, not a comment; use '#' for line comments",
                    )
                    .with_primary_span(self.source, tok.span),
                );
                None
            }
            _ => {
                let tok = self.advance();
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN,
                        format!("unexpected token '{:?}' in expression", tok.kind),
                    )
                    .with_primary_span(self.source, tok.span),
                );
                None
            }
        }
    }

    #[inline(never)]
    fn parse_short_lambda_single(&mut self, ident: Ident) -> Option<Expr> {
        let span = ident.span;
        self.advance(); // consume '=>'
        let body = self.parse_pratt_expr(Precedence::Lowest)?;
        let full_span = span.merge(body.span());
        let param = Param {
            name: ident,
            is_mutable: false,
            type_annotation: None,
            default: None,
            span,
        };
        let ret_stmt = Stmt::Return(Some(body.clone()), body.span());
        Some(Expr::Fn(FunctionExpr {
            is_async: false,
            params: vec![param],
            return_type: None,
            body: vec![ret_stmt],
            span: full_span,
        }))
    }

    #[inline(never)]
    fn parse_short_lambda_discard(&mut self, span: SourceSpan) -> Option<Expr> {
        self.advance(); // consume '=>'
        let body = self.parse_pratt_expr(Precedence::Lowest)?;
        let full_span = span.merge(body.span());
        let param = Param {
            name: Ident::new("_".into(), span),
            is_mutable: false,
            type_annotation: None,
            default: None,
            span,
        };
        let ret_stmt = Stmt::Return(Some(body.clone()), body.span());
        Some(Expr::Fn(FunctionExpr {
            is_async: false,
            params: vec![param],
            return_type: None,
            body: vec![ret_stmt],
            span: full_span,
        }))
    }

    #[inline(never)]
    fn parse_short_lambda_tuple(&mut self) -> Option<Expr> {
        let start = self.advance().span; // consume '('
        let params = self.parse_params()?;
        self.expect(&TokenKind::RParen, "expected ')' after lambda parameters")?;
        self.expect(
            &TokenKind::FatArrow,
            "expected '=>' after lambda parameters",
        )?;
        let body = self.parse_pratt_expr(Precedence::Lowest)?;
        let full_span = start.merge(body.span());
        let ret_stmt = Stmt::Return(Some(body.clone()), body.span());
        Some(Expr::Fn(FunctionExpr {
            is_async: false,
            params,
            return_type: None,
            body: vec![ret_stmt],
            span: full_span,
        }))
    }

    #[inline(never)]
    fn parse_paren_expr(&mut self) -> Option<Expr> {
        self.advance();
        self.skip_newlines();
        let old = self.allow_comma_is;
        self.allow_comma_is = true;
        let inner = self.parse_expr()?;
        self.allow_comma_is = old;
        self.skip_newlines();
        self.expect(
            &TokenKind::RParen,
            "expected ')' after parenthesized expression",
        )?;
        Some(inner)
    }

    fn parse_infix_expr(&mut self, left: Expr) -> Option<Expr> {
        let op_tok = self.advance();
        let op_span = op_tok.span;

        match op_tok.kind {
            TokenKind::Plus => self.binary_expr(left, BinaryOp::Add, Precedence::Sum, op_span),
            TokenKind::Minus => self.binary_expr(left, BinaryOp::Sub, Precedence::Sum, op_span),
            TokenKind::Star => self.binary_expr(left, BinaryOp::Mul, Precedence::Product, op_span),
            TokenKind::Slash => self.binary_expr(left, BinaryOp::Div, Precedence::Product, op_span),
            TokenKind::SlashSlash => {
                self.binary_expr(left, BinaryOp::IntDiv, Precedence::Product, op_span)
            }
            TokenKind::Percent => {
                self.binary_expr(left, BinaryOp::Mod, Precedence::Product, op_span)
            }
            TokenKind::EqualEqual => {
                self.binary_expr(left, BinaryOp::Equal, Precedence::Comparison, op_span)
            }
            TokenKind::BangEqual => {
                self.binary_expr(left, BinaryOp::NotEqual, Precedence::Comparison, op_span)
            }
            TokenKind::Less => {
                self.binary_expr(left, BinaryOp::Less, Precedence::Comparison, op_span)
            }
            TokenKind::LessEqual => {
                self.binary_expr(left, BinaryOp::LessEqual, Precedence::Comparison, op_span)
            }
            TokenKind::Greater => {
                self.binary_expr(left, BinaryOp::Greater, Precedence::Comparison, op_span)
            }
            TokenKind::GreaterEqual => self.binary_expr(
                left,
                BinaryOp::GreaterEqual,
                Precedence::Comparison,
                op_span,
            ),
            TokenKind::Is => {
                if self.check(&TokenKind::Not) {
                    let not_span = self.advance().span;
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN,
                            "'is not' is not supported; use canonical negation 'not value is Type' instead",
                        )
                        .with_primary_span(self.source, op_span.merge(not_span)),
                    );
                    return None;
                }
                let was_in_is_type = self.in_is_type;
                self.in_is_type = true;
                let right = self.parse_pratt_expr(Precedence::Comparison);
                self.in_is_type = was_in_is_type;
                let right = right?;
                let is_nullable = self.match_token(&TokenKind::Question);
                let op = if is_nullable {
                    BinaryOp::IsNullable
                } else {
                    BinaryOp::Is
                };
                let end_span = if is_nullable {
                    self.tokens[self.cursor - 1].span
                } else {
                    right.span()
                };
                let full_span = left.span().merge(end_span);
                Some(Expr::Binary(op, Box::new(left), Box::new(right), full_span))
            }
            TokenKind::And => {
                if is_comparison_token(self.peek())
                    && let Some(subject) = extract_comparison_subject(&left)
                {
                    let op_tok = self.advance();
                    if op_tok.kind == TokenKind::Is {
                        if self.check(&TokenKind::Not) {
                            let not_span = self.advance().span;
                            self.diagnostics.push(
                                    Diagnostic::error(
                                        DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN,
                                        "'is not' is not supported; use canonical negation 'not value is Type' instead",
                                    )
                                    .with_primary_span(self.source, op_tok.span.merge(not_span)),
                                );
                            return None;
                        }
                        let was_in_is_type = self.in_is_type;
                        self.in_is_type = true;
                        let right = self.parse_pratt_expr(Precedence::Comparison);
                        self.in_is_type = was_in_is_type;
                        let right = right?;
                        let is_nullable = self.match_token(&TokenKind::Question);
                        let comp_op = if is_nullable {
                            BinaryOp::IsNullable
                        } else {
                            BinaryOp::Is
                        };
                        let end_span = if is_nullable {
                            self.tokens[self.cursor - 1].span
                        } else {
                            right.span()
                        };
                        let comp_span = subject.span().merge(end_span);
                        let continuation =
                            Expr::Binary(comp_op, Box::new(subject), Box::new(right), comp_span);
                        let full_span = left.span().merge(continuation.span());
                        return Some(Expr::Binary(
                            BinaryOp::And,
                            Box::new(left),
                            Box::new(continuation),
                            full_span,
                        ));
                    }
                    let comp_op = match op_tok.kind {
                        TokenKind::EqualEqual => BinaryOp::Equal,
                        TokenKind::BangEqual => BinaryOp::NotEqual,
                        TokenKind::Less => BinaryOp::Less,
                        TokenKind::LessEqual => BinaryOp::LessEqual,
                        TokenKind::Greater => BinaryOp::Greater,
                        TokenKind::GreaterEqual => BinaryOp::GreaterEqual,
                        _ => unreachable!(),
                    };
                    let right = self.parse_pratt_expr(Precedence::Comparison)?;
                    let comp_span = subject.span().merge(right.span());
                    let continuation =
                        Expr::Binary(comp_op, Box::new(subject), Box::new(right), comp_span);
                    let full_span = left.span().merge(continuation.span());
                    return Some(Expr::Binary(
                        BinaryOp::And,
                        Box::new(left),
                        Box::new(continuation),
                        full_span,
                    ));
                }
                self.binary_expr(left, BinaryOp::And, Precedence::And, op_span)
            }
            TokenKind::Or => self.binary_expr(left, BinaryOp::Or, Precedence::Or, op_span),
            TokenKind::OrElse => {
                self.binary_expr(left, BinaryOp::OrElse, Precedence::OrElse, op_span)
            }
            TokenKind::Pipeline => {
                self.binary_expr(left, BinaryOp::Pipeline, Precedence::Pipeline, op_span)
            }
            TokenKind::DotDot => {
                self.binary_expr(left, BinaryOp::Range, Precedence::Range, op_span)
            }

            // Call postfix: `callee(...)`
            TokenKind::LParen => {
                self.skip_newlines();
                let mut args = Vec::new();
                while !self.check(&TokenKind::RParen) && !self.is_at_end() {
                    // Check for named argument `name = value`
                    let is_named = matches!(self.peek(), TokenKind::Identifier(_))
                        && self.peek_ahead(1) == &TokenKind::Equal;
                    let (arg_name, value) = if is_named {
                        let id = self.parse_ident()?;
                        self.advance(); // consume '='
                        self.skip_newlines();
                        let old = self.allow_comma_is;
                        self.allow_comma_is = false;
                        let val = self.parse_expr()?;
                        self.allow_comma_is = old;
                        (Some(id), val)
                    } else {
                        let old = self.allow_comma_is;
                        self.allow_comma_is = false;
                        let val = self.parse_expr()?;
                        self.allow_comma_is = old;
                        (None, val)
                    };
                    let span = arg_name
                        .as_ref()
                        .map(|n| n.span.merge(value.span()))
                        .unwrap_or_else(|| value.span());
                    args.push(CallArg {
                        name: arg_name,
                        value,
                        span,
                    });
                    self.skip_newlines();
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                    self.skip_newlines();
                }
                self.skip_newlines();
                let end = self
                    .expect(&TokenKind::RParen, "expected ')' after call arguments")?
                    .span;

                // Check for trailing block `do ... { ... }`
                let trailing_block = if self.match_token(&TokenKind::Do) {
                    self.parse_trailing_block()
                } else {
                    None
                };

                let span = if let Some(b) = &trailing_block {
                    left.span().merge(b.span)
                } else {
                    left.span().merge(end)
                };

                Some(Expr::Call(CallExpr {
                    callee: Box::new(left),
                    args,
                    trailing_block,
                    span,
                }))
            }

            // Dot call or field access: `target.field`
            TokenKind::Dot => {
                let field = self.parse_member_name()?;
                let span = left.span().merge(field.span);
                if self.check(&TokenKind::LBrace) {
                    // Same body-brace rule as bare identifiers: `match task.done {`
                    // leaves the gap-separated `{` for the caller.
                    if self.in_block_header
                        && let Some(brace_tok) = self.peek_token()
                    {
                        let gap = self
                            .source
                            .text()
                            .get(field.span.end..brace_tok.span.start)
                            .unwrap_or("");
                        if !gap.is_empty() {
                            return Some(Expr::Dot(Box::new(left), field, span));
                        }
                    }
                    if field.name.chars().next().is_some_and(|c| c.is_uppercase()) {
                        let (fields, end) =
                            self.parse_braced_fields("expected '{' in construction")?;
                        let target_name = match &left {
                            Expr::Identifier(pkg) => format!("{}.{}", pkg.name, field.name),
                            _ => field.name.clone(),
                        };
                        return Some(Expr::Construct(ConstructExpr {
                            target: Ident::new(target_name, span),
                            fields,
                            span: span.merge(end),
                        }));
                    }
                    let mut chars = field.name.chars();
                    let suggestion: String = match chars.next() {
                        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                        None => field.name.clone(),
                    };
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN,
                            format!(
                                "struct names start with an uppercase letter: did you mean `{suggestion}`?"
                            ),
                        )
                        .with_primary_span(self.source, field.span),
                    );
                    return None;
                }
                Some(Expr::Dot(Box::new(left), field, span))
            }

            // Safe navigation: `target?.field`
            TokenKind::QuestionDot => {
                let field = self.parse_member_name()?;
                let span = left.span().merge(field.span);
                Some(Expr::QuestionDot(Box::new(left), field, span))
            }

            // Failure propagation: `expr?` propagates a `Failure` and otherwise
            // yields the value, so it chains postfix like `foo()?.bar?`.
            TokenKind::Question => {
                let span = left.span().merge(op_span);
                Some(Expr::Try(Box::new(left), span))
            }

            // Functional struct update: `base with { field: value, ... }` produces
            // a new instance; the base is copied and only the named fields are
            // replaced, so the field block lists what changes and nothing else.
            TokenKind::With => {
                let start = left.span();
                let (fields, end) = self.parse_braced_fields("expected '{' after 'with'")?;
                let span = start.merge(end);
                Some(Expr::With(Box::new(left), WithExpr { fields, span }))
            }

            // Index access: `target[index]`, including half-open slices with omitted
            // bounds (`target[..2]`, `target[2..]`, `target[..]`), which canon allows.
            TokenKind::LBracket => {
                let index_expr = self.parse_index_expr()?;
                let end = self
                    .expect(&TokenKind::RBracket, "expected ']' after index")?
                    .span;
                let span = left.span().merge(end);
                Some(Expr::Index(Box::new(left), Box::new(index_expr), span))
            }

            // Trailing block sugar without parens: `callee do ... end`
            TokenKind::Do => {
                let block = self.parse_trailing_block()?;
                let span = left.span().merge(block.span);
                Some(Expr::Call(CallExpr {
                    callee: Box::new(left),
                    args: Vec::new(),
                    trailing_block: Some(block),
                    span,
                }))
            }

            _ => None,
        }
    }

    fn binary_expr(
        &mut self,
        left: Expr,
        op: BinaryOp,
        prec: Precedence,
        op_span: SourceSpan,
    ) -> Option<Expr> {
        if matches!(
            op,
            BinaryOp::Less
                | BinaryOp::LessEqual
                | BinaryOp::Greater
                | BinaryOp::GreaterEqual
                | BinaryOp::Equal
                | BinaryOp::NotEqual
        ) && matches!(
            &left,
            Expr::Binary(
                BinaryOp::Less
                    | BinaryOp::LessEqual
                    | BinaryOp::Greater
                    | BinaryOp::GreaterEqual
                    | BinaryOp::Equal
                    | BinaryOp::NotEqual,
                _,
                _,
                _
            )
        ) {
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN,
                    "chained comparisons like `a < b < c` are not supported in Aipo V1; write `a < b and b < c` or use elided continuation `a < b and < c`",
                )
                .with_primary_span(self.source, op_span),
            );
        }
        let right = self.parse_pratt_expr(prec)?;
        let span = left.span().merge(right.span());
        Some(Expr::Binary(op, Box::new(left), Box::new(right), span))
    }

    fn peek_precedence(&self) -> Precedence {
        // The `?` after an `is` type belongs to the nullable marker, not to propagation,
        // so it must not out-rank the operand's own precedence and pull the Pratt loop.
        if self.in_is_type && matches!(self.peek(), TokenKind::Question) {
            return Precedence::Lowest;
        }
        match self.peek() {
            TokenKind::LParen
            | TokenKind::Dot
            | TokenKind::QuestionDot
            | TokenKind::Question
            | TokenKind::LBracket
            | TokenKind::With
            | TokenKind::Do => Precedence::Call,
            TokenKind::Star | TokenKind::Slash | TokenKind::SlashSlash | TokenKind::Percent => {
                Precedence::Product
            }
            TokenKind::Plus | TokenKind::Minus => Precedence::Sum,
            TokenKind::DotDot => Precedence::Range,
            TokenKind::EqualEqual
            | TokenKind::BangEqual
            | TokenKind::Less
            | TokenKind::LessEqual
            | TokenKind::Greater
            | TokenKind::GreaterEqual
            | TokenKind::Is => Precedence::Comparison,
            TokenKind::And => Precedence::And,
            TokenKind::Or => Precedence::Or,
            TokenKind::OrElse => Precedence::OrElse,
            TokenKind::Pipeline => Precedence::Pipeline,
            _ => Precedence::Lowest,
        }
    }

    fn parse_construct_expr(&mut self, target: Ident) -> Option<Expr> {
        let start = target.span;
        let (fields, end) = self.parse_braced_fields("expected '{' in struct construction")?;
        Some(Expr::Construct(ConstructExpr {
            target,
            fields,
            span: start.merge(end),
        }))
    }

    /// Parses a `{ ... }` field list shared by construction and `with` updates.
    ///
    /// Both spellings accept the same field syntax — named (`x: 1`, `x = 1`) or positional —
    /// so the loop lives here once. Returns the fields and the span of the closing brace.
    fn parse_braced_fields(
        &mut self,
        missing_brace: &str,
    ) -> Option<(Vec<ConstructField>, SourceSpan)> {
        self.expect(&TokenKind::LBrace, missing_brace)?;
        self.skip_newlines();

        let mut fields = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let is_named = matches!(self.peek(), TokenKind::Identifier(_))
                && matches!(self.peek_ahead(1), TokenKind::Colon | TokenKind::Equal);
            let (name, value) = if is_named {
                let id = self.parse_ident()?;
                self.advance(); // consume ':' or '='
                self.skip_newlines();
                let old = self.allow_comma_is;
                self.allow_comma_is = false;
                let val = self.parse_expr()?;
                self.allow_comma_is = old;
                (Some(id), val)
            } else {
                let old = self.allow_comma_is;
                self.allow_comma_is = false;
                let val = self.parse_expr()?;
                self.allow_comma_is = old;
                (None, val)
            };
            let span = name
                .as_ref()
                .map(|n| n.span.merge(value.span()))
                .unwrap_or_else(|| value.span());
            fields.push(ConstructField { name, value, span });
            self.skip_newlines();
            if !self.match_token(&TokenKind::Comma) {
                break;
            }
            self.skip_newlines();
        }
        self.skip_newlines();
        let end = self
            .expect(
                &TokenKind::RBrace,
                "expected '}' to close struct construction",
            )?
            .span;
        Some((fields, end))
    }

    fn parse_trailing_block(&mut self) -> Option<TrailingBlock> {
        let start = self.tokens[self.cursor - 1].span; // 'do'
        let mut params = Vec::new();
        if self.match_token(&TokenKind::LParen) {
            while !self.check(&TokenKind::RParen) && !self.is_at_end() {
                params.push(self.parse_ident()?);
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
            self.expect(
                &TokenKind::RParen,
                "expected ')' after trailing block parameters",
            )?;
        } else if !matches!(self.peek(), TokenKind::Newline | TokenKind::LBrace) {
            while !self.check(&TokenKind::Newline)
                && !self.check(&TokenKind::LBrace)
                && !self.is_at_end()
            {
                params.push(self.parse_ident()?);
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.skip_newlines();

        self.expect(&TokenKind::LBrace, "expected '{' to open trailing block")?;
        self.skip_newlines();

        let mut body = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if let Some(s) = self.parse_stmt() {
                body.push(s);
            }
            self.skip_newlines();
        }
        let end_span = self
            .expect(&TokenKind::RBrace, "expected '}' to close trailing block")?
            .span;
        Some(TrailingBlock {
            params,
            body,
            span: start.merge(end_span),
        })
    }

    fn parse_fn_expr(&mut self) -> Option<Expr> {
        let start = self.advance().span; // 'fn'
        self.parse_fn_expr_rest(start, false)
    }

    /// Shared body of `fn(...)` / `async fn(...)` anonymous functions.
    fn parse_fn_expr_rest(&mut self, start: SourceSpan, is_async: bool) -> Option<Expr> {
        self.expect(
            &TokenKind::LParen,
            "expected '(' in anonymous function params",
        )?;
        let params = self.parse_params()?;
        self.expect(
            &TokenKind::RParen,
            "expected ')' after anonymous function params",
        )?;
        let return_type = self.parse_optional_return_type();
        self.skip_newlines();

        self.expect(
            &TokenKind::LBrace,
            "expected '{' to open anonymous function body",
        )?;
        self.skip_newlines();

        let mut body = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if let Some(s) = self.parse_stmt() {
                body.push(s);
            }
            self.skip_newlines();
        }
        let end_span = self
            .expect(
                &TokenKind::RBrace,
                "expected '}' to close anonymous function",
            )?
            .span;
        Some(Expr::Fn(FunctionExpr {
            is_async,
            params,
            return_type,
            body,
            span: start.merge(end_span),
        }))
    }

    fn parse_list_expr(&mut self) -> Option<Expr> {
        let start = self.advance().span; // '['
        self.skip_newlines();
        let mut items = Vec::new();
        while !self.check(&TokenKind::RBracket) && !self.is_at_end() {
            let old = self.allow_comma_is;
            self.allow_comma_is = false;
            let item = self.parse_expr()?;
            self.allow_comma_is = old;
            items.push(item);
            self.skip_newlines();
            if !self.match_token(&TokenKind::Comma) {
                break;
            }
            self.skip_newlines();
        }
        self.skip_newlines();
        let end = self
            .expect(&TokenKind::RBracket, "expected ']' after list literal")?
            .span;
        Some(Expr::List(items, start.merge(end)))
    }

    fn parse_dict_expr(&mut self) -> Option<Expr> {
        let start = self.advance().span; // '{'
        self.skip_newlines();
        let mut pairs = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let old_key = self.allow_comma_is;
            self.allow_comma_is = false;
            let key = self.parse_expr()?;
            self.allow_comma_is = old_key;
            self.skip_newlines();
            self.expect(
                &TokenKind::Colon,
                "expected ':' between dictionary key and value",
            )?;
            self.skip_newlines();
            let old_val = self.allow_comma_is;
            self.allow_comma_is = false;
            let val = self.parse_expr()?;
            self.allow_comma_is = old_val;
            pairs.push((key, val));
            self.skip_newlines();
            if !self.match_token(&TokenKind::Comma) {
                break;
            }
            self.skip_newlines();
        }
        self.skip_newlines();
        let end = self
            .expect(&TokenKind::RBrace, "expected '}' after dict literal")?
            .span;
        Some(Expr::Dict(pairs, start.merge(end)))
    }

    fn parse_ident(&mut self) -> Option<Ident> {
        if let TokenKind::Identifier(name) = self.peek().clone() {
            let span = self.advance().span;
            Some(Ident::new(name, span))
        } else {
            let span = self
                .peek_token()
                .map(|t| t.span)
                .unwrap_or_else(|| SourceSpan::empty(self.source.len()));
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::AIPO_PARSE_UNEXPECTED_TOKEN,
                    format!("expected identifier, found '{:?}'", self.peek()),
                )
                .with_primary_span(self.source, span),
            );
            None
        }
    }

    fn parse_member_name(&mut self) -> Option<Ident> {
        if let Some(tok) = self.peek_token().cloned()
            && let Some(text) = self.source.slice(tok.span)
            && !text.is_empty()
            && (text.starts_with(|c: char| c.is_alphabetic() || c == '_')
                && text.chars().all(|c| c.is_alphanumeric() || c == '_'))
        {
            self.advance();
            return Some(Ident::new(text.to_string(), tok.span));
        }
        self.parse_ident()
    }

    fn parse_ident_or_contextual(&mut self) -> Option<Ident> {
        self.parse_ident()
    }
}

fn stmt_span(stmt: &Stmt) -> SourceSpan {
    match stmt {
        Stmt::Let(_, _, span)
        | Stmt::Var(_, _, span)
        | Stmt::Assign(_, _, span)
        | Stmt::CompoundAssign(_, _, _, span)
        | Stmt::Loop(_, span)
        | Stmt::While(_, _, span)
        | Stmt::Repeat(_, _, _, span)
        | Stmt::Each(_, _, _, span)
        | Stmt::Break(span)
        | Stmt::Continue(span)
        | Stmt::Return(_, span)
        | Stmt::Fail(_, span)
        | Stmt::AwaitDo(_, span) => *span,
        Stmt::If(s) => s.span,
        Stmt::Match(s) => s.span,
        Stmt::Attempt(s) => s.span,
        Stmt::Fn(f) => f.span,
        Stmt::Expr(e) => e.span(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Precedence {
    Lowest = 1,
    Pipeline,   // |>
    OrElse,     // or_else
    Or,         // or
    And,        // and
    Not,        // not
    Comparison, // == != < <= > >= is
    Range,      // ..
    Sum,        // + -
    Product,    // * / // %
    Prefix,     // - +
    Call,       // . ?. () [] do
}

/// Returns whether `kind` is a comparison token supported in continuation chains.
fn is_comparison_token(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::EqualEqual
            | TokenKind::BangEqual
            | TokenKind::Less
            | TokenKind::LessEqual
            | TokenKind::Greater
            | TokenKind::GreaterEqual
            | TokenKind::Is
    )
}

/// Extracts the single subject of a comparison expression or comparison chain.
fn extract_comparison_subject(expr: &Expr) -> Option<Expr> {
    match expr {
        Expr::Binary(op, left, right, _) => match op {
            BinaryOp::Equal
            | BinaryOp::NotEqual
            | BinaryOp::Less
            | BinaryOp::LessEqual
            | BinaryOp::Greater
            | BinaryOp::GreaterEqual
            | BinaryOp::Is
            | BinaryOp::IsNullable => Some((**left).clone()),
            BinaryOp::And => extract_comparison_subject(right),
            _ => None,
        },
        _ => None,
    }
}
