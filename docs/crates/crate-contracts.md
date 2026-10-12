# Aipo — Crate Contracts

**Status:** normative (architecture level)
**Authority:** subordinate to `docs/canon/` canonical decisions
**Scope:** initial set of workspace crates with responsibility, dependency, and invariants
**Update Triggers:** crate added/removed/merged/split, dependency graph change
**Related:** `docs/canon/Aipo — Arquitetura Modular do Workspace …md` (topology baseline)

The canonical baseline lists 18 crates. **Wave 1 implements only the crates required by
the MVP pipeline** (marked MVP below). The remaining crates are declared here so the
dependency direction is fixed before they exist; their contracts are activated when their
wave starts. Wave 2 (`aipo-js`) and Wave 3 (async parts of `aipo-vm`/`aipo-runtime`) are
activated and their waves closed; still pending are Wave 4 (`aipo-host`, `aipo-poppy`),
Wave 5 (`aipo-lsp`) and Wave 6 (packages in `aipo-cli`/`aipo-stdlib`).

Common fields for every crate: **Responsibility / Owned concepts / Inputs / Outputs /
Public API / Allowed dependencies / Forbidden dependencies / Invariants / Error model /
Threading assumptions / Unsafe policy / Performance constraints / Testing strategy /
Related ADPs.**

## Delta de runtime — P07-G01, reconstrução de 2026-10-09

- `aipo-bytecode`: RegEmitter retorna Result, analisa alturas de stack pelo CFG e rejeita IR/operandos não representáveis. RegVM não é um backend certificado de paridade geral.
- `aipo-vm`: Reg usa helpers Value, checagens locais de bytecode, handlers por ativação, guardas e contratos fundamentais/nominais; possui orçamento opt-in. Janelas de registradores e verificador integral continuam pendentes.
- `aipo-js`: iteração primária de Dict retorna chave, conforme canon. Teste diferencial e self-test foram atualizados, mas não executados nesta reconstrução.
- `aipo-wasm`: opções de fuel, MissingExport e erros do writer; emissão continua separada da feature de JIT. Buffer de saída/memória não têm limites novos.
- `aipo-cli`/`aipo-diagnostics`: diagnóstico AIPO_COMPILE_REG_UNSUPPORTED, títulos EN/PT-BR e sugestão --engine=vm; Failure de topo termina com saída 1.

Este delta descreve source reconstruído, não certifica gates. Consulte [guia](../development/runtime-hardening-guide.md) e [evidência](../evidence/P07-G01-runtime-hardening.md).

## Global rules (apply to all crates)

- Dependency graph is acyclic; frontend never depends on backend/runtime; `aipo-ir`
  is target-neutral; `aipo-bytecode` does not know Poppy; `aipo-host` defines general
  contracts, `aipo-poppy` adapts them; portable stdlib does not depend on a specific host;
  LSP reuses compiler services; generated code stays isolated.
- `pub` is opt-in: smallest visibility possible; every public item has rustdoc.
- Safe Rust by default. `unsafe` is forbidden at workspace level
  (`unsafe_code = "forbid"` in workspace lints); a crate may re-enable it only with a
  written unsafe policy justification in its contract (no Wave 1 crate needs it).
- Panics never carry Aipo user-facing errors: parser/semalyzer/runtime errors are `Result`.
- No `clone()` as borrow-checker strategy; no `Rc<RefCell>`/`Arc<Mutex>` as default architecture.

## Crates

