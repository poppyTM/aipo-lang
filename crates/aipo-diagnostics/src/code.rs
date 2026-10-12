//! Stable diagnostic codes catalog for Aipo.
//!
//! Defined in normative document `docs/diagnostics/catalog.md`.
//! Codes are added and never repurposed or removed.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::severity::Severity;

/// Stable diagnostic code identifying the error, fault, or warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub enum DiagnosticCode {
    /// Experimental register backend cannot represent this program safely.
    AIPO_COMPILE_REG_UNSUPPORTED,

    // --- Source (AIPO_SRC_*) ---
    /// File bytes are not valid UTF-8.
    AIPO_SRC_INVALID_UTF8,

    // --- Lexical (AIPO_LEX_*) ---
    /// String literal was never closed before EOF.
    AIPO_LEX_UNTERMINATED_STRING,
    /// Unknown escape sequence in string literal.
    AIPO_LEX_UNKNOWN_ESCAPE,
    /// Unicode escape `\u{...}` outside valid Unicode scalar range.
    AIPO_LEX_INVALID_UNICODE_ESCAPE,
    /// Malformed numeric literal.
    AIPO_LEX_INVALID_NUMBER,
    /// Unexpected character that does not start any valid token.
    AIPO_LEX_UNEXPECTED_CHARACTER,

    // --- Parse (AIPO_PARSE_*) ---
    /// Block, parenthesis, or brace not closed before EOF.
    AIPO_PARSE_UNCLOSED_BLOCK,
    /// Token cannot start or continue this syntactic construct.
    AIPO_PARSE_UNEXPECTED_TOKEN,
    /// Block opened but closing `end` keyword is missing.
    AIPO_PARSE_MISSING_END,
    /// Assignment target is not a valid mutable path.
    AIPO_PARSE_INVALID_TARGET,
    /// Nesting exceeds the parser recursion bound (robustness limit, not syntax).
    AIPO_PARSE_NESTING_TOO_DEEP,
    /// Invalid batch association syntax with `::`.
    AIPO_PARSE_LOTE_INVALIDO,

    // --- Semantic (AIPO_SEM_*) ---
    /// Identifier could not be resolved in the current lexical scope.
    AIPO_SEM_UNKNOWN_NAME,
    /// Identifier redeclared within the same lexical scope.
    AIPO_SEM_REDECLARED_IN_SCOPE,
    /// Attempted mutation through an immutable binding or path.
    AIPO_SEM_READONLY_MUTATION,
    /// Assignment to a `fixed` field after struct construction.
    AIPO_SEM_FIXED_REASSIGN,
    /// Incompatible number of arguments in function call.
    AIPO_SEM_ARITY_MISMATCH,
    /// Named argument does not match any parameter or field name.
    AIPO_SEM_NAMED_ARG_UNKNOWN,
    /// Duplicate named argument in function call.
    AIPO_SEM_DUPLICATE_NAMED_ARG,
    /// Incompatible mixing of value and no-result return in function.
    AIPO_SEM_RETURN_VALUE_MISMATCH,
    /// Path in value-producing function ends without returning a value.
    AIPO_SEM_PATH_MISSING_RETURN_VALUE,
    /// Condition in `if`, `while`, `and`, `or`, or `not` is not a Bool.
    AIPO_SEM_NON_BOOL_CONDITION,
    /// Match over an enum has uncovered variants without a fallback `else` arm.
    AIPO_SEM_NON_EXHAUSTIVE_MATCH,
    /// Circular dependency detected in module imports.
    AIPO_SEM_IMPORT_CYCLE,
    /// Module import path could not be resolved.
    AIPO_SEM_UNKNOWN_MODULE,
    /// Exported symbol does not exist in the module.
    AIPO_SEM_EXPORT_UNKNOWN,
    /// Statically provable contract violation at call site.
    AIPO_SEM_CONTRACT_VIOLATION_STATIC,
    /// Explicit `await` outside statement, initializer or return position.
    AIPO_SEM_AWAIT_IN_SUBEXPRESSION,
    /// Known-`Task` value discarded without await or group combinator.
    AIPO_SEM_FORGOTTEN_TASK,
    /// Redundant nested `await do` block.
    AIPO_SEM_NESTED_AWAIT_DO,
    /// Parametric `Name[Args]` contract before parametric contracts exist.
    AIPO_SEM_PARAMETRIC_CONTRACT,
    /// Item is deprecated and its use emits a warning.
    AIPO_SEM_DEPRECATED,
    /// Item is marked with #!todo as pending work.
    AIPO_SEM_TODO,
    /// Method name resembles a hook (`constructor`, `validar`, etc.).
    AIPO_SEM_NOME_DE_HOOK,
    /// Invariant hook declared with an empty body.
    AIPO_SEM_HOOK_VAZIO,
    /// Hook (`init` or `invariant`) declared more than once on the same type.
    AIPO_SEM_HOOK_DUPLICADO,

    // --- Package (AIPO_PKG_*) ---
    /// Package manifest, dependency graph, or capability resolution failed.
    AIPO_PKG_RESOLUTION,
    /// Package lockfile is missing, invalid, or does not match the resolved graph.
    AIPO_PKG_LOCK_STALE,
    /// Public package fetch failed or returned an invalid artifact.
    AIPO_PKG_FETCH,

    // --- Runtime fault (AIPO_RT_*) ---
    /// Integer arithmetic exceeded range ±(2^53 - 1).
    AIPO_RT_OVERFLOW,
    /// Floating-point operation resulted in NaN or Infinity.
    AIPO_RT_NON_FINITE_FLOAT,
    /// Division or modulo by zero.
    AIPO_RT_DIV_ZERO,
    /// Index out of range for collection or string.
    AIPO_RT_INDEX_OUT_OF_RANGE,
    /// Strict dictionary access missing requested key.
    AIPO_RT_KEY_NOT_FOUND,
    /// Attempted to invoke a value that is not callable.
    AIPO_RT_NOT_CALLABLE,
    /// Collection structurally mutated during `each` iteration.
    AIPO_RT_MUTATION_DURING_ITERATION,
    /// Runtime type mismatch or contract boundary violation.
    AIPO_RT_TYPE_MISMATCH,
    /// Cancelled task driven or awaited.
    AIPO_RT_CANCELLED,
    /// Task awaiting itself, directly or transitively.
    AIPO_RT_AWAIT_CYCLE,
    /// Blocking operation (`await`, `sleep`, join) inside a synchronous
    /// callback driven by `invoke`: the host Rust stack cannot suspend.
    AIPO_RT_AWAIT_IN_CALLBACK,
    /// Host operation attempted without the capability it requires.
    AIPO_RT_CAPABILITY_DENIED,
    /// Host handle addressed after its slot was released or reused.
    AIPO_RT_STALE_HANDLE,
    /// Scoped host binding reached a heap-publication point outside its scope.
    AIPO_RT_SCOPE_ESCAPE,
    /// Module top-level evaluation failed during initialization.
    AIPO_RT_MODULE_INIT_FAILED,
    /// Host surface description is internally inconsistent.
    AIPO_RT_INVALID_SCHEMA,

    // --- Runtime failure (AIPO_RT_FAILURE_*) ---
    /// Uncaught failure value reached top level.
    AIPO_RT_FAILURE_UNCAUGHT,
}

