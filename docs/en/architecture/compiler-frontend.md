# Compiler frontend

The Aipo frontend is responsible for processing plain-text source code up to the generation of the
validated semantic representation.

---

## Pipeline stages

### 1. `aipo-source`
- Represents the source file and its in-memory content.
- Provides line/column indexing safe with respect to UTF-8 boundaries (`is_char_boundary`),
  guaranteeing compatibility with the declared MSRV (Rust 1.96).

### 2. `aipo-lexer`
- Converts the UTF-8 character stream into a contiguous sequence of tokens.
- Zero-allocation optimization in keyword identification through direct matching on `&str`.
- Canonical rejection of malformed decimal numbers (such as `1.e5` or `1._5`), producing the
  static diagnostic `AIPO_LEX_INVALID_NUMBER`.

### 3. `aipo-syntax` & `aipo-ast`
- Recursive-descent parser with resilient error recovery (`synchronize`).
- Preservation of `async` annotations on `interface` methods.
- Desugaring in the parser of continuous comparisons (such as `val >= 0 and <= 100`).
- Support for inline conditional expressions (`if c then a else b`).

### 4. `aipo-hir`
- Lowering from AST to High-Level Intermediate Representation.
- Unification of spans in lifecycle hooks (`init` and `invariant`).
- Resolution of local and module scope references.

### 5. `aipo-sema`
- Semantic validator with strict type checking and contract verification:
  - Verification of `interface` contracts through automatic structural subtyping (required
    methods, arities, receiver mutability `self` vs `var self`, parameter and return types).
  - Static prohibition of mutating immutable fields of structures (`AIPO_SEM_FIXED_REASSIGN`).
  - Dedicated diagnostics for concurrency (`AIPO_SEM_AWAIT_IN_SUBEXPRESSION`,
    `AIPO_SEM_FORGOTTEN_TASK`, `AIPO_SEM_NESTED_AWAIT_DO`).
