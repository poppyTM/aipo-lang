# Aipo — security and trust contract

**Status:** normative
**Scope:** trust boundaries, permission model, secrets model, recovery
**Blocking question:** Which actions require explicit approval?

## Trust boundaries

1. **Host vs. guest boundary**: Aipo source code executed by the interpreter is untrusted user code. It must never cause crashes, undefined behavior or unauthorized memory access on the host system.
2. **Compiler frontend boundary**: source input is assumed potentially hostile (e.g. deeply nested ASTs, exponential expansions). The lexer and parser must enforce recursion limits and bounded allocations.
3. **Agent and automation boundary**: automated code agents work within declared sandbox permissions.

## Permission model and approval requirements

### Which actions require explicit approval?
- Modifying repository governance policies or bypassing CI quality gates.
- Introducing dependencies containing `unsafe` Rust code.
- Altering published canonical diagnostic codes or grammar contracts.
- Executing destructive file-system operations outside target workspace directories.

### Approved dependency exceptions

| Dependency | Used by | Contains `unsafe` | Justification | Approved |
|---|---|---|---|---|
| `unicode-normalization` 0.1.25 | `aipo-stdlib` (`string.reverse` NFC re-normalization) | yes — `char::from_u32_unchecked` confined to Hangul decomposition in `src/normalize.rs` | Canon names this crate as the preferred Rust provider for NFC (`Aipo — Stdlib V1 Canônica …` → "Implementação Rust — referências preferidas"); canon requires the result of `reverse()` to keep the `String` NFC invariant; no workspace crate enables `unsafe` | 2026-09-15, explicit user approval during slice S9 (`P00-G09`) |
| `memchr` 2.8.3 | transitive via `serde_json` (JSONL diagnostics) | yes — SIMD dispatch paths, `unsafe` pointer reads guarded by runtime CPU detection | Mainstream crate (BurntSushi), `cargo audit` green, no advisories; performance-critical text search with no safe equivalent at equal throughput | P01-G02 maintainer inventory (this gauntlet), `cargo audit` 0 findings / 31 deps |
| `serde` / `serde_core` 1.0.229 | serialization framework (`serde`, `serde_json`) | yes — internal `unsafe` in private impls (ptr reads, uninitialized buffers) | Foundation crate of the ecosystem; `cargo audit` green; workspace uses only the safe public API | P01-G02 maintainer inventory, `cargo audit` 0 findings |
| `serde_json` 1.0.151 | JSONL diagnostics | yes — `unsafe` float parsing/serialization fast paths | Same as `serde`; JSONL is the machine interface, no alternative vetted | P01-G02 maintainer inventory, `cargo audit` 0 findings |
| `syn` 3.0.5 + `proc-macro2` 1.0.107 + `quote` (safe) + `unicode-ident` 1.0.24 | build-time proc macros (`serde_derive`) | yes — `syn`/`proc-macro2` use `unsafe` for parsing buffers; `unicode-ident` table lookup | Build-time only (never ships in binaries); mainstream; `cargo audit` green | P01-G02 maintainer inventory, `cargo audit` 0 findings |
| `byteorder` 1.5.0 | bytecode encode/decode (`aipo-bytecode`) | yes — `unsafe` slice casts in cited paths | Pinned 1.5.0; `cargo audit` green; used only for explicit-endian u16/i16 operands | P01-G02 maintainer inventory, `cargo audit` 0 findings |
| `itoa` 1.0.18 | transitive via `serde_json` | yes — `unsafe` digit-table writes | Same justification as `serde_json` | P01-G02 maintainer inventory, `cargo audit` 0 findings |
| `zmij` 1.0.23 | transitive JSON machinery | yes — confined `unsafe` blocks | `cargo audit` green | P01-G02 maintainer inventory, `cargo audit` 0 findings |
| `ureq` 3.4.2 + Rustls stack | opt-in GitHub package fetch (`aipo-package/http`, `aipo-cli/github-http`) | yes — transitive `ring`/Rustls primitives; `ureq` itself forbids unsafe | Explicitly feature-gated, pinned, HTTPS-only, public by default, optional environment-backed bearer auth, bounded responses and redirects disabled; each source in an explicit recursive graph is cached/output locally and digest-verified | P04-G05/P04-G06/P04-G11, explicit user approval for opt-in GitHub fetch/auth adapter |

Verified unsafe-free (explicit `#![deny/forbid(unsafe_code)]` upstream): `unicode-segmentation`
1.13.3, `tinyvec` 1.13.3, `serde_derive`, `quote`.

Dependencies added to the workspace must be scanned before use. For slice S9 the remaining
additions were verified unsafe-free: `unicode-segmentation` 1.13.3 (`#![deny(unsafe_code)]`) and its
transitive `tinyvec` 1.13.3 (`#![forbid(unsafe_code)]`).

## Secrets model

- Zero secrets committed to version control.
- No environment variables containing credentials may be echoed to log outputs or test snapshots.

## Recovery

- In the event of a security violation or regression:
  1. Offending changes are reverted immediately.
  2. A reproducing regression fixture is committed under `docs/conformance/` (runnable program,
     diagnostic fixture, or fuzzer seed — the smallest minimized artifact, per the regression
     policy).
  3. Quality gate checks are re-run to confirm remediation.
