//! Lexer implementation converting source text into token streams.

use aipo_diagnostics::{Diagnostic, DiagnosticCode};
use aipo_source::{Source, SourceSpan};
use unicode_normalization::UnicodeNormalization;

use crate::number::number_is_well_formed;
use crate::token::{StringPrefix, Token, TokenKind};

/// Lexer state scanning over an immutable Aipo source.
pub struct Lexer<'a> {
    source: &'a Source,
    byte_offset: usize,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    /// Creates a new lexer for the given source.
    #[must_use]
    pub fn new(source: &'a Source) -> Self {
        Self {
            source,
            byte_offset: 0,
            diagnostics: Vec::new(),
        }
    }

    /// Tokenizes the entire source file, returning all tokens including EOF and diagnostics.
    pub fn tokenize(mut self) -> (Vec<Token>, Vec<Diagnostic>) {
        let mut tokens = Vec::new();

        loop {
            let token = self.next_token();
            let is_eof = token.kind == TokenKind::Eof;
            tokens.push(token);
            if is_eof {
                break;
            }
        }

        (tokens, self.diagnostics)
    }

    fn peek(&self) -> Option<char> {
        self.source.text()[self.byte_offset..].chars().next()
    }

    fn peek_ahead(&self, offset: usize) -> Option<char> {
        let mut it = self.source.text()[self.byte_offset..].chars();
        for _ in 0..offset {
            it.next()?;
        }
        it.next()
    }

    fn current_offset(&self) -> usize {
        self.byte_offset
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.source.text()[self.byte_offset..].chars().next()?;
        self.byte_offset += ch.len_utf8();
        Some(ch)
    }

    fn next_token(&mut self) -> Token {
        self.skip_horizontal_whitespace();

        let start = self.current_offset();

        let ch = match self.advance() {
            Some(c) => c,
            None => {
                return Token::new(TokenKind::Eof, SourceSpan::empty(start));
            }
        };

        // 1. Newlines (collapse multiple consecutive newlines)
        if ch == '\n' {
            while let Some('\n') = self.peek() {
                self.advance();
            }
            let end = self.current_offset();
            return Token::new(TokenKind::Newline, SourceSpan::new(start, end));
        }

        // 2. Directives `#!name ...` — a compiler annotation on the next item.
        // The `!` is what separates an annotation from a comment; `#` alone stays a comment.
        if ch == '#' && self.peek() == Some('!') {
            self.advance(); // consume '!'
            let mut directive = String::new();
            while let Some(next_ch) = self.peek() {
                if next_ch == '\n' {
                    break;
                }
                directive.push(next_ch);
                self.advance();
            }
            let end = self.current_offset();
            return Token::new(TokenKind::Directive(directive), SourceSpan::new(start, end));
        }

        // 3. Comments `# ...`
        if ch == '#' {
            let mut comment_text = String::new();
            while let Some(next_ch) = self.peek() {
                if next_ch == '\n' {
                    break;
                }
                comment_text.push(next_ch);
                self.advance();
            }
            let end = self.current_offset();
            return Token::new(
                TokenKind::Comment(comment_text),
                SourceSpan::new(start, end),
            );
        }

        // 3. String literals (direct or prefixed: f, r, fr)
        if ch == '"' {
            return self.lex_string(start, StringPrefix::Normal);
        }

        // Check for string prefixes: f", r", fr", rf"
        if (ch == 'f' || ch == 'r')
            && (self.peek() == Some('"')
                || (self.peek_ahead(1) == Some('"')
                    && (self.peek() == Some('r') || self.peek() == Some('f'))))
        {
            let prefix = if ch == 'f' && self.peek() == Some('"') {
                StringPrefix::Format
            } else if ch == 'r' && self.peek() == Some('"') {
                StringPrefix::Raw
            } else if (ch == 'f' && self.peek() == Some('r'))
                || (ch == 'r' && self.peek() == Some('f'))
            {
                self.advance(); // consume the second prefix character
                StringPrefix::FormatRaw
            } else {
                StringPrefix::Normal
            };

            if self.peek() == Some('"') {
                self.advance(); // consume opening quote
                return self.lex_string(start, prefix);
            }
        }

        // 4. Numbers (Integer or Float)
        if ch.is_ascii_digit() {
            return self.lex_number(start, ch);
        }

        // 5. Identifiers, keywords, discard
        if is_ident_start(ch) {
            return self.lex_identifier_or_keyword(start, ch);
        }

        // 6. Operators and delimiters
        let kind = match ch {
            '+' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::PlusEq
                } else {
                    TokenKind::Plus
                }
            }
            '-' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::MinusEq
                } else {
                    TokenKind::Minus
                }
            }
            '*' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::StarEq
                } else {
                    TokenKind::Star
                }
            }
            '/' => {
                if self.peek() == Some('/') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        TokenKind::SlashSlashEq
                    } else {
                        TokenKind::SlashSlash
                    }
                } else if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::SlashEq
                } else {
                    TokenKind::Slash
                }
            }
            '%' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::PercentEq
                } else {
                    TokenKind::Percent
                }
            }
            '=' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::EqualEqual
                } else if self.peek() == Some('>') {
                    self.advance();
                    TokenKind::FatArrow
                } else {
                    TokenKind::Equal
                }
            }
            '!' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::BangEqual
                } else {
                    TokenKind::Bang
                }
            }
            '<' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::LessEqual
                } else {
                    TokenKind::Less
                }
            }
            '>' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::GreaterEqual
                } else {
                    TokenKind::Greater
                }
            }
            '|' => {
                if self.peek() == Some('>') {
                    self.advance();
                    TokenKind::Pipeline
                } else {
                    self.record_error(
                        start,
                        format!("unexpected character '{ch}', did you mean '|>'?"),
                    );
                    TokenKind::Error(ch.to_string())
                }
            }
            '.' => {
                if self.peek() == Some('.') {
                    self.advance();
                    TokenKind::DotDot
                } else {
                    TokenKind::Dot
                }
            }
            '?' => {
                if self.peek() == Some('.') {
                    self.advance();
                    TokenKind::QuestionDot
                } else {
                    TokenKind::Question
                }
            }
            ':' => {
                // `::` é o token de associação em lote (`Retangulo::[f1, f2]`); um
                // `:` sozinho continua anotação de tipo, construção ou `satisfy`.
                if self.peek() == Some(':') {
                    self.advance();
                    TokenKind::ColonColon
                } else {
                    TokenKind::Colon
                }
            }
            ',' => TokenKind::Comma,
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            '[' => TokenKind::LBracket,
            ']' => TokenKind::RBracket,
            '{' => TokenKind::LBrace,
            '}' => TokenKind::RBrace,
            _ => {
                self.record_error(start, format!("unexpected character '{ch}'"));
                TokenKind::Error(ch.to_string())
            }
        };

        let end = self.current_offset();
        Token::new(kind, SourceSpan::new(start, end))
    }

    fn skip_horizontal_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch == ' ' || ch == '\t' || ch == '\r' {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn lex_identifier_or_keyword(&mut self, start: usize, _first_ch: char) -> Token {
        while let Some(ch) = self.peek() {
            if is_ident_continue(ch) {
                self.advance();
            } else {
                break;
            }
        }

        let raw = &self.source.text()[start..self.current_offset()];

        let end = self.current_offset();
        let span = SourceSpan::new(start, end);

        if raw == "_" {
            return Token::new(TokenKind::Discard, span);
        }

        let kind = match raw {
            "let" => TokenKind::Let,
            "var" => TokenKind::Var,
            "fixed" => TokenKind::Fixed,
            "fn" => TokenKind::Fn,
            "async" => TokenKind::Async,
            "await" => TokenKind::Await,
            "with" => TokenKind::With,
            "struct" => TokenKind::Struct,
            "enum" => TokenKind::Enum,
            "interface" => TokenKind::Interface,
            "init" => TokenKind::Init,
            "invariant" => TokenKind::Invariant,
            "if" => TokenKind::If,
            "elif" => TokenKind::Elif,
            "else" => TokenKind::Else,
            "then" => TokenKind::Then,
            "match" => TokenKind::Match,
            "when" => TokenKind::When,
            "loop" => TokenKind::Loop,
            "while" => TokenKind::While,
            "repeat" => TokenKind::Repeat,
            "as" => TokenKind::As,
            "each" => TokenKind::Each,
            "in" => TokenKind::In,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "return" => TokenKind::Return,
            "fail" => TokenKind::Fail,
            "or_else" => TokenKind::OrElse,
            "attempt" => TokenKind::Attempt,
            "failed" => TokenKind::Failed,
            "import" => TokenKind::Import,
            "export" => TokenKind::Export,
            "is" => TokenKind::Is,
            "not" => TokenKind::Not,
            "and" => TokenKind::And,
            "or" => TokenKind::Or,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "none" => TokenKind::None,
            "self" => TokenKind::SelfVal,
            "do" => TokenKind::Do,
            _ => TokenKind::Identifier(raw.to_string()),
        };

        Token::new(kind, span)
    }

    fn lex_number(&mut self, start: usize, first_ch: char) -> Token {
        let mut raw = String::new();
        raw.push(first_ch);

        // Check for base prefix (0x, 0b, 0o)
        if first_ch == '0'
            && let Some(base_ch) = self.peek()
            && (base_ch == 'x'
                || base_ch == 'X'
                || base_ch == 'b'
                || base_ch == 'B'
                || base_ch == 'o'
                || base_ch == 'O')
        {
            raw.push(base_ch);
            self.advance();
            while let Some(ch) = self.peek() {
                if ch.is_ascii_hexdigit() || ch == '_' {
                    raw.push(ch);
                    self.advance();
                } else {
                    break;
                }
            }
            let end = self.current_offset();
            if !number_is_well_formed(&raw) {
                self.record_invalid_number(start, &raw);
            }
            return Token::new(TokenKind::IntLiteral(raw), SourceSpan::new(start, end));
        }

        // Decimal digits
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() || ch == '_' {
                raw.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        // Check for float dot: only if followed by digit, `_`, or exponent (not `..` range or method call)
        let is_float = if self.peek() == Some('.')
            && (self.peek_ahead(1).is_some_and(|c| c.is_ascii_digit())
                || self.peek_ahead(1) == Some('_')
                || self.peek_ahead(1) == Some('e')
                || self.peek_ahead(1) == Some('E'))
        {
            raw.push('.');
            self.advance(); // consume '.'
            while let Some(ch) = self.peek() {
                if ch.is_ascii_digit() || ch == '_' {
                    raw.push(ch);
                    self.advance();
                } else {
                    break;
                }
            }
            true
        } else {
            false
        };

        // Scientific exponent e/E
        let is_float_exp = if self.peek() == Some('e') || self.peek() == Some('E') {
            raw.push('e');
            self.advance();
            if (self.peek() == Some('+') || self.peek() == Some('-'))
                && let Some(sign) = self.advance()
            {
                raw.push(sign);
            }
            while let Some(ch) = self.peek() {
                if ch.is_ascii_digit() || ch == '_' {
                    raw.push(ch);
                    self.advance();
                } else {
                    break;
                }
            }
            true
        } else {
            false
        };

        let end = self.current_offset();
        let span = SourceSpan::new(start, end);

        if !number_is_well_formed(&raw) {
            self.record_invalid_number(start, &raw);
        }

        if is_float || is_float_exp {
            Token::new(TokenKind::FloatLiteral(raw), span)
        } else {
            Token::new(TokenKind::IntLiteral(raw), span)
        }
    }

    fn lex_string(&mut self, start: usize, prefix: StringPrefix) -> Token {
        let is_multiline = if self.peek() == Some('"') && self.peek_ahead(1) == Some('"') {
            self.advance(); // second '"'
            self.advance(); // third '"'
            true
        } else {
            false
        };

        let is_raw = matches!(prefix, StringPrefix::Raw | StringPrefix::FormatRaw);
        let is_format = matches!(prefix, StringPrefix::Format | StringPrefix::FormatRaw);
        let mut content = String::new();
        let mut brace_depth = 0usize;

        loop {
            match self.advance() {
                Some('"') => {
                    if brace_depth > 0 {
                        content.push('"');
                        while let Some(ch) = self.advance() {
                            content.push(ch);
                            if ch == '\\' {
                                if let Some(esc) = self.advance() {
                                    content.push(esc);
                                }
                            } else if ch == '"' {
                                break;
                            }
                        }
                    } else if is_multiline {
                        if self.peek() == Some('"') && self.peek_ahead(1) == Some('"') {
                            self.advance();
                            self.advance();
                            break;
                        } else {
                            content.push('"');
                        }
                    } else {
                        break;
                    }
                }
                Some('{') if is_format => {
                    if self.peek() == Some('{') {
                        self.advance();
                        content.push('{');
                        content.push('{');
                    } else {
                        brace_depth += 1;
                        content.push('{');
                    }
                }
                Some('}') if is_format => {
                    if self.peek() == Some('}') {
                        self.advance();
                        content.push('}');
                        content.push('}');
                    } else {
                        brace_depth = brace_depth.saturating_sub(1);
                        content.push('}');
                    }
                }
                Some('\\') if !is_raw => {
                    match self.advance() {
                        Some('n') => content.push('\n'),
                        Some('r') => content.push('\r'),
                        Some('t') => content.push('\t'),
                        Some('\\') => content.push('\\'),
                        Some('"') => content.push('"'),
                        Some('0') => content.push('\0'),
                        Some('u') if self.peek() == Some('{') => {
                            self.advance(); // consume '{'
                            let mut hex_str = String::new();
                            let mut closed = false;
                            while let Some(ch) = self.advance() {
                                if ch == '}' {
                                    closed = true;
                                    break;
                                }
                                hex_str.push(ch);
                            }
                            if !closed {
                                self.record_error(start, "unterminated unicode escape sequence");
                            } else {
                                match u32::from_str_radix(&hex_str, 16)
                                    .ok()
                                    .and_then(char::from_u32)
                                {
                                    Some(unicode_char) => content.push(unicode_char),
                                    None => {
                                        let end = self.current_offset();
                                        self.diagnostics.push(
                                            Diagnostic::error(
                                                DiagnosticCode::AIPO_LEX_INVALID_UNICODE_ESCAPE,
                                                format!(
                                                    "invalid unicode escape '\\u{{{hex_str}}}'"
                                                ),
                                            )
                                            .with_primary_span(
                                                self.source,
                                                SourceSpan::new(start, end),
                                            ),
                                        );
                                    }
                                }
                            }
                        }
                        Some(other) => {
                            let end = self.current_offset();
                            self.diagnostics.push(
                                Diagnostic::error(
                                    DiagnosticCode::AIPO_LEX_UNKNOWN_ESCAPE,
                                    format!("unknown escape sequence '\\{other}'"),
                                )
                                .with_primary_span(self.source, SourceSpan::new(start, end)),
                            );
                            content.push(other);
                        }
                        None => {
                            let end = self.current_offset();
                            self.diagnostics.push(
                                Diagnostic::error(
                                    DiagnosticCode::AIPO_LEX_UNTERMINATED_STRING,
                                    "unterminated string literal",
                                )
                                .with_primary_span(self.source, SourceSpan::new(start, end)),
                            );
                            return Token::new(
                                TokenKind::Error("unterminated string".to_string()),
                                SourceSpan::new(start, end),
                            );
                        }
                    }
                }
                Some(ch) => {
                    if ch == '\n' && !is_multiline {
                        let end = self.current_offset();
                        self.diagnostics.push(
                            Diagnostic::error(
                                DiagnosticCode::AIPO_LEX_UNTERMINATED_STRING,
                                "single-line string literal cannot span across newlines",
                            )
                            .with_primary_span(self.source, SourceSpan::new(start, end)),
                        );
                        return Token::new(
                            TokenKind::Error("unterminated string".to_string()),
                            SourceSpan::new(start, end),
                        );
                    }
                    content.push(ch);
                }
                None => {
                    let end = self.current_offset();
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::AIPO_LEX_UNTERMINATED_STRING,
                            "unterminated string literal before EOF",
                        )
                        .with_primary_span(self.source, SourceSpan::new(start, end)),
                    );
                    return Token::new(
                        TokenKind::Error("unterminated string".to_string()),
                        SourceSpan::new(start, end),
                    );
                }
            }
        }

        // Canon makes NFC an invariant of `String` — "normalizada automaticamente ... antes de ser
        // exposta ao programa" — and puts the boundary where text is decoded. The decoded literal
        // is normalized here, while the source bytes (and therefore the span) are left untouched,
        // exactly as canon keeps source spans pointing at what the author wrote. Raw strings keep
        // their escapes raw but are still Unicode-valid NFC.
        let content: String = content.nfc().collect();

        let end = self.current_offset();
        Token::new(
            TokenKind::StringLiteral {
                content,
                prefix,
                is_multiline,
            },
            SourceSpan::new(start, end),
        )
    }

    fn record_error(&mut self, start: usize, msg: impl Into<String>) {
        let end = self.current_offset();
        self.diagnostics.push(
            Diagnostic::error(DiagnosticCode::AIPO_LEX_UNEXPECTED_CHARACTER, msg)
                .with_primary_span(self.source, SourceSpan::new(start, end)),
        );
    }

    fn record_invalid_number(&mut self, start: usize, raw: &str) {
        let end = self.current_offset();
        self.diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::AIPO_LEX_INVALID_NUMBER,
                format!("malformed numeric literal '{raw}'"),
            )
            .with_primary_span(self.source, SourceSpan::new(start, end)),
        );
    }
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_alphabetic()
}

fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}
