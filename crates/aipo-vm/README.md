# aipo-vm

`aipo-vm` implements the stack-based virtual machine interpreter for Aipo, executing verified bytecode modules.

## Architecture & Guarantees
- **Value Model (`Value`)**: Dynamic values supporting `None`, `Bool`, `Int` within ±(2^53 - 1), finite `Float`, `String`, ordered `List`, insertion-ordered `Dict`, `Struct` with field mutability tracking, and `Failure` (Model B).
- **Safe Numeric Bounds**:
  - `Int`: Strict range ±(2^53 - 1) (`MAX_SAFE_INT: 9_007_199_254_740_991`). Arithmetic overflow triggers non-recoverable runtime fault `AIPO_RT_OVERFLOW`.
  - `Float`: Finite-only. Operations producing NaN or Infinity trigger `AIPO_RT_NON_FINITE_FLOAT`.
  - `Division`: Division or modulo by zero triggers `AIPO_RT_DIV_ZERO`.
- **Call Frames & Execution**:
  - `CallFrame`: Activation frames tracking return address and stack base offsets.
  - Opcodes: Constant loading, variable get/set (local and global), arithmetic, comparisons, jumps, function calls, and returns.
- **Data & Collections**:
  - `List`: Ordered sequence supporting negative indexing (e.g. `-1` for last element) and bounds checking (`AIPO_RT_INDEX_OUT_OF_RANGE`).
  - `Dict`: Key-value map preserving insertion order.
  - `Struct`: Named instances with immutability by default, `var` mutability tracking, and post-mutation `invariant` validation.
- **Error Model (Model B)**:
  - `Failure`: Recoverable error value created via `fail(message)` with mandatory `.message` property, automatically propagating across unhandled expressions and calls.
  - Recovery: `attempt { ... } failed err { ... }` blocks register `HandlerFrame` to catch failures; `or_else` provides expression-level fallback.
  - Runtime Faults: Unrecoverable programming/contract violations (`VmFault`) that bypass `attempt` handlers and halt execution.
- **Safe Rust Embedding Engine (`AipoEngine` & `AipoScope`)**:
  - `AipoEngineBuilder`: Configures deny-by-default capabilities (`CapabilitySet`) and instruction budgets (`limits`).
  - `AipoScope`: RAII-bounded execution context. Binds host userdata and generational handles to the scope's lifetime, ensuring automatic invalidation and drop on exit.
  - Generational UserData (`HandleTable<Box<dyn Any>>`): Host types lent to the VM resolve safely via downcast; attempted access post-scope triggers non-catchable `AIPO_RT_SCOPE_ESCAPE` or `AIPO_RT_STALE_HANDLE`.
- **Safety**: `#![forbid(unsafe_code)]`. Zero unhandled panics escaping as user-facing errors.

