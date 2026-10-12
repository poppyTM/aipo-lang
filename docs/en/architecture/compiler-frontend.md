# Compiler Frontend

The Aipo frontend ingests raw UTF-8 source text and produces a validated semantic representation.

---

## Pipeline Stages

### 1. `aipo-source`
- Encapsulates source files and in-memory buffers.
- Delivers UTF-8 boundary-safe line and column offsets (`is_char_boundary`), ensuring strict MSRV compatibility (Rust 1.85+).

### 2. `aipo-lexer`
- Converts the UTF-8 character stream into a contiguous sequence of tokens.
- Zero-copy optimizations for keywords and operators via direct pattern matching over `&str`.
- Canonical rejection of malformed floating-point literals (such as `1.e5` or `1._5`), generating the diagnostic `AIPO_LEX_INVALID_NUMBER`.

### 3. `aipo-syntax` & `aipo-ast`
- Recursive-descent parser with resilient panic-mode synchronization recovery (`synchronize`).
- Preserves `async` annotations in `interface` method contracts.
- Parser desugaring of chained comparisons (e.g., `val >= 0 and <= 100`).
- First-class support for inline ternary conditionals (`if c then a else b`).

### 4. `aipo-hir`
- Lowers the AST into High-Level Intermediate Representation (HIR).
- Unifies source spans across lifecycle hooks (`init` and `invariant`).
- Resolves local lexical bindings and top-level module scope identifiers.

### 5. `aipo-sema`
- Semantic validator enforcing strict structural typing and contract safety:
  - Enforces `interface` contracts via automatic structural subtyping (validating required methods, parameter arities, `self` vs `var self` receiver mutability, and type compatibility).
  - Statically disallows reassigning immutable struct fields (`AIPO_SEM_FIXED_REASSIGN`).
  - Emits concurrency diagnostics (`AIPO_SEM_AWAIT_IN_SUBEXPRESSION`, `AIPO_SEM_FORGOTTEN_TASK`, `AIPO_SEM_NESTED_AWAIT_DO`).
