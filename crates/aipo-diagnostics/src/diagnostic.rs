//! Diagnostic model, primary span resolution, notes, suggestions, and JSONL emission.

use aipo_source::{Source, SourceLocation, SourceMap, SourceSpan};
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::code::DiagnosticCode;
use crate::severity::Severity;

/// Position details for the primary span of a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrimarySpan {
    /// File name or path of the source.
    pub file: String,
    /// Byte start offset (inclusive).
    pub start: usize,
    /// Byte end offset (exclusive).
    pub end: usize,
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number.
    pub column: usize,
}

impl PrimarySpan {
    /// Resolves a `SourceSpan` against a `Source` to create a `PrimarySpan`.
    #[must_use]
    pub fn from_source(source: &Source, span: SourceSpan) -> Self {
        let loc = source.location(span.start).unwrap_or(SourceLocation {
            line: 1,
            column: 1,
            offset: span.start,
        });

        Self {
            file: source.name().to_string(),
            start: span.start,
            end: span.end,
            line: loc.line,
            column: loc.column,
        }
    }
}

/// A code fix suggestion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suggestion {
    /// Descriptive explanation of the suggested fix.
    pub message: String,
    /// The replacement code text.
    pub replacement: String,
    /// Start byte offset to replace.
    pub start: usize,
    /// End byte offset to replace.
    pub end: usize,
}

/// Structured diagnostic representation consumed by IDEs, agents, and CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// The unique stable diagnostic code.
    pub code: DiagnosticCode,
    /// Severity of the diagnostic.
    pub severity: Severity,
    /// Human-readable explanation.
    pub message: String,
    /// Primary location in source text, if applicable.
    pub primary_span: Option<PrimarySpan>,
    /// Additional contextual notes.
    pub notes: Vec<String>,
    /// Automated or suggested fixes.
    pub suggestions: Vec<Suggestion>,
}

impl Diagnostic {
    /// Creates a new error diagnostic.
    #[must_use]
    pub fn error(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: Severity::Error,
            message: message.into(),
            primary_span: None,
            notes: Vec::new(),
            suggestions: Vec::new(),
        }
    }

    /// Creates a new runtime fault diagnostic.
    #[must_use]
    pub fn fault(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: Severity::Fault,
            message: message.into(),
            primary_span: None,
            notes: Vec::new(),
            suggestions: Vec::new(),
        }
    }

    /// Creates a new warning diagnostic.
    #[must_use]
    pub fn warning(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: Severity::Warning,
            message: message.into(),
            primary_span: None,
            notes: Vec::new(),
            suggestions: Vec::new(),
        }
    }

    /// Attaches primary span information resolved from a `Source`.
    #[must_use]
    pub fn with_primary_span(mut self, source: &Source, span: SourceSpan) -> Self {
        self.primary_span = Some(PrimarySpan::from_source(source, span));
        self
    }

    /// Attaches an already resolved `PrimarySpan`.
    #[must_use]
    pub fn with_resolved_span(mut self, primary_span: PrimarySpan) -> Self {
        self.primary_span = Some(primary_span);
        self
    }

    /// Appends a contextual note.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// Appends a code replacement suggestion.
    #[must_use]
    pub fn with_suggestion(mut self, suggestion: Suggestion) -> Self {
        self.suggestions.push(suggestion);
        self
    }

    /// Serializes this diagnostic into a compact, single-line JSONL format.
    pub fn to_jsonl(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Formats the diagnostic as a human-readable compiler error string with source snippet.
    #[must_use]
    pub fn render_human(&self, source_map: Option<&SourceMap>) -> String {
        self.render_human_with_locale(source_map, crate::locale::Locale::detect())
    }

    /// Formats the diagnostic as a human-readable compiler error string using the specified locale.
    #[must_use]
    pub fn render_human_with_locale(
        &self,
        source_map: Option<&SourceMap>,
        locale: crate::locale::Locale,
    ) -> String {
        let mut out = String::new();
        let code_str = self.code.as_str();
        let severity_str = locale.severity_label(self.severity);

        if let Some(span) = &self.primary_span {
            out.push_str(&format!(
                "{}:{}:{}: {}: [{}] {}\n",
                span.file, span.line, span.column, severity_str, code_str, self.message
            ));

            if let Some(map) = source_map
                && let Some(source) = map.get_by_name(&span.file)
                && let Some(line_str) = source.line_content(span.line)
            {
                out.push_str(&format!(" {:>4} | {}\n", span.line, line_str));
                let pad = " ".repeat(span.column.saturating_sub(1));
                let carets = "^".repeat((span.end.saturating_sub(span.start)).max(1));
                out.push_str(&format!("      | {pad}{carets}\n"));
            }
        } else {
            out.push_str(&format!("{severity_str}: [{code_str}] {}\n", self.message));
        }

        let note_lbl = locale.note_label();
        for note in &self.notes {
            out.push_str(&format!("  = {note_lbl}: {note}\n"));
        }

        let help_lbl = locale.help_label();
        for sug in &self.suggestions {
            out.push_str(&format!(
                "  = {help_lbl}: {}: {}\n",
                sug.message, sug.replacement
            ));
        }

        out
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.render_human(None))
    }
}
