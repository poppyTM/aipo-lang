//! `aipo-formatter` renders Aipo source in the canonical style.
//!
//! Canonical rules come from concrete canon (`Aipo V1 — Language Reference`,
//! `Aipo Language — Especificação Viva`):
//!
//! - indentation carries no semantic meaning and the official formatter uses **4 spaces
//!   per level**;
//! - blocks are explicitly closed with `end`, so indentation is derived from the block
//!   keywords themselves, never inferred from the input's original spacing;
//! - `import`/`export` may appear anywhere in the top level; the formatter only
//!   normalizes whitespace and never reorders declarations.
//!
//! The formatter is a whitespace normalizer over the **token stream**: token contents
//! (identifiers, literal spelling, string escapes, comments) are copied from the
//! original text, so formatting can never change a program's meaning or lose a comment.
//! Formatting is idempotent: `format(format(x)) == format(x)`.
//!
//! ```
//! use aipo_formatter::format_text;
//!
//! let formatted = format_text("main.aipo", "let x=1\nif x>0 {\nio.println(\"positive\")\n}")
//!     .expect("valid source formats");
//! assert_eq!(formatted, "let x = 1\nif x > 0 {\n    io.println(\"positive\")\n}\n");
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod tokens;

use aipo_lexer::{Lexer, Token, TokenKind};
use aipo_source::{Source, SourceId};
use std::fmt;
use tokens::{
    Class, classify, group_delta, is_branch_keyword, minus_is_prefix, opens_block, text_of,
};

/// Indentation width used by the canonical formatter.
pub const INDENT: &str = "    ";

/// Failure to format a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatError {
    /// Human-readable reason, including the source position when known.
    pub message: String,
    /// Byte offset of the offending token, when the failure is positional.
    pub offset: Option<usize>,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for FormatError {}

/// Formats Aipo source text, returning the canonical rendering.
///
/// # Errors
/// Returns [`FormatError`] when the input cannot be tokenized, so that `aipo fmt` never
/// rewrites a file it cannot fully understand.
pub fn format_text(name: &str, text: &str) -> Result<String, FormatError> {
    let source = Source::new(SourceId::next(), name, text);
    format_source(&source)
}

/// Formats an already-constructed [`Source`].
///
/// # Errors
/// Returns [`FormatError`] when the input cannot be tokenized.
pub fn format_source(source: &Source) -> Result<String, FormatError> {
    let (tokens, diagnostics) = Lexer::new(source).tokenize();
    if let Some(first) = diagnostics.first() {
        return Err(FormatError {
            message: first.message.clone(),
            offset: first.primary_span.as_ref().map(|span| span.start),
        });
    }

    let lines = split_lines(&tokens);
    Ok(render(source, &lines))
}

/// A logical source line together with the blank-line information carried by the
/// newline token that precedes it.
struct Line<'a> {
    tokens: Vec<&'a Token>,
    blank_before: bool,
}

fn split_lines<'a>(tokens: &'a [Token]) -> Vec<Line<'a>> {
    let mut lines: Vec<Line<'a>> = Vec::new();
    let mut current: Vec<&Token> = Vec::new();
    let mut pending_blank = false;
    let mut started = false;

    for token in tokens {
        match &token.kind {
            TokenKind::Eof => break,
            TokenKind::Newline => {
                // The lexer collapses runs of newlines into one token; the raw span tells
                // us whether the author left a blank line between statements.
                let width = token.span.end.saturating_sub(token.span.start);
                let newlines = width.max(1);
                if started && current.is_empty() && newlines >= 2 {
                    pending_blank = true;
                    continue;
                }
                if !current.is_empty() {
                    lines.push(Line {
                        tokens: std::mem::take(&mut current),
                        blank_before: pending_blank,
                    });
                    pending_blank = false;
                }
                if newlines >= 2 {
                    pending_blank = true;
                }
                started = true;
            }
            _ => {
                started = true;
                current.push(token);
            }
        }
    }

    if !current.is_empty() {
        lines.push(Line {
            tokens: current,
            blank_before: pending_blank,
        });
    }

    lines
}

