# aipo-testkit

`aipo-testkit` provides shared deterministic test harnesses, generators, execution pipelines, and leak canaries for the Aipo workspace.

## Modules & Capabilities

- **`pipeline`**: In-process execution pipeline runners (`check_text`, `compile_text`, `run_capture`) connecting parsing, HIR lowering, semantic checking, bytecode emission, and VM execution.
- **`corpus`**: Canonical test programs and snapshot suites for cross-backend differential validation.
- **`smith`**: Deterministic grammar-based program generator for fuzzing and stress testing language semantics.
- **`rng` & `timez`**: Seeded pseudo-random number generator and virtual deterministic clock abstraction.
- **`proc` & `meta`**: Process isolation, output capturing, and execution metadata verification.
- **`leak_canaries` (`tests/leak_canaries.rs`)**: Global counting allocator (`CountingAllocator`) monitoring heap allocation delta across lifecycle churn (VM, closures, register VM, frontend analysis, bytecode compilation, Wasm compilation, generational handles, and scoped engine embedding) ensuring net-zero memory retention.

## Guarantees

- **Determinism**: Every test fixture and generated program is reproducible from an explicit seed.
- **Zero Production Semantics**: This crate is strictly a test-only utility (`publish = false`) and never introduces language semantics.
- **Production Safety**: `src/lib.rs` strictly enforces `#![forbid(unsafe_code)]`.