impl DiagnosticCode {
    /// Returns the stable string identifier for this diagnostic code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AIPO_COMPILE_REG_UNSUPPORTED => "AIPO_COMPILE_REG_UNSUPPORTED",
            Self::AIPO_SRC_INVALID_UTF8 => "AIPO_SRC_INVALID_UTF8",
            Self::AIPO_LEX_UNTERMINATED_STRING => "AIPO_LEX_UNTERMINATED_STRING",
            Self::AIPO_LEX_UNKNOWN_ESCAPE => "AIPO_LEX_UNKNOWN_ESCAPE",
            Self::AIPO_LEX_INVALID_UNICODE_ESCAPE => "AIPO_LEX_INVALID_UNICODE_ESCAPE",
            Self::AIPO_LEX_INVALID_NUMBER => "AIPO_LEX_INVALID_NUMBER",
            Self::AIPO_LEX_UNEXPECTED_CHARACTER => "AIPO_LEX_UNEXPECTED_CHARACTER",
            Self::AIPO_PARSE_UNCLOSED_BLOCK => "AIPO_PARSE_UNCLOSED_BLOCK",
            Self::AIPO_PARSE_UNEXPECTED_TOKEN => "AIPO_PARSE_UNEXPECTED_TOKEN",
            Self::AIPO_PARSE_MISSING_END => "AIPO_PARSE_MISSING_END",
            Self::AIPO_PARSE_INVALID_TARGET => "AIPO_PARSE_INVALID_TARGET",
            Self::AIPO_PARSE_NESTING_TOO_DEEP => "AIPO_PARSE_NESTING_TOO_DEEP",
            Self::AIPO_PARSE_LOTE_INVALIDO => "AIPO_PARSE_LOTE_INVALIDO",
            Self::AIPO_SEM_UNKNOWN_NAME => "AIPO_SEM_UNKNOWN_NAME",
            Self::AIPO_SEM_REDECLARED_IN_SCOPE => "AIPO_SEM_REDECLARED_IN_SCOPE",
            Self::AIPO_SEM_READONLY_MUTATION => "AIPO_SEM_READONLY_MUTATION",
            Self::AIPO_SEM_FIXED_REASSIGN => "AIPO_SEM_FIXED_REASSIGN",
            Self::AIPO_SEM_ARITY_MISMATCH => "AIPO_SEM_ARITY_MISMATCH",
            Self::AIPO_SEM_NAMED_ARG_UNKNOWN => "AIPO_SEM_NAMED_ARG_UNKNOWN",
            Self::AIPO_SEM_DUPLICATE_NAMED_ARG => "AIPO_SEM_DUPLICATE_NAMED_ARG",
            Self::AIPO_SEM_RETURN_VALUE_MISMATCH => "AIPO_SEM_RETURN_VALUE_MISMATCH",
            Self::AIPO_SEM_PATH_MISSING_RETURN_VALUE => "AIPO_SEM_PATH_MISSING_RETURN_VALUE",
            Self::AIPO_SEM_NON_BOOL_CONDITION => "AIPO_SEM_NON_BOOL_CONDITION",
            Self::AIPO_SEM_NON_EXHAUSTIVE_MATCH => "AIPO_SEM_NON_EXHAUSTIVE_MATCH",
            Self::AIPO_SEM_IMPORT_CYCLE => "AIPO_SEM_IMPORT_CYCLE",
            Self::AIPO_SEM_UNKNOWN_MODULE => "AIPO_SEM_UNKNOWN_MODULE",
            Self::AIPO_SEM_EXPORT_UNKNOWN => "AIPO_SEM_EXPORT_UNKNOWN",
            Self::AIPO_SEM_CONTRACT_VIOLATION_STATIC => "AIPO_SEM_CONTRACT_VIOLATION_STATIC",
            Self::AIPO_SEM_AWAIT_IN_SUBEXPRESSION => "AIPO_SEM_AWAIT_IN_SUBEXPRESSION",
            Self::AIPO_SEM_FORGOTTEN_TASK => "AIPO_SEM_FORGOTTEN_TASK",
            Self::AIPO_SEM_NESTED_AWAIT_DO => "AIPO_SEM_NESTED_AWAIT_DO",
            Self::AIPO_SEM_PARAMETRIC_CONTRACT => "AIPO_SEM_PARAMETRIC_CONTRACT",
            Self::AIPO_SEM_DEPRECATED => "AIPO_SEM_DEPRECATED",
            Self::AIPO_SEM_TODO => "AIPO_SEM_TODO",
            Self::AIPO_SEM_NOME_DE_HOOK => "AIPO_SEM_NOME_DE_HOOK",
            Self::AIPO_SEM_HOOK_VAZIO => "AIPO_SEM_HOOK_VAZIO",
            Self::AIPO_SEM_HOOK_DUPLICADO => "AIPO_SEM_HOOK_DUPLICADO",
            Self::AIPO_PKG_RESOLUTION => "AIPO_PKG_RESOLUTION",
            Self::AIPO_PKG_LOCK_STALE => "AIPO_PKG_LOCK_STALE",
            Self::AIPO_PKG_FETCH => "AIPO_PKG_FETCH",
            Self::AIPO_RT_OVERFLOW => "AIPO_RT_OVERFLOW",
            Self::AIPO_RT_NON_FINITE_FLOAT => "AIPO_RT_NON_FINITE_FLOAT",
            Self::AIPO_RT_DIV_ZERO => "AIPO_RT_DIV_ZERO",
            Self::AIPO_RT_INDEX_OUT_OF_RANGE => "AIPO_RT_INDEX_OUT_OF_RANGE",
            Self::AIPO_RT_KEY_NOT_FOUND => "AIPO_RT_KEY_NOT_FOUND",
            Self::AIPO_RT_NOT_CALLABLE => "AIPO_RT_NOT_CALLABLE",
            Self::AIPO_RT_MUTATION_DURING_ITERATION => "AIPO_RT_MUTATION_DURING_ITERATION",
            Self::AIPO_RT_TYPE_MISMATCH => "AIPO_RT_TYPE_MISMATCH",
            Self::AIPO_RT_CANCELLED => "AIPO_RT_CANCELLED",
            Self::AIPO_RT_AWAIT_CYCLE => "AIPO_RT_AWAIT_CYCLE",
            Self::AIPO_RT_AWAIT_IN_CALLBACK => "AIPO_RT_AWAIT_IN_CALLBACK",
            Self::AIPO_RT_CAPABILITY_DENIED => "AIPO_RT_CAPABILITY_DENIED",
            Self::AIPO_RT_STALE_HANDLE => "AIPO_RT_STALE_HANDLE",
            Self::AIPO_RT_SCOPE_ESCAPE => "AIPO_RT_SCOPE_ESCAPE",
            Self::AIPO_RT_MODULE_INIT_FAILED => "AIPO_RT_MODULE_INIT_FAILED",
            Self::AIPO_RT_INVALID_SCHEMA => "AIPO_RT_INVALID_SCHEMA",
            Self::AIPO_RT_FAILURE_UNCAUGHT => "AIPO_RT_FAILURE_UNCAUGHT",
        }
    }

    /// Returns the default severity level for this diagnostic code.
    #[must_use]
    pub const fn default_severity(self) -> Severity {
        match self {
            Self::AIPO_COMPILE_REG_UNSUPPORTED
            | Self::AIPO_SRC_INVALID_UTF8
            | Self::AIPO_LEX_UNTERMINATED_STRING
            | Self::AIPO_LEX_UNKNOWN_ESCAPE
            | Self::AIPO_LEX_INVALID_UNICODE_ESCAPE
            | Self::AIPO_LEX_INVALID_NUMBER
            | Self::AIPO_LEX_UNEXPECTED_CHARACTER
            | Self::AIPO_PARSE_UNCLOSED_BLOCK
            | Self::AIPO_PARSE_UNEXPECTED_TOKEN
            | Self::AIPO_PARSE_MISSING_END
            | Self::AIPO_PARSE_INVALID_TARGET
            | Self::AIPO_PARSE_NESTING_TOO_DEEP
            | Self::AIPO_PARSE_LOTE_INVALIDO
            | Self::AIPO_SEM_UNKNOWN_NAME
            | Self::AIPO_SEM_REDECLARED_IN_SCOPE
            | Self::AIPO_SEM_READONLY_MUTATION
            | Self::AIPO_SEM_FIXED_REASSIGN
            | Self::AIPO_SEM_ARITY_MISMATCH
            | Self::AIPO_SEM_NAMED_ARG_UNKNOWN
            | Self::AIPO_SEM_DUPLICATE_NAMED_ARG
            | Self::AIPO_SEM_RETURN_VALUE_MISMATCH
            | Self::AIPO_SEM_PATH_MISSING_RETURN_VALUE
            | Self::AIPO_SEM_NON_BOOL_CONDITION
            | Self::AIPO_SEM_NON_EXHAUSTIVE_MATCH
            | Self::AIPO_SEM_IMPORT_CYCLE
            | Self::AIPO_SEM_UNKNOWN_MODULE
            | Self::AIPO_SEM_EXPORT_UNKNOWN
            | Self::AIPO_SEM_CONTRACT_VIOLATION_STATIC
            | Self::AIPO_SEM_AWAIT_IN_SUBEXPRESSION
            | Self::AIPO_SEM_FORGOTTEN_TASK
            | Self::AIPO_SEM_NESTED_AWAIT_DO
            | Self::AIPO_SEM_PARAMETRIC_CONTRACT
            | Self::AIPO_SEM_HOOK_DUPLICADO
            | Self::AIPO_PKG_RESOLUTION
            | Self::AIPO_PKG_LOCK_STALE
            | Self::AIPO_PKG_FETCH
            | Self::AIPO_RT_FAILURE_UNCAUGHT => Severity::Error,

            Self::AIPO_SEM_DEPRECATED
            | Self::AIPO_SEM_TODO
            | Self::AIPO_SEM_NOME_DE_HOOK
            | Self::AIPO_SEM_HOOK_VAZIO => Severity::Warning,

            Self::AIPO_RT_OVERFLOW
            | Self::AIPO_RT_NON_FINITE_FLOAT
            | Self::AIPO_RT_DIV_ZERO
            | Self::AIPO_RT_INDEX_OUT_OF_RANGE
            | Self::AIPO_RT_KEY_NOT_FOUND
            | Self::AIPO_RT_NOT_CALLABLE
            | Self::AIPO_RT_MUTATION_DURING_ITERATION
            | Self::AIPO_RT_TYPE_MISMATCH
            | Self::AIPO_RT_CANCELLED
            | Self::AIPO_RT_AWAIT_CYCLE
            | Self::AIPO_RT_AWAIT_IN_CALLBACK
            | Self::AIPO_RT_CAPABILITY_DENIED
            | Self::AIPO_RT_STALE_HANDLE
            | Self::AIPO_RT_SCOPE_ESCAPE
            | Self::AIPO_RT_MODULE_INIT_FAILED
            | Self::AIPO_RT_INVALID_SCHEMA => Severity::Fault,
        }
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.as_str())
    }
}