fn render(source: &Source, lines: &[Line<'_>]) -> String {
    let mut out = String::new();
    let mut block_level: i64 = 0;
    let mut group_depth: i64 = 0;

    for (index, line) in lines.iter().enumerate() {
        let first = line
            .tokens
            .first()
            .map(|token| token.kind.clone())
            .unwrap_or(TokenKind::Eof);

        let starts_with_branch = is_branch_keyword(&first);
        // A branch keyword closes the branch opened by the previous `when`/`elif`/`else`/
        // `failed` before opening its own body.
        let closes_previous_branch = matches!(
            first,
            TokenKind::When | TokenKind::Elif | TokenKind::Else | TokenKind::Failed
        );
        let leading_rbraces = line
            .tokens
            .iter()
            .take_while(|t| matches!(t.kind, TokenKind::RBrace))
            .count() as i64;
        let starts_with_close = matches!(first, TokenKind::RParen | TokenKind::RBracket);

        let mut line_block = block_level;
        if starts_with_branch {
            line_block -= 1;
        }
        if leading_rbraces > 0 {
            line_block -= leading_rbraces;
        }
        let mut line_group = group_depth;
        if starts_with_close {
            line_group -= 1;
        }

        // An open group adds a single continuation level, not one per open bracket: a call
        // nested in a call is still written one step in, the way the block bodies inside it
        // are. Without this clamp, `f(g(fn () ... end))` would indent its body twice.
        let indent = (line_block.max(0) + line_group.clamp(0, 1)) as usize;
        let rendered = render_line(source, &line.tokens);

        if line.blank_before && index > 0 && !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str(&INDENT.repeat(indent));
        out.push_str(&rendered);
        out.push('\n');

        if closes_previous_branch {
            block_level -= 1;
        }

        for (position, token) in line.tokens.iter().enumerate() {
            group_depth += i64::from(group_delta(&token.kind));
            if matches!(token.kind, TokenKind::LBrace) {
                block_level += 1;
            }
            if matches!(token.kind, TokenKind::RBrace) {
                block_level -= 1;
            }
            if opens_block_here(&token.kind, position, &line.tokens) {
                block_level += 1;
            }
        }
    }

    while out.ends_with('\n') {
        out.pop();
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// `fn` opens a block except in a type annotation, where it names a function type.
fn opens_block_here(kind: &TokenKind, position: usize, tokens: &[&Token]) -> bool {
    if !opens_block(kind) {
        return false;
    }
    // If the line contains an opening brace `{`, the brace already manages the block level.
    if tokens.iter().any(|t| matches!(t.kind, TokenKind::LBrace)) {
        return false;
    }
    if matches!(kind, TokenKind::Fn) {
        let previous = if position == 0 {
            None
        } else {
            Some(&tokens[position - 1].kind)
        };
        if matches!(previous, Some(TokenKind::Colon)) {
            return false;
        }
    }
    true
}

fn render_line(source: &Source, tokens: &[&Token]) -> String {
    let mut out = String::new();
    let mut previous: Option<Class> = None;
    let mut previous_kind: Option<&TokenKind> = None;

    for token in tokens {
        let class = if matches!(token.kind, TokenKind::Minus) && minus_is_prefix(previous_kind) {
            Class::PrefixTight
        } else {
            classify(&token.kind)
        };

        if needs_space(previous, previous_kind, class, &token.kind, tokens) {
            out.push(' ');
        }
        out.push_str(&text_of(token, source));
        previous = Some(class);
        previous_kind = Some(&token.kind);
    }

    out
}

/// Canonical spacing rule between two adjacent tokens of a line.
fn needs_space(
    previous: Option<Class>,
    previous_kind: Option<&TokenKind>,
    next: Class,
    next_kind: &TokenKind,
    tokens: &[&Token],
) -> bool {
    let Some(previous) = previous else {
        return false;
    };

    // Nothing may follow a comment on the same logical line.
    if previous == Class::Comment {
        return false;
    }
    if next == Class::Comment {
        return true;
    }

    // Single-line non-empty braces `{ ... }` get inner spacing for visual clarity and accessibility,
    // while empty `{}` stays tight and trailing `{` before a newline has no trailing whitespace.
    if matches!(previous_kind, Some(TokenKind::LBrace)) {
        if matches!(next_kind, TokenKind::RBrace | TokenKind::Newline) {
            return false;
        }
        return true;
    }
    if matches!(next_kind, TokenKind::RBrace) {
        if matches!(previous_kind, Some(TokenKind::LBrace)) {
            return false;
        }
        return true;
    }

    // Tight on the left.
    if matches!(
        next,
        Class::Close | Class::Comma | Class::Colon | Class::Dot | Class::Postfix | Class::Range
    ) {
        return false;
    }

    // Tight on the right.
    if matches!(
        previous,
        Class::Dot | Class::Range | Class::Open | Class::PrefixTight
    ) {
        return false;
    }

    // `Tipo::[fn1, fn2]` batch binding is tight around `::`.
    if matches!(next_kind, TokenKind::ColonColon)
        && let Some(TokenKind::Identifier(name)) = previous_kind
        && name.chars().next().is_some_and(|c| c.is_uppercase())
    {
        return false;
    }
    if matches!(previous_kind, Some(TokenKind::ColonColon))
        && matches!(next_kind, TokenKind::LBracket)
    {
        return false;
    }

    // `Tipo:nome(...)` method/hook declaration is tight after `:`.
    if matches!(previous_kind, Some(TokenKind::Colon))
        && tokens.len() >= 2
        && matches!(&tokens[0].kind, TokenKind::Identifier(name) if name.chars().next().is_some_and(|c| c.is_uppercase()))
        && matches!(&tokens[1].kind, TokenKind::Colon)
    {
        return false;
    }

    // `->` return type arrow is tight between `-` and `>`.
    if matches!(previous_kind, Some(TokenKind::Minus)) && matches!(next_kind, TokenKind::Greater) {
        return false;
    }

    if matches!(next, Class::Open) {
        let binds_to_previous = match next_kind {
            TokenKind::LParen => matches!(
                previous_kind,
                Some(
                    TokenKind::Identifier(_)
                        | TokenKind::Init
                        | TokenKind::Invariant
                        | TokenKind::SelfVal
                        | TokenKind::RParen
                        | TokenKind::RBracket
                )
            ),
            TokenKind::LBracket => matches!(
                previous_kind,
                Some(
                    TokenKind::Identifier(_)
                        | TokenKind::StringLiteral { .. }
                        | TokenKind::SelfVal
                        | TokenKind::RParen
                        | TokenKind::RBracket
                )
            ),
            TokenKind::LBrace => {
                // A brace closing the physical line opens a body (`if v is Int {`),
                // never a construction: it always takes a space. Only a brace with
                // more tokens after it on the same line can be `Type{...}`.
                let brace_is_last = tokens
                    .iter()
                    .rposition(|t| matches!(t.kind, TokenKind::LBrace))
                    .is_some_and(|idx| {
                        tokens[idx + 1..]
                            .iter()
                            .all(|t| matches!(t.kind, TokenKind::Newline))
                    });
                if brace_is_last {
                    return true;
                }
                let is_decl = tokens
                    .first()
                    .map(|t| {
                        matches!(
                            t.kind,
                            TokenKind::Struct | TokenKind::Enum | TokenKind::Interface
                        )
                    })
                    .unwrap_or(false);
                let has_arrow = tokens.windows(2).any(|w| {
                    matches!(
                        (&w[0].kind, &w[1].kind),
                        (TokenKind::Minus, TokenKind::Greater)
                    )
                });
                if !is_decl && !has_arrow {
                    if let Some(TokenKind::Identifier(name)) = previous_kind {
                        name.chars().next().is_some_and(|c| c.is_uppercase())
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            _ => false,
        };
        // `f(x)`, `Type{...}` and `items[0]` are tight; `if (cond)` and `= (` are not.
        return !binds_to_previous;
    }

    if matches!(previous, Class::Comma | Class::Colon | Class::Operator) {
        return true;
    }

    if matches!(next_kind, TokenKind::Newline) {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(text: &str) -> String {
        format_text("test.aipo", text).expect("source formats")
    }

    #[test]
    fn test_reindents_block_with_four_spaces() {
        let out = format("if x > 0 {\nio.println(\"yes\")\n}");
        assert_eq!(out, "if x > 0 {\n    io.println(\"yes\")\n}\n");
    }

    #[test]
    fn test_normalizes_operator_spacing() {
        let out = format("let total=a+b*2");
        assert_eq!(out, "let total = a + b * 2\n");
    }

    #[test]
    fn test_preserves_comments_and_strings() {
        let out = format("let path = r\"C:\\tmp\" # keep me\n");
        assert_eq!(out, "let path = r\"C:\\tmp\" # keep me\n");
    }

    #[test]
    fn test_is_idempotent() {
        let once = format("fn add(a,b) {\nreturn a+b\n}\nif true {\nlet x = [1,2,3]\n}");
        let twice = format(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn test_match_branches_align_with_match() {
        let out = format(
            "match status {\nwhen \"a\"\nio.print(1)\nwhen \"b\"\nio.print(2)\nelse\nio.print(3)\n}",
        );
        assert_eq!(
            out,
            "match status {\nwhen \"a\"\n    io.print(1)\nwhen \"b\"\n    io.print(2)\nelse\n    io.print(3)\n}\n"
        );
    }

    #[test]
    fn test_reports_unterminated_string() {
        let err = format_text("test.aipo", "let x = \"unterminated").unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_modern_brace_blocks_and_functions() {
        let out = format(
            "fn calculate(a, b) {\nif a > b {\nreturn a // b\n} else {\nreturn b // a\n}\n}",
        );
        assert_eq!(
            out,
            "fn calculate(a, b) {\n    if a > b {\n        return a // b\n    } else {\n        return b // a\n    }\n}\n"
        );
    }

    #[test]
    fn test_modern_struct_and_construct() {
        let out = format(
            "struct User {\nid\nvar status\n}\n\nlet dev = User{ id: 42, status: \"active\" }",
        );
        assert_eq!(
            out,
            "struct User {\n    id\n    var status\n}\n\nlet dev = User{ id: 42, status: \"active\" }\n"
        );
    }

    #[test]
    fn test_modern_method_with_var_self() {
        let out = format("User:update(var self, new_status) {\nself.status = new_status\n}");
        assert_eq!(
            out,
            "User:update(var self, new_status) {\n    self.status = new_status\n}\n"
        );
    }

    #[test]
    fn test_format_type_association_and_batch_and_enum() {
        let code = "Point:dist() -> Float {\nreturn 0.0\n}\n\nPoint:init(x, y) {\nself.x = x\n}\n\nPonto::[area, perimetro]\n\nenum Status {\nAtivo,\nInativo,\n}\n";
        let out = format(code);
        assert_eq!(
            out,
            "Point:dist() -> Float {\n    return 0.0\n}\n\nPoint:init(x, y) {\n    self.x = x\n}\n\nPonto::[area, perimetro]\n\nenum Status {\n    Ativo,\n    Inativo,\n}\n"
        );
    }
}
