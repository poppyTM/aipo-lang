# Semantic compatibility matrix: Aipo-lang vs. omniLua (Lua 5.4)

> **Cardinal rule of the technical directive:**
> Aipo's semantics are defined canonically by its ADRs (ADR-001, ADP-011, ADP-012), its
> specifications in `docs/canon/` and its conformance corpus. **omniLua and the Lua
> specification have NO semantic authority over Aipo.** No divergence may be resolved by
> changing Aipo's linguistic behavior to fit Lua.

---

## 1. Comparative matrix of fundamental domains

| Semantic dimension | Aipo-lang (canonical) | omniLua (Lua 5.4.7) | Compatibility degree | Architectural decision |
|---|---|---|---|---|
| **Syntax & block structure** | Universal braces `{ ... }`, no mandatory parentheses in `if`/`while`, complete words, integer division with `//`. | Delimiting keywords `then ... end`, `do ... end`. | **INCOMPATIBLE** | **REJECT:** keep 100% of Aipo's canonical grammar (ADR-001, ADP-012). |
| **String model** | Pure Unicode text (UTF-8) with canonical NFC normalization preserved and guaranteed at concatenation boundaries. | Opaque sequence of arbitrary bytes (`&[u8]`). May contain invalid UTF-8 sequences. | **INCOMPATIBLE** | **REJECT:** never replace Aipo's `String` with unvalidated byte-strings. |
| **Collections & data structures** | Explicit, separate types: `List`, `Dict`, `Set` (insertion order), `Sequence` (lazy), `Bytes` (binary packing), typed nominal `Struct`. | Single unified structure: `Table` (hybrid array/hash indexed at 1) with dynamic dispatch via `Metatable`. | **INCOMPATIBLE** | **REJECT:** do not introduce Lua-style metatables or universal tables into Aipo. |
| **Immutability and object orientation** | Structs immutable by default (mutability requires the explicit `var` prefix). Static and runtime structural subtyping via interfaces. **No mandatory classical OOP.** | Total mutability by default in tables; ad-hoc prototyping via metatables (`__index`). | **INCOMPATIBLE** | **REJECT:** preserve Aipo's immutable-by-default and structural subtyping. |
| **Error and failure model** | **Model B+:** strict distinction between recoverable `Failure` (with structured payload, handled via `attempt { ... }` or `or_else`) and integrity `Fault` (fatal). | Arbitrary dynamic errors captured universally by `pcall` / `xpcall`. | **DIVERGENT** | **INSPIRE / ADAPT:** use omniLua's `aborting` technique to guarantee that limit-exhaustion `Fault`s (instructions/memory) are uncatchable by `attempt`. |
| **Concurrency and asynchrony** | Native `async fn`, `await` expression, deterministic cooperative `Task` scheduler with virtual time and combinators (`task.all`, `task.race`, etc.). | Manual asymmetric coroutines with explicit suspension via `coroutine.yield()` and reactivation via `coroutine.resume()`. | **DIVERGENT** | **ADAPT:** study omniLua's context and execution-stack isolation, but keep Aipo's canonical scheduler semantics. |
| **Numeric values** | `Int` (64-bit signed with overflow checking), `Float` (64-bit IEEE 754 finite; NaN and infinity are `Fault`s), `Byte` (u8). | 64-bit integer and 64-bit float with standard C semantics (NaN and infinity propagate as normal values). | **DIVERGENT** | **REJECT:** Aipo rejects NaN/infinity in the value model with an active failure. Keep `Value` invariants. |
| **Security and capabilities** | Deny-by-default model; hierarchical `CapabilitySet` validated on the host before native dispatch. | `SandboxLimits` hook for instructions/memory; optional capability stripping in `_G` via `Capability stripping`. | **COMPATIBLE IN PURPOSE** | **ADAPT:** Aipo already has superior capabilities; adapt omniLua's periodic fuel metering in the dispatcher. |
| **Rust embedding API** | Currently based on the C ABI (`aipo-c-abi`) and internal VM structures. | High-level `Lua::scope` API with safe RAII loans and lifetime-bounded borrows. | **HIGHLY ADAPTABLE** | **ADAPT (P0 priority):** develop the equivalent `AipoEngine::scope` layer for safe native Rust embedding. |

---

## 2. What may NEVER be imported from omniLua

1. **Lua parser and lexer:** Aipo's syntax was designed under neurodivergent cognitive
   guidelines (ADR-001 / ND-First) for maximum clarity, predictability and low mental overhead.
   Do not use Lua conventions (`~=`, `end`, `then`, 1-based indexing).
2. **Arbitrary byte strings as text:** Aipo guarantees NFC-normalized UTF-8 text. Lua's string
   model would corrupt Aipo's internationalization contracts.
3. **Metatables and open prototyping:** Aipo uses typed structs with invariant checking
   (`invariant()`), static field validation and interface conformance. Metatables would
   introduce non-deterministic dynamism incompatible with the model.
4. **Unrestricted capture of fatal errors:** a script must never have the power to capture a
   host security violation or a forced termination by a CPU quota.
