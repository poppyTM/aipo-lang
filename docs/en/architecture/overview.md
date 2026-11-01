# Aipo — architecture overview (Waves 1–2)

**Status:** historical — this document describes the Waves 1–2 slice.
**Current architecture:** see [`docs/architecture/crates.md`](crates.md) and the portal at `website/content/engineering/`.
**Authority:** subordinate to the canonical documents in `docs/canon/`
**Scope:** implementation architecture of Waves 1–2: pipeline, data ownership, boundaries.
Wasm, host ABI, packages, C ABI and GUI/game adapters were added later and do not appear in this slice.
**Blocking questions:**
- Who owns the canonical state?
- Which components may mutate it?

## System boundaries and main components

The Aipo system is partitioned into strictly delimited, acyclic workspace components:

1. **Source & Diagnostics**: `aipo-source` (manages files, byte spans, line indexing) and `aipo-diagnostics` (stable error-code catalog and JSONL serialization).
2. **Compiler frontend**: `aipo-lexer` (tokenization), `aipo-syntax` (lossless CST and parser with recovery), `aipo-ast` (typed AST), `aipo-hir` (desugaring and lowering) and `aipo-sema` (scope resolution, contracts, mutability).
3. **Target-neutral middle-end**: `aipo-ir` (Core IR).
4. **Bytecode & execution runtime**: `aipo-bytecode` (instruction encoding and verifier) and `aipo-vm` (stack-based virtual machine over shared values `Rc<RefCell<…>>` — single-threaded by decision, no GC crate).
5. **Runtime services & stdlib**: `aipo-runtime` (module registration, native registration) and `aipo-stdlib` (Prelude, `math`, `string`, `io`, collection methods).
6. **JavaScript backend (Wave 2)**: `aipo-js` (Core IR → ESM bundle + versioned runtime shim + source maps; depends on `aipo-ir` and `aipo-diagnostics`, never on bytecode).
7. **Tooling & orchestration**: `aipo-formatter` and `aipo-cli` (`aipo run`, `aipo check`, `aipo build`, `aipo fmt`).
8. **Test-only harnesses**: `aipo-testkit` (deterministic RNG, program generator, pipeline/JS runners — never ships language semantics) and `aipo-bench` (benchmark runner binary).

## Dependency direction

Dependencies are strictly unidirectional and acyclic:
`CLI → Runtime / VM → Bytecode → Core IR → Sema → HIR → AST → Syntax → Lexer → Source → Diagnostics`,
with `aipo-js` branching off Core IR (`JS → Core IR` only).
No backend or execution layer may be imported into the frontend or the syntax tree.

## External integrations

Wave 1 has minimal external integrations:
- Operating-system file system (via stdlib file reads for `.aipo` source files).
- Standard streams (`stdin`, `stdout`, `stderr`) for program execution and diagnostics.

## Ownership and mutation of canonical state

### Who owns the canonical state?
- **Compile time**: the `SourceMap` in `aipo-source` owns the canonical source text. Each compilation phase produces immutable data artifacts (`Source` → `Tokens` → `SyntaxTree` → `Ast` → `Hir` → `CoreIr` → `BytecodeModule`, or `CoreIr` → JS bundle).
- **Runtime**: the `aipo-vm` virtual machine owns the execution state (call stack, frames and shared values `Rc<RefCell<…>>`); the JS backend mirrors the same value model in its versioned shim.

### Which components may mutate it?
- Compile-time data structures are append-only or immutable transformations across phase boundaries.
- At runtime, only the active evaluation frame of the VM and the execution loop may mutate local variables and mutable data structures (bindings `var`, mutable list/dict elements), according to Aipo's mutability semantics.

## Pipeline (Waves 1–2)

```
.aipo file
  → aipo-source     (load, UTF-8, BOM/CRLF normalize, SourceMap)
  → aipo-lexer      (TokenKind + SourceSpan; keywords; f/r/fr strings)
  → aipo-syntax     (lossless tree + recovery; recursion bounds per ADP-005)
  → aipo-ast        (typed AST, module structural split)
  → aipo-hir        (early lowering: trailing blocks → fn literals, |>, ellipsis)
  → aipo-sema       (scopes, mutability paths, contracts, modules, interfaces)
  → aipo-ir         (target-neutral Core IR)
  → ┬→ aipo-bytecode (instruction selection + encoding + verifier)
  │   → aipo-vm     (stack interpreter; aipo-runtime + aipo-stdlib linked in)
  └──→ aipo-js       (ESM bundle + versioned shim + source maps)
  → aipo-cli        (aipo run / aipo check / aipo build / aipo fmt)
```

Diagnostics flow from every stage to `aipo-diagnostics`; user-facing output is rendered for
humans or as `--message-format=jsonl` (stable codes, machine-readable).

## Failure vs fault (runtime model)

- **Failure** = recoverable, created by `fail(...)` or by runtime operations documented as
  fallible; propagates automatically (Model B); captured by `or_else` / `attempt...failed`.
- **Fault** = programming error (overflow, division by zero, index/key errors, mutation
  during iteration, contract violation discovered at runtime, non-Bool condition); not
  capturable; terminates execution with a structured runtime-fault diagnostic. Never a Rust
  panic.
