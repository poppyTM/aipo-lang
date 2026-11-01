# Workspace crate contracts

The Aipo repository is composed of **24 Rust crates**, each with responsibility boundaries and
acyclic internal dependencies.

Source: `cargo metadata --no-deps --format-version 1` and the workspace `Cargo.toml` files.
Test dependencies (`dev-dependencies`) do not appear in this table.

---

## Crate inventory

| Crate | Primary responsibility | Normal internal dependencies |
| :--- | :--- | :--- |
| **`aipo-source`** | Source loading and safe UTF-8 indexing | None |
| **`aipo-diagnostics`** | Stable error catalog, spans, locale and JSONL | `aipo-source` |
| **`aipo-lexer`** | Tokenization, keywords, literals and spans | `aipo-source`, `aipo-diagnostics` |
| **`aipo-ast`** | Abstract syntax tree types | `aipo-source` |
| **`aipo-syntax`** | Recursive-descent parser with error recovery | `aipo-ast`, `aipo-diagnostics`, `aipo-lexer`, `aipo-source` |
| **`aipo-hir`** | High-level intermediate representation and lowering | `aipo-ast`, `aipo-diagnostics`, `aipo-source` |
| **`aipo-sema`** | Semantic analysis, scopes, contracts and interfaces | `aipo-ast`, `aipo-diagnostics`, `aipo-hir`, `aipo-source` |
| **`aipo-ir`** | Target-neutral Core IR | `aipo-ast`, `aipo-diagnostics`, `aipo-hir`, `aipo-lexer`, `aipo-sema`, `aipo-source` |
| **`aipo-bytecode`** | Emission, verification, `.aibc` serialization and execution plan | `aipo-diagnostics`, `aipo-ir`, `aipo-source` |
| **`aipo-vm`** | Stack VM, RegVM, async scheduler and host adapter | `aipo-bytecode`, `aipo-diagnostics`, `aipo-host`, `aipo-source` |
| **`aipo-js`** | JavaScript ESM emitter, runtime shim and source maps | `aipo-diagnostics`, `aipo-ir` |
| **`aipo-host`** | Host ABI, capabilities and generational handles | `aipo-diagnostics` |
| **`aipo-runtime`** | Module and native-function registration | `aipo-bytecode`, `aipo-diagnostics` |
| **`aipo-stdlib`** | Standard library and collection methods | `aipo-diagnostics`, `aipo-host`, `aipo-runtime`, `aipo-vm` |
| **`aipo-poppy`** | ECS adapter and deterministic simulation | `aipo-bytecode`, `aipo-diagnostics`, `aipo-host`, `aipo-runtime`, `aipo-stdlib`, `aipo-vm` |
| **`aipo-package`** | Manifest, lockfile, resolver, cache and vendor | `aipo-host` |
| **`aipo-formatter`** | Canonical syntax auto-formatter | `aipo-diagnostics`, `aipo-lexer`, `aipo-source` |
| **`aipo-testkit`** | Test harness: deterministic RNG, generators and runners | `aipo-ast`, `aipo-bytecode`, `aipo-diagnostics`, `aipo-hir`, `aipo-host`, `aipo-ir`, `aipo-js`, `aipo-lexer`, `aipo-runtime`, `aipo-sema`, `aipo-source`, `aipo-stdlib`, `aipo-syntax`, `aipo-vm`, `aipo-wasm` |
| **`aipo-bench`** | Benchmark runner and cross-language comparison | `aipo-bytecode`, `aipo-formatter`, `aipo-hir`, `aipo-ir`, `aipo-js`, `aipo-lexer`, `aipo-sema`, `aipo-source`, `aipo-syntax`, `aipo-testkit` |
| **`aipo-c-abi`** | Synchronous C ABI, bytecode, stepping and Wasm | `aipo-bytecode`, `aipo-diagnostics`, `aipo-hir`, `aipo-host`, `aipo-ir`, `aipo-runtime`, `aipo-sema`, `aipo-source`, `aipo-stdlib`, `aipo-syntax`, `aipo-vm`, `aipo-wasm` |
| **`aipo-wasm`** | HIR → WebAssembly compiler, JIT runner and limits | `aipo-ast`, `aipo-diagnostics`, `aipo-hir`, `aipo-ir`, `aipo-lexer`, `aipo-source`, `aipo-syntax` |
| **`aipo-egui`** | Immediate-mode GUI adapter and eGUI bindings | `aipo-bytecode`, `aipo-diagnostics`, `aipo-host`, `aipo-runtime`, `aipo-stdlib`, `aipo-vm` |
| **`aipo-game-host`** | Desktop/game host, Miniquad, audio and FFI bridge | `aipo-ast`, `aipo-bytecode`, `aipo-cli`, `aipo-diagnostics`, `aipo-egui`, `aipo-hir`, `aipo-ir`, `aipo-lexer`, `aipo-runtime`, `aipo-sema`, `aipo-source`, `aipo-stdlib`, `aipo-syntax`, `aipo-vm` |
| **`aipo-cli`** | CLI entry point, formatter, packages, tooling and LSP | `aipo-ast`, `aipo-bytecode`, `aipo-diagnostics`, `aipo-formatter`, `aipo-hir`, `aipo-host`, `aipo-ir`, `aipo-js`, `aipo-lexer`, `aipo-package`, `aipo-runtime`, `aipo-sema`, `aipo-source`, `aipo-stdlib`, `aipo-syntax`, `aipo-vm`, `aipo-wasm` |

## Dependency rules

- The normal graph is acyclic. Apparent cycles in `cargo metadata` come from `dev-dependencies` used by tests.
- The frontend (`source` through `sema`) does not depend on bytecode, VM, JS, Wasm, host or GUI.
- `aipo-js` depends on Core IR (`aipo-ir`) and diagnostics; it does not depend on bytecode or the VM.
- `aipo-wasm` consumes HIR, not Core IR.
- `aipo-cli`, `aipo-game-host` and `aipo-egui` are product leaves: no language crate depends on them.
- `aipo-c-abi` is the only crate that enables `unsafe_code`; the exception is restricted to the FFI boundary and documented in `docs/crates/crate-contracts.md`.

## Internal backend modules

A crate with three independent domains in a single file is the god-module signal that the
architecture doctrine asks to address. The two largest consumers have already been split by
responsibility; the split is internal to the crate and does not change the dependency direction.

| crate | modules | boundary |
|---|---|---|
| `aipo-wasm` | `analysis` / `runtime` / `compiler` | static analysis that recovers what the untagged representation loses; Wasm helpers emitted in every module; code generation |
| `aipo-cli` | `packages` + `lib` | resolution/fetch/lock/cache of packages vs. compilation driver, execution and tooling |
