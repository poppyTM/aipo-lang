# Architectural decisions (ADPs)

In Aipo's engineering we follow a strict **no-invention policy**: no undocumented
arbitrary technical decision is made silently in the code.

Every open question or structural change is formalized as an **Architectural Decision
Proposal** (ADP), containing context, evaluated alternatives, rationale, impact on
documentation and acceptance criteria.

---

## Registry of approved decisions

| ADP | Title | Status | Decision summary |
| :--- | :--- | :--- | :--- |
| **[ADP-001](/decisions/adp-001)** | Core types and Bytes as values | Approved | Treats `Bytes` as a first-class VM type; tolerant slicing and `clamp` limits. |
| **[ADP-002](/decisions/adp-002)** | Construction hooks and runtime contracts | Approved | Formalization of `init()` and `invariant()` hooks with rollback in `attempt` blocks. |
| **[ADP-003](/decisions/adp-003)** | Execution budgets | Approved | Instruction-step counting mechanism for sandboxing untrusted scripts. |
| **[ADP-004](/decisions/adp-004)** | Unicode identifier policy | Approved | Mandatory Unicode NFC normalization in identifiers and string literals. |
| **[ADP-005](/decisions/adp-005)** | Parser recursion bounds | Approved | Strict recursive-depth limit in the parser to mitigate denial-of-service (DoS) attacks. |
| **[ADP-006](/decisions/adp-006)** | Open decisions of Waves 3 and 4 | Approved | Resolution of async semantics, task combinators and capability scope. |
| **[ADP-007](/decisions/adp-007)** | Package identity and distribution | Approved | `namespace.package` coordinates, GitHub dependencies pinned by commit SHA and offline cache. |
| **[ADP-008](/decisions/adp-008)** | Delimitation of the v0.1.0 language release | Approved | Language-first focus with closed scope; defers graphical engines and the web registry to post-v1. |
| **[ADP-009](/decisions/adp-009)** | Versioned synchronous C ABI | Approved | Stable C binary interface with opaque types to embed Aipo in C/C++/Zig. |
| **[ADP-010](/decisions/adp-010)** | Thin interoperability proofs | Approved | Three thin proofs demonstrating embedding in Rust, C and JavaScript. |
| **[ADP-011](/decisions/adp-011)** | Performance & ergonomics roadmap | Approved | `Value` compaction, `InvokeMethod` fusion, hoisted loop, typed failures and pattern matching. |
| **[ADP-012](/decisions/adp-012)** | Syntactic ergonomics specification | Approved | Brace-delimited blocks `{ ... }`, immutability by default in structs, `var self`, symmetric `:` and automatic interfaces. |
| **[ADP-013](/decisions/adp-013)** | WebAssembly execution substrate and self-hosting | Approved | Freezing v0.1.0 on the legacy branch, migration to WebAssembly (Wasm 2.0 / WASI) and self-hosting roadmap. |
| **[ADP-014](/decisions/adp-014)** | Embedded profile, Dual-Track architecture and `aipo-vm` evolution | Proposed | Dual-Track architecture (Wasm Web vs. native VM), nano/shell profile Lua-style, subsystem cuts and arenas. |
