//! Token classification and canonical text used by the formatter.
//!
//! The formatter works on the lexer's token stream rather than the AST so comments,
//! string escapes and literal spelling survive formatting untouched. `aipo fmt` is
//! therefore a *whitespace* normalizer: it rewrites indentation and inter-token spacing
//! and never rewrites token contents.

use aipo_lexer::{StringPrefix, Token, TokenKind};
use aipo_source::Source;

/// How a token participates in canonical spacing and block nesting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// A keyword, identifier or literal: separated from neighbours by one space.
    Word,
    /// Opening delimiter: `(`, `[` or `{`.
    Open,
    /// Closing delimiter: `)`, `]` or `}`.
    Close,
    /// `,`
    Comma,
    /// `:`
    Colon,
    /// `.` or `?.`
    Dot,
    /// A binary or assignment operator, spaced on both sides.
    Operator,
    /// A prefix operator with a tight right side (`-`).
    PrefixTight,
    /// A prefix operator with a spaced right side (`not`).
    PrefixLoose,
    /// `..`, which binds tightly on both sides.
    Range,
    /// A postfix sigil with no leading space (`!`, `?`).
    Postfix,
    /// A comment: separated from the preceding tokens by one space.
    Comment,
    /// A lexical error or EOF.
    Other,
}

/// Returns the token's class.
#[must_use]
pub fn classify(kind: &TokenKind) -> Class {
    match kind {
        TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => Class::Open,
        TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => Class::Close,
        TokenKind::Comma => Class::Comma,
        TokenKind::Colon => Class::Colon,
        TokenKind::Dot | TokenKind::QuestionDot => Class::Dot,
        TokenKind::DotDot => Class::Range,
        TokenKind::Bang | TokenKind::Question => Class::Postfix,
        TokenKind::Comment(_) | TokenKind::Directive(_) => Class::Comment,
        TokenKind::Newline | TokenKind::Eof | TokenKind::Error(_) => Class::Other,

        TokenKind::Plus
        | TokenKind::Star
        | TokenKind::Slash
        | TokenKind::SlashSlash
        | TokenKind::Percent
        | TokenKind::PlusEq
        | TokenKind::MinusEq
        | TokenKind::StarEq
        | TokenKind::SlashEq
        | TokenKind::SlashSlashEq
        | TokenKind::PercentEq
        | TokenKind::EqualEqual
        | TokenKind::BangEqual
        | TokenKind::Less
        | TokenKind::LessEqual
        | TokenKind::Greater
        | TokenKind::GreaterEqual
        | TokenKind::Equal
        | TokenKind::FatArrow
        | TokenKind::Pipeline
        | TokenKind::And
        | TokenKind::Or
        | TokenKind::Is
        | TokenKind::ColonColon
        | TokenKind::In => Class::Operator,

        // `-` is unary in prefix position and binary otherwise; the caller resolves
        // that with `minus_is_prefix`.
        TokenKind::Minus => Class::Operator,
        TokenKind::Not => Class::PrefixLoose,

        TokenKind::Let
        | TokenKind::Var
        | TokenKind::Fixed
        | TokenKind::Fn
        | TokenKind::Async
        | TokenKind::Await
        | TokenKind::With
        | TokenKind::Struct
        | TokenKind::Enum
        | TokenKind::Interface
        | TokenKind::Init
        | TokenKind::Invariant
        | TokenKind::If
        | TokenKind::Elif
        | TokenKind::Else
        | TokenKind::Then
        | TokenKind::Match
        | TokenKind::When
        | TokenKind::Loop
        | TokenKind::While
        | TokenKind::Repeat
        | TokenKind::As
        | TokenKind::Each
        | TokenKind::Break
        | TokenKind::Continue
        | TokenKind::Return
        | TokenKind::Fail
        | TokenKind::OrElse
        | TokenKind::Attempt
        | TokenKind::Failed
        | TokenKind::Import
        | TokenKind::Export
        | TokenKind::True
        | TokenKind::False
        | TokenKind::None
        | TokenKind::SelfVal
        | TokenKind::Do
        | TokenKind::Identifier(_)
        | TokenKind::Discard
        | TokenKind::IntLiteral(_)
        | TokenKind::FloatLiteral(_)
        | TokenKind::StringLiteral { .. } => Class::Word,
    }
}

/// Returns whether a `-` token in this position is a prefix (unary) minus.
#[must_use]
pub fn minus_is_prefix(previous: Option<&TokenKind>) -> bool {
    match previous {
        None => true,
        Some(kind) => {
            matches!(
                classify(kind),
                Class::Open | Class::Comma | Class::Colon | Class::Operator | Class::Range
            ) || matches!(kind, TokenKind::Not | TokenKind::Minus)
        }
    }
}

/// Returns whether the token opens a block that must be indented under it.
#[must_use]
pub fn opens_block(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Fn
            | TokenKind::Struct
            | TokenKind::Enum
            | TokenKind::Interface
            | TokenKind::Init
            | TokenKind::Invariant
            | TokenKind::If
            | TokenKind::Elif
            | TokenKind::Else
            | TokenKind::Match
            | TokenKind::When
            | TokenKind::Loop
            | TokenKind::While
            | TokenKind::Repeat
            | TokenKind::Each
            | TokenKind::Attempt
            | TokenKind::Failed
            | TokenKind::Do
    )
}

/// Returns whether the token closes a block started by an earlier branch keyword.
///
/// These tokens belong to the indentation level of their opening statement, so the line
/// that starts with them is outdented before the token itself opens the next branch body.
#[must_use]
pub fn is_branch_keyword(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Elif | TokenKind::Else | TokenKind::When | TokenKind::Failed
    )
}

