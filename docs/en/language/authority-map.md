# Aipo — document authority map

**Status:** normative
**Authority:** Level 2 (process/document governance; subordinate to the canonical decisions recorded in `docs/canon/`)
**Scope:** which Aipo document defines what, who may override whom, and which documents are canonical, derived, historical or experimental
**Domain/Owner:** Aipo language implementation — documentation governance
**Dependencies:** `docs/PRUMO.md` (router), `prumo.json`
**Update triggers:** any change to documentation structure, authority or supersession
**Related ADPs:** none yet
**Supersedes:** none
**Superseded by:** none

---

## Authority hierarchy (applies on conflict)

When two documents disagree, resolve by the first rule that applies:

1. **Canonical decisions explicitly approved** and materialized in this repository (wave
   gates, accepted ADPs, MVP scope, gate definitions).
2. **Language Reference + Canonical Syntax + Semantics** (`docs/canon/Aipo V1 — Language
   Reference …md`, `docs/canon/Aipo V1 — Sintaxe Canônica Consolidada …md`, semantic sections
   of `docs/canon/Aipo Language — Especificação Viva …md`).
3. **Accepted ADPs** (`docs/adp/`).
4. **Architecture contracts and crate contracts** (`docs/architecture/`, `docs/crates/`).
5. **Approved conformance fixtures and snapshots** (`docs/conformance/`, fixtures in crates).
6. **Implementation docs** (`docs/implementation/`, `docs/journal/`).
7. **Generated docs/adapters/indexes** (anything marked `derived`).
8. **Historical/experimental docs** (`docs/canon/` materials marked historical; rejected
   experiments).

Recency alone never resolves a contradiction. A contradiction is registered via `prumo docs
contradictions`, recorded as an ADP/open question, and only the ambiguous segment is blocked
(no-invention policy).

## Document classification

| Document | Location | Class | Defines |
|---|---|---|---|
| Language Reference V1 | `docs/canon/Aipo V1 — Language Reference …` | **canonical (normative)** | user-facing language semantics |
| Canonical Syntax V1 | `docs/canon/Aipo V1 — Sintaxe Canônica Consolidada …` | **canonical (normative)** | V1 syntax surface |
| Living Spec | `docs/canon/Aipo Language — Especificação Viva …` | canonical, historical-laden | decisions with rationale; normative where not superseded |
| Stdlib V1 Canônica | `docs/canon/Aipo — Stdlib V1 Canônica …` | canonical (normative) | stdlib contracts (MVP subset selected in `docs/stdlib/`) |
| Waves/Gauntlet | `docs/canon/Aipo — Waves, Vertical Slices …` | canonical (normative) | slice/wave/gate process |
| Rust Engineering Standard | `docs/canon/Aipo — Rust Engineering Standard …` | canonical (normative) | Rust code quality policy |
| Workspace Architecture | `docs/canon/Aipo — Arquitetura Modular …` | canonical (normative) | crate topology + dependency rules |
| Doc System for Agents | `docs/canon/Aipo — Sistema de Documentação …` | canonical (normative) | documentation & agent process |
| Fechamento Arquitetural | `docs/canon/Aipo — Fechamento Arquitetural 10 10 …` | canonical (normative) | closed design areas (JS backend, sandbox, packages, tooling, perf) |
| Governance/Evolution | `docs/canon/Governança de Design e Evolução …` | canonical (normative) | feature admission, ADP process |
| Rust/Poppy Pivot & Roadmap | `docs/canon/Aipo — Rust Poppy Pivot …` | canonical (normative) | host profiles, roadmap |
| Standard Library Architecture | `docs/canon/Aipo — Standard Library Architecture …` | canonical | stdlib layering |
| Implementation book + chapters + interludes | `docs/canon/Construindo a Aipo …`, `Capítulo …`, `Interlúdio …`, `Parte …`, `01–06 …` | **historical / pedagogical** | how we learn/build; Odin-era material is historical. Decision content already superseded by Language Reference is not normative. |
| Backlog pós-V1 | `docs/canon/Aipo — Backlog de Sintaxe …` | non-normative | ideas explicitly not in V1 |
| Biblioteca de Referências | `docs/canon/Aipo — Biblioteca de Referências …` | non-normative | external references |
| authority-map (this file) | `docs/language/authority-map.md` | meta | documentation authority |
| crate contracts | `docs/crates/*.md` | normative (architecture level) | per-crate contracts |
| architecture docs | `docs/architecture/*.md` | normative (architecture level) | workspace, IR, bytecode, VM boundaries |
| waves plan | `docs/waves/wave-1-mvp.md` | normative (process level) | Wave 1 slices + MVP gate |
| feature packs | `docs/features/*/*.md` | normative per feature | feature contracts |
| diagnostics catalog | `docs/diagnostics/catalog.md` | normative | stable diagnostic codes |
| testing strategy | `docs/development/testing-strategy.md` | normative | validation levels + gates |
| ADPs | `docs/adp/ADP-*.md` | normative when accepted; draft otherwise | design decisions |
| journal | `docs/journal/*.md` | record (non-normative) | implementation decisions log |
| evidence | `docs/evidence/*.md` | record | proof attachments for gates |
| docs/PRUMO.md | `docs/PRUMO.md` | derived router | navigation; never overrides canon |