### aipo-source (MVP)
- **Responsibility:** load `.aipo` files into memory; validate UTF-8; normalize BOM/line endings; expose byte offsets, line/column mapping; own `SourceMap`.
- **Owns:** `Source`, `SourceMap`, `SourceSpan` (half-open `[start,end)` UTF-8 byte offsets), `SourceError`.
- **Inputs:** file path or in-memory text.
- **Outputs:** valid `Source` for lexer; line/col rendering for diagnostics.
- **Allowed deps:** std only.
- **Forbidden:** lexer/parser crates, VM, IO beyond reading source bytes.
- **Invariants:** spans always valid within source bytes; invalid UTF-8 is a `SourceError`, never a panic; LF and CRLF normalize before lexing; BOM ignored.
- **Error model:** `Result<Source, SourceError>` (invalid UTF-8, unreadable file).
- **Threading:** `Source` is immutable after creation; `Send + Sync`.
- **Unsafe:** forbidden.
- **Performance:** O(1) span→line/col via precomputed line index; no copying of source text per token.
- **Testing:** unit tests (BOM, CRLF, invalid UTF-8, line/col edge cases); fuzz target for byte input safety.
- **Related ADPs:** none.

### aipo-diagnostics (MVP)
- **Responsibility:** diagnostic model (codes, severity, spans, messages, notes, suggestions) independent of rendering; JSONL serialization for agents/IDEs.
- **Owns:** `Diagnostic`, `DiagnosticCode` (stable catalog), `Severity`, `Label`, JSONL emitter.
- **Inputs:** diagnostics emitted by any pipeline stage.
- **Outputs:** structured diagnostics; human rendering (basic, Wave 1) and `--message-format=jsonl`.
- **Allowed deps:** aipo-source (spans).
- **Forbidden:** parser/VM; terminal styling crates (rendering stays minimal).
- **Invariants:** codes are stable across versions once published; a diagnostic always carries a primary span; no Rust panic content as Aipo diagnostic message.
- **Testing:** golden fixtures per diagnostic; JSONL schema test.
- **Related ADPs:** none.

### aipo-lexer (MVP)
- **Responsibility:** convert `Source` into token stream (`TokenKind + SourceSpan`); classify keywords/identifiers (Unicode XID, NFC), numbers, strings with prefixes (`f`, `r`, `fr`, multiline), operators, punctuation, NEWLINE, EOF.
- **Owns:** `TokenKind`, `Token`, `Lexer`; newline/indentation-agnostic mode (indent is visual only).
- **Inputs:** `aipo_source::Source`.
- **Outputs:** tokens + lex-time diagnostics (unknown escape, unterminated string, invalid byte).
- **Allowed deps:** aipo-source, aipo-diagnostics.
- **Forbidden:** AST/parser knowledge, VM.
- **Invariants:** the lexer stage never converts numeric text to values (classification only); the crate still owns the numeric-literal rules for the whole pipeline and exposes the matching conversion (`number_is_well_formed` plus `parse_int_literal`/`parse_float_literal`) so a stage that must materialize a literal parses exactly what the lexer accepted; identifiers normalize NFC for identity while spans keep original bytes; `_` alone is discard marker; keywords only when the full name matches.
- **Error model:** recovery tokens + diagnostics; lexer never panics on any input.
- **Testing:** unit tests; property test (token spans re-slice exactly to source text); fuzz target.
- **Related ADPs:** none.

### aipo-syntax (MVP)
- **Responsibility:** lossless syntax model + recursive descent parser with Pratt expressions; parser recovery so one error does not stop the file.
- **Owns:** grammar rules, `SyntaxNode` (lossless, with trivia), parse events, recovery strategy.
- **Inputs:** tokens from aipo-lexer.
- **Outputs:** syntax tree (lossless) + parse diagnostics; typed view consumed by aipo-ast builders.
- **Allowed deps:** aipo-source, aipo-diagnostics, aipo-lexer.
- **Forbidden:** semantic analysis, IR, VM, Tree-sitter (canonical parser is handwritten).
- **Invariants:** lossless round-trip: concatenating node text reproduces source exactly; parser consumes all input to EOF; recovery always advances.
- **Testing:** snapshot tests for the canonical syntax corpus; lossless round-trip property test; fuzz target (never panic, never hang).
- **Related ADPs:** none.

