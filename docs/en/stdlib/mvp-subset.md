# Aipo — MVP stdlib subset (Wave 1, slices S9 + backend completion)

**Status:** normative (implementation level)
**Authority:** subordinate to `docs/canon/` (`Aipo — Stdlib V1 Canônica …`, `Aipo V1 — Language
Reference`, `Aipo Language — Especificação Viva`) and to `docs/waves/wave-1-mvp.md` (S9
deliverable)
**Scope:** the closed V1 stdlib surface implemented by `aipo-stdlib`, `aipo-runtime` and the
collection methods bound by `aipo-vm`
**Crates:** `crates/aipo-runtime`, `crates/aipo-stdlib`, `crates/aipo-vm`, `crates/aipo-cli`
**Update triggers:** function added/removed/renamed, arity change, argument rule change, new
runtime value kind, any change to the Failure/fault split below
**Related:** `docs/crates/crate-contracts.md`,
`docs/adp/ADP-001-byte-and-core-types-as-values.md`,
`docs/adp/ADP-002-construction-hooks-and-runtime-contracts.md`,
`docs/evidence/P00-G15-static-contracts-and-interface-conformance.md`

This document records **what is implemented today**, not what canon plans for later waves.
Anything canon decides that is not listed here is either deferred (see [Deferred surface](#deferred-surface))
or recorded as a decision in `docs/adp/` — ADP-001 (types as values, tolerant slicing, NFC,
`clamp` bounds) and ADP-002 (construction hooks, invariants and runtime contracts) are both
resolved, so every choice below has a canon source.

## Runtime layer (`aipo-runtime`)

- **`ModuleGraph`** — registered module records keyed by canonical path.
- **`topological_init_order()`** — deterministic initialization order: every dependency is
  initialized before its dependents; ties are broken lexicographically by canonical path.
  Missing dependencies yield `ModuleNotFound`; circular imports yield `CyclicDependency` with
  the offending chain.
- **`ModuleRecord` / `ModuleState`** — lifecycle `Uninitialized → Initializing → Initialized`
  (`Failed(reason)` on error). A module publishes nothing until it reaches `Initialized`
  (failed init publishes nothing).
- **`NativeRegistry`** — catalog of native function metadata (name, arity, owning module,
  documentation, declared capabilities and asyncness) used by tooling to describe the Prelude
  and built-in modules. It is not an authorization boundary; VM callbacks enforce the
  effective host policy.
- **`EnvironmentSource` / `FilesystemSource` / `HostContext`** — optional environment and
  filesystem providers stored per VM. Each provider is installed explicitly and exposed through
  read-only access; installation never grants a capability. No filesystem provider is
  process-global, and the VM does not perform host filesystem I/O.
- Diagnostics: `ModuleNotFound → AIPO_SEM_UNKNOWN_MODULE`,
  `CyclicDependency → AIPO_SEM_IMPORT_CYCLE`,
  `ExportNotFound → AIPO_SEM_EXPORT_UNKNOWN`.

Module *execution* (`import`/`export`/init-once wiring into the VM) was not part of S9; it is
implemented in `crates/aipo-cli/src/modules.rs` by the backend-completion goal (`P00-G10`),
which consumes the registry and ordering above: one `.aipo` file is one module, dependencies
are loaded before dependents, a module's top-level statements run exactly once, and privacy is
enforced by name resolution (a non-exported declaration is renamed to `<module>::<name>`, so an
importer that names it gets `AIPO_SEM_EXPORT_UNKNOWN`).

## Prelude V1

Available without import. Canon forbids turning the Prelude into a convenience dump, so
`print`, `map`, `filter`, `reduce`, `random`, `sleep`, `open`, `json`, `assert`, `min` and
`max` are deliberately **not** global.

### Values

| Name | Kind |
|---|---|
| `none` | `none` |
| `true` | `Bool` |
| `false` | `Bool` |

Every core type is also a **first-class type value** (`Value::Type`), so the names `none`,
`Bool`, `Int`, `Float`, `Byte`, `String`, `List`, `Dict`, `Bytes` and `Range` are all bound in
the Prelude with an identity, a canonical `name` and structural equality. `Int`, `Float`, `Byte`
and `String` additionally accept a call, which is the conversion documented below; `List`,
`Dict` and `Bytes` are type values only — they have no call form in V1 (`list(x)` is not
defined).

### Functions

| Signature | Behavior |
|---|---|
| `len(value)` | Code point count for `String`, element count for `List`/`Dict`. Propagates `Failure`; any other type is a fault. |
| `copy(value)` | Shallow copy of `List`, `Dict` or `Struct` (new identity); scalars are returned unchanged. |
| `same(a, b)` | Reference identity for `List`/`Dict`/`Struct`, content equality for primitives. |
| `some(value)` | `false` only for `none`; every other value (including `false` and `0`) is `some`. |
| `fail(message)` | Builds a recoverable `Failure` value. |

### Conversions

| Signature | Behavior |
|---|---|
| `Int(value)` | `Float` truncates toward zero; `String` is parsed as a whole. Out-of-range or invalid text yields a recoverable `Failure`. `Bool`/collections are a fault. |
| `Float(value)` | Widens `Int`; parses `String`. Invalid text or a non-finite result yields a recoverable `Failure`. |
| `Byte(value)` | Verified `0..=255` from `Int`, integral `Float` or `String`. Never wraps around, never saturates: out-of-range is a recoverable `Failure`. |
| `String(value)` | Canonical textual conversion for `String`, `Int`, `Float`, `Bool`, `none`, using the VM's rendering, so `String(v)` and `io.print(v)` agree. Collections, structs and functions are a fault (no magic stringification). |

Text parsing is strict: surrounding whitespace is not silently skipped (`string.trim` first if
needed).

## `List` / `Dict` methods

Bound by `aipo-stdlib` and dispatched by the VM through the receiver, so `xs.len()` and
`len(xs)` agree.

| Signature | Behavior |
|---|---|
| `list.add(value)` | Appends; returns the list. |
| `list.insert(index, value)` | Inserts at a code-point-style index; out-of-range is a fault. |
| `list.remove(value)` | Removes the first equal element; a missing element is a no-op. |
| `list.remove_at(index)` / `list.remove_last()` | Remove by position; out-of-range is a fault. |
| `list.clear()` | Empties in place. |
| `list.contains(value)` / `list.find(value)` | Membership; `find` returns the index or `none`. |
| `list.first()` / `list.last()` | First/last element, or `none` when empty. |
| `list.is_empty()` / `list.len()` / `list.count(value)` | Size and counting. |
| `list.reverse()` | Reverses in place using the canonical ordering. |
| `list.sort()` / `list.sort_by(key_fn)` | In-place sort; `sort_by` orders by the key the callable returns. |
| `list.transform(fn)` / `list.filter(fn)` / `list.sort_by(fn)` | Higher-order: the callable is invoked **through the VM** (not from Rust), so closures, `fail` and faults behave exactly as at any other call site. |
| `dict.has(key)` / `dict.get(key)` | Presence and lookup; a missing key in `get` is `none`. |
| `dict.keys()` / `dict.values()` | Ordered projections. |
| `dict.remove(key)` / `dict.clear()` / `dict.is_empty()` / `dict.len()` | Removal and size. |

Mutating a `List`/`Dict` while it is being iterated (`each`, `filter`, `transform`) raises
the unconditional runtime fault `AIPO_RT_MUTATION_DURING_ITERATION`.

### `Bytes`

`Bytes` is a managed, mutable binary block (`Value::Bytes`). Canon's construction form is
`Bytes(count: Int)`, which allocates `count` zero-filled bytes; indexing works by byte and
yields `Byte`, `len` reports the count and slicing returns a new block. A negative or
oversized count is a recoverable `Failure` (`BYTES_MAX_ALLOCATION` is a provisional 64 MiB cap
so a source-level size cannot exhaust memory). Packing APIs (`read_i32`, `write_f32`, …) and
`String.encode()`/`Bytes.decode()` are outside this subset.

## `math`

| Signature | Behavior |
|---|---|
| `abs(x)` | `Int` → `Int`, `Float` → `Float`; overflow of `Int::MIN` is a fault. |
| `min(a, b)` / `max(a, b)` | Mixed operands follow `Int -> Float` promotion. |
| `floor(x)` / `ceil(x)` / `round(x)` / `truncate(x)` | Return `Int` when representable; the result outside the `Int` range is a fault. `round` is half **away from zero** (`round(2.5) == 3`, `round(-2.5) == -3`). |
| `sqrt(x)` | Always `Float`; a negative input yields a recoverable `Failure`. |
| `pow(base, exponent)` | `Int ** Int(>= 0)` stays `Int` while it fits; otherwise `Float`. Non-finite results are a fault. |
| `clamp(value, min, max)` | Inclusive interval; mixed operands promote to `Float`. Inverted bounds (`min > max`) produce a recoverable `Failure`: a bound is a *value* outside a valid range, and canon reserves `Failure` for exactly that (like `Byte(value)` out of range), while an *index* outside a range is a fault. `or_else` recovers (ADP-001 Q3, closed by `P00-G15`). |

Constants: `math.pi`, `math.e`, `math.tau`.

## `string`

All text operations work on Unicode **code points**, never raw bytes. `len` counts code points;
`byte_len` exposes the UTF-8 byte length.

| Signature | Behavior |
|---|---|
| `len(text)` | Code point count. |
| `byte_len(text)` | UTF-8 byte length. |
| `contains(text, sub)` / `starts_with(text, prefix)` / `ends_with(text, suffix)` | The empty needle matches (`true`). |
| `find(text, sub)` | Code point index of the first match as `Int`, or `none` when absent. `find(text, "") == 0`. |
| `lower(text)` / `upper(text)` | Locale-neutral, deterministic, full Unicode case mapping. Normalized to NFC, like every operation that can join a base character with a combining mark. |
| `capitalize(text)` | Titlecases the first cased character, lowercases the remaining cased characters; leading punctuation, digits and symbols are preserved; text without cased characters is unchanged. |
| `reverse(text)` | Reverses **extended grapheme clusters** (composed units stay intact) and re-normalizes the result to NFC. |
| `trim(text)` | Strips leading/trailing whitespace. |
| `split(text, separator)` | List of substrings; **separator must be non-empty**; empty fields are preserved, including at the edges (`"a,,b" → ["a", "", "b"]`, `"" → [""]`). |
| `join(separator, list)` | `join(separator, []) == ""`; a one-element list returns that element; **every element must already be `String`** — there is no implicit textual coercion (use `String(value)`). |
| `replace(text, from, to)` | Replaces all non-overlapping matches; **`from` must be non-empty**. |
| `slice(text, start, end)` | Half-open `[start, end)` over code points; negative indices count from the end; out-of-range bounds are clamped to the string (canon: "slices fora da faixa são tolerantes/clamped" — the slice category rule, so it holds for `List`, `String` and `Bytes` alike; ADP-001 Q4, closed by `P00-G15`). |
| `format(template, values)` | Named `{name}` placeholders with `values` as a `Dict`; `{{`/`}}` are literal braces; extra keys are ignored; missing key, malformed template or positional placeholder (`{0}`, `{}`) yields a recoverable `Failure`. |

## `io`

| Signature | Behavior |
|---|---|
| `print(value)` | Writes the textual rendering without a trailing newline; returns `none`. |
| `println(value)` | Same, with a trailing newline. |

`io::set_output_sink` lets tests and embedders capture output deterministically.

## `time`

Canon classifies non-deterministic sources as **host capabilities**, so the clock is not a
language primitive: it is a service the host installs, and a program cannot tell a real clock
from a replay's fixed one.

| Signature | Behavior |
|---|---|
| `now()` | Wall clock as `Duration` seconds since the Unix epoch. Requires the `clock.wall` capability. |
| `monotonic()` | Monotonic clock as `Duration` seconds from an arbitrary fixed origin, never decreasing. Requires the `clock.monotonic` capability. |

Neither function has a fallback: with no clock installed the reading is a runtime fault with
the stable code `AIPO_RT_CAPABILITY_DENIED` naming the capability, exactly like any other
denied operation. `time::install_clock` grants the capability (the CLI profile installs
`SystemClock`, so `aipo run` reads the operating system's clocks) and `time::revoke_clock`
denies it; a deterministic profile installs its own `ClockSource`, which is what makes a
replay replay. The `aipo-js` shim mirrors this: the emitted entry installs the system clock,
and a host that needs determinism installs its own source on `globalThis.__aipoClock` before the
bundle loads, or sets it to `null` to deny.

`Date`/`TimeOfDay`/`DateTime` are **not** part of this surface: their calendar, offset and
IANA timezone contracts are a separate decision, and inventing a representation here would
preempt it.

## `env`

Environment access is capability-aware and per-VM. `env` is always published as a module, but
its callbacks run against the current VM's `HostContext` rather than a process-global service.

| Signature | Behavior |
|---|---|
| `get(name)` | Returns the copied `String` value, or `none` when the name is absent. Requires `env.read` and an installed `EnvironmentSource`. |
| `has(name)` | Returns `true` or `false` for a name. Requires `env.read` and an installed `EnvironmentSource`. |

A missing name is data (`none`/`false`). A missing provider is not a value: the callback raises
the same `AIPO_RT_CAPABILITY_DENIED` fault as a missing grant, after checking `env.read`.
Installing a provider does not grant the capability, and the CLI does not install an environment
provider or implicitly grant `env.read`. Tests and embedders can use `MapEnvironmentSource` with
`env::install_environment_source`; separate VMs keep separate providers and grants.

The `NativeFunctionMeta` entries for `env.get` and `env.has` declare `env.read` for tooling
and manifest auditing, but `NativeRegistry` remains a catalog rather than a security gate.

## `fs`

Filesystem access is host-only, per-VM and async-first. This slice intentionally exposes no
write, open, delete, raw-path or recursive-directory operation.

| Signature | Behavior |
|---|---|
| `read_text(path)` | Returns `Task[String]` immediately. The provider read runs only when the scheduler drives the task and awaits it. Requires `filesystem.read`; `path` must be `String`. |
| `roots()` | Returns the provider-owned roots as a sorted `List[String]`. Requires the separate `filesystem.roots` capability. |

`MapFilesystemSource` is a deterministic provider for tests and embedders. It owns ordered roots
and a path-to-content map, copies text across the boundary, rejects invalid roots, parent escapes
and paths outside the configured roots, and never calls `std::fs`. The deterministic map has no
symlinks; a real provider must reject symlink escape before reading. A missing file or provider
read error is a recoverable `Failure`; a missing provider or capability is the stable
`AIPO_RT_CAPABILITY_DENIED` fault. Installing a provider does not grant `filesystem.read` or
`filesystem.roots`, and the CLI does not install either provider or grant either capability.

The AHS exposed by `aipo_stdlib::fs::schema` declares the exact per-function capabilities and
`read_text.is_async = true`. `NativeRegistry` remains catalog-only. The JavaScript/Web target
does not advertise either filesystem capability and has no filesystem fallback; an embedder must
provide a host implementation explicitly.

## `String` and Unicode normalization

NFC is an **invariant** of every `String` value, not an operation performed at `==`. It is
established where a `String` is constructed and where an operation can introduce a
non-normalized result (ADP-001 Q5, closed by `P00-G15`):

| Boundary | Where |
|---|---|
| Literal decoding (all prefixes, multi-line strings, identifiers) | `crates/aipo-lexer` |
| Conversion `String(value)` | `crates/aipo-vm::convert` |
| Concatenation `+`, which interpolation (`f"…"`) lowers to | `crates/aipo-vm::value` |
| `lower`, `upper`, `capitalize`, `replace`, `join`, `format`, `reverse` | `crates/aipo-stdlib` |

Operations that are pure substrings (`slice`, `split`, `trim`) preserve the invariant for free:
removing characters never makes two previously non-adjacent characters adjacent. `==` stays
structural equality over the stored value. Reading a constant out of the bytecode pool is *not* a
normalization point, because the only producer of program-visible string constants is the lexer;
the point would need revisiting if an input boundary (host text input, `Bytes.decode()`) enters
the MVP.

## Error model

Two distinct channels, never mixed:

- **Recoverable `Failure`** (Model B) — domain problems a program can recover from with
  `or_else` / `attempt … failed … end`: invalid text in conversions, out-of-range `Byte`,
  missing `format` key, negative `sqrt`, inverted `clamp` bounds, and `fs.read_text`
  missing-file or provider-read failures.
- **Runtime fault (`VmFault`)** — programming or contract violations that are not recoverable:
  wrong operand category, wrong arity, empty `split`/`replace` separator, non-`String` `join`
  element, numeric overflow past the `Int` range, non-finite `Float`, invalid filesystem
  path/provider contracts, and missing host capabilities (including an absent filesystem
  provider).

No Rust panic escapes as an Aipo error; every native returns `Result`.

## Contracts and invariant boundaries

The subset keeps two written promises, and both are checked while the program runs:

- **Signature contracts** (`name: Type`, `name!: Type`, `-> T`, `T?`) are checked at the call
  boundary — parameters in the callee's prologue, after the default prologue so a defaulted
  value is checked too, and returns immediately before each `return expr`. `T?` means exactly `T`
  or `none`; the simple `Function` contract only requires the value to be callable; a `struct`
  name is matched against the instance's type. A violation discovered at runtime is the fault
  channel above (`AIPO_RT_TYPE_MISMATCH`). A contract the analyzer can already prove wrong — a
  literal argument or return against a written core-type contract — is reported *before*
  execution as `AIPO_SEM_CONTRACT_VIOLATION_STATIC`, so a provable mistake never waits for the
  run.
- **Interface contracts** are structural, as canon keeps interfaces: naming one asks whether the
  value exposes the operations it declares, compared by **caller-visible arity** (the receiver
  is not an argument at a call site). A value without the operation, or with a same-named
  operation of a different arity, is a contract fault, and the message names the declared struct
  type. `none` satisfies `T?` without exposing anything.
- **`invariant()`** is verified at the end of construction — a fault, because the instance
  must never be published in a forbidden state — and at stable mutable boundaries, which are the
  **recoverable `Failure`** channel, because there is an entry state to restore. A field
  assignment to a published instance is provisional until the enclosing boundary (the end of
  the operation, or the mutation statement itself in the entry script): when the predicate fails,
  the direct fields of every participating instance return to their entry values and the
  operation produces a `Failure`. Validation never runs after each internal assignment, so an
  operation may pass through a transiently invalid state.

## Deferred surface

Outside this slice (tracked in the canon backlog): `format` positional placeholders and format
specifiers (`{price:.2f}`), `Bytes` packing APIs (`read_i32`/`write_i32`/…,
`String.encode()`/`Bytes.decode()`), `casefold`, `graphemes`, `words`, `lines`, explicit
advanced Unicode normalization, `Set`, lazy `Sequence`, regex/json/http and the remaining
capability-aware modules. Wave 1 non-delivery that Wave 2 slice W2-1 has since closed (see
`docs/evidence/P01-G01-js-parity-mvp-subset.md`): the `aipo-js` backend now ships the MVP
subset with differential parity (CLI is `run`/`check`/`build`/`fmt`). Wave 3 has since closed
`Set`, lazy `Sequence`, the `Bytes` packing APIs (`read_i32`/`write_i32`/…,
`String.encode()`/`Bytes.decode()`), `Duration` and the whole async surface (`Task`/`Group`
values, a cooperative scheduler, `async fn`/`await`/`await do` and the `task.*` combinators) —
see `docs/evidence/P02-G01-wave3-types-and-values.md`.

Wave 4 has since delivered the host ABI (`aipo-host`), the VM host-task path, the restricted
VM-only `fs` read/roots surface, the `time` clock module and the Poppy ECS host. Filesystem
writes, arbitrary handles and JS/Web fallback remain outside this stdlib slice.

Still explicitly deferred for the language release: LSP and REPL, registry/publication, a
DOM/storage web profile, general network modules, a visual editor, full hot reload and the
future Aipo/Petunia3D engine. `regex`, `json`, `Date`/`TimeOfDay`/`DateTime`, `aipo-poppy`
and the local/GitHub package path are already delivered. The C ABI, JS interoperability
boundary and minimal `aipo test` runner are the next `aipo v0.1.0` targets. Type values for
`List`/`Dict`/`Bytes` and a dedicated `Byte` runtime kind were listed here previously and are
now delivered — see `docs/evidence/P00-G10-backend-completion.md`.