## Rule sets that bind everything

- **No-invention policy:** a material semantic question without a canon answer becomes an open
  question → proposal/ADP → fixture. Only non-ambiguous parts get implemented.
- **One canonical form:** no alias surfaces are added; superseded forms are recorded as
  superseded.
- **Evidence policy:** "implemented/compatible/safe/fast/10" claims require attached evidence.
- **Drift policy:** changes to grammar/semantics/IR/bytecode/stdlib/diagnostics/CLI must run
  `prumo docs impact` / `docs delta` / `docs contradictions`; stale docs are not silently
  ignored.

## Supersession ledger (extract)

Last update: **2026-10-06** (ADR-001 — canonical syntax). For the full design and
implementation order, see
[`docs/decisions/adr-001-canonical-syntax.md`](../decisions/adr-001-canonical-syntax.md).

| Superseded form | Canonical form | Source |
|---|---|---|
| `for` iteration | `each` | Canonical Syntax 2026-09-07 |
| `where`/`@` field rules | `invariant()` + immutable-by-default fields | Canonical Syntax 2026-09-07 → ADR-001 |
| `fn Type.name(...)` / `impl Tipo { }` | `Tipo:nome` + `self`/`var self` | ADR-001 2026-10-06 |
| `satisfy Tipo: Interface` | `#!satisfies` (comment directive) | ADR-001 2026-10-06 |
| `div` and `div=` | `//` and `//=` | ADR-001 2026-10-06 |
| `end` as block terminator | `{ }` exclusively | ADR-001 2026-10-06 |
| `self!` (mutable receiver) | `var self` | ADP-012 → ADR-001 2026-10-06 |
| `nome!` (mutable parameter) | `var nome` | ADR-001 2026-10-06 |
| `then` optional in block | never in block; always inline | ADR-001 2026-10-06 |
| `case` | `when` (match) | Language Reference |
| `try` / `or` / `try … else … end` | `or_else` + `attempt`/`failed` | Language Reference (Lote 5) |
| `expression else fallback` | `expression or_else fallback` | Language Reference |
| `fixed` | field without `var` (immutable by default) | ADP-012 → ADR-001 |
| Int as full signed 64-bit | Int = ±(2^53 − 1) | Canonical Syntax 2026-09-10 |
| `T?` beyond signatures/`is` | contracts only in signatures | Language Reference |

### Prevailing sources

In order of authority for the **syntax surface**:

1. **`docs/decisions/adr-001-canonical-syntax.md`** — norm of the target surface
2. **`SYNTAX.md`** (root) — canonical reference, generated from the compiler
3. **`docs/manual/*`** — narrative, does not define grammar
4. **`docs/canon/*`** — historical and dated decisions; when it contradicts ADR-001, ADR-001 prevails
5. **`crates/aipo-{lexer,syntax,ast}`** — the *implemented* state; while they diverge from
   ADR-001, the divergence is pending work (see `SYNTAX.md` §16), not authority

A document in (4) or (5) that states `end`, `div`, `impl`, `satisfy` or `self!` as the canonical
form is **superseded** by ADR-001.