### aipo-ast (MVP)
- **Responsibility:** typed AST built from the lossless tree; module-level structural declarations vs executable statements split.
- **Owns:** typed AST nodes with spans, literal kinds (incl. f/r/fr strings, multiline), match/when, attempt/failed, trailing blocks (pre-lowering).
- **Allowed deps:** aipo-source, aipo-diagnostics, aipo-syntax (consumes its output).
- **Forbidden:** name resolution decisions, IR, VM.
- **Invariants:** every AST node has a span; structural phase (import/export/fn/struct/interface/impl) is distinguishable from executable phase.
- **Testing:** builder unit tests; snapshot tests; property test (AST spans point at valid syntax).
- **Related ADPs:** none.

### aipo-hir (MVP)
- **Responsibility:** High-level IR after early lowering: trailing blocks → `FunctionLiteral`, pipelines → calls, comparison ellipsis → plain chains, conditional value form normalized.
- **Owns:** HIR nodes, lowering passes (order documented in docs/implementation/lowering.md).
- **Allowed deps:** aipo-source, aipo-diagnostics, aipo-ast.
- **Forbidden:** backend knowledge (bytecode/JS), VM.
- **Invariants:** lowering is semantics-preserving and span-preserving; HIR is deterministic for identical input.
- **Testing:** lowering snapshot tests; differential property (AST→HIR→AST structure invariants).
- **Related ADPs:** none.

### aipo-sema (MVP)
- **Responsibility:** semantic analysis: name resolution, lexical scopes, shadowing/redeclaration rules, mutability-path checks (let/var, struct field immutability, `var self`), arity and signature contract checks, structural module resolution (imports acyclic, exports), value/no-result function classification, `is`/`T?` handling, structural interface compatibility (`is Interface`).
- **Owns:** resolver, scope tree, semantic facts (resolved symbols, mutability), type/category lattice for opportunist analysis, interface compatibility checker.
- **Allowed deps:** aipo-source, aipo-diagnostics, aipo-ast, aipo-hir.
- **Forbidden:** bytecode, VM, JS; no IR construction (aipo-ir builds it).
- **Invariants:** no invention: unresolved semantic questions become ADPs, not silent choices; contracts verified at boundaries; modules acyclic.
- **Testing:** pass/fail fixtures; fixture = diagnostic expectation; table tests for interface compatibility matrix.
- **Related ADPs:** none.

### aipo-ir (MVP)
- **Responsibility:** target-neutral Core IR construction from validated HIR+sema facts; failure/fault distinction materialized; constant pool-ready literal model.
- **Owns:** `CoreIr` module/function/instruction-level structures, `IrId`s.
- **Allowed deps:** aipo-source, aipo-diagnostics, aipo-hir, aipo-sema (consumes facts), and aipo-lexer **for the numeric-literal rules only** (a literal must mean the same thing at the layer that accepted it and the layer that materializes it; the direct edge shortcuts the existing `Core IR → Sema → HIR → AST → Syntax → Lexer` path, it does not add a cycle).
- **Forbidden:** bytecode format details, VM internals, JS emission; any other use of the lexer crate (tokens, scanning, spans).
- **Invariants:** IR never references source text for lost facts; stable IDs survive to bytecode for diagnostics/source maps.
- **Testing:** IR snapshot tests per feature fixture.
- **Related ADPs:** none.

### aipo-bytecode (MVP)
- **Responsibility:** compact instruction encoding + constant pool + symbol interning; emitter Core IR → bytecode; verifier (structure, jump targets, stack effects) before load.
- **Owns:** instruction set (`crates/aipo-bytecode/README.md` and the `opcode` module), `aibc` module format, verifier.
- **Allowed deps:** aipo-ir, aipo-diagnostics.
- **Forbidden:** Poppy concepts, host capabilities, JS specifics.
- **Invariants:** bytecode is loadable only if verifier passes; encoding is versioned; every instruction has deterministic stack effect documented.
- **Testing:** round-trip emit→verify→disassemble tests; verifier negative tests; differential: fixture programs compile to expected disassembly snapshots.
- **Related ADPs:** none.

