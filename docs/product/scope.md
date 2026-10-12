# Aipo — Project Scope

**Status:** normative
**Scope:** in-scope capabilities, non-goals, compatibility constraints
**Blocking question:** What is explicitly out of scope?

## `aipo v0.1.0` language release

The first product boundary is a usable language release, not a complete platform. It must deliver:

- V1 language surface: syntax, modules, contracts, `Failure`/fault separation and the essential standard library.
- Async language surface: `async fn`, `await`, `task.*` and deterministic cooperative scheduling.
- Minimal CLI: `aipo run`, `aipo check`, `aipo build`, `aipo fmt` and `aipo test`.
- Rust embedding and a versioned, synchronous C ABI with opaque handles and explicit capabilities.
- Explicit, bounded JavaScript interoperability; no implicit Node or browser globals.
- One small real interoperability proof for each boundary: Rust, C and JavaScript.
- Local and pinned-GitHub packages, lockfile, cache and opt-in authentication already delivered by P04.
- Executable examples, documentation, migration notes and all repository quality gates.

The release does not promise a complete port of any external library. It proves that the boundary is safe, explicit and usable. The C ABI boundary is specified in `docs/adp/ADP-009-synchronous-c-abi.md`; the interoperability proof criteria are in `docs/adp/ADP-010-interoperability-thin-proofs.md`.

## Post-v1 product boundaries

The following remain product work after the language release:

- Godot and other engine adapters.
- The future Aipo/Petunia3D engine; its core design is intentionally deferred.
- Complete IDE tooling: navigation/refactoring, DAP, lexical frame inspection and interactive line editing beyond the initial tools.
- DOM/storage web profile, `fetch`, workers and WebAssembly.
- Registry, publication and SemVer range solving; distributed/resource-aware reload beyond the initial guest transaction.
- Visual editors, lifecycle scripts and complete third-party framework ports.

## Current implementation status

The language, VM, JavaScript and Wasm backends, async surface, host contracts, C ABI, test command and local/GitHub package path/cache are implemented. P07-G02 adds register verification and explicit canonical execution plans, persistent sessions and guest-heap reload, cooperative C execution, initial LSP/debug/profile/watch/new commands, offline vendor trees and native packaging automation.

This is implementation status, not release certification. Tests were explicitly not executed in P07-G02; platform workflows and thin-proof execution remain gates to assess from actual evidence. See the [usage/reimplementation guide](../development/runtime-and-tooling-guide.md) and [evidence](../evidence/P07-G02/README.md).

## Compatibility constraints

- Implementation edition: Rust Edition 2024; workspace MSRV 1.96 (locked Wasmtime/Cranelift dependency requirements).
- Target platforms: Tier 1 Linux (x86_64, aarch64), macOS (Apple Silicon, Intel), Windows (x86_64).
- Safe Rust first: `unsafe_code = "forbid"` for the language; narrowly documented C ABI/arena FFI exceptions retain their boundary rules.
- No Rust panic may ever leak as an Aipo runtime error.