/// Returns the canonical source text of a token.
///
/// Literals and comments are reproduced from the original text so escapes, string
/// prefixes and comment content survive byte for byte.
#[must_use]
pub fn text_of(token: &Token, source: &Source) -> String {
    let mut out = String::new();
    write_text_of(token, source, &mut out);
    out
}

/// Appends the canonical source text of a token to `out`.
///
/// This is the form the renderer uses: a `String` per token is an allocation per
/// token, and the formatter already knows where the whole line is going.
pub fn write_text_of(token: &Token, source: &Source, out: &mut String) {
    match &token.kind {
        TokenKind::Identifier(name) => out.push_str(name),
        TokenKind::IntLiteral(raw) | TokenKind::FloatLiteral(raw) => out.push_str(raw),
        TokenKind::Comment(_) | TokenKind::Directive(_) | TokenKind::Error(_) => {
            write_slice(source, token, out)
        }
        TokenKind::StringLiteral { .. } => write_slice(source, token, out),
        other => out.push_str(keyword_text(other)),
    }
}

fn write_slice(source: &Source, token: &Token, out: &mut String) {
    out.push_str(&source.text()[token.span.start..token.span.end]);
}

/// Returns the canonical spelling of a non-literal token kind.
#[must_use]
pub fn keyword_text(kind: &TokenKind) -> &'static str {
    match kind {
        TokenKind::Let => "let",
        TokenKind::Var => "var",
        TokenKind::Fixed => "fixed",
        TokenKind::Fn => "fn",
        TokenKind::Async => "async",
        TokenKind::Await => "await",
        TokenKind::With => "with",
        TokenKind::Struct => "struct",
        TokenKind::Enum => "enum",
        TokenKind::Interface => "interface",
        TokenKind::Init => "init",
        TokenKind::Invariant => "invariant",
        TokenKind::If => "if",
        TokenKind::Elif => "elif",
        TokenKind::Else => "else",
        TokenKind::Then => "then",
        TokenKind::Match => "match",
        TokenKind::When => "when",
        TokenKind::Loop => "loop",
        TokenKind::While => "while",
        TokenKind::Repeat => "repeat",
        TokenKind::As => "as",
        TokenKind::Each => "each",
        TokenKind::In => "in",
        TokenKind::Break => "break",
        TokenKind::Continue => "continue",
        TokenKind::Return => "return",
        TokenKind::Fail => "fail",
        TokenKind::OrElse => "or_else",
        TokenKind::Attempt => "attempt",
        TokenKind::Failed => "failed",
        TokenKind::Import => "import",
        TokenKind::Export => "export",
        TokenKind::Is => "is",
        TokenKind::Not => "not",
        TokenKind::And => "and",
        TokenKind::Or => "or",
        TokenKind::True => "true",
        TokenKind::False => "false",
        TokenKind::None => "none",
        TokenKind::SelfVal => "self",
        TokenKind::Do => "do",
        TokenKind::Discard => "_",
        TokenKind::Plus => "+",
        TokenKind::Minus => "-",
        TokenKind::Star => "*",
        TokenKind::Slash => "/",
        TokenKind::SlashSlash => "//",
        TokenKind::Percent => "%",
        TokenKind::PlusEq => "+=",
        TokenKind::MinusEq => "-=",
        TokenKind::StarEq => "*=",
        TokenKind::SlashEq => "/=",
        TokenKind::SlashSlashEq => "//=",
        TokenKind::PercentEq => "%=",
        TokenKind::EqualEqual => "==",
        TokenKind::BangEqual => "!=",
        TokenKind::Less => "<",
        TokenKind::LessEqual => "<=",
        TokenKind::Greater => ">",
        TokenKind::GreaterEqual => ">=",
        TokenKind::Equal => "=",
        TokenKind::FatArrow => "=>",
        TokenKind::Pipeline => "|>",
        TokenKind::DotDot => "..",
        TokenKind::Dot => ".",
        TokenKind::QuestionDot => "?.",
        TokenKind::Bang => "!",
        TokenKind::Question => "?",
        TokenKind::Colon => ":",
        TokenKind::ColonColon => "::",
        TokenKind::Comma => ",",
        TokenKind::LParen => "(",
        TokenKind::RParen => ")",
        TokenKind::LBracket => "[",
        TokenKind::RBracket => "]",
        TokenKind::LBrace => "{",
        TokenKind::RBrace => "}",
        TokenKind::Newline => "\n",
        // Literal-bearing kinds are handled by `text_of`.
        TokenKind::Identifier(_)
        | TokenKind::IntLiteral(_)
        | TokenKind::FloatLiteral(_)
        | TokenKind::StringLiteral { .. }
        | TokenKind::Comment(_)
        | TokenKind::Directive(_)
        | TokenKind::Eof
        | TokenKind::Error(_) => "",
    }
}

/// Returns the nesting effect of a delimiter token on the currently open group.
#[must_use]
pub fn group_delta(kind: &TokenKind) -> i32 {
    match kind {
        TokenKind::LParen | TokenKind::LBracket => 1,
        TokenKind::RParen | TokenKind::RBracket => -1,
        _ => 0,
    }
}

/// Renders a lexical error for the formatter's `Result`.
#[must_use]
pub fn describe_prefix(prefix: &StringPrefix) -> &'static str {
    match prefix {
        StringPrefix::Normal => "",
        StringPrefix::Format => "f",
        StringPrefix::Raw => "r",
        StringPrefix::FormatRaw => "fr",
    }
}