### aipo-vm (MVP)
- **Responsibility:** stack-based bytecode interpreter: value model, frames, call stack, closures/upvalues, collections (List/Dict ordered), structs with fixed/invariant, Failure propagation (Model B), runtime faults, iteration safety (structural mutation during each = fault). Shared mutable values (`List`, `Dict`, struct instances, upvalue cells) use `Rc<RefCell<…>>`.
- **Owns:** interpreter loop, call frames, upvalues, builtins glue, runtime object representations, operand-stack depth limit. Instruction/fuel budgets, memory accounting and interruption are **not** implemented — see `docs/adp/ADP-003-execution-budgets.md` (draft).
- **Allowed deps:** aipo-bytecode, aipo-runtime, and aipo-host — the host adapter (`crate::host`) is the single place that converts host values, maps host faults onto `VmFault` and gates privileged operations on a capability.
- **Host boundary invariants:** the VM holds the host services in one `HostContext` (granted capabilities, the objects behind their handles, the open scopes). It only ever *checks* against what the host decided: capabilities are granted by the embedder, handles are minted and released by the embedder, and a scope is opened and closed by the embedder around a callback. The scoped-escape rule is enforced where a value becomes heap-reachable — `SetGlobal`, `Return`, `SetField`, `SetIndex`, `BuildList`, `BuildDict` — and nowhere else, because a later read cannot know which scope a binding came from. The check is guarded by "has any scope closed?", so a program with no host bindings (every Wave 1–3 program) pays one boolean per publication point and nothing else.
- **Forbidden:** parser/frontend crates (loads verified bytecode only), host-specific modules, Poppy.
- **Invariants:** `Int` range ±(2^53−1) enforced semantically; `Float` finite-only (NaN/Inf are faults); no silent wraparound; conditions require Bool; `div`/`%`/`/` semantics per Language Reference; no Rust panic escapes as Aipo error (all VM errors are `Result`).
- **Error model:** recoverable `Failure` values + runtime faults as distinct internal variants surfaced through the CLI without panics.
- **Threading:** single-threaded interpreter by decision, not by accident: `Value` is `!Send` (`Rc<RefCell<…>>` sharing gives closures, shared `var` capture and in-place collection mutation without a GC). A concurrent host or the Wave 3 scheduler must either keep one VM per thread or replace the sharing primitive first — do not wrap this VM in threads expecting `Send`.
- **Unsafe:** forbidden (`unsafe_code = "forbid"` at workspace level; no GC crate is used).
- **Performance:** no JIT/Cranelift/NaN-boxing before benchmarks prove need; instruction dispatch measured via `aipo bench` later.
- **Testing:** integration on fixture programs; fault tests (overflow, index errors); closure capture per-iteration tests; VM↔JS differential suite (`P01-G01`).
- **Related ADPs:** none.

### aipo-runtime (MVP)
- **Responsibility:** shared runtime services used by VM: module registry + initialization model (once per run, deterministic order, acyclic), stdlib native function registry, failure object internals.
- **Allowed deps:** aipo-bytecode (module images), aipo-diagnostics.
- **Forbidden:** frontend crates; direct host APIs (capability layer comes in Wave 4).
- **Invariants:** module init order: dependency graph topo, tie-break by canonical path; failed init publishes nothing; imports are eager.
- **Testing:** module graph unit tests; init-order golden tests.
- **Related ADPs:** none.

### aipo-stdlib (MVP)
- **Responsibility:** Prelude V1 (`none, true, false, Int, Float, Byte, String, List, Dict, Bytes, len, copy, same, some, fail`) + MVP subset of portable stdlib (see `docs/stdlib/mvp-subset.md`); native implementations behind portable contracts.
- **Allowed deps:** aipo-runtime, aipo-vm (registration), aipo-diagnostics.
- **Forbidden:** host-specific crates (fs/http/net come later with capabilities).
- **Invariants:** no concept outside Prelude is global; one canonical name per operation (no aliases); String NFC invariant; no implicit coercions.
- **Implemented surface (Wave 1, S9):** `docs/stdlib/mvp-subset.md`.
- **Representation gaps for `Byte`/`Bytes`/type values:** `docs/adp/ADP-001-byte-and-core-types-as-values.md`.
- **Testing:** per-function unit tests; VM↔JS differential test plan deferred to Wave 2.
- **Related ADPs:** none.

