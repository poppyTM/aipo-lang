# Risk, threat and architectural-safeguard mapping

Audit of risks in incorporating omniLua-inspired techniques into Aipo-lang (Phase 0).

---

## 1. Linguistic and semantic integrity risks

### Risk 1: semantic contamination by Lua
- **Description:** trying to "simplify" collections, strings or error handling by adopting the
  omniLua pattern (e.g. allowing opaque byte-strings instead of strict UTF-8; tolerating
  dynamic metatables).
- **Impact:** destruction of Aipo's identity, breakage of the type and static-contract model
  (ADP-012), and loss of cognitive coherence (ADR-001 / ND-First).
- **Mandatory mitigation:** explicit `REJECT` of any Lua-specific semantics. Every candidate
  code must be validated against Aipo's canonical conformance corpus in `docs/conformance/`.

### Risk 2: multi-backend parity breakage (VM ↔ JS ↔ Wasm)
- **Description:** adding functionality or changing dispatcher behavior in the Rust VM so that
  the `aipo-js` and `aipo-wasm` emitters fall behind or become incompatible.
- **Impact:** language fragmentation and breakage of Aipo's universal portability contract.
- **Mandatory mitigation:** no semantic change enters without the differential suites
  (`crates/aipo-js/tests/differential.rs` and `crates/aipo-wasm/tests/`) staying 100% green
  with bit-identical results.

---

## 2. Sandbox security and reliability risks

### Risk 3: false sense of security in in-process sandboxing
- **Description:** believing that limiting instructions (fuel) and sampled memory prevents all
  attacks by untrusted scripts.
- **Technical fact:** per-opcode fuel only interrupts execution in the VM dispatcher; a call to
  a blocking host native (`HostNative`) (e.g. heavy regex computation, a native image loop, or an
  external socket read) cannot be preemptively interrupted by opcode fuel without explicit
  suspension points.
- **Mandatory mitigation:**
  1. Keep a strict `CapabilitySet` (deny-by-default): untrusted scripts do not receive access to
     blocking resources.
  2. Document that in-process sandboxing is a containment barrier against accidental errors and
     logic timeouts, not cryptographic OS-level isolation.
  3. For strict isolation against malicious adversaries, recommend a separate process or WASI
     isolation.

### Risk 4: breakage of the `#![forbid(unsafe_code)]` invariant
- **Description:** trying to copy low-level optimizations or raw pointers to speed up the GC or
  the VM.
- **Impact:** violation of the institutional Zero Unsafe policy established in the workspace's
  root `Cargo.toml`.
- **Mandatory mitigation:** unconditional rejection of `unsafe` blocks. All adaptations for Aipo
  must be written in 100% idiomatic Safe Rust, validated by the compiler.

---

## 3. Performance and maintenance risks

### Risk 5: hot-path degradation from invasive metering
- **Description:** inserting fuel counters, atomic calls or complex checks on every instruction
  executed in the dispatch loop (`Vm::step`), raising the dispatch floor above ~100 ns/opcode.
- **Mandatory mitigation:**
  - Adopt omniLua's **periodic sampling** technique (`interval metering`): meter fuel in blocks
    (e.g. every 64 or 256 steps) or couple counting to the existing trap mask, ensuring the
    normal-path cost stays close to zero.
  - Require a paired comparative benchmark (`scripts/perf/paired.sh`) before accepting the
    change.

### Risk 6: third-party license and provenance compliance
- **Description:** copying omniLua code without keeping the due license attributions.
- **Impact:** violation of third-party MIT copyright and of the PUC-Rio notice terms (Lua 5.4.7).
- **Mandatory mitigation:**
  - Every adapted or copied code snippet must keep explicit attribution to the omniLua
    contributors and to PUC-Rio in the file header and in `THIRD_PARTY_CODE.md`.