### aipo-formatter (MVP)
- **Responsibility:** deterministic formatter for implemented syntax; 4-space indent, no semantic rewrite; idempotency guarantee.
- **Allowed deps:** aipo-syntax (lossless tree), aipo-diagnostics.
- **Forbidden:** semantic analysis (formatting is syntax-level in Wave 1).
- **Invariants:** format(format(x)) == format(x); preserves comments and string content; no semantic interpretation.
- **Testing:** golden corpus; idempotency property test.
- **Related ADPs:** none.

### aipo-cli (MVP)
- **Responsibility:** thin orchestration: `aipo run`, `aipo check`, `aipo fmt` (Wave 1 baseline); JSONL diagnostics output; exit codes.
- **Allowed deps:** all pipeline crates (facade only).
- **Forbidden:** business logic beyond orchestration; it must not become a second semantic source.
- **Invariants:** no panics across the boundary — user-facing errors are Aipo diagnostics or CLI usage errors with non-zero exit; exit codes documented.
- **Testing:** CLI integration tests (run programs, expect stdout/exit).
- **Related ADPs:** none.

### aipo-js (Wave 2, delivered by `P01-G01`)
- **Responsibility:** JavaScript emitter (ESM), versioned runtime shim (`RUNTIME_VERSION`),
  ECMA-426 source maps, `aipo build` bundle layout. Started in Wave 2; was not started in Wave 1.
- **Allowed deps:** aipo-ir, aipo-diagnostics (plus serde_json for bundle encoding).
- **Forbidden:** bytecode format coupling.
- **Notes:** differential VM↔JS suite per Fechamento Arquitetural §10
  (`crates/aipo-js/tests/differential.rs`, `crates/aipo-cli/tests/js_build.rs`).

### aipo-lsp (Wave 5)
- **Responsibility:** LSP over compiler services; never duplicates analyzer logic.

### aipo-host (Wave 4, started by `P03-G01`)
- **Responsibility:** host ABI contracts: AHS consumption, capability model, host values/handles. General abstractions only.
- **Owns:** `HostSchema` (the AHS model and its validation), `Capability`/`CapabilitySet`, `Handle`/`HandleTable`, `HostValue`, `HostFault`.
- **Inputs:** a host surface description as data; a profile's granted capabilities.
- **Outputs:** the validated surface, capability decisions, host values and handle resolutions as `Result`.
- **Allowed deps:** aipo-diagnostics, serde, serde_json.
- **Forbidden:** any specific engine or engine type; VM, IR or bytecode internals (the VM adapter that consumes this crate is written in `aipo-vm`, not here).
- **Invariants:** capabilities are deny-by-default and a declared set is an upper bound that policy may only narrow; a handle is dereferenced only through the table that minted it, and a released generation is never reused; a host value is plain data, with `Int` within ±(2^53−1) and `Float` finite; no Rust reference, lifetime or panic crosses the boundary.
- **Error model:** `HostFault` with one stable code per refusal (`AIPO_RT_CAPABILITY_DENIED`, `AIPO_RT_STALE_HANDLE`, `AIPO_RT_SCOPE_ESCAPE`, and a contract fault for an invalid host value).
- **Testing:** unit tests per guarantee inside the crate plus the VM-level fixtures once the adapter lands.
- **Related ADPs:** ADP-006 §I (Wave 4 deviations).

### aipo-poppy (Wave 4)
- **Responsibility:** Poppy Game Engine adapter over aipo-host: ECS scopes/command buffer, behaviors/events, game.random. Depends on host contracts; bytecode/IR never know it.
